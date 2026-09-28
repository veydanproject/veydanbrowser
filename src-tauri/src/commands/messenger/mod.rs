// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Tauri adapter for the messenger module.
//!
//! This is the only place where Veydan Space and the messenger crates meet
//! (docs/messenger-spec.md §4.5). It owns three things and nothing else:
//!
//! 1. `MessengerState`: starts `messenger_runtime::MessengerRuntime` with a
//!    data dir under the app's data dir and the host `SecretStore`.
//! 2. `HostSecretStore`: `SecretStore` backed by the password vault. Every
//!    value is XChaCha20-Poly1305-encrypted with the vault key and kept in
//!    the host `app_settings` table; a closed vault yields `SecretsLocked`.
//! 3. `messenger_*` commands that forward to the runtime and translate
//!    `MessengerError` into `AppError`.
//!
//! Keep this file free of messenger logic; if something needs more than a
//! forwarding call, it belongs in a messenger crate.

use crate::error::{AppError, CmdResult};
use crate::{vault, AppState};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use messenger_core::{MessengerConfig, MessengerError, SecretStore};
use messenger_runtime::{CreatedIdentity, Identity, ManifestInfo, MessengerRuntime, RelayView, RuntimeStatus};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use zeroize::Zeroizing;

/// Emitted with `Vec<RelayView>` whenever relay state changes.
pub const EVENT_RELAY_STATUS: &str = "messenger://relay-status";
/// Emitted with a runtime `UiEvent` (`{name, payload}`): inbound.dm,
/// inbound.meta, ignored, error, notify.
pub const EVENT_RUNTIME: &str = "messenger://event";

/// Host `app_settings` key that shows or hides the module in the UI.
const ENABLED_KEY: &str = "messenger_enabled";

/// Prefix of `app_settings` keys holding vault-encrypted messenger secrets.
/// Not in the sync whitelist, so secrets never leave this device.
const SECRET_PREFIX: &str = "messenger_secret:";

/// Sub-directory of the app data dir owned entirely by the messenger.
const DATA_SUBDIR: &str = "messenger";

/// Held in `AppState.messenger`. `runtime` is `None` when start-up failed;
/// the status command reports the error instead of the app refusing to boot.
pub struct MessengerState {
    runtime: Option<Arc<MessengerRuntime>>,
    start_error: Option<String>,
}

impl MessengerState {
    /// Blocking start, called from Tauri `setup` before `AppState` is managed.
    /// Never panics: a broken messenger must not take the host down.
    pub fn start(app: tauri::AppHandle, app_data_dir: &Path) -> Self {
        // Two rustls providers are linked (host: aws-lc-rs, nostr-sdk: ring).
        // Pick the host's one for the whole process before any TLS handshake;
        // a second call (already installed) is not an error worth reporting.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let config = MessengerConfig::new(app_data_dir.join(DATA_SUBDIR));
        let secrets: Arc<dyn SecretStore> = Arc::new(HostSecretStore { app: app.clone() });
        match tauri::async_runtime::block_on(MessengerRuntime::start(config, secrets)) {
            Ok(rt) => {
                let rt = Arc::new(rt);
                spawn_relay_status_watcher(app.clone(), rt.clone());
                spawn_ui_event_forwarder(app, rt.clone());
                Self { runtime: Some(rt), start_error: None }
            }
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

/// Polls relay state and emits `EVENT_RELAY_STATUS` when it changes. The
/// UI relies on this instead of polling commands itself.
fn spawn_relay_status_watcher(app: tauri::AppHandle, rt: Arc<MessengerRuntime>) {
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let Ok(list) = rt.relays().list().await else { continue };
            let snapshot = serde_json::to_string(&list).unwrap_or_default();
            if snapshot != last {
                last = snapshot;
                let _ = app.emit(EVENT_RELAY_STATUS, &list);
            }
        }
    });
}

