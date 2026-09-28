// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! The one object a host embeds.
//!
//! `MessengerRuntime::start(config, secret_store)` opens the store, the
//! identity service and the relay pool. The host only ever talks to this
//! type: the Tauri adapter today, a standalone app or the `messenger-cli`
//! tomorrow. Nothing here knows about Tauri.

pub mod relays;

use messenger_core::traits::RelayState;
use messenger_core::{MessengerConfig, MessengerError, Result, SecretStore};
use messenger_identity::IdentityService;
use messenger_store::Store;
use nostr::key::Keys;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

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
    /// Whether the relay pool currently holds a signer (NIP-42 capable).
    pub signer_loaded: bool,
    pub relays_total: usize,
    pub relays_connected: usize,
    pub silent_mode: bool,
    pub manifest_serial: Option<u64>,
    pub region: String,
}

pub struct MessengerRuntime {
    config: MessengerConfig,
    store: Store,
    secrets: Arc<dyn SecretStore>,
    identity: IdentityService,
    relays: RelayService,
    /// Public key of the signer the pool was built with, if any.
    signer_pubkey: Mutex<Option<String>>,
}

impl MessengerRuntime {
    pub async fn start(config: MessengerConfig, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let store = Store::open(&config).await?;
        let identity = IdentityService::new(store.clone(), secrets.clone());
        let signer = load_signer(&identity).await;
        let signer_pubkey = signer.as_ref().map(|k| k.public_key().to_hex());
        let relays = RelayService::init(store.clone(), signer).await?;
        Ok(Self {
            config,
            store,
            secrets,
            identity,
            relays,
            signer_pubkey: Mutex::new(signer_pubkey),
        })
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

    /// Re-check which key should sign for the pool and rebuild it when that
    /// changed. Call after identity create/import/delete and after the host
    /// unlocks secrets. Idempotent and cheap when nothing changed.
    pub async fn refresh_signer(&self) -> Result<bool> {
        let signer = load_signer(&self.identity).await;
        let pubkey = signer.as_ref().map(|k| k.public_key().to_hex());
        let mut current = self.signer_pubkey.lock().await;
        if *current == pubkey {
            return Ok(false);
        }
        self.relays.set_signer(signer).await?;
        *current = pubkey;
        Ok(true)
    }

    pub async fn status(&self) -> Result<RuntimeStatus> {
        let relays = self.relays.list().await?;
        Ok(RuntimeStatus {
            version: messenger_core::VERSION.to_string(),
            data_dir: self.config.data_dir().to_string_lossy().into_owned(),
            schema_version: self.store.schema_version().await?,
            secrets_unlocked: self.secrets.is_unlocked().await,
            identity_present: self.identity.get().await?.is_some(),
            signer_loaded: self.signer_pubkey.lock().await.is_some(),
            relays_total: relays.iter().filter(|r| r.enabled).count(),
            relays_connected: relays.iter().filter(|r| r.state == RelayState::Connected).count(),
            silent_mode: self.relays.is_silent().await?,
            manifest_serial: self.relays.manifest_serial().await?,
            region: self.relays.region().await?,
        })
    }

    /// Stop background work and close the database. Idempotent.
    pub async fn shutdown(&self) {
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
        assert!(st.schema_version >= 2);
        assert!(st.secrets_unlocked);
        assert!(!st.identity_present);
        assert!(!st.signer_loaded);
        assert_eq!(st.manifest_serial, Some(1));
        assert!(st.relays_total >= 1);
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
    async fn signer_follows_identity_and_lock_state() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = MessengerConfig::new(dir.path().join("messenger"));
        let secrets = Arc::new(MemorySecretStore::unlocked());
        let rt = MessengerRuntime::start(cfg, secrets.clone()).await.unwrap();
        assert!(!rt.refresh_signer().await.unwrap(), "nothing to do without identity");

        rt.identity().create("pw").await.unwrap();
        assert!(rt.refresh_signer().await.unwrap(), "pool rebuilt with the new key");
        assert!(rt.status().await.unwrap().signer_loaded);
        assert!(!rt.refresh_signer().await.unwrap(), "idempotent");

        secrets.set_unlocked(false);
        assert!(rt.refresh_signer().await.unwrap(), "locked secrets drop the signer");
        assert!(!rt.status().await.unwrap().signer_loaded);

        secrets.set_unlocked(true);
        assert!(rt.refresh_signer().await.unwrap());
        rt.identity().delete().await.unwrap();
        assert!(rt.refresh_signer().await.unwrap());
        assert!(!rt.status().await.unwrap().signer_loaded);
        rt.shutdown().await;
    }
}
