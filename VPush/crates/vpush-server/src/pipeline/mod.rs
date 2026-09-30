//! From an event on a relay to pushes on phones.
//!
//! The server sees events as a relay does, sealed, so it decides little:
//! whom the event is for, whether its author marked it as not worth a push,
//! whether it was dealt with already, and whether the device was woken a
//! moment ago, or too often. Everything that needs the content is the device's to decide:
//! the push carries the event itself, and the device opens it.
//!
//! Every event gets a `trace`. It is in every log line about the event and
//! in the pushes it caused, so one search shows the whole way from the relay
//! to the phone.

pub mod classify;
pub mod throttle;

use std::sync::Arc;
use std::time::{Duration, Instant};

use nostr::event::Event;
use tokio::sync::Semaphore;
use vpush_proto::{Payload, PushType};

use crate::api::{next_request_id, now};
use crate::config::PipelineConfig;
use crate::counters::Counters;
use crate::delivery::retry::{self, RetryPolicy};
use crate::delivery::{mask, Message, Outcome, ProviderKind, Providers, Target};
use crate::store::{AllStore, Recipient, Seen};
use classify::Subject;
use throttle::{DeviceCap, Named, Room, Throttle, Verdict};

/// How long the push service keeps a push for a phone that is off.
const TTL: Duration = Duration::from_secs(24 * 3600);
/// Pushes on their way to the push services at one time.
const SENDING: usize = 32;
/// For how long an event that was dealt with is remembered. A relay is
/// asked two days back, where the senders of sealed messages date them, so
/// an event seen longer ago than this does not come again.
pub const SEEN_FOR: u64 = 3 * 86_400;
/// How far ahead of the server's clock an event may be dated: clocks differ
/// by minutes, not by more.
const AHEAD: u64 = 15 * 60;

/// What became of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dealt {
    /// Not an event the server has anything to do with.
    Ignored,
    /// It was on the relay before the watch began.
    Baseline,
    /// Marked by its author as not worth a push.
    Quiet,
    /// For nobody who is registered.
    Nobody,
    /// Dealt with before: from another relay, or before a reconnect.
    Duplicate,
    /// Dated further from now than an event that was just written can be.
    Misdated,
    Pushed {
        /// Pushed at once.
        now: usize,
        /// Counted, for the push at the end of the window.
        later: usize,
        /// Devices of the author, which are not told.
        own: usize,
        /// Devices that were pushed to as often as a device may be:
        /// counted, for their `sync` push.
        over: usize,
    },
}

/// How often a device is woken.
#[derive(Debug, Clone, Copy)]
pub struct Waking {
    /// About one chat: once in this long.
    pub chat_every: Duration,
    /// About anything: this many times within `device_window`.
    pub device_cap: usize,
    pub device_window: Duration,
}

impl From<&PipelineConfig> for Waking {
    fn from(config: &PipelineConfig) -> Self {
        Self {
            chat_every: Duration::from_secs(config.throttle_secs),
            device_cap: config.device_cap as usize,
            device_window: Duration::from_secs(config.device_window_secs),
        }
    }
}

pub struct Pipeline {
    store: Arc<dyn AllStore>,
    providers: Arc<Providers>,
    throttle: Throttle,
    cap: DeviceCap,
    sending: Arc<Semaphore>,
    retry: RetryPolicy,
    counters: Arc<Counters>,
}

/// A push on its way out: what is sent, to whom, and what the log says of it.
struct Parcel {
    trace: String,
    /// `dm`, `group`, `sync`.
    kind: &'static str,
    /// The relay the event came from. A `sync` push is about no one event.
    relay: Option<String>,
    to: Recipient,
    /// How many events the push stands for.
    count: u32,
    message: Message,
}

/// A push that is decided on and not built yet.
#[derive(Clone)]
struct Order {
    trace: String,
    relay: String,
    event_id: String,
    /// The event as the relay gave it, JSON: what the push carries when it
    /// fits. Shared by the orders of every recipient of the event.
    event: Arc<str>,
    subject: Subject,
    to: Recipient,
}

impl Order {
    /// What the push is about, as the throttle and the phone tell chats apart.
    fn chat(&self) -> String {
        match &self.subject {
            Subject::Dm { .. } => "dm".to_string(),
            Subject::Group { id } => format!("group:{id}"),
        }
    }

    fn key(&self) -> throttle::Key {
        (self.to.pubkey.clone(), self.to.device_id.clone(), self.chat())
    }

    fn device(&self) -> throttle::Device {
        (self.to.pubkey.clone(), self.to.device_id.clone())
    }

    /// The event as the push at the end of a window names it.
    fn named(&self) -> Named {
        Named {
            event_id: self.event_id.clone(),
            relay: self.relay.clone(),
        }
    }
}

impl Pipeline {
    pub fn new(
        store: Arc<dyn AllStore>,
        providers: Arc<Providers>,
        waking: Waking,
        counters: Arc<Counters>,
    ) -> Arc<Self> {
        Self::with_retry(store, providers, waking, counters, RetryPolicy::default())
    }

