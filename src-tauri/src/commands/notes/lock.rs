// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! App lock: Argon2 hash of the PIN or password in `app_settings`,
//! unlock state in memory, auto-lock after inactivity.

use crate::error::{AppError, CmdResult};
use crate::AppState;
use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use crate::vault::{LockMeta, SyncedLock, DEFAULT_SECRET};
use serde::Serialize;
use sqlx::{Pool, Sqlite, Transaction};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

const HASH_KEY: &str = "notes_lock_hash";
const TIMEOUT_KEY: &str = "notes_lock_timeout_min";
/// `pin` | `password`; missing means `password`.
const KIND_KEY: &str = "lock_kind";
const HINT_KEY: &str = "lock_hint";
const KIND_PIN: &str = "pin";
const KIND_PASSWORD: &str = "password";
const MIN_SECRET_LEN: usize = 4;
const DEFAULT_TIMEOUT_MIN: u32 = 5;
const TICK: Duration = Duration::from_secs(10);

pub const EVENT_LOCKED: &str = "notes://locked";
pub const EVENT_UNLOCKED: &str = "notes://unlocked";

/// Wrong attempts in a row before each further one waits.
const FAIL_FREE_ATTEMPTS: u32 = 3;
const FAIL_DELAY: Duration = Duration::from_secs(1);

/// Last user activity while unlocked; `None` means locked.
#[derive(Default)]
pub struct NotesLock {
    last_activity: Mutex<Option<Instant>>,
    failed_attempts: Mutex<u32>,
    /// Hash the open session was verified against; a synced change relocks.
    session_hash: Mutex<Option<String>>,
}

impl NotesLock {
    fn is_unlocked(&self, timeout_min: u32) -> bool {
        match *self.last_activity.lock().unwrap() {
            None => false,
            Some(_) if timeout_min == 0 => true,
            Some(t) => t.elapsed() < Duration::from_secs(u64::from(timeout_min) * 60),
        }
    }

    pub(crate) fn touch(&self) {
        *self.last_activity.lock().unwrap() = Some(Instant::now());
        *self.failed_attempts.lock().unwrap() = 0;
    }

    /// Unlocked with the given hash; the hash is remembered for the relock check.
    pub(crate) fn open_session(&self, hash: &str) {
        *self.session_hash.lock().unwrap() = Some(hash.to_string());
        self.touch();
    }

    fn clear(&self) {
        *self.last_activity.lock().unwrap() = None;
        *self.session_hash.lock().unwrap() = None;
    }

    fn session_hash(&self) -> Option<String> {
        self.session_hash.lock().unwrap().clone()
    }

    /// Slows brute force: after a few wrong passwords each attempt pauses first.
    async fn throttle(&self) {
        let failed = *self.failed_attempts.lock().unwrap();
        if failed >= FAIL_FREE_ATTEMPTS {
            tokio::time::sleep(FAIL_DELAY).await;
        }
    }

    fn record_failure(&self) {
        let mut failed = self.failed_attempts.lock().unwrap();
        *failed = failed.saturating_add(1);
    }
}

#[derive(Debug, Serialize)]
pub struct LockStatus {
    pub enabled: bool,
    pub locked: bool,
    pub timeout_min: u32,
    /// `none` | `ok` | `mismatch`
    pub vault: String,
    /// `pin` | `password`
    pub kind: String,
    pub hint: Option<String>,
    pub has_recovery: bool,
}

/// Status plus the recovery key when one was just created. The key is shown once.
#[derive(Debug, Serialize)]
pub struct LockSetResult {
    #[serde(flatten)]
    pub status: LockStatus,
    pub recovery_key: Option<String>,
}

