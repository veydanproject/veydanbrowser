// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! `RelayPool`: `core::Transport` over `nostr_sdk::Client`.
//!
//! The pool owns the relay connections, forwards every event it sees as a
//! `RawEvent`, and executes `Outbound` requests. It does not interpret
//! events. Silent mode drops all connections and refuses every send.

use async_trait::async_trait;
use futures_util::StreamExt;
use messenger_core::traits::{RelayState, RelayStatus, RelayStatusSnapshot};
use messenger_core::{
    Ack, EventId, EventSource, MessengerError, Outbound, PubKey, RawEvent, RelayUrl, Result, Timestamp,
    Transport,
};
use nostr_sdk::prelude::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc;

/// One relay to keep in the pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayConfig {
    pub url: RelayUrl,
    pub read: bool,
    pub write: bool,
}

pub struct RelayPool {
    client: Client,
    silent: AtomicBool,
    tx: mpsc::Sender<RawEvent>,
    rx: Mutex<Option<mpsc::Receiver<RawEvent>>>,
    /// Configured relays; nostr-sdk keeps the live set.
    configured: Mutex<Vec<RelayConfig>>,
}

impl RelayPool {
    /// `signer` enables NIP-42 authentication and is required for relays
    /// that demand it. Without a signer the pool is read-only for such relays.
    pub fn new(signer: Option<Keys>) -> Self {
        let client = match signer {
            Some(keys) => Client::builder()
                .authenticator(SignerAuthenticator::new(keys))
                .build(),
            None => Client::new(),
        };
        let (tx, rx) = mpsc::channel(1024);
        let pool = Self {
            client,
            silent: AtomicBool::new(false),
            tx,
            rx: Mutex::new(Some(rx)),
            configured: Mutex::new(Vec::new()),
        };
        pool.spawn_forwarder();
        pool
    }

    fn spawn_forwarder(&self) {
        let mut stream = self.client.notifications();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(n) = stream.next().await {
                match n {
                    ClientNotification::Event { relay_url, event, .. } => {
                        let Some(raw) = to_raw(&event, EventSource::Relay { url: to_core_url(&relay_url) }) else {
                            continue;
                        };
                        if tx.send(raw).await.is_err() {
                            break;
                        }
                    }
                    ClientNotification::Shutdown => break,
                    _ => {}
                }
            }
        });
    }

    /// Replace the relay set. Existing connections to relays that stay are
    /// kept; removed ones are dropped; new ones are added and connected
    /// unless silent mode is on.
    pub async fn set_relays(&self, relays: Vec<RelayConfig>) -> Result<()> {
        let wanted: HashSet<String> = relays.iter().map(|r| r.url.as_str().to_string()).collect();
        let current = self.client.relays().await;
        for (url, _) in current {
            if !wanted.contains(url.as_str_without_trailing_slash()) {
                let _ = self.client.remove_relay(url.clone()).await;
            }
        }
        for r in &relays {
            self.client
                .add_relay(r.url.as_str())
                .await
                .map_err(|e| MessengerError::Transport(e.to_string()))?;
        }
        *self.configured.lock().unwrap() = relays;
        if !self.is_silent() {
            self.client.connect().await;
        }
        Ok(())
    }

    pub fn configured(&self) -> Vec<RelayConfig> {
        self.configured.lock().unwrap().clone()
    }

    pub async fn connect(&self) {
        if !self.is_silent() {
            self.client.connect().await;
        }
    }

    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }

    pub fn is_silent(&self) -> bool {
        self.silent.load(Ordering::Relaxed)
    }

    /// Silent mode: no connections, no sends. Turning it off reconnects.
    pub async fn set_silent(&self, on: bool) {
        self.silent.store(on, Ordering::Relaxed);
        if on {
            self.client.disconnect().await;
        } else {
            self.client.connect().await;
        }
    }

    pub async fn shutdown(&self) {
        self.client.shutdown().await;
    }

    fn guard_send(&self) -> Result<()> {
        if self.is_silent() {
            return Err(MessengerError::Transport("silent mode".into()));
        }
        Ok(())
    }

    fn parse_event(json: &serde_json::Value) -> Result<Event> {
        let event: Event = serde_json::from_value(json.clone())
            .map_err(|e| MessengerError::Invalid(format!("wire event: {e}")))?;
        event
            .verify()
            .map_err(|_| MessengerError::Invalid("wire event has an invalid signature".into()))?;
        Ok(event)
    }

    fn parse_filter(f: &messenger_core::outbound::Filter) -> Result<Filter> {
        serde_json::from_value(f.0.clone()).map_err(|e| MessengerError::Invalid(format!("filter: {e}")))
    }
}

