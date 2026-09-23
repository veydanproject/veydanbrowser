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

/// Last user activity while unlocked; `None` means locked.
#[derive(Default)]
pub struct NotesLock {
    last_activity: Mutex<Option<Instant>>,
}

impl NotesLock {
    fn is_unlocked(&self, timeout_min: u32) -> bool {
        match *self.last_activity.lock().unwrap() {
            None => false,
            Some(_) if timeout_min == 0 => true,
            Some(t) => t.elapsed() < Duration::from_secs(u64::from(timeout_min) * 60),
        }
    }

    fn touch(&self) {
        *self.last_activity.lock().unwrap() = Some(Instant::now());
    }

    fn clear(&self) {
        *self.last_activity.lock().unwrap() = None;
    }
}

#[derive(Debug, Serialize)]
pub struct LockStatus {
    pub enabled: bool,
    pub locked: bool,
    pub timeout_min: u32,
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

fn hash_password(password: &str) -> Result<String, AppError> {
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
    LockStatus {
        enabled,
        locked,
        timeout_min,
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
            write_setting(&state, HASH_KEY, Some(&hash_password(&p)?)).await?;
            state.notes_lock.touch();
        }
        None => {
            write_setting(&state, HASH_KEY, None).await?;
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
    let Some(phc) = read_setting(&state, HASH_KEY).await else {
        return Ok(status(&state).await);
    };
    if !verify(&password, &phc) {
        return Err(AppError::other("Wrong password").into());
    }
    state.notes_lock.touch();
    let _ = app.emit(EVENT_UNLOCKED, ());
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn notes_lock_lock(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<LockStatus> {
    state.notes_lock.clear();
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
