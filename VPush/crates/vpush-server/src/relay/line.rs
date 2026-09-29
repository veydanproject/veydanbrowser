//! One relay on the line.
//!
//! This is the only place that knows nostr-sdk. The connection is its own:
//! it comes back by itself after it is lost, for as long as it takes, and
//! checks that the relay still answers.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use nostr::filter::{Filter, SingleLetterTag};
use nostr::message::{RelayMessage, SubscriptionId};
use nostr::prelude::{Kind, RelayUrl, Timestamp};
use nostr_sdk::relay::{Relay, RelayNotification, RelayOptions, RelayStatus};
use tokio::sync::watch;

use super::plan::{subscriptions, Sub};
use super::Watch;
use crate::api::now;
use crate::pipeline::classify::{self, Subject, KIND_GROUP, KIND_WRAP};
use crate::pipeline::Pipeline;
use crate::relays::AllowedRelay;
use crate::store::{AllStore, RelayPlan, WatchKind};

/// A relay that is on the line is noted as such this often.
const ALIVE_EVERY: Duration = Duration::from_secs(60);
/// Subscriptions are made anew this often: what is asked for is "from two
/// days ago", and "ago" moves.
const RENEW_EVERY: Duration = Duration::from_secs(3600);

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

struct Line {
    url: String,
    relay: Relay,
    watch: Arc<Watch>,
    store: Arc<dyn AllStore>,
    pipeline: Arc<Pipeline>,
    /// What stock was taken of on this relay.
    known: BTreeSet<(WatchKind, String)>,
    /// By subscription: the keys and groups the relay is asked about for
    /// the first time. What comes for them before the end of the stored
    /// events was there before the watch began.
    fresh: HashMap<String, BTreeSet<(WatchKind, String)>>,
}

impl Line {
    async fn subscribe(&mut self, plan: &RelayPlan) {
        // Fails when the relay is off the line; the list is cleared anyway.
        let _ = self.relay.unsubscribe_all().await;
        self.fresh.clear();
        let last_alive = self.store.relay_last_alive(&self.url).await.ok().flatten();
        let subs = subscriptions(plan, now(), last_alive);
        self.watch.update(&self.url, |h| {
            h.dm = plan.dm.len();
            h.groups = plan.groups.len();
            h.refused = None;
        });
        for sub in subs {
            let fresh: BTreeSet<_> = sub
                .targets
                .iter()
                .map(|t| (sub.kind, t.clone()))
                .filter(|t| !self.known.contains(t))
                .collect();
            if !fresh.is_empty() {
                self.fresh.insert(sub.id.clone(), fresh);
            }
            let asked = self
                .relay
                .subscribe(filter(&sub))
                .with_id(SubscriptionId::new(sub.id.clone()))
                .await;
            if let Err(e) = asked {
                // Kept by nostr-sdk and sent when the relay is on the line.
                tracing::debug!(relay = %self.url, sub = %sub.id, error = %e, "subscription waits for the relay");
            }
        }
        tracing::debug!(
            relay = %self.url,
            dm = plan.dm.len(),
            groups = plan.groups.len(),
            fresh = self.fresh.values().map(|f| f.len()).sum::<usize>(),
            "subscribed"
        );
    }

    /// Is the event of a key or a group stock has not been taken of yet.
    fn is_fresh(&self, sub: &str, event: &nostr::event::Event) -> bool {
        let Some(fresh) = self.fresh.get(sub) else {
            return false;
        };
        match classify::subject(event) {
            Some(Subject::Dm { recipients }) => recipients
                .iter()
                .all(|p| fresh.contains(&(WatchKind::Dm, p.clone()))),
            Some(Subject::Group { id }) => fresh.contains(&(WatchKind::Group, id)),
            None => false,
        }
    }

