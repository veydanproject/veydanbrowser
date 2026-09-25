// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Sync module (beta): notes replicated through an E2E-encrypted vault that
//! lives in a plain folder, an S3 bucket or a WebDAV collection.
//!
//! - `config`      — settings + storage adapter factory
//! - `state`       — persistence of log positions and per-entity state
//! - `notes`       — note files plus tags, folders, smart views and pin/archive
//! - `attachments` — note attachments as sync entity (push / pull, LWW)
//! - `rows`        — app table rows (profiles, proxies, ssh, ...) as sync entities (LWW)
//! - `profile_files` — Firefox profile directories: lease + file snapshots
//!
//! Nothing here runs unless `sync_enabled` is "1" and a vault was joined.

pub(crate) mod attachments;
mod config;
mod fs_hash;
mod gc;
mod notes;
#[cfg(desktop)]
mod profile_files;
mod rows;
mod state;

pub use config::{check_install_marker, SyncConfig};
pub(crate) use rows::EVENT_CHANGED;

use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use config::{build_storage, load_binding, load_config, VaultBinding};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use veydan_sync::{CancelFlag, Engine, HlcClock, Op, Probe, Vmk};

const TICK_SEC: u64 = 1;
/// Minimum gap between cycles started by `sync_trigger` (app resume, screen open).
const MIN_TRIGGER_GAP_SEC: i64 = 15;
const COMPACT_AFTER_CHUNKS: usize = 64;
const TOMBSTONE_TTL_MS: u64 = 180 * 24 * 60 * 60 * 1000;
pub const EVENT_STATUS: &str = "sync://status";
pub const EVENT_PROGRESS: &str = "sync://progress";
pub const EVENT_DEBUG: &str = "sync://debug";
const DEBUG_CAP: usize = 400;
const DEBUG_SETTING: &str = "sync_debug_log";

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    pub phase: String,
    pub percent: u32,
    pub current: u32,
    pub total: u32,
    pub detail: String,
}

pub(crate) fn emit_progress(
    app: &AppHandle,
    phase: &str,
    percent: u32,
    current: u32,
    total: u32,
    detail: &str,
) {
    let _ = app.emit(
        EVENT_PROGRESS,
        SyncProgress {
            phase: phase.to_string(),
            percent: percent.min(100),
            current,
            total,
            detail: detail.to_string(),
        },
    );
}

pub(crate) fn progress_pct(lo: u32, hi: u32, current: u32, total: u32) -> u32 {
    if total == 0 || hi <= lo {
        return lo;
    }
    lo + (hi - lo).saturating_mul(current) / total
}

/// One line of the developer sync log.
#[derive(Debug, Clone, Serialize)]
pub struct SyncDebugEntry {
    pub seq: u64,
    pub at: String,
    pub cycle: u64,
    pub source: String,
    pub level: String,
    pub step: String,
    pub message: String,
}

/// Toggle plus the in-memory ring the developer page reads.
#[derive(Debug, Clone, Serialize)]
pub struct SyncDebugState {
    pub enabled: bool,
    pub entries: Vec<SyncDebugEntry>,
}

#[derive(Debug, Default)]
struct DebugLog {
    seq: u64,
    cycle: u64,
    source: String,
    entries: VecDeque<SyncDebugEntry>,
}

/// Guards against overlapping cycles (manual + scheduled).
#[derive(Default)]
pub struct SyncManager {
    running: AtomicBool,
    /// File hashes keyed by path, valid while mtime and size match.
    file_hashes: fs_hash::HashCache,
    /// Cancel flags of large-file transfers in flight, keyed by `note_id/name`.
    transfers: Mutex<HashMap<String, CancelFlag>>,
    debug_enabled: AtomicBool,
    /// True after the setting has been copied into `debug_enabled`.
    debug_loaded: Mutex<bool>,
    debug: Mutex<DebugLog>,
    /// Scheduler already reported "already running" for the current overlap.
    sched_skip: AtomicBool,
}

impl SyncManager {
    /// Register a transfer; the returned flag is what the UI can cancel.
    pub(crate) fn begin_transfer(&self, key: &str) -> CancelFlag {
        let flag = CancelFlag::default();
        self.transfers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), flag.clone());
        flag
    }

    pub(crate) fn end_transfer(&self, key: &str) {
        self.transfers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
    }

    /// Ask a running transfer to stop; returns whether one was in flight.
    pub(crate) fn cancel_transfer(&self, key: &str) -> bool {
        match self
            .transfers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
        {
            Some(flag) => {
                flag.cancel();
                true
            }
            None => false,
        }
    }
}

