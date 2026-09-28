// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! The one object a host embeds.
//!
//! `MessengerRuntime::start(config, secret_store)` opens the store, the
//! identity service and the relay pool, and starts a session (ingress loop
//! and outbox pump) as soon as signing keys are available. The host only ever
//! talks to this type: the Tauri adapter today, a standalone app or the
//! `messenger-cli` tomorrow. Nothing here knows about Tauri.

pub mod relays;
pub mod session;

use messenger_core::outbound::WireEvent;
use messenger_core::traits::{RelayState, SystemClock, UiEvent};
use messenger_core::{Envelope, EventId, MessengerConfig, MessengerError, Outbound, PubKey, Result, SecretStore};
use messenger_identity::IdentityService;
use messenger_ingress::{Dispatcher, Outbox};
use messenger_store::Store;
use nostr::key::Keys;
use nostr::nips::nip17::PrivateDirectMessageBuilder;
use nostr::prelude::*;
use serde::{Deserialize, Serialize};
use session::{DebugDmHandler, DebugMetaHandler, Session};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

pub use messenger_identity::{CreatedIdentity, Identity};
pub use relays::{ManifestInfo, RelayService, RelayView};

/// Facts for the host's status screen. Never contains secrets.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStatus {
    /// Messenger crate version (not the host application version).
    pub version: String,
    pub data_dir: String,
    pub schema_version: i64,
    pub secrets_unlocked: bool,
    pub identity_present: bool,
    /// Whether a session (signer + ingress loop) is running.
    pub session_active: bool,
    pub relays_total: usize,
    pub relays_connected: usize,
    pub silent_mode: bool,
    pub manifest_serial: Option<u64>,
    pub region: String,
    pub ingress: IngressCounters,
    pub outbox_pending: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IngressCounters {
    pub received: u64,
    pub duplicates: u64,
    pub dispatched: u64,
    pub dm: u64,
    pub ignored: u64,
}

pub struct MessengerRuntime {
    config: MessengerConfig,
    store: Store,
    secrets: Arc<dyn SecretStore>,
    identity: IdentityService,
    relays: RelayService,
    outbox: Outbox,
    dispatcher: Arc<Dispatcher>,
    ui: broadcast::Sender<UiEvent>,
    session: Mutex<Option<Session>>,
}

impl MessengerRuntime {
    pub async fn start(config: MessengerConfig, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let store = Store::open(&config).await?;
        let identity = IdentityService::new(store.clone(), secrets.clone());
        let signer = load_signer(&identity).await;
        let relays = RelayService::init(store.clone(), signer.clone()).await?;
        let outbox = Outbox::new(store.clone(), Arc::new(SystemClock));
        let dispatcher = Arc::new(
            Dispatcher::new()
                .with_dm(Arc::new(DebugDmHandler))
                .with_meta(Arc::new(DebugMetaHandler)),
        );
        let (ui, _) = broadcast::channel(256);
        let rt = Self {
            config,
            store,
            secrets,
            identity,
            relays,
            outbox,
            dispatcher,
            ui,
            session: Mutex::new(None),
        };
        if let Some(keys) = signer {
            rt.start_session(keys).await?;
        }
        Ok(rt)
    }

