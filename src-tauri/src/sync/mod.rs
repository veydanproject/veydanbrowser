// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Sync module (beta): notes replicated through an E2E-encrypted vault that
//! lives in a plain folder, an S3 bucket or a WebDAV collection.
//!
//! - `config`      — settings + storage adapter factory
//! - `state`       — persistence of log positions and per-entity state
//! - `notes`       — notes as sync entity (push / pull / merge)
//! - `attachments` — note attachments as sync entity (push / pull, LWW)
//! - `rows`        — table rows (profiles, proxies, ssh, ...) as sync entities (LWW)
//! - `profile_files` — Firefox profile directories: lease + file snapshots
//!
//! Nothing here runs unless `sync_enabled` is "1" and a vault was joined.

mod attachments;
mod config;
mod fs_hash;
mod gc;
mod notes;
mod profile_files;
mod rows;
mod state;

pub use config::SyncConfig;

use crate::commands::notes::MergeResult;
use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use config::{build_storage, load_binding, load_config, VaultBinding};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use veydan_sync::{Engine, HlcClock, Probe, Vmk};

const TICK_SEC: u64 = 1;
const COMPACT_AFTER_CHUNKS: usize = 64;
const TOMBSTONE_TTL_MS: u64 = 180 * 24 * 60 * 60 * 1000;
pub const EVENT_STATUS: &str = "sync://status";

/// Guards against overlapping cycles (manual + scheduled).
#[derive(Default)]
pub struct SyncManager {
    running: AtomicBool,
    /// File hashes keyed by path, valid while mtime and size match.
    file_hashes: fs_hash::HashCache,
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

/// Profile currently leased by some device.
#[derive(Debug, Serialize)]
pub struct LeaseInfo {
    pub profile_id: String,
    pub device_id: String,
    pub device_name: String,
    pub own: bool,
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
    /// Profiles whose files diverged (id, name).
    pub profile_conflicts: Vec<ConflictInfo>,
    pub profile_leases: Vec<LeaseInfo>,
    /// Remote ops received in the last cycle.
    pub last_applied: Option<u64>,
    pub gc_last: Option<String>,
    pub blobs_total: Option<u64>,
    pub blobs_removed_last_gc: Option<u64>,
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
    // A fresh binding starts from scratch: no peer heads, no entity positions.
    state::clear_all_states(db).await?;
    config::clear_binding(db).await?;
    config::save_binding(db, &VaultBinding { vault_id: vault_id.to_string(), vmk_b64: vmk.to_base64() }).await?;
    config::set_setting(db, "sync_enabled", "1").await
}

/// Forget the vault on this device. Nothing in the storage is touched.
#[tauri::command]
pub async fn sync_leave(app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<SyncStatus> {
    let db = &state.db;
    state::clear_all_states(db).await?;
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

/// Structured merge blocks for the conflict UI.
#[tauri::command]
pub async fn sync_conflict_get(note_id: String, state: tauri::State<'_, AppState>) -> CmdResult<MergeResult> {
    notes::conflict_merge(&state, &note_id).await
}

/// Store the resolved text and push it right away.
#[tauri::command]
pub async fn sync_conflict_resolve(
    note_id: String,
    content: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    notes::resolve_conflict(&state, &note_id, content).await?;
    let _ = app.emit(EVENT_STATUS, ());
    if !state.sync.running.load(Ordering::SeqCst) {
        run_cycle(&app).await?;
    }
    status(&state).await
}

/// Conflict choice for profile files: replace local files with the remote snapshot.
#[tauri::command]
pub async fn sync_profile_files_take_remote(
    profile_id: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    let engine = open_engine(&state).await?;
    profile_files::take_remote(&engine, &state, &profile_id).await?;
    let _ = app.emit(EVENT_STATUS, ());
    status(&state).await
}

/// Conflict choice for profile files: keep local files and publish them.
#[tauri::command]
pub async fn sync_profile_files_push_mine(
    profile_id: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncStatus> {
    profile_files::push_mine(&state, &profile_id).await?;
    trigger_cycle(&app);
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
        device_id,
        peers: local.peers.len(),
        last_run: config::get_setting(db, "sync_last_run").await,
        last_error: config::get_setting(db, "sync_last_error").await.filter(|s| !s.is_empty()),
        conflicts,
        profile_conflicts,
        profile_leases,
        last_applied: config::get_setting(db, "sync_last_applied").await.and_then(|v| v.parse().ok()),
        gc_last: config::get_setting(db, gc::LAST_RUN_KEY)
            .await
            .and_then(|v| v.parse::<i64>().ok())
            .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
            .map(|t| t.to_rfc3339()),
        blobs_total: config::get_setting(db, gc::BLOBS_TOTAL_KEY).await.and_then(|v| v.parse().ok()),
        blobs_removed_last_gc: config::get_setting(db, gc::REMOVED_KEY).await.and_then(|v| v.parse().ok()),
    })
}

/// Engine for the joined vault.
async fn open_engine(state: &AppState) -> CmdResult<Engine> {
    let db = &state.db;
    let cfg = load_config(db).await;
    let binding = load_binding(db).await.ok_or_else(|| AppError::other("no vault joined"))?;
    let vmk = Vmk::from_base64(&binding.vmk_b64).map_err(AppError::other)?;
    let device = config::device_id(db).await?;
    Ok(Engine::with_key(build_storage(&cfg)?, &vmk, &binding.vault_id, &device))
}

/// Sync is on, a vault is joined and profile files are included.
async fn profile_files_active(state: &AppState) -> bool {
    let cfg = load_config(&state.db).await;
    cfg.enabled && cfg.profile_files && load_binding(&state.db).await.is_some()
}

/// Run a cycle in the background unless one is already running.
pub fn trigger_cycle(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if state.sync.running.load(Ordering::SeqCst) || !load_config(&state.db).await.enabled {
            return;
        }
        if let Err(e) = run_cycle(&app).await {
            eprintln!("sync: {e}");
        }
    });
}