/// Copy `sync_debug_log` into memory once. Later toggles update the flag directly.
async fn ensure_debug_loaded(state: &AppState) {
    {
        let loaded = state
            .sync
            .debug_loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *loaded {
            return;
        }
    }
    let on = config::get_setting(&state.db, DEBUG_SETTING)
        .await
        .as_deref()
        == Some("1");
    let mut loaded = state
        .sync
        .debug_loaded
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if *loaded {
        return;
    }
    state.sync.debug_enabled.store(on, Ordering::Relaxed);
    *loaded = true;
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Counts of pulled ops by entity type. Empty input is `0 ops`.
fn op_summary(ops: &[veydan_sync::Op]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for op in ops {
        *counts.entry(op.entity_type.as_str()).or_default() += 1;
    }
    if counts.is_empty() {
        return "0 ops".into();
    }
    counts
        .into_iter()
        .map(|(kind, n)| format!("{kind} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn debug_on(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .sync
        .debug_enabled
        .load(Ordering::Relaxed)
}

/// Remember which cycle the following `trace` lines belong to.
fn begin_debug_cycle(app: &AppHandle, source: &str) {
    if !debug_on(app) {
        return;
    }
    let state = app.state::<AppState>();
    let mut log = state.sync.debug.lock().unwrap_or_else(|e| e.into_inner());
    log.cycle += 1;
    log.source = source.to_string();
}

/// Append a line for the current cycle. No-op while the developer log is off.
fn trace(app: &AppHandle, level: &str, step: &str, message: &str) {
    if !debug_on(app) {
        return;
    }
    let state = app.state::<AppState>();
    let (cycle, source) = {
        let log = state.sync.debug.lock().unwrap_or_else(|e| e.into_inner());
        (log.cycle, log.source.clone())
    };
    push_debug(app, level, step, &source, cycle, message);
}

/// Append a line that is not part of a cycle (a skipped start).
fn trace_skip(app: &AppHandle, source: &str, step: &str, message: &str) {
    push_debug(app, "skip", step, source, 0, message);
}

fn trace_retry(app: &AppHandle, step: &str, retry: &Option<String>) {
    if let Some(reason) = retry {
        trace(app, "retry", step, reason);
    }
}

fn push_debug(app: &AppHandle, level: &str, step: &str, source: &str, cycle: u64, message: &str) {
    if !debug_on(app) {
        return;
    }
    let state = app.state::<AppState>();
    let entry = {
        let mut log = state.sync.debug.lock().unwrap_or_else(|e| e.into_inner());
        log.seq += 1;
        let entry = SyncDebugEntry {
            seq: log.seq,
            at: Utc::now().to_rfc3339(),
            cycle,
            source: source.to_string(),
            level: level.to_string(),
            step: step.to_string(),
            message: message.to_string(),
        };
        if log.entries.len() >= DEBUG_CAP {
            log.entries.pop_front();
        }
        log.entries.push_back(entry.clone());
        entry
    };
    let _ = app.emit(EVENT_DEBUG, entry);
}

pub struct RunningGuard<'a>(&'a AtomicBool);
impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl SyncManager {
    /// Hold the cycle slot so a bulk data rewrite cannot interleave with a sync.
    /// Waits up to `max_wait` for a running cycle to finish.
    pub async fn pause(&self, max_wait: std::time::Duration) -> Option<RunningGuard<'_>> {
        let deadline = std::time::Instant::now() + max_wait;
        loop {
            if !self.running.swap(true, Ordering::SeqCst) {
                return Some(RunningGuard(&self.running));
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConflictInfo {
    pub note_id: String,
    pub title: String,
}

/// Profile currently leased by some device.
#[derive(Debug, Serialize)]
pub struct LeaseInfo {
    pub profile_id: String,
    pub device_id: String,
    pub device_name: String,
    pub own: bool,
}

#[derive(Debug, Serialize)]
pub struct NoteSyncInfo {
    /// The vault knows this note.
    pub tracked: bool,
    /// Local edits not yet pushed.
    pub pending: bool,
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
    pub last_warning: Option<String>,
    pub conflicts: Vec<ConflictInfo>,
    /// Profiles whose files diverged (id, name).
    pub profile_conflicts: Vec<ConflictInfo>,
    pub profile_leases: Vec<LeaseInfo>,
    /// Remote ops received in the last cycle.
    pub last_applied: Option<u64>,
    /// Devices seen in storage on the last background cycle.
    pub storage_devices: Vec<StorageDevice>,
    pub gc_last: Option<String>,
    pub blobs_total: Option<u64>,
    pub blobs_removed_last_gc: Option<u64>,
    /// Large-file v2 objects (manifests + chunks) after the last GC.
    pub lf_total: Option<u64>,
    pub lf_removed_last_gc: Option<u64>,
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
    Ok(match Engine::probe(storage.as_ref())
        .await
        .map_err(AppError::other)?
    {
        Probe::Empty => "empty",
        Probe::Vault(_) => "vault",
        Probe::Foreign => "foreign",
    }
    .to_string())
}

/// New vault in an empty storage. Refuses if anything is already there.
#[tauri::command]
pub async fn sync_create_vault(
    passphrase: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    if passphrase.len() < 8 {
        return Err(AppError::other("passphrase must be at least 8 characters"));
    }
    let db = &state.db;
    let cfg = load_config(db).await;
    let device = config::device_id(db).await?;
    let storage = build_storage(&cfg)?;
    let (engine, vmk) = Engine::create(storage, &passphrase, &device)
        .await
        .map_err(AppError::other)?;
    finish_join(&app, db, engine.vault_id(), &vmk).await?;
    status(&state).await
}

/// Join the vault found in the configured storage.
#[tauri::command]
pub async fn sync_join_vault(
    passphrase: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let device = config::device_id(db).await?;
    let storage = build_storage(&cfg)?;
    let (engine, vmk) = Engine::open(storage, &passphrase, &device)
        .await
        .map_err(AppError::other)?;
    finish_join(&app, db, engine.vault_id(), &vmk).await?;
    status(&state).await
}

/// Save the binding and start a cycle without blocking the caller.
async fn finish_join(
    app: &AppHandle,
    db: &sqlx::Pool<sqlx::Sqlite>,
    vault_id: &str,
    vmk: &Vmk,
) -> CmdResult<()> {
    bind(db, vault_id, vmk).await?;
    let _ = app.emit(EVENT_STATUS, ());
    trigger_cycle(app, "join");
    Ok(())
}

/// One device found under `devices/` on the last cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDevice {
    pub id: String,
    pub name: String,
    pub own: bool,
}

async fn cached_devices(
    db: &sqlx::Pool<sqlx::Sqlite>,
    own_id: &str,
    own_name: &str,
) -> Vec<StorageDevice> {
    let raw = config::get_setting(db, "sync_devices")
        .await
        .unwrap_or_default();
    let mut list: Vec<StorageDevice> = serde_json::from_str(&raw).unwrap_or_default();
    for d in &mut list {
        d.own = d.id == own_id;
        if d.own && d.name.is_empty() {
            d.name = own_name.to_string();
        }
    }
    list
}

async fn bind(db: &sqlx::Pool<sqlx::Sqlite>, vault_id: &str, vmk: &Vmk) -> CmdResult<()> {
    // A fresh binding starts from scratch: no peer heads, no entity positions.
    state::clear_all_states(db).await?;
    config::clear_binding(db).await?;
    config::save_binding(
        db,
        &VaultBinding {
            vault_id: vault_id.to_string(),
            vmk_b64: vmk.to_base64(),
        },
    )
    .await?;
    config::set_setting(db, "sync_enabled", "1").await
}

/// Forget the vault on this device. Nothing in the storage is touched.
#[tauri::command]
pub async fn sync_leave(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    let db = &state.db;
    state::clear_all_states(db).await?;
    config::clear_binding(db).await?;
    config::set_setting(db, "sync_enabled", "0").await?;
    let _ = app.emit(EVENT_STATUS, ());
    status(&state).await
}

#[tauri::command]
pub async fn sync_change_passphrase(
    old: String,
    new: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    if new.len() < 8 {
        return Err(AppError::other("passphrase must be at least 8 characters"));
    }
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db)
        .await
        .ok_or_else(|| AppError::other("no vault joined"))?;
    let vmk = Vmk::from_base64(&binding.vmk_b64).map_err(AppError::other)?;
    let device = config::device_id(db).await?;
    let engine = Engine::with_key(build_storage(&cfg)?, &vmk, &binding.vault_id, &device);
    engine
        .change_passphrase(&vmk, &old, &new)
        .await
        .map_err(AppError::other)
}

#[tauri::command]
pub async fn sync_status(state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    status(&state).await
}

/// Developer log: current toggle and the lines kept in memory.
#[tauri::command]
pub async fn sync_debug_get(state: tauri::State<'_, AppState>) -> CmdResult<SyncDebugState> {
    ensure_debug_loaded(&state).await;
    let enabled = state.sync.debug_enabled.load(Ordering::Relaxed);
    let entries = state
        .sync
        .debug
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries
        .iter()
        .cloned()
        .collect();
    Ok(SyncDebugState { enabled, entries })
}

/// Turn the developer sync log on or off. The choice survives a restart; the lines do not.
#[tauri::command]
pub async fn sync_debug_set(enabled: bool, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    config::set_setting(&state.db, DEBUG_SETTING, if enabled { "1" } else { "0" }).await?;
    let mut loaded = state
        .sync
        .debug_loaded
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    state.sync.debug_enabled.store(enabled, Ordering::Relaxed);
    *loaded = true;
    Ok(())
}

/// Drop the in-memory lines. The toggle stays as it is.
#[tauri::command]
pub async fn sync_debug_clear(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state
        .sync
        .debug
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries
        .clear();
    Ok(())
}

/// One full cycle right now. Errors are also recorded as `last_error`.
#[tauri::command]
pub async fn sync_run_now(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    run_cycle(&app, "manual").await?;
    status(&state).await
}

/// Structured merge blocks for the conflict UI plus a token for `sync_conflict_resolve`.
#[tauri::command]
pub async fn sync_conflict_get(
    note_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<notes::ConflictView> {
    notes::conflict_merge(&state, &note_id).await
}

/// Store the resolved text and push it right away. `token` must match `sync_conflict_get`.
#[tauri::command]
pub async fn sync_conflict_resolve(
    note_id: String,
    token: String,
    content: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    notes::resolve_conflict(&state, &note_id, &token, content).await?;
    let _ = app.emit(EVENT_STATUS, ());
    if !state.sync.running.load(Ordering::SeqCst) {
        run_cycle(&app, "conflict").await?;
    } else {
        trace_skip(&app, "conflict", "trigger", "already running");
    }
    status(&state).await
}

/// Sync position of one note: `tracked` when the vault knows it, `pending` when
/// the local file differs from the last pushed/applied version.
#[tauri::command]
pub async fn note_sync_info(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteSyncInfo> {
    let Some(st) = state::load_note_state(&state.db, &id).await? else {
        return Ok(NoteSyncInfo {
            tracked: false,
            pending: true,
        });
    };
    let file_path: Option<String> =
        sqlx::query_scalar("SELECT file_path FROM notes WHERE id = ? AND deleted = 0")
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?;
    let hash = file_path
        .and_then(|p| {
            std::fs::read(crate::commands::notes::resolve_note_abs_path(
                &state.app_data_dir,
                &p,
            ))
            .ok()
        })
        .map(|raw| veydan_sync::sha256_hex(&raw))
        .unwrap_or_default();
    Ok(NoteSyncInfo {
        tracked: true,
        pending: hash != st.synced_hash,
    })
}

/// Stop a large attachment transfer in flight. The file stays dirty and is
/// retried on the next cycle unless it was removed.
#[tauri::command]
pub async fn sync_attachment_cancel(
    note_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<bool> {
    Ok(state
        .sync
        .cancel_transfer(&attachments::transfer_key(&note_id, &name)))
}

/// Start a cycle in the background (app resumed, screen opened). No-op when one
/// is running, sync is off or the last cycle started less than `MIN_TRIGGER_GAP_SEC` ago.
#[tauri::command]
pub async fn sync_trigger(app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    if load_binding(&state.db).await.is_none() {
        return Ok(());
    }
    let due = match config::get_setting(&state.db, "sync_last_started").await {
        None => true,
        Some(last) => chrono::DateTime::parse_from_rfc3339(&last)
            .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds() >= MIN_TRIGGER_GAP_SEC)
            .unwrap_or(true),
    };
    if due {
        trigger_cycle(&app, "trigger");
    } else {
        trace_skip(
            &app,
            "trigger",
            "trigger",
            "last cycle started less than 15s ago",
        );
    }
    Ok(())
}

/// Conflict choice for profile files: replace local files with the remote snapshot.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_profile_files_take_remote(
    profile_id: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    let engine = open_engine(&state).await?;
    profile_files::take_remote(&engine, &state, &app, &profile_id).await?;
    let _ = app.emit(EVENT_STATUS, ());
    status(&state).await
}

/// Conflict choice for profile files: keep local files and publish them.
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_profile_files_push_mine(
    profile_id: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    profile_files::push_mine(&state, &profile_id).await?;
    trigger_cycle(&app, "profile-push");
    status(&state).await
}

async fn status(state: &AppState) -> CmdResult<SyncStatus> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db).await;
    let (local, _) = state::load_local_state(db).await?;
    let device_id = config::device_id(db).await?;
    let conflicts = state::conflicts(db)
        .await?
        .into_iter()
        .map(|(note_id, title)| ConflictInfo { note_id, title })
        .collect();
    let profile_conflicts = state::profile_conflicts(db)
        .await?
        .into_iter()
        .map(|(note_id, title)| ConflictInfo { note_id, title })
        .collect();
    let profile_leases = state::profile_leases(db)
        .await?
        .into_iter()
        .map(|(profile_id, lease_device, device_name)| LeaseInfo {
            profile_id,
            own: lease_device == device_id,
            device_id: lease_device,
            device_name,
        })
        .collect();
    Ok(SyncStatus {
        enabled: cfg.enabled,
        joined: binding.is_some(),
        running: state.sync.running.load(Ordering::SeqCst),
        vault_id: binding.map(|b| b.vault_id),
        device_id: device_id.clone(),
        peers: local.peers.len(),
        storage_devices: cached_devices(db, &device_id, &cfg.device_name).await,
        last_run: config::get_setting(db, "sync_last_run").await,
        last_error: config::get_setting(db, "sync_last_error")
            .await
            .filter(|s| !s.is_empty()),
        last_warning: config::get_setting(db, "sync_last_warning")
            .await
            .filter(|s| !s.is_empty()),
        conflicts,
        profile_conflicts,
        profile_leases,
        last_applied: config::get_setting(db, "sync_last_applied")
            .await
            .and_then(|v| v.parse().ok()),
        gc_last: config::get_setting(db, gc::LAST_RUN_KEY)
            .await
            .and_then(|v| v.parse::<i64>().ok())
            .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
            .map(|t| t.to_rfc3339()),
        blobs_total: config::get_setting(db, gc::BLOBS_TOTAL_KEY)
            .await
            .and_then(|v| v.parse().ok()),
        blobs_removed_last_gc: config::get_setting(db, gc::REMOVED_KEY)
            .await
            .and_then(|v| v.parse().ok()),
        lf_total: config::get_setting(db, gc::LF_TOTAL_KEY)
            .await
            .and_then(|v| v.parse().ok()),
        lf_removed_last_gc: config::get_setting(db, gc::LF_REMOVED_KEY)
            .await
            .and_then(|v| v.parse().ok()),
    })
}

/// Engine for the joined vault.
pub(crate) async fn open_engine(state: &AppState) -> CmdResult<Engine> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db)
        .await
        .ok_or_else(|| AppError::other("no vault joined"))?;
    let vmk = Vmk::from_base64(&binding.vmk_b64).map_err(AppError::other)?;
    let device = config::device_id(db).await?;
    Ok(Engine::with_key(
        build_storage(&cfg)?,
        &vmk,
        &binding.vault_id,
        &device,
    ))
}

