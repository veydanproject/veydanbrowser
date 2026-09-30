//! Watching relays.
//!
//! What is watched is not kept here: it is read from the store, where the
//! registrations are. Whenever they change, and once a minute anyway, the
//! plan is read again and every relay is given its part. So a device that
//! leaves cannot take away what another device still needs, and a restart
//! changes nothing.
//!
//! Every relay has a task of its own. A relay that is slow, down or hostile
//! holds back nobody but itself.

pub mod plan;
mod line;
mod stock;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;
use vpush_proto::RelayStatus;

use crate::api::now;
use crate::pipeline::{Pipeline, SEEN_FOR};
use crate::relays::RelayPolicy;
use crate::store::{AllStore, RelayPlan};

/// The plan is read again after this long, asked or not.
const REPLAN_EVERY: Duration = Duration::from_secs(60);
/// Registrations come in bursts; the relays are told once about a burst.
const SETTLE: Duration = Duration::from_secs(1);

/// How a relay is doing, for `vpush ctl relays` and for the clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayHealth {
    pub url: String,
    /// `connecting`, `connected`, `disconnected`.
    pub state: String,
    /// Unix seconds.
    pub since: u64,
    pub dm: usize,
    pub groups: usize,
    pub events: u64,
    /// Subscriptions the relay refused, with its reason.
    pub refused: Option<String>,
}

/// What the rest of the server knows of the watcher.
#[derive(Default)]
pub struct Watch {
    changed: Notify,
    health: Mutex<BTreeMap<String, RelayHealth>>,
}

impl Watch {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A registration changed: the plan is to be read again.
    pub fn plan_changed(&self) {
        self.changed.notify_one();
    }

    pub fn health(&self) -> Vec<RelayHealth> {
        self.health.lock().unwrap().values().cloned().collect()
    }

    /// What to tell a client about a relay the server agreed to watch.
    pub fn status(&self, url: &str) -> RelayStatus {
        match self.health.lock().unwrap().get(url) {
            Some(h) if h.refused.is_some() => RelayStatus::Restricted,
            Some(h) if h.state == "connected" => RelayStatus::Ok,
            // A minute is given to a relay to come back before it is called
            // unreachable: relays restart, and networks blink.
            Some(h) if h.state == "disconnected" && now().saturating_sub(h.since) > 60 => {
                RelayStatus::Unreachable
            }
            _ => RelayStatus::Pending,
        }
    }

    fn update(&self, url: &str, change: impl FnOnce(&mut RelayHealth)) {
        let mut all = self.health.lock().unwrap();
        let health = all.entry(url.to_string()).or_insert_with(|| RelayHealth {
            url: url.to_string(),
            state: "connecting".to_string(),
            since: now(),
            dm: 0,
            groups: 0,
            events: 0,
            refused: None,
        });
        change(health);
    }

    fn forget(&self, url: &str) {
        self.health.lock().unwrap().remove(url);
    }
}

/// A relay that has a task of its own.
struct OnTheLine {
    /// The way to give the relay its plan.
    plan: watch::Sender<RelayPlan>,
    task: JoinHandle<()>,
}

/// Runs until `stop` says so.
pub async fn run(
    watch: Arc<Watch>,
    store: Arc<dyn AllStore>,
    policy: Arc<RelayPolicy>,
    pipeline: Arc<Pipeline>,
    mut stop: watch::Receiver<bool>,
) {
    let mut relays: BTreeMap<String, OnTheLine> = BTreeMap::new();
    let mut purged = 0u64;

    loop {
        match store.watch_plan(now()).await {
            Err(e) => tracing::error!(error = %e, "the plan of the watch cannot be read"),
            Ok(plans) => {
                let wanted: BTreeMap<String, RelayPlan> = plans
                    .into_iter()
                    .filter(|p| policy.get(&p.url).is_some())
                    .map(|p| (p.url.clone(), p))
                    .collect();

                let unwanted: Vec<String> = relays
                    .keys()
                    .filter(|url| !wanted.contains_key(*url))
                    .cloned()
                    .collect();
                for url in unwanted {
                    let Some(relay) = relays.remove(&url) else { continue };
                    tracing::info!(relay = %url, "nothing to watch here any more");
                    // Dropping the sender ends the relay's task. The end is
                    // waited for: a task on its way out may still take
                    // stock, and must not after the stock of its relay is
                    // forgotten below. The wait is short whatever the
                    // relay does: the task waits for its relay only where
                    // it waits for its plan too.
                    drop(relay.plan);
                    let _ = relay.task.await;
                    watch.forget(&url);
                }
                // A relay nobody watches goes on taking events. When it is
                // watched again they are old, so the stock taken there is
                // worth nothing: keys and groups start on it as fresh.
                // Every time, not only when a relay was let go: a server
                // that fell between the two left the stock behind.
                let watched: Vec<String> = wanted.keys().cloned().collect();
                match store.keep_baselined(&watched).await {
                    Ok(0) => {}
                    Ok(n) => tracing::debug!(targets = n, "stock of relays no longer watched forgotten"),
                    Err(e) => tracing::error!(error = %e, "stock of relays no longer watched cannot be forgotten"),
                }

                for (url, plan) in wanted {
                    // A task that ended while its relay is wanted never
                    // began to watch: the address was not taken. It is
                    // started again now, and every time the plan is read.
                    if relays.get(&url).is_some_and(|relay| relay.task.is_finished()) {
                        relays.remove(&url);
                    }
                    match relays.get(&url) {
                        Some(relay) => {
                            relay.plan.send_if_modified(|current| {
                                let changed = *current != plan;
                                if changed {
                                    *current = plan.clone();
                                }
                                changed
                            });
                        }
                        None => {
                            let Some(allowed) = policy.get(&url).cloned() else { continue };
                            let (sender, receiver) = watch::channel(plan);
                            let task = tokio::spawn(line::run(
                                allowed,
                                receiver,
                                Arc::clone(&watch),
                                Arc::clone(&store),
                                Arc::clone(&pipeline),
                            ));
                            relays.insert(url, OnTheLine { plan: sender, task });
                        }
                    }
                }
            }
        }

        // Once an hour, what was seen too long ago to come again is forgotten.
        if now().saturating_sub(purged) >= 3600 {
            purged = now();
            match store.purge_seen(now().saturating_sub(SEEN_FOR)).await {
                Ok(0) => {}
                Ok(n) => tracing::debug!(events = n, "old events forgotten"),
                Err(e) => tracing::error!(error = %e, "old events not forgotten"),
            }
        }

        tokio::select! {
            _ = watch.changed.notified() => tokio::time::sleep(SETTLE).await,
            _ = tokio::time::sleep(REPLAN_EVERY) => {}
            _ = stop.changed() => break,
        }
        if *stop.borrow() {
            break;
        }
    }
    // The senders go with `relays`, and the tasks of the relays with them.
}