// ── Profile launch hooks ─────────────────────────────────────────────────────

pub const ERR_PROFILE_IN_USE: &str = "profile_in_use";

/// Before the browser starts: refuse a profile leased elsewhere (unless forced),
/// apply a snapshot that waited, take the lease.
pub async fn before_profile_launch(app: &AppHandle, profile_id: &str, force: bool) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if !profile_files_active(&state).await {
        return Ok(());
    }
    if !force {
        if let Some((_, name)) = profile_files::foreign_lease(&state, profile_id).await? {
            return Err(AppError::other(format!("{ERR_PROFILE_IN_USE}:{name}")));
        }
        if profile_files::has_pending(&state, profile_id).await? {
            let engine = open_engine(&state).await?;
            profile_files::apply_pending(&engine, &state, profile_id).await?;
        }
    }
    profile_files::acquire_lease(&state, profile_id).await?;
    trigger_cycle(app);
    Ok(())
}

/// After the browser exited: mark files dirty, release the lease, sync soon.
pub async fn on_profile_stopped(app: &AppHandle, profile_id: &str) {
    let state = app.state::<AppState>();
    if !profile_files_active(&state).await {
        return;
    }
    if let Err(e) = profile_files::on_profile_stopped(&state, profile_id).await {
        eprintln!("sync: {e}");
    }
    trigger_cycle(app);
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
    config::set_setting(db, "sync_last_started", &Utc::now().to_rfc3339()).await?;
    // Let the UI show the running state for scheduled cycles too.
    let _ = app.emit(EVENT_STATUS, ());

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
    let engine = open_engine(state).await?;
    engine.verify_manifest().await.map_err(AppError::other)?;

    let (mut local, last_hlc) = state::load_local_state(db).await?;
    let mut clock = HlcClock::new(engine.device_id().to_string(), last_hlc.as_ref());

    // Push: attachments first so a note never references a missing file.
    let table_rows = rows::collect_local_changes(state, &mut clock).await?;
    let attachments = attachments::collect_local_changes(&engine, state, &mut clock).await?;
    let changes = notes::collect_local_changes(&engine, state, &mut clock).await?;
    let files = match cfg.profile_files {
        true => profile_files::collect_local_changes(&engine, state, &mut clock).await?,
        false => profile_files::LocalChanges { ops: Vec::new(), states: Vec::new() },
    };
    let mut ops = table_rows.ops;
    ops.extend(attachments.ops);
    ops.extend(changes.ops);
    ops.extend(files.ops);
    if !ops.is_empty() {
        engine.push(&mut local, ops).await.map_err(AppError::other)?;
        state::save_own_state(db, &local, &clock.last()).await?;
        for st in &table_rows.states {
            state::save_row_state(db, st).await?;
        }
        for st in &attachments.states {
            state::save_attachment_state(db, st).await?;
        }
        for st in &changes.states {
            state::save_note_state(db, st).await?;
        }
        for st in &files.states {
            state::save_profile_files_state(db, st).await?;
        }
    }

    // Pull
    let pulled = engine.pull(&mut local).await.map_err(AppError::other)?;
    for op in &pulled.ops {
        clock.observe(&op.hlc);
    }
    config::set_setting(db, "sync_last_applied", &pulled.ops.len().to_string()).await?;
    let att_outcome = attachments::apply_remote(&engine, app, &pulled.ops).await?;
    let outcome = notes::apply_remote(&engine, app, pulled.ops.clone(), &mut clock, &mut local).await?;
    // Reindex first: note flags need the note rows that new files create.
    notes::finish_apply(app, &outcome).await?;
    let rows_outcome = rows::apply_remote(app, &pulled.ops).await?;
    // Files last: a snapshot needs the profile row from `rows`.
    let files_outcome = match cfg.profile_files {
        true => profile_files::apply_remote(&engine, app, &pulled.ops).await?,
        false => profile_files::ApplyOutcome::default(),
    };
    state::save_own_state(db, &local, &clock.last()).await?;
    let mut warnings: Vec<String> = pulled.errors.into_iter().map(|(peer, e)| format!("{peer}: {e}")).collect();
    let retry = outcome
        .retry
        .as_ref()
        .or(att_outcome.retry.as_ref())
        .or(rows_outcome.retry.as_ref())
        .or(files_outcome.retry.as_ref());
    let clean = retry.is_none() && warnings.is_empty();
    match retry {
        None => state::save_peer_heads(db, &local).await?,
        Some(reason) => warnings.push(format!("retry next cycle: {reason}")),
    }
    attachments::notify(app, &att_outcome);
    rows::finish_apply(app, &rows_outcome).await;

    // Compact own log once it grows past the threshold.
    if engine.own_chunk_count().await.map_err(AppError::other)? > COMPACT_AFTER_CHUNKS {
        let now_ms = Utc::now().timestamp_millis().max(0) as u64;
        engine.compact(&local, now_ms, TOMBSTONE_TTL_MS).await.map_err(AppError::other)?;
    }

    // Blob GC only after a fully clean cycle, so every reference was seen.
    if clean && gc::due(db).await {
        if let Err(e) = gc::run(&engine, db).await {
            warnings.push(format!("gc skipped: {e}"));
        }
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
            if state.sync.running.load(Ordering::SeqCst) {
                continue;
            }
            let db = &state.db;
            if config::get_setting(db, "sync_enabled").await.as_deref() != Some("1") || load_binding(db).await.is_none() {
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
            if due {
                if let Err(e) = run_cycle(&app).await {
                    eprintln!("sync: {e}");
                }
            }
        }
    });
}