/// Vault joined and background sync is on.
#[cfg(desktop)]
async fn vault_active(state: &AppState) -> bool {
    let cfg = load_config(&state.db).await;
    cfg.enabled && load_binding(&state.db).await.is_some()
}

/// Sync is on, a vault is joined and profile files are included.
#[cfg(desktop)]
async fn profile_files_active(state: &AppState) -> bool {
    load_config(&state.db).await.profile_files && vault_active(state).await
}

/// Run a cycle in the background unless one is already running.
pub fn trigger_cycle(app: &AppHandle, source: &str) {
    let app = app.clone();
    let source = source.to_string();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if state.sync.running.load(Ordering::SeqCst) {
            trace_skip(&app, &source, "trigger", "already running");
            return;
        }
        if !load_config(&state.db).await.enabled {
            trace_skip(&app, &source, "trigger", "sync disabled");
            return;
        }
        if let Err(e) = run_cycle(&app, &source).await {
            eprintln!("sync: {e}");
        }
    });
}

// ── Profile launch hooks ─────────────────────────────────────────────────────

#[cfg(desktop)]
pub const ERR_PROFILE_IN_USE: &str = "profile_in_use";

/// Before the browser starts: refuse a profile leased elsewhere (unless forced),
/// apply a snapshot that waited, take the lease.
#[cfg(desktop)]
pub async fn before_profile_launch(
    app: &AppHandle,
    profile_id: &str,
    force: bool,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if !vault_active(&state).await {
        return Ok(());
    }
    if !force {
        if let Some((_, name)) = profile_files::foreign_lease(&state, profile_id).await? {
            return Err(AppError::other(format!("{ERR_PROFILE_IN_USE}:{name}")));
        }
        if profile_files_active(&state).await
            && profile_files::has_pending(&state, profile_id).await?
        {
            let engine = open_engine(&state).await?;
            profile_files::apply_pending(&engine, &state, app, profile_id).await?;
        }
    }
    profile_files::acquire_lease(&state, profile_id).await?;
    trigger_cycle(app, "profile-launch");
    Ok(())
}

