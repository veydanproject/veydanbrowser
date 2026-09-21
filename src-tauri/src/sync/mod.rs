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

mod attachments;
mod config;
mod fs_hash;
mod gc;
mod notes;
#[cfg(desktop)]
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
/// Minimum gap between cycles started by `sync_trigger` (app resume, screen open).
const MIN_TRIGGER_GAP_SEC: i64 = 15;
const COMPACT_AFTER_CHUNKS: usize = 64;
const TOMBSTONE_TTL_MS: u64 = 180 * 24 * 60 * 60 * 1000;
pub const EVENT_STATUS: &str = "sync://status";
pub const EVENT_PROGRESS: &str = "sync://progress";

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    pub phase: String,
    pub percent: u32,
    pub current: u32,
    pub total: u32,
    pub detail: String,
}

pub(crate) fn emit_progress(app: &AppHandle, phase: &str, percent: u32, current: u32, total: u32, detail: &str) {
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

/// Sync position of one note: `tracked` when the vault knows it, `pending` when
/// the local file differs from the last pushed/applied version.
#[tauri::command]
pub async fn note_sync_info(id: String, state: tauri::State<'_, AppState>) -> CmdResult<NoteSyncInfo> {
    let Some(st) = state::load_note_state(&state.db, &id).await? else {
        return Ok(NoteSyncInfo { tracked: false, pending: true });
    };
    let file_path: Option<String> = sqlx::query_scalar("SELECT file_path FROM notes WHERE id = ? AND deleted = 0")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;
    let hash = file_path
        .and_then(|p| std::fs::read(crate::commands::notes::resolve_note_abs_path(&state.app_data_dir, &p)).ok())
        .map(|raw| veydan_sync::sha256_hex(&raw))
        .unwrap_or_default();
    Ok(NoteSyncInfo { tracked: true, pending: hash != st.synced_hash })
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
        trigger_cycle(&app);
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
        last_warning: config::get_setting(db, "sync_last_warning").await.filter(|s| !s.is_empty()),
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

#[cfg(desktop)]
pub const ERR_PROFILE_IN_USE: &str = "profile_in_use";

/// Before the browser starts: refuse a profile leased elsewhere (unless forced),
/// apply a snapshot that waited, take the lease.
#[cfg(desktop)]
pub async fn before_profile_launch(app: &AppHandle, profile_id: &str, force: bool) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if !vault_active(&state).await {
        return Ok(());
    }
    if !force {
        if let Some((_, name)) = profile_files::foreign_lease(&state, profile_id).await? {
            return Err(AppError::other(format!("{ERR_PROFILE_IN_USE}:{name}")));
        }
        if profile_files_active(&state).await && profile_files::has_pending(&state, profile_id).await? {
            let engine = open_engine(&state).await?;
            profile_files::apply_pending(&engine, &state, app, profile_id).await?;
        }
    }
    profile_files::acquire_lease(&state, profile_id).await?;
    trigger_cycle(app);
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
        }
        Err(e) => {
            config::set_setting(db, "sync_last_error", &e.to_string()).await?;
            emit_progress(app, "error", 0, 0, 0, &e.to_string());
        }
    }
    let _ = app.emit(EVENT_STATUS, ());
    result.map(|_| ())
}