fn to_core_url(u: &nostr::types::url::RelayUrl) -> RelayUrl {
    RelayUrl::parse(u.as_str_without_trailing_slash())
        .unwrap_or_else(|| RelayUrl::parse("wss://invalid.invalid").expect("static url"))
}

fn to_raw(event: &Event, source: EventSource) -> Option<RawEvent> {
    Some(RawEvent {
        id: EventId::parse(&event.id.to_hex())?,
        kind: event.kind.as_u16(),
        pubkey: PubKey::parse(&event.pubkey.to_hex())?,
        created_at: Timestamp(event.created_at.as_secs() as i64),
        json: serde_json::to_value(event).ok()?,
        source,
    })
}

fn ack_from<T, S>(out: &Output<T, S>) -> Ack {
    Ack {
        accepted_by: out.success.keys().map(to_core_url).collect(),
        rejected_by: out.failed.iter().map(|(u, e)| (to_core_url(u), e.clone())).collect(),
    }
}

fn map_state(s: nostr_sdk::relay::RelayStatus) -> RelayState {
    use nostr_sdk::relay::RelayStatus as S;
    match s {
        S::Connected => RelayState::Connected,
        S::Pending | S::Connecting => RelayState::Connecting,
        S::Banned | S::Sleeping => RelayState::Paused,
        _ => RelayState::Disconnected,
    }
}

#[async_trait]
impl Transport for RelayPool {
    async fn send(&self, out: Outbound) -> Result<Ack> {
        self.guard_send()?;
        match out {
            Outbound::PublishOwn { event } | Outbound::PublishScoped { event, .. } => {
                let ev = Self::parse_event(&event.json)?;
                let res = self
                    .client
                    .send_event(&ev)
                    .await
                    .map_err(|e| MessengerError::Transport(e.to_string()))?;
                Ok(ack_from(&res))
            }
            Outbound::PublishToInbox { event, hint_relays, .. } => {
                let ev = Self::parse_event(&event.json)?;
                // Stage 2: hints that are in the pool are used; otherwise the
                // event goes to every write relay. Stage 5 adds inbox
                // resolution and ephemeral connections.
                let pool_urls: HashSet<String> =
                    self.client.relays().await.keys().map(|u| u.as_str_without_trailing_slash().to_string()).collect();
                let targets: Vec<String> = hint_relays
                    .iter()
                    .map(|u| u.as_str().to_string())
                    .filter(|u| pool_urls.contains(u))
                    .collect();
                let res = if targets.is_empty() {
                    self.client.send_event(&ev).await
                } else {
                    self.client.send_event(&ev).to(targets).await
                }
                .map_err(|e| MessengerError::Transport(e.to_string()))?;
                Ok(ack_from(&res))
            }
            Outbound::Subscribe { id, filter, .. } => {
                let f = Self::parse_filter(&filter)?;
                self.client
                    .subscribe(f)
                    .with_id(SubscriptionId::new(id.0))
                    .await
                    .map_err(|e| MessengerError::Transport(e.to_string()))?;
                Ok(Ack { accepted_by: vec![], rejected_by: vec![] })
            }
            Outbound::Unsubscribe { id } => {
                // Unsubscribing from a relay that already dropped the
                // subscription is not an error worth surfacing.
                let _ = self.client.unsubscribe(&SubscriptionId::new(id.0)).await;
                Ok(Ack { accepted_by: vec![], rejected_by: vec![] })
            }
            Outbound::Sync { .. } => Err(MessengerError::Transport("negentropy sync arrives in stage 3".into())),
        }
    }

    fn events(&self) -> mpsc::Receiver<RawEvent> {
        self.rx.lock().unwrap().take().expect("events() may be called once")
    }