/// After the browser exited: mark files dirty, release the lease, sync soon.
#[cfg(desktop)]
pub async fn on_profile_stopped(app: &AppHandle, profile_id: &str) {
    let state = app.state::<AppState>();
    if !vault_active(&state).await {
        return;
    }
    if let Err(e) = profile_files::on_profile_stopped(&state, profile_id).await {
        eprintln!("sync: {e}");
    }
    trigger_cycle(app, "profile-stop");
}

// ── Cycle ────────────────────────────────────────────────────────────────────

/// push local changes -> pull peers -> apply/merge -> compact -> persist.
pub async fn run_cycle(app: &AppHandle, source: &str) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.sync.running.swap(true, Ordering::SeqCst) {
        trace_skip(app, source, "start", "already running");
        return Err(AppError::other("sync already running"));
    }
    let _guard = RunningGuard(&state.sync.running);
    begin_debug_cycle(app, source);
    let db = &state.db;
    if debug_on(app) {
        let interval = config::get_setting(db, "sync_interval_sec")
            .await
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(config::DEFAULT_INTERVAL_SEC as i64);
        match (
            config::device_id(db).await,
            state::load_local_state(db).await,
        ) {
            (Ok(device), Ok((local, _))) => trace(
                app,
                "info",
                "start",
                &format!(
                    "interval {interval}s device {} peers {} seq {}",
                    short_id(&device),
                    local.peers.len(),
                    local.own_seq
                ),
            ),
            _ => trace(app, "info", "start", "cycle started"),
        }
    }
    config::set_setting(db, "sync_last_started", &Utc::now().to_rfc3339()).await?;
    config::set_setting(db, "sync_last_error", "").await?;
    // Let the UI show the running state for scheduled cycles too.
    let _ = app.emit(EVENT_STATUS, ());

    emit_progress(app, "collect", 0, 0, 0, "");
    let result = cycle_inner(app, &state).await;
    let now = Utc::now().to_rfc3339();
    match &result {
        Ok(warnings) => {
            config::set_setting(db, "sync_last_run", &now).await?;
            config::set_setting(db, "sync_last_error", "").await?;
            config::set_setting(db, "sync_last_warning", &warnings.join("; ")).await?;
            emit_progress(app, "done", 100, 0, 0, "");
            trace(app, "info", "done", "cycle finished");
        }
        Err(e) => {
            config::set_setting(db, "sync_last_error", &e.to_string()).await?;
            emit_progress(app, "error", 0, 0, 0, &e.to_string());
            trace(app, "error", "done", &e.to_string());
        }
    }
    let _ = app.emit(EVENT_STATUS, ());
    result.map(|_| ())
}

