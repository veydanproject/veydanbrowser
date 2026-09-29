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

pub mod push;

use crate::error::{AppError, CmdResult};
use crate::{vault, AppState};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use messenger_core::{MessengerConfig, MessengerError, SecretStore};
use messenger_core::PubKey;
use messenger_runtime::{
    ChatView, ContactPatch, ContactView, CreatedIdentity, DmAction, GroupKind, GroupOp, GroupView, Identity, InviteView,
    LinkPreview, LinkView, ManifestInfo, MessageView,
    MediaKind, MediaServerInput, MediaServerView, MessengerRuntime, Recording, RelationView, TransferView,
    ProfileView, RelayView,
    RuntimeStatus, SharedCounts, SharedSection,
};
use serde::Deserialize;
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
                #[cfg(target_os = "android")]
                push::spawn_bridge(app.clone(), rt.clone());
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
        // Validation messages reach the UI as they are: relationship
        // refusals are stable codes (`dm_waiting_approval`, …) it translates.
        MessengerError::Invalid(m) => AppError::Other(m),
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
    // The push server is told to forget the device while there is still a
    // key to sign the request with. It is not a reason to keep the identity:
    // a registration nobody renews runs out by itself.
    if let Err(e) = rt.push_unregister().await {
        eprintln!("messenger push: the registration was not taken back: {e}");
    }
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

// ─── Profiles / contacts ────────────────────────────────────────────────────

fn parse_pubkey(hex: &str) -> CmdResult<PubKey> {
    PubKey::parse(hex).ok_or_else(|| AppError::Other("expected a 64-hex public key".into()))
}

#[derive(Deserialize)]
pub struct ContactPatchInput {
    /// `null` clears; missing leaves untouched.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub nickname: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub note: Option<Option<String>>,
    #[serde(default)]
    pub is_muted: Option<bool>,
    #[serde(default)]
    pub notification_level: Option<String>,
}

fn deserialize_double_option<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(d)?))
}