async fn read_setting(state: &AppState, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

async fn write_setting(
    executor: impl sqlx::SqliteExecutor<'_>,
    key: &str,
    value: Option<&str>,
) -> Result<(), AppError> {
    match value {
        Some(v) => sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(v)
        .execute(executor)
        .await
        .map(|_| ())
        .map_err(AppError::db),
        None => sqlx::query("DELETE FROM app_settings WHERE key = ?")
            .bind(key)
            .execute(executor)
            .await
            .map(|_| ())
            .map_err(AppError::db),
    }
}

/// Store kind and hint next to the hash inside the same transaction.
async fn write_kind_and_hint(
    tx: &mut Transaction<'_, Sqlite>,
    kind: &str,
    hint: Option<&str>,
) -> Result<(), AppError> {
    write_setting(&mut **tx, KIND_KEY, Some(kind)).await?;
    write_setting(&mut **tx, HINT_KEY, hint).await
}

fn parse_kind(kind: Option<String>) -> Result<String, AppError> {
    match kind.as_deref() {
        None | Some(KIND_PASSWORD) => Ok(KIND_PASSWORD.into()),
        Some(KIND_PIN) => Ok(KIND_PIN.into()),
        Some(_) => Err(AppError::other("Unknown lock kind")),
    }
}

fn clean_hint(hint: Option<String>) -> Option<String> {
    hint.map(|h| h.trim().to_string()).filter(|h| !h.is_empty())
}

/// Trimmed secret that satisfies the rules for its kind.
fn validate_secret(kind: &str, secret: &str) -> Result<String, AppError> {
    let secret = secret.trim().to_string();
    if secret.chars().count() < MIN_SECRET_LEN {
        return Err(AppError::other("Must be at least 4 characters"));
    }
    if kind == KIND_PIN && !secret.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::other("PIN must contain only digits"));
    }
    Ok(secret)
}

/// Create the recovery key once the vault row exists and has none yet.
/// A mismatched vault leaves recovery to the device that owns it.
async fn ensure_recovery(state: &AppState) -> Result<Option<String>, AppError> {
    let Ok((key, vault_id)) = crate::vault::ensure_key(state).await else {
        return Ok(None);
    };
    if crate::vault::has_recovery(&state.db).await? {
        return Ok(None);
    }
    Ok(Some(crate::vault::install_recovery(&state.db, &key, &vault_id).await?))
}

async fn local_lock_meta(state: &AppState) -> Option<LockMeta> {
    Some(LockMeta {
        hash: read_setting(state, HASH_KEY).await?,
        kind: read_setting(state, KIND_KEY).await,
        hint: read_setting(state, HINT_KEY).await,
    })
}

/// Copy the local lock into the vault row so peers receive it with the matching wrap.
/// Without a lock the row is marked as wrapped with the built-in secret.
/// Only an open vault is written: a mismatched row belongs to another secret.
pub(crate) async fn publish_lock_meta(state: &AppState) -> Result<(), AppError> {
    if state.vault.open_key().is_none() {
        return Ok(());
    }
    let local = local_lock_meta(state).await;
    let synced = crate::vault::synced_lock(&state.db).await?;
    let up_to_date = match (&local, &synced) {
        (Some(meta), SyncedLock::Meta(current)) => current == meta,
        (None, SyncedLock::Default) => true,
        _ => false,
    };
    if up_to_date {
        return Ok(());
    }
    crate::vault::store_lock_meta(&state.db, local.as_ref()).await
}

/// Without a PIN the vault opens by itself with the built-in secret.
pub(crate) async fn open_default(state: &AppState) -> Result<(), AppError> {
    if read_setting(state, HASH_KEY).await.is_some() {
        return Ok(());
    }
    crate::vault::open_with_password(state, DEFAULT_SECRET).await
}