    pub fn with_retry(
        store: Arc<dyn AllStore>,
        providers: Arc<Providers>,
        waking: Waking,
        counters: Arc<Counters>,
        retry: RetryPolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            providers,
            throttle: Throttle::new(waking.chat_every),
            cap: DeviceCap::new(waking.device_cap, waking.device_window),
            sending: Arc::new(Semaphore::new(SENDING)),
            retry,
            counters,
        })
    }

    /// Deals with an event seen on `relay`. `baseline`: the event is of a
    /// key or a group the server has only begun to watch there, and was on
    /// the relay before.
    pub async fn event(self: &Arc<Self>, relay: &str, event: &Event, baseline: bool) -> Dealt {
        let trace = next_request_id();
        let dealt = match self.deal(&trace, relay, event, baseline).await {
            Ok(dealt) => dealt,
            Err(e) => {
                tracing::error!(trace, relay, error = %e, "event not dealt with");
                return Dealt::Ignored;
            }
        };
        tracing::debug!(
            trace,
            relay,
            event = %event.id.to_hex(),
            kind = event.kind.as_u16(),
            dealt = ?dealt,
            "event"
        );
        dealt
    }

    async fn deal(
        self: &Arc<Self>,
        trace: &str,
        relay: &str,
        event: &Event,
        baseline: bool,
    ) -> crate::store::Result<Dealt> {
        let Some(subject) = classify::subject(event) else {
            return Ok(Dealt::Ignored);
        };
        let id = event.id.to_hex();
        let at = now();

        if baseline {
            self.store.first_seen(&id, Seen::Baseline, at).await?;
            return Ok(Dealt::Baseline);
        }
        // An event dated far from now was not written just now. Dated long
        // ago, it may have been pushed and then forgotten with all that is
        // older than `SEEN_FOR`. Dated ahead, it is newer than any date a
        // relay is asked from for as long as its date is ahead, and is
        // given again after it was forgotten here. Pushed, either is
        // pushed once more every time it is forgotten.
        let dated = event.created_at.as_secs();
        if dated > at + AHEAD || dated < at.saturating_sub(SEEN_FOR) {
            self.store.first_seen(&id, Seen::Misdated, at).await?;
            return Ok(Dealt::Misdated);
        }
        if classify::is_silent(event) {
            self.store.first_seen(&id, Seen::Quiet, at).await?;
            return Ok(Dealt::Quiet);
        }

        let recipients = match &subject {
            Subject::Dm { recipients } => {
                let mut all = Vec::new();
                for pubkey in recipients {
                    all.extend(self.store.dm_recipients(pubkey, at).await?);
                }
                all
            }
            Subject::Group { id } => self.store.group_recipients(id, at).await?,
        };
        if recipients.is_empty() {
            self.store.first_seen(&id, Seen::Nobody, at).await?;
            return Ok(Dealt::Nobody);
        }
        if !self.store.first_seen(&id, Seen::Pushed, at).await? {
            return Ok(Dealt::Duplicate);
        }

        let json: Arc<str> = event.as_json().into();
        let (mut pushed, mut later, mut own, mut over) = (0, 0, 0, 0);
        for to in recipients {
            // A group event says nothing of its author on the outside but
            // the author's mark, which only the author's device can match.
            if matches!(subject, Subject::Group { .. })
                && classify::written_by(event, to.author_key.as_deref())
            {
                own += 1;
                continue;
            }
            let order = Order {
                trace: trace.to_string(),
                relay: relay.to_string(),
                event_id: id.clone(),
                event: Arc::clone(&json),
                subject: subject.clone(),
                to,
            };
            match self.throttle.event(&order.key(), Instant::now(), order.named()) {
                Verdict::Now => {
                    if self.push(order, 1) {
                        pushed += 1;
                    } else {
                        over += 1;
                    }
                }
                Verdict::Later { wake_in } => {
                    later += 1;
                    if let Some(wait) = wake_in {
                        self.send_later(order, wait);
                    }
                }
            }
        }
        Ok(Dealt::Pushed { now: pushed, later, own, over })
    }

    /// Comes back when the window ends and pushes once for what was counted.
    fn send_later(self: &Arc<Self>, mut order: Order, wait: Duration) {
        let pipeline = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            if let Some(due) = pipeline.throttle.due(&order.key(), Instant::now()) {
                // The push names the last event counted. With one counted,
                // that is the event of this order, and the push carries it.
                order.event_id = due.last.event_id;
                order.relay = due.last.relay;
                pipeline.push(order, due.count);
            }
        });
    }

    /// Sends the push for `count` events, unless the device was pushed to
    /// as often as a device may be. Then the events are counted, and told
    /// in one `sync` push when the time is up. False when they were counted.
    fn push(self: &Arc<Self>, order: Order, count: u32) -> bool {
        match self.cap.push(&order.device(), Instant::now(), count) {
            Room::Yes => {
                self.send(Parcel {
                    message: Self::message(&order, count),
                    trace: order.trace,
                    kind: order.subject.kind(),
                    relay: Some(order.relay),
                    to: order.to,
                    count,
                });
                true
            }
            Room::No { wake_in } => {
                if let Some(wait) = wake_in {
                    self.sync_later(order, wait);
                }
                false
            }
        }
    }

    /// Comes back when the time is up and says, in one push, how many
    /// events came over what the device is pushed about one by one. The
    /// push carries the trace of the first of them.
    fn sync_later(self: &Arc<Self>, order: Order, wait: Duration) {
        let pipeline = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            if let Some(count) = pipeline.cap.due(&order.device()) {
                pipeline.counters.sync_pushes.add();
                pipeline.send(Parcel {
                    message: Self::sync_message(&order.trace, count),
                    trace: order.trace,
                    kind: PushType::Sync.as_str(),
                    relay: None,
                    to: order.to,
                    count,
                });
            }
        });
    }

    /// The push that says how many came, and nothing of any one of them:
    /// no event, no id, no group. The device shows the number, and takes
    /// the messages from its relays itself.
    fn sync_message(trace: &str, count: u32) -> Message {
        let mut payload = Payload::new(PushType::Sync);
        payload.count = Some(count);
        payload.trace = Some(trace.to_string());
        Message {
            payload,
            fallback: None,
            // One number replaces another while the phone is off; the app
            // fetches what there is, however many the last push named.
            collapse_key: Some(Self::collapse(PushType::Sync.as_str())),
            ttl: TTL,
            urgent: true,
        }
    }

    /// The push carries the event itself when it fits and stands for that
    /// one event. A push for several events names the last of them: the
    /// device cannot show them all from one, and takes them from the relay.
    fn message(order: &Order, count: u32) -> Message {
        let mut payload = match &order.subject {
            Subject::Dm { .. } => Payload::new(PushType::Dm),
            Subject::Group { id } => {
                let mut p = Payload::new(PushType::Group);
                p.group_id = Some(id.clone());
                p
            }
        };
        payload.count = (count > 1).then_some(count);
        payload.trace = Some(order.trace.clone());

        let mut by_id = payload.clone();
        by_id.event_id = Some(order.event_id.clone());
        by_id.relay = Some(order.relay.clone());

        let mut whole = payload;
        whole.event = Some(order.event.to_string());

        let (payload, fallback) = if count == 1 && whole.fits() {
            (whole, Some(by_id))
        } else {
            (by_id, None)
        };
        Message {
            payload,
            fallback,
            collapse_key: Some(Self::collapse(&order.chat())),
            ttl: TTL,
            urgent: true,
        }
    }

    /// A key the push service replaces a waiting push by: the newest word
    /// about a chat is the one a phone that was off gets.
    fn collapse(about: &str) -> String {
        mask(about).trim_start_matches('#').to_string()
    }

    fn send(self: &Arc<Self>, parcel: Parcel) {
        let pipeline = Arc::clone(self);
        tokio::spawn(async move {
            let Ok(_permit) = pipeline.sending.acquire().await else {
                return;
            };
            let Parcel { trace, kind, relay, to, count, message } = parcel;
            let target = Target { token: to.token.clone() };
            let provider = to
                .provider
                .parse::<ProviderKind>()
                .and_then(|kind| pipeline.providers.get(&to.app_id, kind));
            let provider = match provider {
                Ok(provider) => provider,
                Err(e) => {
                    tracing::warn!(
                        trace = %trace,
                        device = %to.device_id,
                        error = %e,
                        "push not sent: no way to this device"
                    );
                    return;
                }
            };
            let delivery =
                retry::deliver(provider.as_ref(), &target, &message, pipeline.retry).await;
            let outcome = match delivery.outcome {
                Outcome::Delivered => "delivered",
                Outcome::DeadToken => "dead_token",
                Outcome::Rejected => "rejected",
                Outcome::Retry => "retry",
            };
            let last = delivery.attempts.last();
            tracing::info!(
                trace = %trace,
                kind,
                relay = relay.as_deref(),
                owner = %mask(&to.pubkey),
                device = %to.device_id,
                token = %target.masked(),
                count,
                with_event = message.payload.event.is_some(),
                bytes = message.payload.data_len(),
                outcome,
                attempts = delivery.attempts.len(),
                http_status = last.and_then(|a| a.http_status),
                code = last.and_then(|a| a.code.as_deref()),
                "push"
            );
            if let Err(e) = pipeline
                .store
                .record_outcome(&to.pubkey, &to.device_id, &to.token, outcome, now())
                .await
            {
                tracing::error!(trace = %trace, error = %e, "outcome not recorded");
            }
        });
    }

    /// Chats held back by the throttle now, for the status.
    pub fn held(&self) -> usize {
        self.throttle.len()
    }
}
