//! One relay on the line.
//!
//! This is the only place that knows nostr-sdk. The connection is its own:
//! it comes back by itself after it is lost, for as long as it takes, and
//! checks that the relay still answers.
//!
//! What a relay sends is not dealt with where it is received. One loop, the
//! intake, only moves it into a queue; the work is done by a task of its
//! own, one thing at a time and in the order it came. An event costs several
//! trips to the database, and a relay sends a thousand in the time of one.
//!
//! The queue has an end. When it is full the line starts over: the relay is
//! let go, connected to anew, and asked again as after any time away. What
//! was put into the queue is remembered and not taken a second time, so the
//! line that starts over has room for what it lost, not for what it had.
//!
//! nostr-sdk hands on what a relay says through a channel that drops the
//! oldest when it is full, and its stream of notifications does not say
//! that it did. So the line lets an event into nostr-sdk only while that
//! channel has room for it (see [`Door`]): nothing is dropped where nobody
//! would know.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use nostr::event::{Event, EventId};
use nostr::filter::{Filter, SingleLetterTag};
use nostr::message::{RelayMessage, SubscriptionId};
use nostr::prelude::{Kind, RelayUrl, Timestamp};
use nostr_sdk::policy::{AdmitPolicy, AdmitStatus};
use nostr_sdk::relay::{Relay, RelayNotification, RelayOptions, RelayStatus};
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::sync::{watch, Notify, Semaphore};
use tokio::time::Instant;

use super::plan::Sub;
use super::stock::Stock;
use super::{Shared, Tuning, Watch};
use crate::api::now;
use crate::pipeline::classify::{self, KIND_GROUP, KIND_WRAP};
use crate::pipeline::Pipeline;
use crate::relays::AllowedRelay;
use crate::store::{AllStore, RelayPlan, WatchKind};

/// A relay that is on the line is noted as such this often.
const ALIVE_EVERY: Duration = Duration::from_secs(60);
/// Subscriptions are made anew this often: what is asked for is "from two
/// days ago", and "ago" moves.
const RENEW_EVERY: Duration = Duration::from_secs(3600);
/// How many events a line remembers having put into its queue. Ten queues:
/// a relay that holds more than this for the keys watched there sends the
/// oldest of it again with every asking, and the store tells those apart.
const REMEMBERED: usize = 100_000;
/// How long an event waits at the door for room in nostr-sdk's channel. The
/// intake empties that channel in no time at all; an event that waited this
/// long waits for something that is not going to happen.
const DOOR_WAIT: Duration = Duration::from_secs(10);

/// The address to connect to. The key of a gated relay goes into the query;
/// it is the only place the key is ever put, and the log writer takes it out
/// of whatever is logged.
fn address(relay: &AllowedRelay) -> String {
    match &relay.api_key {
        Some(key) => format!("{}/?key={key}", relay.url),
        None => relay.url.clone(),
    }
}

fn filter(sub: &Sub) -> Filter {
    let (kind, letter) = match sub.kind {
        WatchKind::Dm => (KIND_WRAP, SingleLetterTag::LOWERCASE_P),
        WatchKind::Group => (KIND_GROUP, SingleLetterTag::LOWERCASE_H),
    };
    Filter::new()
        .kind(Kind::from(kind))
        .custom_tags(letter, sub.targets.iter().cloned())
        .since(Timestamp::from_secs(sub.since))
}

/// The events a line has put into its queue, by id.
///
/// A relay sends what it has again whenever it is asked again: every hour,
/// after every reconnect, and when the line starts over. What is remembered
/// here is turned away at the door. The oldest id goes when there are too
/// many; an event forgotten here and sent again is told apart by the store,
/// which forgets nothing for three days.
struct Memory {
    ids: HashSet<EventId>,
    /// The same ids, the oldest first.
    order: VecDeque<EventId>,
    max: usize,
}

impl Memory {
    fn new(max: usize) -> Self {
        Self {
            ids: HashSet::new(),
            order: VecDeque::new(),
            max,
        }
    }

    fn knows(&self, id: &EventId) -> bool {
        self.ids.contains(id)
    }

    fn remember(&mut self, id: EventId) {
        if !self.ids.insert(id) {
            return;
        }
        self.order.push_back(id);
        if self.order.len() > self.max {
            if let Some(oldest) = self.order.pop_front() {
                self.ids.remove(&oldest);
            }
        }
    }
}

