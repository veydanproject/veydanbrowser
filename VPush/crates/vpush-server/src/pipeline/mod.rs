//! From an event on a relay to pushes on phones.
//!
//! The server sees events as a relay does, sealed, so it decides little:
//! whom the event is for, whether its author marked it as not worth a push,
//! whether it was dealt with already, and whether the device was woken a
//! moment ago. Everything that needs the content is the device's to decide:
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
use crate::delivery::retry::{self, RetryPolicy};
use crate::delivery::{mask, Message, Outcome, ProviderKind, Providers, Target};
use crate::store::{AllStore, Recipient, Seen};
use classify::Subject;
use throttle::{Named, Throttle, Verdict};

/// How long the push service keeps a push for a phone that is off.
const TTL: Duration = Duration::from_secs(24 * 3600);
/// Pushes on their way to the push services at one time.
const SENDING: usize = 32;

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
    Pushed {
        /// Pushed at once.
        now: usize,
        /// Counted, for the push at the end of the window.
        later: usize,
        /// Devices of the author, which are not told.
        own: usize,
    },
}

pub struct Pipeline {
    store: Arc<dyn AllStore>,
    providers: Arc<Providers>,
    throttle: Throttle,
    sending: Arc<Semaphore>,
    retry: RetryPolicy,
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

    /// The event as the push at the end of a window names it.
    fn named(&self) -> Named {
        Named {
            event_id: self.event_id.clone(),
            relay: self.relay.clone(),
        }
    }
}

impl Pipeline {
    pub fn new(store: Arc<dyn AllStore>, providers: Arc<Providers>, window: Duration) -> Arc<Self> {
        Self::with_retry(store, providers, window, RetryPolicy::default())
    }

    pub fn with_retry(
        store: Arc<dyn AllStore>,
        providers: Arc<Providers>,
        window: Duration,
        retry: RetryPolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            providers,
            throttle: Throttle::new(window),
            sending: Arc::new(Semaphore::new(SENDING)),
            retry,
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
        let (mut pushed, mut later, mut own) = (0, 0, 0);
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
                    pushed += 1;
                    self.send(order, 1);
                }
                Verdict::Later { wake_in } => {
                    later += 1;
                    if let Some(wait) = wake_in {
                        self.send_later(order, wait);
                    }
                }
            }
        }
        Ok(Dealt::Pushed { now: pushed, later, own })
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
                pipeline.send(order, due.count);
            }
        });
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
            collapse_key: Some(Self::collapse_key(order)),
            ttl: TTL,
            urgent: true,
        }
    }

    /// A key the push service replaces a waiting push by: the newest word
    /// about a chat is the one a phone that was off gets.
    fn collapse_key(order: &Order) -> String {
        mask(&order.chat()).trim_start_matches('#').to_string()
    }

    fn send(self: &Arc<Self>, order: Order, count: u32) {
        let pipeline = Arc::clone(self);
        tokio::spawn(async move {
            let Ok(_permit) = pipeline.sending.acquire().await else {
                return;
            };
            let target = Target { token: order.to.token.clone() };
            let provider = order
                .to
                .provider
                .parse::<ProviderKind>()
                .and_then(|kind| pipeline.providers.get(&order.to.app_id, kind));
            let provider = match provider {
                Ok(provider) => provider,
                Err(e) => {
                    tracing::warn!(
                        trace = %order.trace,
                        device = %order.to.device_id,
                        error = %e,
                        "push not sent: no way to this device"
                    );
                    return;
                }
            };
            let message = Self::message(&order, count);
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
                trace = %order.trace,
                kind = order.subject.kind(),
                relay = %order.relay,
                owner = %mask(&order.to.pubkey),
                device = %order.to.device_id,
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
                .record_outcome(&order.to.pubkey, &order.to.device_id, outcome, now())
                .await
            {
                tracing::error!(trace = %order.trace, error = %e, "outcome not recorded");
            }
        });
    }

    /// Chats held back by the throttle now, for the status.
    pub fn held(&self) -> usize {
        self.throttle.len()
    }
}