/// Blob missing for an op older than the GC grace: it was collected, not delayed.
pub(super) fn blob_gone(op: &Op, now_ms: u64) -> bool {
    now_ms.saturating_sub(op.hlc.wall_ms) >= gc::GRACE_MS as u64
}

/// FOREIGN KEY failure in an apply step: record it as "retry next cycle" and go on.
fn fk_retry<T: Default>(
    step: &str,
    result: CmdResult<T>,
    deferred: &mut Option<String>,
) -> CmdResult<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) if rows::is_fk_error(&e) => {
            deferred.get_or_insert_with(|| format!("{step}: {e}"));
            Ok(T::default())
        }
        Err(e) => Err(e),
    }
}

/// A collision means another install writes to our log (restored backup, cloned
/// DB): move to a fresh device log; the next cycle re-collects and pushes there.
async fn push_error(db: &sqlx::SqlitePool, e: veydan_sync::SyncError) -> AppError {
    if let veydan_sync::SyncError::OwnLogCollision(_) = e {
        if let Err(re) = config::rotate_device_id(db).await {
            return re;
        }
        let _ = config::set_setting(
            db,
            "sync_last_warning",
            "device id rotated after log collision",
        )
        .await;
        return AppError::other(
            "own log collision: device id rotated, changes are pushed on the next cycle",
        );
    }
    AppError::other(e)
}