/// Forwards runtime UI events to the webview as `EVENT_RUNTIME`.
fn spawn_ui_event_forwarder(app: tauri::AppHandle, rt: Arc<MessengerRuntime>) {
    tauri::async_runtime::spawn(async move {
        let mut rx = rt.ui_events();
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let _ = app.emit(EVENT_RUNTIME, &ev);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

/// Vault-backed secret storage. Resolves `AppState` lazily because the store
/// is created before the state is managed.
struct HostSecretStore {
    app: tauri::AppHandle,
}

impl HostSecretStore {
    /// `AppState` is managed after the runtime starts, so early calls (the
    /// signer probe in `MessengerRuntime::start`) see "locked" instead of
    /// panicking; `refresh_signer` picks the key up on the first status call.
    fn state(&self) -> messenger_core::Result<tauri::State<'_, AppState>> {
        self.app.try_state::<AppState>().ok_or(MessengerError::SecretsLocked)
    }

    fn setting_key(key: &str) -> String {
        format!("{SECRET_PREFIX}{key}")
    }

    fn aad_id(key: &str) -> String {
        format!("messenger:{key}")
    }

    fn locked(e: AppError) -> MessengerError {
        match e {
            AppError::VaultLocked | AppError::VaultMismatch => MessengerError::SecretsLocked,
            AppError::Db(m) => MessengerError::Storage(m),
            other => MessengerError::Crypto(other.to_string()),
        }
    }
}

#[async_trait]
impl SecretStore for HostSecretStore {
    async fn get(&self, key: &str) -> messenger_core::Result<Option<Zeroizing<Vec<u8>>>> {
        let state = self.state()?;
        let (vk, _) = vault::require_open(&state).map_err(Self::locked)?;
        let stored = sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
            .bind(Self::setting_key(key))
            .fetch_optional(&state.db)
            .await
            .map_err(|e| MessengerError::Storage(e.to_string()))?;
        let Some(stored) = stored else { return Ok(None) };
        let b64 = Zeroizing::new(
            vault::decrypt_field(&vk, &Self::aad_id(key), "secret", &stored).map_err(Self::locked)?,
        );
        let bytes = B64
            .decode(b64.as_bytes())
            .map_err(|_| MessengerError::Crypto("stored secret is not base64".into()))?;
        Ok(Some(Zeroizing::new(bytes)))
    }

    async fn put(&self, key: &str, value: &[u8]) -> messenger_core::Result<()> {
        let state = self.state()?;
        let (vk, _) = vault::ensure_key(&state).await.map_err(Self::locked)?;
        let b64 = Zeroizing::new(B64.encode(value));
        let enc = vault::encrypt_field(&vk, &Self::aad_id(key), "secret", &b64).map_err(Self::locked)?;
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(Self::setting_key(key))
        .bind(enc)
        .execute(&state.db)
        .await
        .map_err(|e| MessengerError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> messenger_core::Result<()> {
        let state = self.state()?;
        sqlx::query("DELETE FROM app_settings WHERE key = ?")
            .bind(Self::setting_key(key))
            .execute(&state.db)
            .await
            .map_err(|e| MessengerError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn is_unlocked(&self) -> bool {
        match self.state() {
            Ok(state) => state.vault.open_key().is_some() || state.vault.pending().is_some(),
            Err(_) => false,
        }
    }
}

fn map_err(e: MessengerError) -> AppError {
    match e {
        MessengerError::SecretsLocked => AppError::VaultLocked,
        MessengerError::Storage(m) => AppError::Db(m),
        MessengerError::Io(m) => AppError::Io(m),
        MessengerError::NotLoggedIn => AppError::NotFound("messenger identity".into()),
        other => AppError::Other(other.to_string()),
    }
}

async fn read_enabled(db: &sqlx::Pool<sqlx::Sqlite>) -> bool {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(ENABLED_KEY)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|v| v == "1")
        .unwrap_or(false)
}

// ─── Status / enable ────────────────────────────────────────────────────────

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

#[tauri::command]
pub async fn messenger_status(state: tauri::State<'_, AppState>) -> CmdResult<MessengerStatus> {
    let enabled = read_enabled(&state.db).await;
    let ms = &state.messenger;
    let runtime = match ms.runtime.as_ref() {
        Some(rt) => {
            // The vault may have been unlocked since start-up; pick up the signer.
            if let Err(e) = rt.refresh_signer().await {
                eprintln!("messenger: refresh_signer: {e}");
            }
            Some(rt.status().await.map_err(map_err)?)
        }
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

// ─── Identity ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn messenger_identity_get(state: tauri::State<'_, AppState>) -> CmdResult<Option<Identity>> {
    state.messenger.runtime()?.identity().get().await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_identity_create(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<CreatedIdentity> {
    let rt = state.messenger.runtime()?;
    let created = rt.identity().create(&password).await.map_err(map_err)?;
    rt.refresh_signer().await.map_err(map_err)?;
    Ok(created)
}

/// `kind` is `nsec` (also accepts hex), `ncryptsec` (needs `password`) or
/// `mnemonic` (`password` is the optional BIP-39 passphrase).
#[tauri::command]
pub async fn messenger_identity_import(
    kind: String,
    secret: String,
    password: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Identity> {
    let rt = state.messenger.runtime()?;
    let svc = rt.identity();
    let pw = password.unwrap_or_default();
    let res = match kind.as_str() {
        "nsec" => svc.import_nsec(&secret).await,
        "ncryptsec" => svc.import_ncryptsec(&secret, &pw).await,
        "mnemonic" => svc.import_mnemonic(&secret, &pw).await,
        other => Err(MessengerError::Invalid(format!("unknown import kind: {other}"))),
    };
    let identity = res.map_err(map_err)?;
    rt.refresh_signer().await.map_err(map_err)?;
    Ok(identity)
}

#[tauri::command]
pub async fn messenger_identity_export(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<String> {
    state.messenger.runtime()?.identity().export_ncryptsec(&password).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_identity_delete(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let rt = state.messenger.runtime()?;
    rt.identity().delete().await.map_err(map_err)?;
    rt.refresh_signer().await.map_err(map_err)?;
    Ok(())
}

// ─── Relays / manifest ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn messenger_relays_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<RelayView>> {
    state.messenger.runtime()?.relays().list().await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_relays_add(
    url: String,
    api_key: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<RelayView> {
    state.messenger.runtime()?.relays().add_user(&url, api_key).await.map_err(map_err)
}

// ─── DM (stage 3 building block) ────────────────────────────────────────────

/// Send a text DM to an npub/hex key. Returns the outbox local id.
#[tauri::command]
pub async fn messenger_dm_send_text(to: String, text: String, state: tauri::State<'_, AppState>) -> CmdResult<String> {
    state.messenger.runtime()?.send_text_dm(&to, &text).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_relays_remove(url: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.relays().remove_user(&url).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_relays_set_enabled(
    url: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.relays().set_enabled(&url, enabled).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_relays_set_silent(enabled: bool, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.relays().set_silent(enabled).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_manifest_info(state: tauri::State<'_, AppState>) -> CmdResult<ManifestInfo> {
    state.messenger.runtime()?.relays().manifest_info().await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_manifest_set_region(region: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.relays().set_region(&region).await.map_err(map_err)
}