#[tauri::command]
pub async fn messenger_profile_get(pubkey: String, state: tauri::State<'_, AppState>) -> CmdResult<Option<ProfileView>> {
    state.messenger.runtime()?.profiles().get(&parse_pubkey(&pubkey)?).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_profile_request(pubkey: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.request_profile(&parse_pubkey(&pubkey)?).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_profile_own_get(state: tauri::State<'_, AppState>) -> CmdResult<Option<ProfileView>> {
    state.messenger.runtime()?.my_profile().await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_profile_own_set(
    input: messenger_contacts_input::ProfileInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<ProfileView> {
    state.messenger.runtime()?.publish_own_profile(&input).await.map_err(map_err)
}

/// Re-export so the command signature can name the type without the adapter
/// depending on the contacts crate directly.
pub mod messenger_contacts_input {
    pub use messenger_runtime::ProfileInput;
}

#[tauri::command]
pub async fn messenger_nip05_verify(pubkey: String, state: tauri::State<'_, AppState>) -> CmdResult<bool> {
    state.messenger.runtime()?.verify_nip05(&parse_pubkey(&pubkey)?).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_contacts_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<ContactView>> {
    state.messenger.runtime()?.contacts().list().await.map_err(map_err)
}

/// `key` is an npub, a hex key or a NIP-05 identifier.
#[tauri::command]
pub async fn messenger_contacts_add(
    key: String,
    nickname: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<ContactView> {
    state.messenger.runtime()?.contact_add(&key, nickname.as_deref()).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_contacts_update(
    pubkey: String,
    patch: ContactPatchInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<ContactView> {
    let p = ContactPatch {
        nickname: patch.nickname,
        note: patch.note,
        is_muted: patch.is_muted,
        notification_level: patch.notification_level,
    };
    state.messenger.runtime()?.contact_update(&parse_pubkey(&pubkey)?, &p).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_contacts_remove(pubkey: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.contact_remove(&parse_pubkey(&pubkey)?).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_contacts_set_followed(
    pubkey: String,
    followed: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.contact_set_followed(&parse_pubkey(&pubkey)?, followed).await.map_err(map_err)
}

// ─── Chats / DM ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn messenger_chats_list(
    include_archived: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<ChatView>> {
    state.messenger.runtime()?.chats(include_archived.unwrap_or(false)).await.map_err(map_err)
}

/// Open (creating if needed) the chat with `peer` (npub or hex).
#[tauri::command]
pub async fn messenger_chat_open(peer: String, state: tauri::State<'_, AppState>) -> CmdResult<ChatView> {
    state.messenger.runtime()?.chat_open(&peer).await.map_err(map_err)
}

/// One page of visible messages, oldest first, strictly older than `before`.
#[tauri::command]
pub async fn messenger_chat_messages(
    chat_id: String,
    before: Option<i64>,
    limit: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<MessageView>> {
    state.messenger.runtime()?.dm().messages(&chat_id, before, limit.unwrap_or(50)).await.map_err(map_err)
}

/// How many messages each section of what a chat has shared holds.
#[tauri::command]
pub async fn messenger_chat_shared_counts(chat_id: String, state: tauri::State<'_, AppState>) -> CmdResult<SharedCounts> {
    state.messenger.runtime()?.shared_counts(&chat_id).await.map_err(map_err)
}

/// One page of a section of what a chat has shared, newest first, strictly older than `before`.
#[tauri::command]
pub async fn messenger_chat_shared(
    chat_id: String,
    section: SharedSection,
    before: Option<i64>,
    limit: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<MessageView>> {
    state.messenger.runtime()?.shared(&chat_id, section, before, limit.unwrap_or(60)).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_chat_mark_read(chat_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.dm().mark_read(&chat_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_chat_set_pinned(
    chat_id: String,
    pinned: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.dm().set_pinned(&chat_id, pinned).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_chat_set_archived(
    chat_id: String,
    archived: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.dm().set_archived(&chat_id, archived).await.map_err(map_err)
}

/// Removes the chat and its messages from this device only.
#[tauri::command]
pub async fn messenger_chat_delete(chat_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.chat_delete(&chat_id).await.map_err(map_err)
}

/// Send a text DM to an npub/hex key; returns the stored message.
#[tauri::command]
pub async fn messenger_dm_send_text(
    to: String,
    text: String,
    reply_to: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<MessageView> {
    state.messenger.runtime()?.dm_send_text(&to, &text, reply_to.as_deref()).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_dm_edit(
    message_id: String,
    text: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<MessageView> {
    state.messenger.runtime()?.dm_edit(&message_id, &text).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_dm_delete(
    message_id: String,
    for_everyone: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.dm_delete(&message_id, for_everyone).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_dm_retry(message_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.dm_retry(&message_id).await.map_err(map_err)
}

// ─── DM relationship ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn messenger_dm_relation(peer: String, state: tauri::State<'_, AppState>) -> CmdResult<RelationView> {
    state.messenger.runtime()?.dm_relation(&peer).await.map_err(map_err)
}

/// `action`: request | accept | decline | block | unblock | remove.
#[tauri::command]
pub async fn messenger_dm_action(
    peer: String,
    action: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<RelationView> {
    let action = match action.as_str() {
        "request" => DmAction::Request,
        "accept" => DmAction::Accept,
        "decline" => DmAction::Decline,
        "block" => DmAction::Block,
        "unblock" => DmAction::Unblock,
        "remove" => DmAction::Remove,
        other => return Err(AppError::Other(format!("unknown action: {other}"))),
    };
    state.messenger.runtime()?.dm_act(&peer, action).await.map_err(map_err)
}

// ─── Groups ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn messenger_groups_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<GroupView>> {
    state.messenger.runtime()?.group_list().await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_group_get(group_id: String, state: tauri::State<'_, AppState>) -> CmdResult<GroupView> {
    state.messenger.runtime()?.group_get(&group_id).await.map_err(map_err)
}

/// `kind`: public | private.
#[tauri::command]
pub async fn messenger_group_create(
    kind: String,
    name: String,
    about: Option<String>,
    history_for_new: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<GroupView> {
    let kind = match kind.as_str() {
        "public" => GroupKind::Public,
        "private" => GroupKind::Private,
        other => return Err(AppError::Other(format!("unknown group kind: {other}"))),
    };
    state
        .messenger
        .runtime()?
        .group_create(kind, &name, about.as_deref().unwrap_or(""), history_for_new.unwrap_or(true))
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn messenger_group_invite(
    group_id: String,
    who: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<InviteView> {
    state.messenger.runtime()?.group_invite(&group_id, &who).await.map_err(map_err)
}

/// `direction`: in (for me) | out (sent by me).
#[tauri::command]
pub async fn messenger_group_invites(direction: String, state: tauri::State<'_, AppState>) -> CmdResult<Vec<InviteView>> {
    state.messenger.runtime()?.group_invites(&direction).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_group_answer_invite(
    invite_id: String,
    accept: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.group_answer_invite(&invite_id, accept).await.map_err(map_err)
}

/// Open a `veydan://group/…` link: join a public group, ask a private one.
#[tauri::command]
pub async fn messenger_group_open_link(
    link: String,
    note: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<GroupView> {
    state.messenger.runtime()?.group_open_link(&link, note.as_deref().unwrap_or("")).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_group_answer_request(
    group_id: String,
    requester: String,
    approve: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<GroupView> {
    state.messenger.runtime()?.group_answer_request(&group_id, &requester, approve).await.map_err(map_err)
}

/// `action` is an operation as the log writes it: `{"op":"remove","who":…}`,
/// `ban`, `unban`, `set_role` (+ `role`), `set_muted` (+ `muted`),
/// `edit_settings`, `transfer_ownership` (+ `to`), `leave`, `disband`.
#[tauri::command]
pub async fn messenger_group_act(
    group_id: String,
    action: serde_json::Value,
    state: tauri::State<'_, AppState>,
) -> CmdResult<GroupView> {
    let body: GroupOp = serde_json::from_value(action).map_err(|e| AppError::Other(format!("group action: {e}")))?;
    state.messenger.runtime()?.group_act(&group_id, body).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_group_rotate_link(group_id: String, state: tauri::State<'_, AppState>) -> CmdResult<GroupView> {
    state.messenger.runtime()?.group_rotate_link(&group_id).await.map_err(map_err)
}

/// The link of the group as a QR code (SVG).
#[tauri::command]
pub async fn messenger_group_link_qr(group_id: String, state: tauri::State<'_, AppState>) -> CmdResult<String> {
    state.messenger.runtime()?.group_link_qr(&group_id).await.map_err(map_err)
}

/// Remove from this device a group I am no longer in.
#[tauri::command]
pub async fn messenger_group_forget(group_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.group_forget(&group_id).await.map_err(map_err)
}

/// Hex keys of everyone I block.
#[tauri::command]
pub async fn messenger_dm_blocked(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    state.messenger.runtime()?.dm_blocked().await.map_err(map_err)
}

// ─── Media ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn messenger_media_servers(state: tauri::State<'_, AppState>) -> CmdResult<Vec<MediaServerView>> {
    state.messenger.runtime()?.media_servers().await.map_err(map_err)
}

/// Add or update a blob server. The S3 secret goes to the vault and never
/// comes back to the UI.
#[tauri::command]
pub async fn messenger_media_server_put(
    input: MediaServerInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<MediaServerView> {
    state.messenger.runtime()?.media_server_put(input).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_media_server_remove(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.media_server_remove(&id).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_media_server_set_enabled(
    id: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.messenger.runtime()?.media_server_set_enabled(&id, enabled).await.map_err(map_err)
}

/// Checks credentials, prepares the bucket, writes and reads a probe.
#[tauri::command]
pub async fn messenger_media_server_check(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.media_server_check(&id).await.map_err(map_err)
}

/// Attach a local file; returns the placeholder message immediately.
#[tauri::command]
pub async fn messenger_dm_send_file(
    to: String,
    path: String,
    caption: Option<String>,
    // The same for files picked together: they are shown as one album.
    batch: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<MessageView> {
    let rt = state.messenger.runtime()?;
    let local = import_picked(&app, &path, rt.config().data_dir()).await?;
    rt.dm_send_file(&to, &local, caption.as_deref(), batch.as_deref()).await.map_err(map_err)
}

/// A picked file as a local path. Desktop pickers give paths. Android gives
/// `content://` URIs that only the system can read: those are copied into
/// the messenger's own folder first (the copy is also what the chat shows
/// for a sent file).
async fn import_picked(app: &tauri::AppHandle, picked: &str, data_dir: &Path) -> CmdResult<std::path::PathBuf> {
    if !picked.starts_with("content://") {
        let _ = (app, data_dir);
        return Ok(std::path::PathBuf::from(picked));
    }
    let source = crate::commands::notes::attachments::open_source(app, picked)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = data_dir.join("outgoing").join(format!("{stamp:x}"));
    // The display name comes from another app: keep only its last segment.
    let name: String = source
        .name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("file")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let name = if name.trim().is_empty() || name.starts_with('.') { "file".to_string() } else { name };
    let dest = dir.join(name);
    let out = dest.clone();
    tokio::task::spawn_blocking(move || -> CmdResult<()> {
        std::fs::create_dir_all(&dir).map_err(AppError::io)?;
        let mut input = (source.open)().map_err(AppError::io)?;
        let mut file = std::fs::File::create(&out).map_err(AppError::io)?;
        std::io::copy(&mut input, &mut file).map_err(AppError::io)?;
        Ok(())
    })
    .await
    .map_err(AppError::io)??;
    Ok(dest)
}

/// Path of the attachment once it is on this device; `null` when an
/// automatic download decided not to start.
#[tauri::command]
pub async fn messenger_media_download(
    message_id: String,
    manual: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<String>> {
    let p = state.messenger.runtime()?.media_download(&message_id, manual).await.map_err(map_err)?;
    Ok(p.map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn messenger_media_transfer(
    message_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<TransferView>> {
    state.messenger.runtime()?.media_transfer(&message_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_media_pause(transfer_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.media_pause(&transfer_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_media_resume(transfer_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.media_resume(&transfer_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_media_cancel(transfer_id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.media_cancel(&transfer_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn messenger_media_save_as(
    message_id: String,
    dest: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let rt = state.messenger.runtime()?;
    if dest.starts_with("content://") {
        // Android: the destination is a document the system opens for us.
        let src = rt
            .media_local_path(&message_id)
            .await
            .map_err(map_err)?
            .ok_or_else(|| AppError::Other("err.not_downloaded".into()))?;
        return tokio::task::spawn_blocking(move || -> CmdResult<()> {
            use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
            let url = tauri::Url::parse(&dest).map_err(AppError::other)?;
            let mut opts = OpenOptions::new();
            opts.write(true).truncate(true);
            let mut out = app.fs().open(FilePath::Url(url), opts).map_err(AppError::io)?;
            std::io::copy(&mut std::fs::File::open(&src).map_err(AppError::io)?, &mut out).map_err(AppError::io)?;
            Ok(())
        })
        .await
        .map_err(AppError::io)?;
    }
    rt.media_save_as(&message_id, Path::new(&dest)).await.map_err(map_err)
}

/// Inline preview (`data:` url) for images, audio and video that are on
/// this device and small enough; `null` otherwise.
#[tauri::command]
pub async fn messenger_media_data_url(
    message_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<String>> {
    state.messenger.runtime()?.media_data_url(&message_id).await.map_err(map_err)
}

/// Local path of the attachment if present (to open or reveal it).
#[tauri::command]
pub async fn messenger_media_local_path(
    message_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<String>> {
    let p = state.messenger.runtime()?.media_local_path(&message_id).await.map_err(map_err)?;
    Ok(p.map(|p| p.to_string_lossy().into_owned()))
}

/// Open a link from a message in the system browser. http(s) only; the
/// opener never goes through a shell.
#[tauri::command]
pub async fn messenger_open_url(url: String, app: tauri::AppHandle) -> CmdResult<()> {
    let url = url.trim();
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_control() || c.is_whitespace());
    if !ok {
        return Err(AppError::Other("only http(s) links can be opened".into()));
    }
    open_external(&app, url, false)
}

// ─── Links ──────────────────────────────────────────────────────────────────

/// What each link leads to, in the order asked. Takes `veydan://…`,
/// `npub1…` and `nostr:npub1…`; anything else comes back as `invalid`.
#[tauri::command]
pub async fn messenger_links_inspect(links: Vec<String>, state: tauri::State<'_, AppState>) -> CmdResult<Vec<LinkView>> {
    state.messenger.runtime()?.links_inspect(&links).await.map_err(map_err)
}

/// The `veydan://contact/…` link of a person, to share.
#[tauri::command]
pub async fn messenger_contact_link(pubkey: String, state: tauri::State<'_, AppState>) -> CmdResult<String> {
    state.messenger.runtime()?.contact_link(&parse_pubkey(&pubkey)?).await.map_err(map_err)
}

/// Title, description and picture of an https page. Asks the page: call
/// it when the user pressed the button, never when a message is shown.
#[tauri::command]
pub async fn messenger_link_preview(url: String, state: tauri::State<'_, AppState>) -> CmdResult<LinkPreview> {
    state.messenger.runtime()?.link_preview(&url).await.map_err(map_err)
}

/// Open the attachment of a message with the system's default application.
#[tauri::command]
pub async fn messenger_media_open(
    message_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let path = state
        .messenger
        .runtime()?
        .media_local_path(&message_id)
        .await
        .map_err(map_err)?
        .ok_or_else(|| AppError::Other("err.not_downloaded".into()))?;
    // A received file is untrusted: anything the system would run is only
    // shown in its folder, never launched.
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if RUNNABLE_EXTENSIONS.contains(&ext.as_str()) || ext.is_empty() {
        return reveal(&app, &path);
    }
    open_external(&app, &path.to_string_lossy(), true)
}

/// File types that execute code when opened with the default handler.
const RUNNABLE_EXTENSIONS: &[&str] = &[
    "exe", "msi", "bat", "cmd", "com", "scr", "pif", "cpl", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta",
    "lnk", "reg", "sh", "bash", "zsh", "fish", "desktop", "appimage", "run", "bin", "jar", "app", "command",
    "dmg", "pkg", "deb", "rpm", "apk", "py", "pl", "rb", "php", "html", "htm", "svg", "url", "scpt",
];

#[cfg(desktop)]
fn reveal(app: &tauri::AppHandle, path: &Path) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(path).map_err(|e| AppError::Other(e.to_string()))
}

#[cfg(not(desktop))]
fn reveal(_app: &tauri::AppHandle, _path: &Path) -> CmdResult<()> {
    Err(AppError::Other("not available on this platform".into()))
}

#[cfg(desktop)]
fn open_external(app: &tauri::AppHandle, target: &str, is_path: bool) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let res = if is_path {
        app.opener().open_path(target, None::<&str>)
    } else {
        app.opener().open_url(target, None::<&str>)
    };
    res.map_err(|e| AppError::Other(e.to_string()))
}

#[cfg(not(desktop))]
fn open_external(_app: &tauri::AppHandle, _target: &str, _is_path: bool) -> CmdResult<()> {
    Err(AppError::Other("not available on this platform".into()))
}

/// What the recorder in the UI produced. Bytes travel as base64: mobile
/// IPC has no raw bodies, and recordings are small.
#[derive(Deserialize)]
pub struct RecordingInput {
    /// `voice` | `circle`
    pub kind: String,
    pub mime: String,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub waveform: Option<Vec<u8>>,
    pub data_base64: String,
}

/// Send a voice message or a video circle recorded in the app.
#[tauri::command]
pub async fn messenger_dm_send_recording(
    to: String,
    recording: RecordingInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<MessageView> {
    let kind = MediaKind::parse(&recording.kind).ok_or_else(|| AppError::Other("unknown recording kind".into()))?;
    let bytes = B64
        .decode(recording.data_base64.as_bytes())
        .map_err(|_| AppError::Other("recording is not base64".into()))?;
    let rec = Recording {
        kind,
        mime: recording.mime,
        duration_ms: recording.duration_ms,
        waveform: recording.waveform,
        bytes,
    };
    state.messenger.runtime()?.dm_send_recording(&to, rec, None).await.map_err(map_err)
}

/// Let the webview answer microphone and camera requests (WebKitGTK denies
/// them unless the app does; other webviews ask the user themselves).
#[tauri::command]
pub fn messenger_media_grant_access(window: tauri::WebviewWindow) -> CmdResult<()> {
    crate::commands::media::media_grant_access(window).map_err(AppError::Other)
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