/// How much is on its way through nostr-sdk, from the door to the intake.
struct Flow {
    /// Room for the events on their way. The door takes a place for every
    /// event it lets in, and the intake gives the place back when the event
    /// comes out.
    room: Semaphore,
    /// The door found no room for as long as it waits.
    stuck: Notify,
}

impl Flow {
    /// An event takes two places of nostr-sdk's channel, as an event and as
    /// what the relay said. Events get half of the channel; the other half
    /// is for the rest of what a relay says.
    fn new(notifications: usize) -> Self {
        Self {
            room: Semaphore::new((notifications / 4).max(1)),
            stuck: Notify::new(),
        }
    }
}

/// What every event of the relay passes before nostr-sdk hands it on.
///
/// nostr-sdk asks it of every event, one at a time, and waits for the
/// answer before it reads the relay further. So an event that waits here
/// holds the relay back, and nothing behind it is dropped meanwhile.
struct Door {
    memory: Arc<Mutex<Memory>>,
    flow: Arc<Flow>,
}

impl std::fmt::Debug for Door {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Door")
    }
}

impl AdmitPolicy for Door {
    fn admit_event<'a>(
        &'a self,
        _relay: &'a RelayUrl,
        _subscription: &'a SubscriptionId,
        event: &'a Event,
    ) -> Pin<Box<dyn Future<Output = Result<AdmitStatus, nostr_sdk::error::Error>> + Send + 'a>>
    {
        // Turned away without a reason: nostr-sdk drops the event and
        // says nothing of it, and a relay sends what it has by the thousand.
        const NO: AdmitStatus = AdmitStatus::Rejected { reason: None };
        Box::pin(async move {
            let known = self.memory.lock().unwrap().knows(&event.id);
            if known {
                return Ok(NO);
            }
            match tokio::time::timeout(DOOR_WAIT, self.flow.room.acquire()).await {
                Ok(Ok(place)) => {
                    // Given back by the intake, when the event comes out.
                    place.forget();
                    Ok(AdmitStatus::Success)
                }
                // The line is starting over, and will ask for the event again.
                Ok(Err(_)) => Ok(NO),
                Err(_) => {
                    self.flow.stuck.notify_one();
                    Ok(NO)
                }
            }
        })
    }
}

/// What waits in the queue of a line, in the order it came.
enum Item {
    Event(Box<Event>),
    /// The relay sent all it had stored for a subscription.
    StoredIsOver(String),
    /// The relay will not serve a subscription, and said why.
    Refused { sub: String, why: String },
    /// What became of the connection, and when.
    Status(RelayStatus, u64),
    /// What is watched changed: the requests are made anew.
    Plan(RelayPlan),
    /// An hour passed: the requests are made anew.
    Renew,
    /// The relay was on the line at this time.
    Alive(u64),
}

impl Item {
    /// What the line does something about, of all that nostr-sdk tells.
    ///
    /// An event is taken from what the relay said, not from nostr-sdk's own
    /// notification of an event: that one is held back for an id nostr-sdk
    /// has seen, and which events were dealt with is for the line to say.
    fn of(notification: RelayNotification) -> Option<Self> {
        match notification {
            RelayNotification::Message { message } => match *message {
                RelayMessage::Event { event, .. } => Some(Self::Event(Box::new(event.into_owned()))),
                RelayMessage::EndOfStoredEvents(id) => Some(Self::StoredIsOver(id.to_string())),
                RelayMessage::Closed { subscription_id, message } => Some(Self::Refused {
                    sub: subscription_id.to_string(),
                    why: message.chars().take(200).collect(),
                }),
                _ => None,
            },
            RelayNotification::RelayStatus { status } => Some(Self::Status(status, now())),
            _ => None,
        }
    }
}