/// Returns non-fatal warnings (per-peer integrity problems).
async fn cycle_inner(app: &AppHandle, state: &AppState) -> CmdResult<Vec<String>> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let engine = open_engine(state).await?;
    engine.verify_manifest().await.map_err(AppError::other)?;

    let (mut local, last_hlc) = state::load_local_state(db).await?;
    let mut clock = HlcClock::new(engine.device_id().to_string(), last_hlc.as_ref());

    // Data first so a stuck profile upload cannot block notes.
    emit_progress(app, "collect", 8, 0, 0, "");
    let note_rows = rows::collect_local_changes(state, &mut clock, rows::RowScope::Notes).await?;
    emit_progress(app, "collect", 12, 0, 0, "");
    let attachments = attachments::collect_local_changes(&engine, app, state, &mut clock).await?;
    emit_progress(app, "collect", 16, 0, 0, "");
    let changes = notes::collect_local_changes(&engine, state, &mut clock).await?;
    emit_progress(app, "collect", 18, 0, 0, "");
    let table_rows = rows::collect_local_changes(state, &mut clock, rows::RowScope::App).await?;
    let note_row_n = note_rows.ops.len();
    let attachment_n = attachments.ops.len();
    let note_n = changes.ops.len();
    let row_n = table_rows.ops.len();
    let mut ops = note_rows.ops;
    ops.extend(attachments.ops);
    ops.extend(changes.ops);
    ops.extend(table_rows.ops);
    #[cfg(desktop)]
    let (lease_states, lease_n) = {
        let leases = profile_files::collect_leases(state, &mut clock).await?;
        let n = leases.ops.len();
        ops.extend(leases.ops);
        (leases.states, n)
    };
    #[cfg(desktop)]
    let collect_msg = format!(
        "note rows {note_row_n}, attachments {attachment_n}, notes {note_n}, app rows {row_n}, leases {lease_n}"
    );
    #[cfg(mobile)]
    let collect_msg = format!(
        "note rows {note_row_n}, attachments {attachment_n}, notes {note_n}, app rows {row_n}"
    );
    trace(app, "info", "collect", &collect_msg);
    emit_progress(app, "push", 22, 0, 0, "");
    let mut warnings: Vec<String> = Vec::new();
    trace(app, "info", "push", &format!("{} ops", ops.len()));
    if !ops.is_empty() {
        if let Some(w) = engine
            .rewind_own_log(&mut local)
            .await
            .map_err(AppError::other)?
        {
            trace(app, "info", "push", &w);
            warnings.push(w);
        }
        if let Err(e) = engine.push(&mut local, ops).await {
            if matches!(e, veydan_sync::SyncError::OwnLogCollision(_)) {
                trace(
                    app,
                    "error",
                    "push",
                    "own log collision, rotating device id",
                );
            } else {
                trace(app, "error", "push", &e.to_string());
            }
            return Err(push_error(db, e).await);
        }
        state::save_own_state(db, &local, &clock.last()).await?;
        for st in note_rows.states.iter().chain(&table_rows.states) {
            state::save_row_state(db, st).await?;
        }
        for st in &attachments.states {
            state::save_attachment_state(db, st).await?;
        }
        for st in &changes.states {
            state::save_note_state(db, st).await?;
        }
        #[cfg(desktop)]
        for st in &lease_states {
            state::save_profile_files_state(db, st).await?;
        }
    }

    emit_progress(app, "pull", 30, 0, 0, "devices/");
    let pulled = engine
        .pull(&mut local, |current, total, key| {
            emit_progress(
                app,
                "pull",
                progress_pct(30, 40, current, total),
                current,
                total,
                key,
            );
        })
        .await
        .map_err(|e| {
            trace(app, "error", "pull", &e.to_string());
            AppError::other(e)
        })?;
    if pulled.peers.is_empty() {
        trace(app, "info", "pull", "no other devices");
    }
    for peer in &pulled.peers {
        let id = short_id(&peer.device_id);
        match &peer.error {
            Some(err) => trace(
                app,
                "error",
                "pull",
                &format!("{id} seq {} files {} {err}", peer.head_seq, peer.files),
            ),
            None => trace(
                app,
                "info",
                "pull",
                &format!(
                    "{id} seq {} files {} ops {}",
                    peer.head_seq, peer.files, peer.ops
                ),
            ),
        }
    }
    for op in &pulled.ops {
        clock.observe(&op.hlc);
    }
    // Persist the advanced clock now: a failure while applying must not let the
    // next cycle stamp ops with an HLC below what it has already seen.
    state::save_own_state(db, &local, &clock.last()).await?;
    config::set_setting(db, "sync_last_applied", &pulled.ops.len().to_string()).await?;
    // A parent row missing on this device is not fatal: the ops come again next cycle.
    let mut deferred: Option<String> = None;
    emit_progress(app, "apply", 40, 0, 0, "");
    let att_outcome = fk_retry(
        "attachments",
        attachments::apply_remote(&engine, app, &pulled.ops).await,
        &mut deferred,
    )?;
    emit_progress(app, "apply", 44, 0, 0, "");
    let note_catalog = fk_retry(
        "notes catalog",
        rows::apply_remote(app, &pulled.ops, rows::RowScope::NotesCatalog).await,
        &mut deferred,
    )?;
    emit_progress(app, "apply", 48, 0, 0, "");
    let outcome = fk_retry(
        "notes",
        notes::apply_remote(&engine, app, pulled.ops.clone(), &mut clock, &mut local).await,
        &mut deferred,
    )?;
    fk_retry(
        "notes index",
        notes::finish_apply(app, &outcome).await,
        &mut deferred,
    )?;
    emit_progress(app, "apply", 52, 0, 0, "");
    let note_meta = fk_retry(
        "notes meta",
        rows::apply_remote(app, &pulled.ops, rows::RowScope::NotesMeta).await,
        &mut deferred,
    )?;
    emit_progress(app, "apply", 54, 0, 0, "");
    let rows_outcome = fk_retry(
        "app rows",
        rows::apply_remote(app, &pulled.ops, rows::RowScope::App).await,
        &mut deferred,
    )?;
    attachments::notify(app, &att_outcome);
    trace_retry(app, "attachments", &att_outcome.retry);
    trace_retry(app, "notes catalog", &note_catalog.retry);
    trace_retry(app, "notes", &outcome.retry);
    trace_retry(app, "notes meta", &note_meta.retry);
    trace_retry(app, "app rows", &rows_outcome.retry);
    for reason in att_outcome.skipped.iter().chain(&outcome.skipped) {
        trace(app, "warn", "apply", reason);
        warnings.push(reason.clone());
    }
    if let Some(reason) = &deferred {
        trace(app, "retry", "apply", reason);
    }
    trace(app, "info", "apply", &op_summary(&pulled.ops));
    let mut notes_rows_out = note_catalog;
    notes_rows_out
        .changed
        .extend(note_meta.changed.iter().cloned());
    if notes_rows_out.retry.is_none() {
        notes_rows_out.retry = note_meta.retry.clone();
    }
    rows::finish_apply(app, &notes_rows_out).await;
    rows::finish_apply(app, &rows_outcome).await;

    for e in &attachments.errors {
        trace(app, "error", "attachments", e);
    }
    warnings.extend(
        pulled
            .errors
            .into_iter()
            .map(|(peer, e)| format!("{peer}: {e}")),
    );
    warnings.extend(
        attachments
            .errors
            .iter()
            .map(|e| format!("attachment upload: {e}")),
    );
    let files_retry = profile_files_phase(
        app,
        state,
        &engine,
        &cfg,
        &pulled.ops,
        &mut local,
        &mut clock,
        &mut warnings,
    )
    .await?;

    state::save_own_state(db, &local, &clock.last()).await?;
    let retry = outcome
        .retry
        .as_ref()
        .or(att_outcome.retry.as_ref())
        .or(notes_rows_out.retry.as_ref())
        .or(rows_outcome.retry.as_ref())
        .or(files_retry.as_ref())
        .or(deferred.as_ref());
    let clean = retry.is_none() && warnings.is_empty();
    match retry {
        None => state::save_peer_heads(db, &local).await?,
        Some(reason) => {
            let line = format!("retry next cycle: {reason}");
            trace(app, "retry", "done", &line);
            warnings.push(line);
        }
    }

    if engine.own_chunk_count().await.map_err(AppError::other)? > COMPACT_AFTER_CHUNKS {
        emit_progress(app, "compact", 94, 0, 0, "");
        trace(app, "info", "compact", "compacting");
        let now_ms = Utc::now().timestamp_millis().max(0) as u64;
        engine
            .compact(&local, now_ms, TOMBSTONE_TTL_MS)
            .await
            .map_err(|e| {
                trace(app, "error", "compact", &e.to_string());
                AppError::other(e)
            })?;
    }

    #[cfg(desktop)]
    if clean && gc::due(db).await {
        emit_progress(app, "gc", 97, 0, 0, "");
        trace(app, "info", "gc", "running");
        if let Err(e) = gc::run(&engine, db).await {
            trace(app, "error", "gc", &e.to_string());
            warnings.push(format!("gc skipped: {e}"));
        }
    }
    #[cfg(mobile)]
    let _ = clean;

    if let Err(e) = engine.publish_device_name(&cfg.device_name).await {
        warnings.push(format!("device name: {e}"));
    }
    match engine.list_device_cards().await {
        Ok(cards) => {
            let own = engine.device_id();
            let devices: Vec<StorageDevice> = cards
                .into_iter()
                .map(|(id, name)| StorageDevice {
                    own: id == own,
                    name: if id == own && name.is_empty() {
                        cfg.device_name.clone()
                    } else {
                        name
                    },
                    id,
                })
                .collect();
            if let Ok(json) = serde_json::to_string(&devices) {
                config::set_setting(db, "sync_devices", &json).await?;
            }
        }
        Err(e) => warnings.push(format!("device list: {e}")),
    }

    Ok(warnings)
}

