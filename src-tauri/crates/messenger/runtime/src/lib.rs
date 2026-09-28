// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! The one object a host embeds.
//!
//! `MessengerRuntime::start(config, secret_store)` opens the store and (in
//! later stages) the transport, ingress and handlers. The host only ever
//! talks to this type: the Tauri adapter today, a standalone app or the
//! `messenger-cli` tomorrow. Nothing here knows about Tauri.

use messenger_core::{MessengerConfig, Result, SecretStore};
use messenger_store::Store;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Facts for the host's status screen. Never contains secrets.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStatus {
    /// Messenger crate version (not the host application version).
    pub version: String,
    pub data_dir: String,
    pub schema_version: i64,
    pub secrets_unlocked: bool,
    /// Stage 1 fills this from `msg_identity`.
    pub identity_present: bool,
}

pub struct MessengerRuntime {
    config: MessengerConfig,
    store: Store,
    secrets: Arc<dyn SecretStore>,
}

impl MessengerRuntime {
    pub async fn start(config: MessengerConfig, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let store = Store::open(&config).await?;
        Ok(Self { config, store, secrets })
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

    pub async fn status(&self) -> Result<RuntimeStatus> {
        Ok(RuntimeStatus {
            version: messenger_core::VERSION.to_string(),
            data_dir: self.config.data_dir().to_string_lossy().into_owned(),
            schema_version: self.store.schema_version().await?,
            secrets_unlocked: self.secrets.is_unlocked().await,
            identity_present: false,
        })
    }

    /// Stop background work and close the database. Idempotent.
    pub async fn shutdown(&self) {
        self.store.close().await;
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
        assert!(st.schema_version >= 1);
        assert!(st.secrets_unlocked);
        assert!(!st.identity_present);
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
    }
}