/// After a synced vault row: take over its lock hash, kind and hint. A row that
/// disappeared or lost its lock turns the lock off. A session opened with an older hash closes.
pub(crate) async fn adopt_synced_lock(app: &tauri::AppHandle, state: &AppState) {
    let synced = match crate::vault::synced_lock(&state.db).await {
        Ok(s) => s,
        Err(_) => return,
    };
    let local = local_lock_meta(state).await;
    let result = match synced {
        SyncedLock::Legacy => Ok(()),
        SyncedLock::NoRow | SyncedLock::Default if local.is_some() => {
            clear_lock_settings(&state.db).await
        }
        SyncedLock::NoRow | SyncedLock::Default => Ok(()),
        SyncedLock::Meta(meta) if local.as_ref() == Some(&meta) => Ok(()),
        SyncedLock::Meta(meta) => write_lock_settings(&state.db, &meta).await,
    };
    if result.is_err() {
        return;
    }
    relock_if_hash_changed(app, state).await;
    let _ = open_default(state).await;
}

async fn write_lock_settings(db: &Pool<Sqlite>, meta: &LockMeta) -> Result<(), AppError> {
    write_setting(db, HASH_KEY, Some(&meta.hash)).await?;
    write_setting(db, KIND_KEY, meta.kind.as_deref()).await?;
    write_setting(db, HINT_KEY, meta.hint.as_deref()).await
}

async fn clear_lock_settings(db: &Pool<Sqlite>) -> Result<(), AppError> {
    for key in [HASH_KEY, KIND_KEY, HINT_KEY] {
        write_setting(db, key, None).await?;
    }
    Ok(())
}

async fn timeout_min(state: &AppState) -> u32 {
    read_setting(state, TIMEOUT_KEY)
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MIN)
}

pub(crate) fn hash_password(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(AppError::other)
}