/// Why a line ended.
enum Ended {
    /// Nobody wants the relay watched any more, or nostr-sdk gave it up.
    Stopped,
    /// More came than the line had room for. Nothing of it is lost: the
    /// relay has it, and is asked again.
    Overrun(&'static str),
}

struct Line {
    url: String,
    relay: Relay,
    watch: Arc<Watch>,
    store: Arc<dyn AllStore>,
    pipeline: Arc<Pipeline>,
    stock: Stock,
    /// What was last asked for, and whether the relay heard all of it.
    plan: RelayPlan,
    unsent: bool,
}

impl Line {
    async fn subscribe(&mut self, plan: &RelayPlan) {
        self.plan = plan.clone();
        self.unsent = false;
        // Every request is taken off the list nostr-sdk keeps, whether the
        // relay can be told or not. `unsubscribe_all` stops at the first it
        // cannot tell, and what stays on the list is asked for again after
        // every reconnect, under an id nobody waits for any more.
        for id in self.relay.subscriptions().await.into_keys() {
            let _ = self.relay.unsubscribe(&id).await;
        }
        let last_alive = self.store.relay_last_alive(&self.url).await.ok().flatten();
        let (subs, left) = self.stock.replan(plan, now(), last_alive);
        if !left.is_empty() {
            match self.store.unset_baselined(&self.url, &left).await {
                Ok(()) => {
                    tracing::debug!(relay = %self.url, targets = left.len(), "stock forgotten")
                }
                Err(e) => tracing::error!(relay = %self.url, error = %e, "stock not forgotten"),
            }
        }
        self.watch.update(&self.url, |h| {
            h.dm = plan.dm.len();
            h.groups = plan.groups.len();
            h.refused = None;
        });
        for sub in &subs {
            let asked = self
                .relay
                .subscribe(filter(sub))
                .with_id(SubscriptionId::new(sub.id.clone()))
                .await;
            if let Err(e) = asked {
                // A relay that is briefly away gets the request when it
                // is back: nostr-sdk keeps it. One that has been failing
                // for long is refused outright, and the request is
                // forgotten; nothing would ask it for these keys and
                // groups until the hourly renewal. So they are asked
                // for again as soon as it is on the line.
                self.unsent = true;
                tracing::debug!(relay = %self.url, sub = %sub.id, error = %e, "subscription not sent");
            }
        }
        tracing::debug!(
            relay = %self.url,
            dm = plan.dm.len(),
            groups = plan.groups.len(),
            fresh = self.stock.fresh(),
            "subscribed"
        );
    }

    /// The relay sent all it had stored for a subscription.
    async fn stored_is_over(&mut self, sub: &str) {
        let targets = self.stock.stored_is_over(sub);
        if targets.is_empty() {
            return;
        }
        match self.store.set_baselined(&self.url, &targets, now()).await {
            Ok(()) => {
                tracing::debug!(relay = %self.url, sub, targets = targets.len(), "stock taken");
                self.stock.taken(targets);
            }
            Err(e) => tracing::error!(relay = %self.url, error = %e, "stock not recorded"),
        }
    }

    /// The connection became something at `at`.
    async fn status(&mut self, status: RelayStatus, at: u64) {
        let state = match status {
            RelayStatus::Connected => "connected",
            RelayStatus::Initialized | RelayStatus::Pending | RelayStatus::Connecting => "connecting",
            _ => "disconnected",
        };
        let mut changed = false;
        self.watch.update(&self.url, |h| {
            changed = h.state != state;
            if changed {
                h.state = state.to_string();
                h.since = at;
            }
        });
        if changed {
            tracing::info!(relay = %self.url, state, "relay");
        }
        if state == "connected" {
            // Before the relay is noted as alive: what is asked
            // for again reaches back to when it was last alive,
            // over the time it was away.
            if self.unsent {
                let plan = self.plan.clone();
                self.subscribe(&plan).await;
            }
            let _ = self.store.relay_alive(&self.url, at).await;
        }
    }

    async fn deal(&mut self, item: Item) {
        match item {
            Item::Event(event) => {
                let baseline =
                    classify::subject(&event).is_some_and(|subject| self.stock.is_fresh(&subject));
                self.watch.update(&self.url, |h| h.events += 1);
                self.pipeline.event(&self.url, &event, baseline).await;
            }
            Item::StoredIsOver(sub) => self.stored_is_over(&sub).await,
            Item::Refused { sub, why } => {
                // The relay will not serve this subscription. What it
                // said is what the client is told.
                tracing::warn!(relay = %self.url, sub, why, "the relay refused a subscription");
                self.watch.update(&self.url, |h| h.refused = Some(why));
            }
            Item::Status(status, at) => self.status(status, at).await,
            Item::Plan(plan) => self.subscribe(&plan).await,
            Item::Renew => {
                let plan = self.plan.clone();
                self.subscribe(&plan).await;
            }
            // Written down when everything that came before it is dealt
            // with, not when the relay was seen alive: a line that starts
            // over asks from here, and must not ask from later than what
            // it has dealt with.
            Item::Alive(at) => {
                let _ = self.store.relay_alive(&self.url, at).await;
            }
        }
    }