    pub fn config(&self) -> &MessengerConfig {
        &self.config
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    pub fn identity(&self) -> &IdentityService {
        &self.identity
    }

    pub fn relays(&self) -> &RelayService {
        &self.relays
    }

    pub fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    /// Subscribe to UI events (`inbound.dm`, `inbound.meta`, `ignored`,
    /// `error`, `notify`). Lagging receivers drop old events.
    pub fn ui_events(&self) -> broadcast::Receiver<UiEvent> {
        self.ui.subscribe()
    }

    /// Public key of the running session, if any.
    pub async fn session_pubkey(&self) -> Option<PubKey> {
        self.session
            .lock()
            .await
            .as_ref()
            .and_then(|s| PubKey::parse(&s.keys.public_key().to_hex()))
    }

    async fn start_session(&self, keys: Keys) -> Result<()> {
        let pool = self.relays.pool().await;
        let session = Session::start(
            self.store.clone(),
            pool,
            keys,
            self.outbox.clone(),
            self.ui.clone(),
            self.dispatcher.clone(),
        )
        .await?;
        *self.session.lock().await = Some(session);
        Ok(())
    }

    async fn stop_session(&self) {
        if let Some(s) = self.session.lock().await.take() {
            s.stop();
        }
    }

    /// Re-check which key should sign and (re)start the session when that
    /// changed. Call after identity create/import/delete and after the host
    /// unlocks secrets. Idempotent and cheap when nothing changed.
    pub async fn refresh_signer(&self) -> Result<bool> {
        let signer = load_signer(&self.identity).await;
        let wanted = signer.as_ref().map(|k| k.public_key().to_hex());
        let current = self.session.lock().await.as_ref().map(|s| s.keys.public_key().to_hex());
        if wanted == current {
            return Ok(false);
        }
        self.stop_session().await;
        // The pool binds its authenticator at construction: rebuild it.
        self.relays.set_signer(signer.clone()).await?;
        if let Some(keys) = signer {
            self.start_session(keys).await?;
        }
        Ok(true)
    }

    /// Relay set changed (add/remove/toggle): the pool object is the same,
    /// nostr-sdk re-applies subscriptions to new relays, nothing to restart.
    /// Kept as an explicit hook for hosts.
    pub async fn relays_changed(&self) -> Result<()> {
        Ok(())
    }

    /// Send a text DM to `to` (hex or npub). Returns the outbox local id.
    /// Stage 3 building block; stage 5 moves this behind the DM handler
    /// with chats, self-copies and the relationship matrix.
    pub async fn send_text_dm(&self, to: &str, text: &str) -> Result<String> {
        let keys = {
            let guard = self.session.lock().await;
            guard.as_ref().map(|s| s.keys.clone()).ok_or(MessengerError::NotLoggedIn)?
        };
        let receiver = PublicKey::parse(to.trim())
            .map_err(|_| MessengerError::Invalid("recipient must be an npub or 64-hex public key".into()))?;
        let content = Envelope::text(text).encode();
        let wrap = PrivateDirectMessageBuilder::new(receiver, content)
            .finalize(&keys)
            .map_err(|e| MessengerError::Crypto(e.to_string()))?;
        let event = WireEvent {
            id: EventId::parse(&wrap.id.to_hex()).expect("event id is hex"),
            json: serde_json::to_value(&wrap)?,
        };
        let recipient = PubKey::parse(&receiver.to_hex()).expect("valid pubkey");
        let local_id = self
            .outbox
            .enqueue(Outbound::PublishToInbox { recipient, event, hint_relays: vec![] })
            .await?;
        let pool = self.relays.pool().await;
        self.outbox.pump(pool.as_ref()).await?;
        Ok(local_id)
    }

    pub async fn status(&self) -> Result<RuntimeStatus> {
        let relays = self.relays.list().await?;
        let (session_active, ingress) = match self.session.lock().await.as_ref() {
            Some(s) => {
                let (received, duplicates, dispatched, dm, ignored) = s.stats();
                (true, IngressCounters { received, duplicates, dispatched, dm, ignored })
            }
            None => (false, IngressCounters::default()),
        };
        Ok(RuntimeStatus {
            version: messenger_core::VERSION.to_string(),
            data_dir: self.config.data_dir().to_string_lossy().into_owned(),
            schema_version: self.store.schema_version().await?,
            secrets_unlocked: self.secrets.is_unlocked().await,
            identity_present: self.identity.get().await?.is_some(),
            session_active,
            relays_total: relays.iter().filter(|r| r.enabled).count(),
            relays_connected: relays.iter().filter(|r| r.state == RelayState::Connected).count(),
            silent_mode: self.relays.is_silent().await?,
            manifest_serial: self.relays.manifest_serial().await?,
            region: self.relays.region().await?,
            ingress,
            outbox_pending: self.outbox.pending().await?,
        })
    }

    /// Stop background work and close the database. Idempotent.
    pub async fn shutdown(&self) {
        self.stop_session().await;
        self.relays.shutdown().await;
        self.store.close().await;
    }
}

/// Keys for the pool signer, or `None` when there is no identity or the
/// secrets are locked. Locked is not an error here: the pool works without
/// a signer until the host unlocks and `refresh_signer` runs.
async fn load_signer(identity: &IdentityService) -> Option<Keys> {
    match identity.load_keys().await {
        Ok(k) => Some(k),
        Err(MessengerError::NotLoggedIn) | Err(MessengerError::SecretsLocked) => None,
        Err(e) => {
            eprintln!("messenger: signer unavailable: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use messenger_testkit::MemorySecretStore;

    #[tokio::test]
    async fn starts_reports_status_and_shuts_down() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = MessengerConfig::new(dir.path().join("messenger"));
        let secrets = Arc::new(MemorySecretStore::unlocked());
        let rt = MessengerRuntime::start(cfg.clone(), secrets).await.unwrap();

        let st = rt.status().await.unwrap();
        assert_eq!(st.version, messenger_core::VERSION);
        assert!(st.schema_version >= 4);
        assert!(st.secrets_unlocked);
        assert!(!st.identity_present);
        assert!(!st.session_active);
        assert_eq!(st.manifest_serial, Some(2));
        assert!(st.relays_total >= 1);
        assert_eq!(st.outbox_pending, 0);
        assert!(cfg.db_path().exists());

        rt.shutdown().await;
    }

    #[tokio::test]
    async fn locked_secret_store_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = MessengerConfig::new(dir.path().join("messenger"));
        let rt = MessengerRuntime::start(cfg, Arc::new(MemorySecretStore::locked()))
            .await
            .unwrap();
        assert!(!rt.status().await.unwrap().secrets_unlocked);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn session_follows_identity_and_lock_state() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = MessengerConfig::new(dir.path().join("messenger"));
        let secrets = Arc::new(MemorySecretStore::unlocked());
        let rt = MessengerRuntime::start(cfg, secrets.clone()).await.unwrap();
        assert!(!rt.refresh_signer().await.unwrap(), "nothing to do without identity");
        assert!(matches!(rt.send_text_dm(&"ab".repeat(32), "x").await, Err(MessengerError::NotLoggedIn)));

        let created = rt.identity().create("pw").await.unwrap();
        assert!(rt.refresh_signer().await.unwrap(), "session started with the new key");
        assert!(rt.status().await.unwrap().session_active);
        assert_eq!(rt.session_pubkey().await.unwrap(), created.identity.pubkey);
        assert!(!rt.refresh_signer().await.unwrap(), "idempotent");

        // Sending queues the wrap; relays may be unreachable in tests, the
        // outbox keeps it either way.
        let peer = Keys::generate().public_key().to_hex();
        let id = rt.send_text_dm(&peer, "hello").await.unwrap();
        assert!(!id.is_empty());
        assert!(rt.send_text_dm("not a key", "x").await.is_err());

        secrets.set_unlocked(false);
        assert!(rt.refresh_signer().await.unwrap(), "locked secrets stop the session");
        assert!(!rt.status().await.unwrap().session_active);

        secrets.set_unlocked(true);
        assert!(rt.refresh_signer().await.unwrap());
        rt.identity().delete().await.unwrap();
        assert!(rt.refresh_signer().await.unwrap());
        assert!(!rt.status().await.unwrap().session_active);
        rt.shutdown().await;
    }
}