/// Leases and Firefox profile files: upload local snapshots, apply remote ones.
/// Returns the retry reason when a remote snapshot could not be applied yet.
#[cfg(desktop)]
#[allow(clippy::too_many_arguments)]
async fn profile_files_phase(
    app: &AppHandle,
    state: &AppState,
    engine: &Engine,
    cfg: &SyncConfig,
    pulled_ops: &[veydan_sync::Op],
    local: &mut veydan_sync::LocalState,
    clock: &mut HlcClock,
    warnings: &mut Vec<String>,
) -> CmdResult<Option<String>> {
    let db = &state.db;
    match profile_files::apply_leases(app, pulled_ops).await {
        Ok(_) => {
            let _ = app.emit(EVENT_STATUS, ());
        }
        Err(e) => {
            trace(app, "error", "profiles", &format!("leases: {e}"));
            warnings.push(format!("leases: {e}"));
        }
    }
    if !cfg.profile_files {
        trace(app, "info", "profiles", "off");
        return Ok(None);
    }
    emit_progress(app, "profiles_up", 55, 0, 0, "");
    match profile_files::collect_local_changes(engine, state, clock, app).await {
        Ok(files) => {
            let n = files.ops.len();
            trace(app, "info", "profiles", &format!("upload {n} ops"));
            if !files.ops.is_empty() {
                if let Err(e) = engine.push(local, files.ops).await {
                    if matches!(e, veydan_sync::SyncError::OwnLogCollision(_)) {
                        trace(
                            app,
                            "error",
                            "profiles",
                            "own log collision, rotating device id",
                        );
                        return Err(push_error(db, e).await);
                    }
                    trace(app, "error", "profiles", &format!("upload: {e}"));
                    warnings.push(format!("profiles upload: {e}"));
                } else {
                    state::save_own_state(db, local, &clock.last()).await?;
                    for st in &files.states {
                        state::save_profile_files_state(db, st).await?;
                    }
                }
            }
        }
        Err(e) => {
            trace(app, "error", "profiles", &format!("upload: {e}"));
            warnings.push(format!("profiles upload: {e}"));
        }
    }
    emit_progress(app, "profiles_down", 78, 0, 0, "");
    match profile_files::apply_remote(engine, app, pulled_ops).await {
        Ok(o) => {
            for reason in &o.skipped {
                trace(app, "warn", "profiles", reason);
                warnings.push(reason.clone());
            }
            match &o.retry {
                Some(reason) => trace(app, "retry", "profiles", reason),
                None => trace(app, "info", "profiles", "download ok"),
            }
            Ok(o.retry)
        }
        Err(e) => {
            trace(app, "error", "profiles", &format!("download: {e}"));
            warnings.push(format!("profiles download: {e}"));
            Ok(None)
        }
    }
}