    async fn status(&self) -> RelayStatusSnapshot {
        let relays = self.client.relays().await;
        let mut list: Vec<RelayStatus> = relays
            .iter()
            .map(|(url, relay)| RelayStatus { url: to_core_url(url), state: map_state(relay.status()) })
            .collect();
        list.sort_by(|a, b| a.url.as_str().cmp(b.url.as_str()));
        RelayStatusSnapshot { relays: list }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use messenger_core::outbound::{Filter as CoreFilter, WireEvent};
    use messenger_core::{Scope, SubId};
    use nostr_sdk::local_relay::LocalRelay;
    use std::time::Duration;

    async fn local_relay() -> (LocalRelay, RelayUrl) {
        let relay = LocalRelay::builder().build();
        relay.run().await.expect("local relay starts");
        let url = relay.url().await;
        (relay, RelayUrl::parse(url.as_str_without_trailing_slash()).unwrap())
    }

    fn signed_note(keys: &Keys, text: &str) -> WireEvent {
        let ev = EventBuilder::new(Kind::from(1u16), text).finalize(keys).unwrap();
        WireEvent { id: EventId::parse(&ev.id.to_hex()).unwrap(), json: serde_json::to_value(&ev).unwrap() }
    }

    #[tokio::test]
    async fn publish_subscribe_roundtrip_through_local_relay() {
        let (_relay, url) = local_relay().await;

        let alice = Keys::generate();
        let sender = RelayPool::new(Some(alice.clone()));
        sender
            .set_relays(vec![RelayConfig { url: url.clone(), read: true, write: true }])
            .await
            .unwrap();

        let receiver = RelayPool::new(None);
        let mut inbox = receiver.events();
        receiver
            .set_relays(vec![RelayConfig { url: url.clone(), read: true, write: true }])
            .await
            .unwrap();

        // Wait for both to connect.
        for _ in 0..50 {
            let a = sender.status().await.relays.iter().all(|r| r.state == RelayState::Connected);
            let b = receiver.status().await.relays.iter().all(|r| r.state == RelayState::Connected);
            if a && b {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(sender.status().await.relays[0].state == RelayState::Connected);

        let filter = CoreFilter(serde_json::json!({ "kinds": [1], "authors": [alice.public_key().to_hex()] }));
        receiver
            .send(Outbound::Subscribe { id: SubId("t".into()), filter, scope: Scope::Own })
            .await
            .unwrap();

        let ack = sender.send(Outbound::PublishOwn { event: signed_note(&alice, "hello") }).await.unwrap();
        assert!(ack.is_delivered(), "relay must accept the note: {ack:?}");

        let got = tokio::time::timeout(Duration::from_secs(5), inbox.recv()).await.expect("event arrives").unwrap();
        assert_eq!(got.kind, 1);
        assert_eq!(got.pubkey.as_hex(), alice.public_key().to_hex());
        assert!(matches!(got.source, EventSource::Relay { .. }));

        sender.shutdown().await;
        receiver.shutdown().await;
    }

    #[tokio::test]
    async fn silent_mode_refuses_sends_and_disconnects() {
        let (_relay, url) = local_relay().await;
        let keys = Keys::generate();
        let pool = RelayPool::new(Some(keys.clone()));
        pool.set_relays(vec![RelayConfig { url, read: true, write: true }]).await.unwrap();
        pool.set_silent(true).await;
        let err = pool.send(Outbound::PublishOwn { event: signed_note(&keys, "x") }).await.unwrap_err();
        assert!(matches!(err, MessengerError::Transport(m) if m.contains("silent")));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(pool.status().await.relays.iter().all(|r| r.state != RelayState::Connected));
        pool.shutdown().await;
    }

    #[tokio::test]
    async fn set_relays_replaces_the_set() {
        let (_r1, u1) = local_relay().await;
        let (_r2, u2) = local_relay().await;
        let pool = RelayPool::new(None);
        pool.set_relays(vec![RelayConfig { url: u1.clone(), read: true, write: true }]).await.unwrap();
        assert_eq!(pool.status().await.relays.len(), 1);
        pool.set_relays(vec![RelayConfig { url: u2.clone(), read: true, write: true }]).await.unwrap();
        let st = pool.status().await;
        assert_eq!(st.relays.len(), 1);
        assert_eq!(st.relays[0].url, u2);
        pool.shutdown().await;
    }

    #[test]
    fn tampered_wire_event_is_rejected() {
        let keys = Keys::generate();
        let mut ev = signed_note(&keys, "a");
        ev.json["content"] = serde_json::Value::String("b".into());
        assert!(RelayPool::parse_event(&ev.json).is_err());
    }
}
