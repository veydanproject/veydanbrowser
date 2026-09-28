// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Tauri adapter for the messenger module.
//!
//! This is the only place where Veydan Space and the messenger crates meet
//! (docs/messenger-spec.md §4.5). It owns three things and nothing else:
//!
//! 1. `MessengerState`: starts `messenger_runtime::MessengerRuntime` with a
//!    data dir under the app's data dir and the host `SecretStore`.
//! 2. `HostSecretStore`: the `SecretStore` implementation backed by the host.
//!    Stage 0 ships a locked placeholder; stage 1 wires `crate::vault`.
//! 3. `messenger_*` commands that forward to the runtime and translate
//!    `MessengerError` into `AppError`.
//!
//! Keep this file free of messenger logic; if something needs more than a
//! forwarding call, it belongs in a messenger crate.

use crate::error::{AppError, CmdResult};
use crate::AppState;
use async_trait::async_trait;
use messenger_core::{MessengerConfig, MessengerError, SecretStore};
use messenger_runtime::{MessengerRuntime, RuntimeStatus};
use serde::Serialize;
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Host `app_settings` key that shows or hides the module in the UI.
const ENABLED_KEY: &str = "messenger_enabled";

/// Sub-directory of the app data dir owned entirely by the messenger.
const DATA_SUBDIR: &str = "messenger";

/// Held in `AppState.messenger`. `runtime` is `None` when start-up failed;
/// the status command reports the error instead of the app refusing to boot.
pub struct MessengerState {
    runtime: Option<Arc<MessengerRuntime>>,
    start_error: Option<String>,
}

impl MessengerState {
    /// Blocking start, called from Tauri `setup`. Never panics: a broken
    /// messenger must not take the host down.
    pub fn start(app_data_dir: &Path, db: Pool<Sqlite>) -> Self {
        let config = MessengerConfig::new(app_data_dir.join(DATA_SUBDIR));
        let secrets: Arc<dyn SecretStore> = Arc::new(HostSecretStore { _db: db });
        match tauri::async_runtime::block_on(MessengerRuntime::start(config, secrets)) {
            Ok(rt) => Self { runtime: Some(Arc::new(rt)), start_error: None },
            Err(e) => {
                eprintln!("messenger: start failed: {e}");
                Self { runtime: None, start_error: Some(e.to_string()) }
            }
        }
    }

    fn runtime(&self) -> CmdResult<&Arc<MessengerRuntime>> {
        self.runtime
            .as_ref()
            .ok_or_else(|| AppError::Other(self.start_error.clone().unwrap_or_default()))
    }
}

/// Host-backed secret storage. Stage 0: not wired, always locked. Stage 1
/// replaces the body with `crate::vault` wrapping (nsec and group keys are
/// encrypted with the vault key; a closed vault yields `SecretsLocked`).
struct HostSecretStore {
    _db: Pool<Sqlite>,
}

#[async_trait]
impl SecretStore for HostSecretStore {
    async fn get(&self, _key: &str) -> messenger_core::Result<Option<Zeroizing<Vec<u8>>>> {
        Err(MessengerError::SecretsLocked)
    }

    async fn put(&self, _key: &str, _value: &[u8]) -> messenger_core::Result<()> {
        Err(MessengerError::SecretsLocked)
    }

    async fn delete(&self, _key: &str) -> messenger_core::Result<()> {
        Err(MessengerError::SecretsLocked)
    }

    async fn is_unlocked(&self) -> bool {
        false
    }
}

fn map_err(e: MessengerError) -> AppError {
    match e {
        MessengerError::SecretsLocked => AppError::VaultLocked,
        MessengerError::Storage(m) => AppError::Db(m),
        MessengerError::Io(m) => AppError::Io(m),
        other => AppError::Other(other.to_string()),
    }
}

/// What the UI needs to decide whether to show the module.
#[derive(Serialize)]
pub struct MessengerStatus {
    /// Always `true` when this command exists; the UI treats a missing
    /// command as `compiled: false`.
    pub compiled: bool,
    /// Host setting: show the module in navigation.
    pub enabled: bool,
    pub runtime: Option<RuntimeStatus>,
    pub error: Option<String>,
}

async fn read_enabled(db: &Pool<Sqlite>) -> bool {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(ENABLED_KEY)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[tauri::command]
pub async fn messenger_status(state: tauri::State<'_, AppState>) -> CmdResult<MessengerStatus> {
    let enabled = read_enabled(&state.db).await;
    let ms = &state.messenger;
    let runtime = match ms.runtime.as_ref() {
        Some(rt) => Some(rt.status().await.map_err(map_err)?),
        None => None,
    };
    Ok(MessengerStatus { compiled: true, enabled, runtime, error: ms.start_error.clone() })
}

#[tauri::command]
pub async fn messenger_set_enabled(enabled: bool, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    // Make sure the runtime is healthy before letting the UI show the module.
    if enabled {
        state.messenger.runtime()?;
    }
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(ENABLED_KEY)
    .bind(if enabled { "1" } else { "0" })
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}