    /// Deals with what is in the queue, one thing at a time, until the
    /// queue is closed and empty. When nobody wants the relay watched any
    /// more, what is still in the queue is nobody's, and is left there.
    async fn work(mut self, mut queued: mpsc::Receiver<Item>, plan: watch::Receiver<RelayPlan>) {
        while let Some(item) = queued.recv().await {
            if plan.has_changed().is_err() {
                break;
            }
            self.deal(item).await;
        }
    }
}

/// Moves what comes into the queue, and does nothing else: whatever takes
/// time is the work's to do. Returns when the line ends.
async fn intake(
    relay: &Relay,
    mut notifications: impl Stream<Item = RelayNotification> + Unpin,
    plan: &mut watch::Receiver<RelayPlan>,
    queue: &mpsc::Sender<Item>,
    memory: &Mutex<Memory>,
    flow: &Flow,
    tuning: Tuning,
) -> Ended {
    const FULL: &str = "more waits to be dealt with than the line has room for";

    let mut alive = tokio::time::interval(ALIVE_EVERY);
    let mut renew = tokio::time::interval(RENEW_EVERY);
    renew.tick().await;
    // When the relay was last asked because what is watched changed; the
    // first asking of a line is one. A change that comes sooner than
    // `ask_every` after it is held until then, and whatever changes
    // meanwhile is asked for with it, once.
    let mut asked = Instant::now();
    let mut held: Option<Instant> = None;
    loop {
        let item = tokio::select! {
            changed = plan.changed() => {
                if changed.is_err() {
                    return Ended::Stopped;
                }
                let due = asked + tuning.ask_every;
                if Instant::now() < due {
                    held = Some(due);
                    continue;
                }
                asked = Instant::now();
                Item::Plan(plan.borrow().clone())
            }
            _ = tokio::time::sleep_until(held.unwrap_or(asked)), if held.is_some() => {
                held = None;
                asked = Instant::now();
                Item::Plan(plan.borrow_and_update().clone())
            }
            notification = notifications.next() => match notification {
                Some(notification) => match Item::of(notification) {
                    Some(item) => item,
                    None => continue,
                },
                None => return Ended::Stopped,
            },
            _ = alive.tick() => {
                if relay.status() != RelayStatus::Connected {
                    continue;
                }
                Item::Alive(now())
            }
            _ = renew.tick() => Item::Renew,
            _ = flow.stuck.notified() => {
                return Ended::Overrun("an event found no way through nostr-sdk");
            }
        };

        let sent = match item {
            Item::Event(event) => {
                // The event is out of nostr-sdk's channel: its place is free.
                flow.room.add_permits(1);
                let mut memory = memory.lock().unwrap();
                // The relay sent it twice before the first was remembered.
                if memory.knows(&event.id) {
                    continue;
                }
                let id = event.id;
                let sent = queue.try_send(Item::Event(event));
                if sent.is_ok() {
                    memory.remember(id);
                }
                sent
            }
            other => queue.try_send(other),
        };
        match sent {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Ended::Overrun(FULL),
            Err(TrySendError::Closed(_)) => return Ended::Overrun("the work of the line ended"),
        }
    }
}

/// One life of a line: from connecting to the relay to letting it go.
async fn once(
    url: &str,
    address: &RelayUrl,
    plan: &mut watch::Receiver<RelayPlan>,
    shared: &Shared,
    memory: &Arc<Mutex<Memory>>,
) -> Ended {
    let tuning = shared.tuning;
    let flow = Arc::new(Flow::new(tuning.notifications));
    // A relay of its own every time. nostr-sdk keeps to itself what it has
    // handed on and what it is asked for; a line that starts over starts
    // with none of that.
    let relay = Relay::builder(address.clone())
        .opts(
            RelayOptions::new()
                .reconnect(true)
                .ping(true)
                .notification_channel_size(tuning.notifications),
        )
        .admit_policy(Door {
            memory: Arc::clone(memory),
            flow: Arc::clone(&flow),
        })
        .build();
    let notifications = relay.notifications();
    let known = match shared.store.baselined(url).await {
        Ok(known) => known,
        Err(e) => {
            tracing::error!(relay = %url, error = %e, "what stock was taken of cannot be read");
            Vec::new()
        }
    };
    let line = Line {
        url: url.to_string(),
        relay: relay.clone(),
        watch: Arc::clone(&shared.watch),
        store: Arc::clone(&shared.store),
        pipeline: Arc::clone(&shared.pipeline),
        stock: Stock::new(known),
        plan: RelayPlan::default(),
        unsent: false,
    };

    shared.watch.update(url, |h| {
        if h.state != "connecting" {
            h.state = "connecting".to_string();
            h.since = now();
        }
    });
    relay.connect();
    let (queue, queued) = mpsc::channel(tuning.queue.max(1));
    // The first thing in the queue, which is empty: what to ask for.
    let first = plan.borrow_and_update().clone();
    let _ = queue.try_send(Item::Plan(first));
    let work = tokio::spawn(line.work(queued, plan.clone()));

    let ended = intake(&relay, notifications, plan, &queue, memory, &flow, tuning).await;

    // Nothing more comes in: the relay is let go, and an event that waits
    // at the door is turned away. What is in the queue is dealt with, as
    // far as somebody still wants it.
    flow.room.close();
    relay.shutdown();
    drop(queue);
    let _ = work.await;
    ended
}

/// Waits until `until`. False when nobody wants the relay watched any more.
async fn wanted_until(plan: &mut watch::Receiver<RelayPlan>, until: Instant) -> bool {
    let wait = tokio::time::sleep_until(until);
    tokio::pin!(wait);
    loop {
        tokio::select! {
            _ = &mut wait => return true,
            changed = plan.changed() => {
                if changed.is_err() {
                    return false;
                }
            }
        }
    }
}

/// Runs until the sender of `plan` is dropped.
pub(super) async fn run(
    allowed: AllowedRelay,
    mut plan: watch::Receiver<RelayPlan>,
    shared: Shared,
) {
    let url = allowed.url.clone();
    let address = match RelayUrl::parse(&address(&allowed)) {
        Ok(address) => address,
        Err(e) => {
            // The watcher starts a line that ended again, every time it
            // reads the plan. Until one holds, the relay is shown to the
            // clients as what it is: out of reach.
            tracing::error!(relay = %url, error = %e, "not an address nostr-sdk takes; the relay is not watched");
            shared.watch.update(&url, |h| h.state = "disconnected".to_string());
            return;
        }
    };

    shared.watch.update(&url, |_| {});
    tracing::info!(relay = %url, "watching");
    let memory = Arc::new(Mutex::new(Memory::new(REMEMBERED)));
    let mut started_over: Option<Instant> = None;
    loop {
        let why = match once(&url, &address, &mut plan, &shared, &memory).await {
            Ended::Stopped => break,
            Ended::Overrun(why) => why,
        };
        shared.counters.lines_started_over.add();
        tracing::warn!(
            relay = %url,
            why,
            "the line starts over: the relay is asked again for what was not dealt with"
        );
        // Starting over asks the relay for everything again, like a change
        // of what is watched, and is held to the same pace. The first time
        // is at once.
        if let Some(last) = started_over {
            if !wanted_until(&mut plan, last + shared.tuning.ask_every).await {
                break;
            }
        }
        started_over = Some(Instant::now());
    }
    shared.watch.forget(&url);
    tracing::info!(relay = %url, "no longer watching");
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::prelude::*;

    fn event(n: u32) -> Event {
        EventBuilder::new(Kind::from(KIND_WRAP), format!("sealed {n}"))
            .finalize(&Keys::generate())
            .unwrap()
    }

    fn door(max: usize, notifications: usize) -> (Door, Arc<Mutex<Memory>>, Arc<Flow>) {
        let memory = Arc::new(Mutex::new(Memory::new(max)));
        let flow = Arc::new(Flow::new(notifications));
        let door = Door {
            memory: Arc::clone(&memory),
            flow: Arc::clone(&flow),
        };
        (door, memory, flow)
    }

    async fn let_in(door: &Door, event: &Event) -> bool {
        let relay = RelayUrl::parse("wss://relay.example.org").unwrap();
        let admitted = door
            .admit_event(&relay, &SubscriptionId::new("dm-0-g1"), event)
            .await
            .unwrap();
        admitted == AdmitStatus::Success
    }

    #[test]
    fn what_was_put_into_the_queue_is_remembered_and_the_oldest_is_forgotten_first() {
        let mut memory = Memory::new(3);
        let events: Vec<Event> = (0..4).map(event).collect();
        for e in &events[..3] {
            memory.remember(e.id);
        }
        // Remembered twice is remembered once: it takes no second place.
        memory.remember(events[0].id);
        assert!(events[..3].iter().all(|e| memory.knows(&e.id)));

        memory.remember(events[3].id);
        assert!(!memory.knows(&events[0].id), "the oldest made room");
        assert!(events[1..].iter().all(|e| memory.knows(&e.id)));
        assert_eq!((memory.ids.len(), memory.order.len()), (3, 3));
    }

    #[tokio::test]
    async fn an_event_the_line_has_in_its_queue_is_turned_away_at_the_door() {
        let (door, memory, _flow) = door(10, 64);
        let (queued, new) = (event(1), event(2));
        memory.lock().unwrap().remember(queued.id);
        assert!(!let_in(&door, &queued).await);
        assert!(let_in(&door, &new).await);
    }

    /// nostr-sdk's channel drops the oldest when it is full, and nobody is
    /// told. The door keeps it from getting full.
    #[tokio::test(start_paused = true)]
    async fn an_event_waits_at_the_door_until_the_one_before_it_is_out() {
        // A channel of four: room for one event on its way.
        let (door, _memory, flow) = door(10, 4);
        let (first, second) = (event(1), event(2));
        assert!(let_in(&door, &first).await);

        let waiting = let_in(&door, &second);
        tokio::pin!(waiting);
        let waited = tokio::time::timeout(Duration::from_secs(1), &mut waiting).await;
        assert!(waited.is_err(), "the second is held back while the first is on its way");

        // The intake took the first out.
        flow.room.add_permits(1);
        assert!(waiting.await);
    }

    #[tokio::test(start_paused = true)]
    async fn an_event_that_finds_no_room_for_long_says_so_and_is_turned_away() {
        let (door, _memory, flow) = door(10, 4);
        assert!(let_in(&door, &event(1)).await);

        let started = Instant::now();
        assert!(!let_in(&door, &event(2)).await);
        assert_eq!(started.elapsed(), DOOR_WAIT);
        // The intake hears of it, and the line starts over.
        tokio::time::timeout(Duration::from_secs(1), flow.stuck.notified())
            .await
            .expect("the door said it was stuck");
    }

    #[tokio::test]
    async fn a_line_that_starts_over_turns_away_what_waits_at_its_door() {
        let (door, _memory, flow) = door(10, 4);
        assert!(let_in(&door, &event(1)).await);
        flow.room.close();
        assert!(!let_in(&door, &event(2)).await);
    }

    #[test]
    fn only_what_the_line_acts_on_is_put_into_the_queue() {
        use std::borrow::Cow;

        let e = event(1);
        let said = |message: RelayMessage<'static>| {
            Item::of(RelayNotification::Message { message: Box::new(message) })
        };
        let sub = || Cow::Owned(SubscriptionId::new("dm-0-g1"));

        let taken = said(RelayMessage::Event { subscription_id: sub(), event: Cow::Owned(e.clone()) });
        assert!(matches!(taken, Some(Item::Event(taken)) if taken.id == e.id));
        let over = said(RelayMessage::EndOfStoredEvents(sub()));
        assert!(matches!(over, Some(Item::StoredIsOver(sub)) if sub == "dm-0-g1"));
        let refused = said(RelayMessage::Closed {
            subscription_id: sub(),
            message: Cow::Owned("auth-required: no".to_string()),
        });
        assert!(matches!(refused, Some(Item::Refused { why, .. }) if why == "auth-required: no"));
        let status = Item::of(RelayNotification::RelayStatus { status: RelayStatus::Connected });
        assert!(matches!(status, Some(Item::Status(RelayStatus::Connected, _))));

        // nostr-sdk tells of an event twice: as an event, and as what the
        // relay said. Taken once, and never from the first.
        let twice = Item::of(RelayNotification::Event {
            subscription_id: SubscriptionId::new("dm-0-g1"),
            event: Box::new(e),
        });
        assert!(twice.is_none());
        assert!(said(RelayMessage::Notice(Cow::Borrowed("hello"))).is_none());
    }
}
