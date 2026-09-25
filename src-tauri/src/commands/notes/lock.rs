// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! App lock for the notes UI: Argon2 password hash in `app_settings`,
//! unlock state in memory, auto-lock after inactivity.

use crate::error::{AppError, CmdResult};
use crate::AppState;
use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

const HASH_KEY: &str = "notes_lock_hash";
const TIMEOUT_KEY: &str = "notes_lock_timeout_min";
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

    fn clear(&self) {
        *self.last_activity.lock().unwrap() = None;
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
}

async fn read_setting(state: &AppState, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

async fn write_setting(state: &AppState, key: &str, value: Option<&str>) -> Result<(), AppError> {
    match value {
        Some(v) => sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(v)
        .execute(&state.db)
        .await
        .map(|_| ())
        .map_err(AppError::db),
        None => sqlx::query("DELETE FROM app_settings WHERE key = ?")
            .bind(key)
            .execute(&state.db)
            .await
            .map(|_| ())
            .map_err(AppError::db),
    }
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
    LockStatus {
        enabled,
        locked,
        timeout_min,
        vault,
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
/// `current` is required whenever a password is already set.
#[tauri::command]
pub async fn notes_lock_set(
    password: Option<String>,
    current: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockStatus> {
    if let Some(existing) = read_setting(&state, HASH_KEY).await {
        let ok = current
            .as_deref()
            .map(|c| verify(c, &existing))
            .unwrap_or(false);
        if !ok {
            return Err(AppError::other("Current password is incorrect").into());
        }
    }
    let password = password
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    match password {
        Some(p) => {
            if p.chars().count() < 4 {
                return Err(AppError::other("Password must be at least 4 characters").into());
            }
            let phc = hash_password(&p)?;
            let mut tx = state.db.begin().await.map_err(AppError::db)?;
            let update = crate::vault::rewrap_in(&mut tx, current.as_deref(), &p).await?;
            sqlx::query(
                "INSERT INTO app_settings (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(HASH_KEY)
            .bind(&phc)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
            tx.commit().await.map_err(AppError::db)?;
            state.vault.apply(update);
            state.notes_lock.touch();
        }
        None => {
            if crate::vault::entry_count(&state.db).await? > 0 {
                return Err(AppError::VaultHasEntries);
            }
            let mut tx = state.db.begin().await.map_err(AppError::db)?;
            sqlx::query("DELETE FROM password_vault")
                .execute(&mut *tx)
                .await
                .map_err(AppError::db)?;
            sqlx::query("DELETE FROM app_settings WHERE key = ?")
                .bind(HASH_KEY)
                .execute(&mut *tx)
                .await
                .map_err(AppError::db)?;
            tx.commit().await.map_err(AppError::db)?;
            state.vault.lock();
            state.notes_lock.clear();
        }
    }
    let _ = app.emit(EVENT_UNLOCKED, ());
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn notes_lock_timeout_set(
    minutes: u32,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockStatus> {
    write_setting(&state, TIMEOUT_KEY, Some(&minutes.to_string())).await?;
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
        write_setting(&state, HASH_KEY, Some(&hash_password(&password)?)).await?;
        state.notes_lock.touch();
        let status = status(&state).await;
        let _ = app.emit(EVENT_UNLOCKED, ());
        return Ok(status);
    };
    if !verify(&password, &phc) {
        state.notes_lock.record_failure();
        return Err(AppError::other("Wrong password").into());
    }
    crate::vault::open_with_password(&state, &password).await?;
    state.notes_lock.touch();
    let status = status(&state).await;
    let _ = app.emit(EVENT_UNLOCKED, ());
    Ok(status)
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
