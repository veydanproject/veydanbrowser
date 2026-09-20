// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Sync module (beta): notes replicated through an E2E-encrypted vault that
//! lives in a plain folder, an S3 bucket or a WebDAV collection.
//!
//! - `config` — settings + storage adapter factory
//! - `state`  — persistence of log positions and per-note state
//! - `notes`  — notes as sync entity (push / pull / merge)
//!
//! Nothing here runs unless `sync_enabled` is "1" and a vault was joined.

mod config;
mod notes;
mod state;

pub use config::SyncConfig;

use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use config::{build_storage, load_binding, load_config, VaultBinding};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use veydan_sync::{Engine, HlcClock, Probe, Vmk};

const TICK_SEC: u64 = 10;
const COMPACT_AFTER_CHUNKS: usize = 64;
const TOMBSTONE_TTL_MS: u64 = 180 * 24 * 60 * 60 * 1000;
pub const EVENT_STATUS: &str = "sync://status";

/// Guards against overlapping cycles (manual + scheduled).
#[derive(Default)]
pub struct SyncManager {
    running: AtomicBool,
}

struct RunningGuard<'a>(&'a AtomicBool);
impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Serialize)]
pub struct ConflictInfo {
    pub note_id: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct SyncStatus {
    pub enabled: bool,
    pub joined: bool,
    pub running: bool,
    pub vault_id: Option<String>,
    pub device_id: String,
    pub peers: usize,
    pub last_run: Option<String>,
    pub last_error: Option<String>,
    pub conflicts: Vec<ConflictInfo>,
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sync_get_config(state: tauri::State<'_, AppState>) -> CmdResult<SyncConfig> {
    Ok(load_config(&state.db).await)
}

#[tauri::command]
pub async fn sync_set_config(cfg: SyncConfig, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    config::save_config(&state.db, &cfg).await
}

/// What the configured storage holds: "empty" | "vault" | "foreign".
#[tauri::command]
pub async fn sync_probe(state: tauri::State<'_, AppState>) -> CmdResult<String> {
    let cfg = load_config(&state.db).await;
    let storage = build_storage(&cfg)?;
    Ok(match Engine::probe(storage.as_ref()).await.map_err(AppError::other)? {
        Probe::Empty => "empty",
        Probe::Vault(_) => "vault",
        Probe::Foreign => "foreign",
    }
    .to_string())
}

/// New vault in an empty storage. Refuses if anything is already there.
#[tauri::command]
pub async fn sync_create_vault(passphrase: String, app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    if passphrase.len() < 8 {
        return Err(AppError::other("passphrase must be at least 8 characters"));
    }
    let db = &state.db;
    let cfg = load_config(db).await;
    let device = config::device_id(db).await?;
    let storage = build_storage(&cfg)?;
    let (engine, vmk) = Engine::create(storage, &passphrase, &device).await.map_err(AppError::other)?;
    bind(db, engine.vault_id(), &vmk).await?;
    let _ = app.emit(EVENT_STATUS, ());
    status(&state).await
}

/// Join the vault found in the configured storage.
#[tauri::command]
pub async fn sync_join_vault(passphrase: String, app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let device = config::device_id(db).await?;
    let storage = build_storage(&cfg)?;
    let (engine, vmk) = Engine::open(storage, &passphrase, &device).await.map_err(AppError::other)?;
    bind(db, engine.vault_id(), &vmk).await?;
    let _ = app.emit(EVENT_STATUS, ());
    status(&state).await
}

async fn bind(db: &sqlx::Pool<sqlx::Sqlite>, vault_id: &str, vmk: &Vmk) -> CmdResult<()> {
    // A fresh binding starts from scratch: no peer heads, no note positions.
    state::clear_note_states(db).await?;
    config::clear_binding(db).await?;
    config::save_binding(db, &VaultBinding { vault_id: vault_id.to_string(), vmk_b64: vmk.to_base64() }).await?;
    config::set_setting(db, "sync_enabled", "1").await
}

/// Forget the vault on this device. Nothing in the storage is touched.
#[tauri::command]
pub async fn sync_leave(app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    let db = &state.db;
    state::clear_note_states(db).await?;
    config::clear_binding(db).await?;
    config::set_setting(db, "sync_enabled", "0").await?;
    let _ = app.emit(EVENT_STATUS, ());
    status(&state).await
}

#[tauri::command]
pub async fn sync_change_passphrase(old: String, new: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    if new.len() < 8 {
        return Err(AppError::other("passphrase must be at least 8 characters"));
    }
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db).await.ok_or_else(|| AppError::other("no vault joined"))?;
    let vmk = Vmk::from_base64(&binding.vmk_b64).map_err(AppError::other)?;
    let device = config::device_id(db).await?;
    let engine = Engine::with_key(build_storage(&cfg)?, &vmk, &binding.vault_id, &device);
    engine.change_passphrase(&vmk, &old, &new).await.map_err(AppError::other)
}

#[tauri::command]
pub async fn sync_status(state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    status(&state).await
}

/// One full cycle right now. Errors are also recorded as `last_error`.
#[tauri::command]
pub async fn sync_run_now(app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    run_cycle(&app).await?;
    status(&state).await
}

async fn status(state: &AppState) -> CmdResult<SyncStatus> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db).await;
    let (local, _) = state::load_local_state(db).await?;
    let conflicts = state::conflicts(db)
        .await?
        .into_iter()
        .map(|(note_id, title)| ConflictInfo { note_id, title })
        .collect();
    Ok(SyncStatus {
        enabled: cfg.enabled,
        joined: binding.is_some(),
        running: state.sync.running.load(Ordering::SeqCst),
        vault_id: binding.map(|b| b.vault_id),
        device_id: config::device_id(db).await?,
        peers: local.peers.len(),
        last_run: config::get_setting(db, "sync_last_run").await,
        last_error: config::get_setting(db, "sync_last_error").await.filter(|s| !s.is_empty()),
        conflicts,
    })
}