    /// The relay sent all it had stored for a subscription.
    async fn stored_is_over(&mut self, sub: &str) {
        let Some(fresh) = self.fresh.remove(sub) else {
            return;
        };
        let targets: Vec<_> = fresh.into_iter().collect();
        match self.store.set_baselined(&self.url, &targets, now()).await {
            Ok(()) => {
                tracing::debug!(relay = %self.url, sub, targets = targets.len(), "stock taken");
                self.known.extend(targets);
            }
            Err(e) => tracing::error!(relay = %self.url, error = %e, "stock not recorded"),
        }
    }

    async fn notified(&mut self, notification: RelayNotification) {
        match notification {
            RelayNotification::Event { subscription_id, event } => {
                let baseline = self.is_fresh(subscription_id.as_str(), &event);
                self.watch.update(&self.url, |h| h.events += 1);
                self.pipeline.event(&self.url, &event, baseline).await;
            }
            RelayNotification::Message { message } => match *message {
                RelayMessage::EndOfStoredEvents(id) => self.stored_is_over(id.as_str()).await,
                RelayMessage::Closed { subscription_id, message } => {
                    // The relay will not serve this subscription. What it
                    // said is what the client is told.
                    let why: String = message.chars().take(200).collect();
                    tracing::warn!(relay = %self.url, sub = %subscription_id, why, "the relay refused a subscription");
                    self.watch.update(&self.url, |h| h.refused = Some(why));
                }
                _ => {}
            },
            RelayNotification::RelayStatus { status } => {
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
                        h.since = now();
                    }
                });
                if changed {
                    tracing::info!(relay = %self.url, state, "relay");
                }
                if state == "connected" {
                    let _ = self.store.relay_alive(&self.url, now()).await;
                }
            }
            _ => {}
        }
    }
}

/// Runs until the sender of `plan` is dropped.
pub async fn run(
    allowed: AllowedRelay,
    mut plan: watch::Receiver<RelayPlan>,
    watch: Arc<Watch>,
    store: Arc<dyn AllStore>,
    pipeline: Arc<Pipeline>,
) {
    let url = allowed.url.clone();
    let address = match RelayUrl::parse(&address(&allowed)) {
        Ok(address) => address,
        Err(e) => {
            tracing::error!(relay = %url, error = %e, "not an address nostr-sdk takes");
            return;
        }
    };
    let relay = Relay::builder(address)
        .opts(RelayOptions::new().reconnect(true).ping(true))
        .build();
    let mut notifications = relay.notifications();
    let known = match store.baselined(&url).await {
        Ok(known) => known.into_iter().collect(),
        Err(e) => {
            tracing::error!(relay = %url, error = %e, "what stock was taken of cannot be read");
            BTreeSet::new()
        }
    };
    let mut line = Line {
        url: url.clone(),
        relay,
        watch: Arc::clone(&watch),
        store,
        pipeline,
        known,
        fresh: HashMap::new(),
    };

    watch.update(&url, |_| {});
    tracing::info!(relay = %url, "watching");
    line.relay.connect();
    let first = plan.borrow_and_update().clone();
    line.subscribe(&first).await;

    let mut alive = tokio::time::interval(ALIVE_EVERY);
    let mut renew = tokio::time::interval(RENEW_EVERY);
    renew.tick().await;
    loop {
        tokio::select! {
            changed = plan.changed() => {
                if changed.is_err() {
                    break;
                }
                let next = plan.borrow_and_update().clone();
                line.subscribe(&next).await;
            }
            notification = notifications.next() => match notification {
                Some(notification) => line.notified(notification).await,
                None => break,
            },
            _ = alive.tick() => {
                if line.relay.status() == RelayStatus::Connected {
                    let _ = line.store.relay_alive(&url, now()).await;
                }
            }
            _ = renew.tick() => {
                let current = plan.borrow().clone();
                line.subscribe(&current).await;
            }
        }
    }
    line.relay.shutdown();
    watch.forget(&url);
    tracing::info!(relay = %url, "no longer watching");
}