/// FOREIGN KEY failure in an apply step: record it as "retry next cycle" and go on.
fn fk_retry<T: Default>(step: &str, result: CmdResult<T>, deferred: &mut Option<String>) -> CmdResult<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) if rows::is_fk_error(&e) => {
            deferred.get_or_insert_with(|| format!("{step}: {e}"));
            Ok(T::default())
        }
        Err(e) => Err(e),
    }
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
    let attachments = attachments::collect_local_changes(&engine, state, &mut clock).await?;
    emit_progress(app, "collect", 16, 0, 0, "");
    let changes = notes::collect_local_changes(&engine, state, &mut clock).await?;
    emit_progress(app, "collect", 18, 0, 0, "");
    let table_rows = rows::collect_local_changes(state, &mut clock, rows::RowScope::App).await?;
    let mut ops = note_rows.ops;
    ops.extend(attachments.ops);
    ops.extend(changes.ops);
    ops.extend(table_rows.ops);
    #[cfg(desktop)]
    let lease_states = {
        let leases = profile_files::collect_leases(state, &mut clock).await?;
        ops.extend(leases.ops);
        leases.states
    };
    emit_progress(app, "push", 22, 0, 0, "");
    if !ops.is_empty() {
        engine.push(&mut local, ops).await.map_err(AppError::other)?;
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
            emit_progress(app, "pull", progress_pct(30, 40, current, total), current, total, key);
        })
        .await
        .map_err(AppError::other)?;
    for op in &pulled.ops {
        clock.observe(&op.hlc);
    }
    config::set_setting(db, "sync_last_applied", &pulled.ops.len().to_string()).await?;
    // A parent row missing on this device is not fatal: the ops come again next cycle.
    let mut deferred: Option<String> = None;
    emit_progress(app, "apply", 40, 0, 0, "");
    let att_outcome = fk_retry("attachments", attachments::apply_remote(&engine, app, &pulled.ops).await, &mut deferred)?;
    emit_progress(app, "apply", 44, 0, 0, "");
    let note_catalog =
        fk_retry("notes catalog", rows::apply_remote(app, &pulled.ops, rows::RowScope::NotesCatalog).await, &mut deferred)?;
    emit_progress(app, "apply", 48, 0, 0, "");
    let outcome = fk_retry(
        "notes",
        notes::apply_remote(&engine, app, pulled.ops.clone(), &mut clock, &mut local).await,
        &mut deferred,
    )?;
    fk_retry("notes index", notes::finish_apply(app, &outcome).await, &mut deferred)?;
    emit_progress(app, "apply", 52, 0, 0, "");
    let note_meta = fk_retry("notes meta", rows::apply_remote(app, &pulled.ops, rows::RowScope::NotesMeta).await, &mut deferred)?;
    emit_progress(app, "apply", 54, 0, 0, "");
    let rows_outcome = fk_retry("app rows", rows::apply_remote(app, &pulled.ops, rows::RowScope::App).await, &mut deferred)?;
    attachments::notify(app, &att_outcome);
    let mut notes_rows_out = note_catalog;
    notes_rows_out.changed.extend(note_meta.changed.iter().cloned());
    if notes_rows_out.retry.is_none() {
        notes_rows_out.retry = note_meta.retry.clone();
    }
    rows::finish_apply(app, &notes_rows_out).await;
    rows::finish_apply(app, &rows_outcome).await;

    let mut warnings: Vec<String> = pulled.errors.into_iter().map(|(peer, e)| format!("{peer}: {e}")).collect();
    let files_retry = profile_files_phase(app, state, &engine, &cfg, &pulled.ops, &mut local, &mut clock, &mut warnings).await?;

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
        Some(reason) => warnings.push(format!("retry next cycle: {reason}")),
    }

    if engine.own_chunk_count().await.map_err(AppError::other)? > COMPACT_AFTER_CHUNKS {
        emit_progress(app, "compact", 94, 0, 0, "");
        let now_ms = Utc::now().timestamp_millis().max(0) as u64;
        engine.compact(&local, now_ms, TOMBSTONE_TTL_MS).await.map_err(AppError::other)?;
    }

    #[cfg(desktop)]
    if clean && gc::due(db).await {
        emit_progress(app, "gc", 97, 0, 0, "");
        if let Err(e) = gc::run(&engine, db).await {
            warnings.push(format!("gc skipped: {e}"));
        }
    }
    #[cfg(mobile)]
    let _ = clean;

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
        Err(e) => warnings.push(format!("leases: {e}")),
    }
    if !cfg.profile_files {
        return Ok(None);
    }
    emit_progress(app, "profiles_up", 55, 0, 0, "");
    match profile_files::collect_local_changes(engine, state, clock, app).await {
        Ok(files) => {
            if !files.ops.is_empty() {
                if let Err(e) = engine.push(local, files.ops).await {
                    warnings.push(format!("profiles upload: {e}"));
                } else {
                    state::save_own_state(db, local, &clock.last()).await?;
                    for st in &files.states {
                        state::save_profile_files_state(db, st).await?;
                    }
                }
            }
        }
        Err(e) => warnings.push(format!("profiles upload: {e}")),
    }
    emit_progress(app, "profiles_down", 78, 0, 0, "");
    match profile_files::apply_remote(engine, app, pulled_ops).await {
        Ok(o) => Ok(o.retry),
        Err(e) => {
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