fn verify(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

pub(crate) async fn status(state: &AppState) -> LockStatus {
    let enabled = read_setting(state, HASH_KEY).await.is_some();
    let timeout_min = timeout_min(state).await;
    let locked = enabled && !state.notes_lock.is_unlocked(timeout_min);
    let vault = crate::vault::status_label(state).await;
    let kind = read_setting(state, KIND_KEY)
        .await
        .unwrap_or_else(|| KIND_PASSWORD.into());
    let hint = read_setting(state, HINT_KEY).await;
    let has_recovery = crate::vault::has_recovery(&state.db).await.unwrap_or(false);
    LockStatus {
        enabled,
        locked,
        timeout_min,
        vault,
        kind,
        hint,
        has_recovery,
    }
}

/// True when the notes UI must not reveal content (used by the capture bridge too).
#[cfg(desktop)]
pub(crate) async fn is_locked(state: &AppState) -> bool {
    status(state).await.locked
}

#[tauri::command]
pub async fn notes_lock_status(state: tauri::State<'_, AppState>) -> CmdResult<LockStatus> {
    Ok(status(&state).await)
}

/// Enable, change or (with `password = None`) remove the lock.
/// `current` is required whenever a secret is already set.
/// The first enable also creates the recovery key and returns it once.
#[tauri::command]
pub async fn notes_lock_set(
    password: Option<String>,
    current: Option<String>,
    kind: Option<String>,
    hint: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockSetResult> {
    let enabled = read_setting(&state, HASH_KEY).await;
    if let Some(existing) = &enabled {
        let ok = current
            .as_deref()
            .map(|c| verify(c, existing))
            .unwrap_or(false);
        if !ok {
            return Err(AppError::other("Current password is incorrect"));
        }
    }
    // Without a lock the vault is wrapped with the built-in secret.
    let current_secret = if enabled.is_some() {
        current.as_deref()
    } else {
        Some(DEFAULT_SECRET)
    };
    let password = password
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let mut recovery_key = None;
    match password {
        Some(p) => {
            let kind = parse_kind(kind)?;
            let p = validate_secret(&kind, &p)?;
            let phc = hash_password(&p)?;
            let prepared = crate::vault::prepare_rewrap(&state.db, current_secret, &p).await?;
            let mut tx = state.db.begin().await.map_err(AppError::db)?;
            crate::vault::apply_rewrap(&mut tx, &prepared).await?;
            write_setting(&mut *tx, HASH_KEY, Some(&phc)).await?;
            write_kind_and_hint(&mut tx, &kind, clean_hint(hint).as_deref()).await?;
            tx.commit().await.map_err(AppError::db)?;
            state.vault.apply(prepared.memory);
            state.notes_lock.open_session(&phc);
            recovery_key = ensure_recovery(&state).await?;
            publish_lock_meta(&state).await?;
        }
        None => {
            // Turning the lock off keeps every password: the vault is rewrapped with the built-in secret.
            let prepared =
                crate::vault::prepare_rewrap(&state.db, current_secret, DEFAULT_SECRET).await?;
            let mut tx = state.db.begin().await.map_err(AppError::db)?;
            crate::vault::apply_rewrap(&mut tx, &prepared).await?;
            crate::vault::store_lock_meta(&mut *tx, None).await?;
            for key in [HASH_KEY, KIND_KEY, HINT_KEY] {
                write_setting(&mut *tx, key, None).await?;
            }
            tx.commit().await.map_err(AppError::db)?;
            state.vault.apply(prepared.memory);
            state.notes_lock.clear();
        }
    }
    let _ = app.emit(EVENT_UNLOCKED, ());
    Ok(LockSetResult {
        status: status(&state).await,
        recovery_key,
    })
}

/// Replace the recovery key. The old one stops working.
#[tauri::command]
pub async fn notes_lock_recovery_regenerate(
    current: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<String> {
    state.notes_lock.throttle().await;
    if let Err(e) = require_lock_password(&state, &current).await {
        state.notes_lock.record_failure();
        return Err(e);
    }
    let (key, vault_id) = crate::vault::ensure_key(&state).await?;
    crate::vault::install_recovery(&state.db, &key, &vault_id).await
}

/// Step 1 of recovery: confirm the code opens the vault, nothing changes.
#[tauri::command]
pub async fn notes_lock_recovery_check(
    code: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    state.notes_lock.throttle().await;
    match crate::vault::open_with_recovery(&state.db, &code).await {
        Ok(_) => Ok(()),
        Err(e) => {
            state.notes_lock.record_failure();
            Err(e)
        }
    }
}

/// Step 2 of recovery: set a new secret with the code, unlock, and hand out
/// a fresh recovery key. The used code stops working.
#[tauri::command]
pub async fn notes_lock_recover(
    code: String,
    password: String,
    kind: Option<String>,
    hint: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockSetResult> {
    state.notes_lock.throttle().await;
    let (key, vault_id) = match crate::vault::open_with_recovery(&state.db, &code).await {
        Ok(opened) => opened,
        Err(e) => {
            state.notes_lock.record_failure();
            return Err(e);
        }
    };
    let kind = parse_kind(kind)?;
    let password = validate_secret(&kind, &password)?;
    let phc = hash_password(&password)?;
    let prepared =
        crate::vault::prepare_rewrap_key(key.clone_key(), vault_id.clone(), password).await?;
    let recovery = crate::vault::prepare_recovery(&key, &vault_id).await?;

    let hint = clean_hint(hint);
    let meta = LockMeta {
        hash: phc.clone(),
        kind: Some(kind.clone()),
        hint: hint.clone(),
    };
    let mut tx = state.db.begin().await.map_err(AppError::db)?;
    crate::vault::apply_rewrap(&mut tx, &prepared).await?;
    crate::vault::store_recovery(&mut *tx, &recovery).await?;
    crate::vault::store_lock_meta(&mut *tx, Some(&meta)).await?;
    write_setting(&mut *tx, HASH_KEY, Some(&phc)).await?;
    write_kind_and_hint(&mut tx, &kind, hint.as_deref()).await?;
    tx.commit().await.map_err(AppError::db)?;

    state.vault.apply(prepared.memory);
    state.notes_lock.open_session(&phc);
    let _ = app.emit(EVENT_UNLOCKED, ());
    Ok(LockSetResult {
        status: status(&state).await,
        recovery_key: Some(recovery.code),
    })
}

#[tauri::command]
pub async fn notes_lock_timeout_set(
    minutes: u32,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockStatus> {
    write_setting(&state.db, TIMEOUT_KEY, Some(&minutes.to_string())).await?;
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn notes_lock_unlock(
    password: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockStatus> {
    state.notes_lock.throttle().await;
    let Some(phc) = read_setting(&state, HASH_KEY).await else {
        // Another device wrapped the vault. The same password installs the local lock.
        if !crate::vault::has_row(&state).await? {
            return Ok(status(&state).await);
        }
        crate::vault::open_with_password(&state, &password).await?;
        if state.vault.open_key().is_none() {
            state.vault.lock();
            state.notes_lock.record_failure();
            return Err(AppError::other("Wrong password").into());
        }
        let phc = hash_password(&password)?;
        write_setting(&state.db, HASH_KEY, Some(&phc)).await?;
        state.notes_lock.open_session(&phc);
        publish_lock_meta(&state).await?;
        let status = status(&state).await;
        let _ = app.emit(EVENT_UNLOCKED, ());
        return Ok(status);
    };
    if !verify(&password, &phc) {
        state.notes_lock.record_failure();
        return Err(AppError::other("Wrong password").into());
    }
    crate::vault::open_with_password(&state, &password).await?;
    state.notes_lock.open_session(&phc);
    // Rows from before the hash travelled with them pick it up here.
    publish_lock_meta(&state).await?;
    let status = status(&state).await;
    let _ = app.emit(EVENT_UNLOCKED, ());
    Ok(status)
}

/// A peer changed the lock: the session was verified against an older hash, so it closes.
/// Also fires when the lock appears or disappears through sync.
async fn relock_if_hash_changed(app: &tauri::AppHandle, state: &AppState) {
    let current = read_setting(state, HASH_KEY).await;
    if current == state.notes_lock.session_hash() {
        return;
    }
    state.notes_lock.clear();
    state.vault.lock();
    let _ = app.emit(EVENT_LOCKED, ());
}

#[tauri::command]
pub async fn notes_lock_lock(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockStatus> {
    state.notes_lock.clear();
    state.vault.lock();
    let _ = app.emit(EVENT_LOCKED, ());
    Ok(status(&state).await)
}

/// Frontend reports user activity so the inactivity timer restarts.
#[tauri::command]
pub async fn notes_lock_touch(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    if state.notes_lock.last_activity.lock().unwrap().is_some() {
        state.notes_lock.touch();
    }
    Ok(())
}

/// Check the configured lock password. Fails when the lock is off or the password is wrong.
pub(crate) async fn require_lock_password(state: &AppState, password: &str) -> Result<(), AppError> {
    let Some(phc) = read_setting(state, HASH_KEY).await else {
        return Err(AppError::VaultLocked);
    };
    if !verify(password, &phc) {
        return Err(AppError::other("Wrong password"));
    }
    Ok(())
}

/// Background task: fires `notes://locked` once the inactivity timeout passes.
pub fn start_auto_lock(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // No PIN: the vault must be usable right away.
        let _ = open_default(&app.state::<AppState>()).await;
        loop {
            tokio::time::sleep(TICK).await;
            let state = app.state::<AppState>();
            let unlocked = state.notes_lock.last_activity.lock().unwrap().is_some();
            if !unlocked {
                continue;
            }
            let timeout = timeout_min(&state).await;
            if timeout > 0 && !state.notes_lock.is_unlocked(timeout) {
                state.notes_lock.clear();
                state.vault.lock();
                let _ = app.emit(EVENT_LOCKED, ());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_roundtrip() {
        let phc = hash_password("secret").unwrap();
        assert!(verify("secret", &phc));
        assert!(!verify("wrong", &phc));
    }
}