/// Mobile has no browser profiles: nothing to lease or upload.
#[cfg(mobile)]
#[allow(clippy::too_many_arguments)]
async fn profile_files_phase(
    _app: &AppHandle,
    _state: &AppState,
    _engine: &Engine,
    _cfg: &SyncConfig,
    _pulled_ops: &[veydan_sync::Op],
    _local: &mut veydan_sync::LocalState,
    _clock: &mut HlcClock,
    _warnings: &mut Vec<String>,
) -> CmdResult<Option<String>> {
    Ok(None)
}

// ── Scheduler ────────────────────────────────────────────────────────────────

/// Background ticker. Does nothing while sync is disabled or no vault is joined.
pub fn start_sync_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        ensure_debug_loaded(&app.state::<AppState>()).await;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(TICK_SEC));
        loop {
            ticker.tick().await;
            let state = app.state::<AppState>();
            let running = state.sync.running.load(Ordering::SeqCst);
            if !running {
                state.sync.sched_skip.store(false, Ordering::Relaxed);
            }
            let db = &state.db;
            if config::get_setting(db, "sync_enabled").await.as_deref() != Some("1")
                || load_binding(db).await.is_none()
            {
                continue;
            }
            let interval_ms = config::get_setting(db, "sync_interval_sec")
                .await
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(config::DEFAULT_INTERVAL_SEC as i64)
                * 1000;
            // Measured from the last cycle start so a failed cycle does not retry every tick.
            let due = match config::get_setting(db, "sync_last_started").await {
                None => true,
                Some(last) => chrono::DateTime::parse_from_rfc3339(&last)
                    .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_milliseconds() >= interval_ms)
                    .unwrap_or(true),
            };
            if !due {
                continue;
            }
            if running {
                if !state.sync.sched_skip.swap(true, Ordering::Relaxed) {
                    trace_skip(&app, "scheduler", "scheduler", "already running");
                }
                continue;
            }
            if let Err(e) = run_cycle(&app, "scheduler").await {
                eprintln!("sync: {e}");
            }
        }
    });
}