// ── Cycle ────────────────────────────────────────────────────────────────────

/// push local changes -> pull peers -> apply/merge -> compact -> persist.
pub async fn run_cycle(app: &AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.sync.running.swap(true, Ordering::SeqCst) {
        return Err(AppError::other("sync already running"));
    }
    let _guard = RunningGuard(&state.sync.running);
    let db = &state.db;

    let result = cycle_inner(app, &state).await;
    let now = Utc::now().to_rfc3339();
    match &result {
        Ok(warnings) => {
            config::set_setting(db, "sync_last_run", &now).await?;
            config::set_setting(db, "sync_last_error", &warnings.join("; ")).await?;
        }
        Err(e) => {
            config::set_setting(db, "sync_last_error", &e.to_string()).await?;
        }
    }
    let _ = app.emit(EVENT_STATUS, ());
    result.map(|_| ())
}

/// Returns non-fatal warnings (per-peer integrity problems).
async fn cycle_inner(app: &AppHandle, state: &AppState) -> CmdResult<Vec<String>> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db).await.ok_or_else(|| AppError::other("no vault joined"))?;
    let vmk = Vmk::from_base64(&binding.vmk_b64).map_err(AppError::other)?;
    let device = config::device_id(db).await?;
    let engine = Engine::with_key(build_storage(&cfg)?, &vmk, &binding.vault_id, &device);
    engine.verify_manifest().await.map_err(AppError::other)?;

    let (mut local, last_hlc) = state::load_local_state(db).await?;
    let mut clock = HlcClock::new(device.clone(), last_hlc.as_ref());

    // Push
    let changes = notes::collect_local_changes(&engine, state, &mut clock).await?;
    if !changes.ops.is_empty() {
        engine.push(&mut local, changes.ops).await.map_err(AppError::other)?;
        state::save_own_state(db, &local, &clock.last()).await?;
        for st in &changes.states {
            state::save_note_state(db, st).await?;
        }
    }

    // Pull
    let pulled = engine.pull(&mut local).await.map_err(AppError::other)?;
    for op in &pulled.ops {
        clock.observe(&op.hlc);
    }
    let outcome = notes::apply_remote(&engine, app, pulled.ops, &mut clock, &mut local).await?;
    state::save_own_state(db, &local, &clock.last()).await?;
    let mut warnings: Vec<String> = pulled.errors.into_iter().map(|(peer, e)| format!("{peer}: {e}")).collect();
    match &outcome.retry {
        None => state::save_peer_heads(db, &local).await?,
        Some(reason) => warnings.push(format!("retry next cycle: {reason}")),
    }
    notes::finish_apply(app, &outcome).await?;

    // Compact own log once it grows past the threshold.
    if engine.own_chunk_count().await.map_err(AppError::other)? > COMPACT_AFTER_CHUNKS {
        let now_ms = Utc::now().timestamp_millis().max(0) as u64;
        engine.compact(&local, now_ms, TOMBSTONE_TTL_MS).await.map_err(AppError::other)?;
    }

    Ok(warnings)
}

// ── Scheduler ────────────────────────────────────────────────────────────────

/// Background ticker. Does nothing while sync is disabled or no vault is joined.
pub fn start_sync_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(TICK_SEC));
        loop {
            ticker.tick().await;
            let state = app.state::<AppState>();
            let cfg = load_config(&state.db).await;
            if !cfg.enabled || load_binding(&state.db).await.is_none() {
                continue;
            }
            if state.sync.running.load(Ordering::SeqCst) {
                continue;
            }
            let due = match config::get_setting(&state.db, "sync_last_run").await {
                None => true,
                Some(last) => chrono::DateTime::parse_from_rfc3339(&last)
                    .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds() >= cfg.interval_sec as i64)
                    .unwrap_or(true),
            };
            if due {
                if let Err(e) = run_cycle(&app).await {
                    eprintln!("sync: {e}");
                }
            }
        }
    });
}