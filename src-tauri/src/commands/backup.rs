// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Full-application backup & restore.
//!
//! A backup is a single encrypted file `veydan-backup-<ts>.vbk` written to a
//! user-chosen folder. Everything the app persists is captured so that a
//! restore reproduces the exact same state — *except* the Camoufox runtime
//! binary (`camoufox/`), which is large and re-downloadable on demand.
//!
//! Pipeline (fully streaming, no intermediate plaintext archive on disk):
//! `tar` (file tree + manifest.json) → `zstd` (compression) → `age`
//! (passphrase / scrypt authenticated encryption) → `*.vbk`.
//!
//! The DB is captured via `VACUUM INTO`, giving a transactionally-consistent
//! single-file snapshot without the live WAL/SHM sidecars.
//!
//! Scheduling: a background task ticks every 60s, reads the schedule from
//! `app_settings`, and runs a backup when one is due (catching up missed runs
//! after the app was closed). Old backups are rotated to the newest N.

use crate::error::{AppError, CmdResult};
use crate::AppState;
use age::secrecy::SecretString;
use chrono::{DateTime, Datelike, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::profiles::is_blacklisted;

const FORMAT_VERSION: u32 = 1;
/// Max scrypt log2(N) accepted on restore (age default is 18-22 on modern HW).
const MAX_SCRYPT_WORK_FACTOR: u8 = 22;
const FILE_PREFIX: &str = "veydan-backup-";
const FILE_EXT: &str = "vbk";

// ── Shared state ────────────────────────────────────────────────────────────

/// Tracks whether a backup is currently running so manual + scheduled runs
/// can't overlap and corrupt the staging area.
#[derive(Default)]
pub struct BackupManager {
    running: AtomicBool,
}

/// RAII guard: clears the running flag on drop so an early `?` return can never
/// leave the backup wedged as "in progress".
struct RunningGuard<'a>(&'a AtomicBool);
impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// ── Config (persisted in app_settings) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    pub dir: Option<String>,
    pub password: Option<String>,
    pub schedule_enabled: bool,
    /// "interval" | "daily" | "weekly"
    pub schedule_mode: String,
    pub interval_hours: i64,
    /// "HH:MM" local time, used by daily/weekly modes.
    pub time: String,
    /// 0 = Monday … 6 = Sunday (weekly mode).
    pub weekday: i64,
    /// How many newest backups to keep (rotation). 0 = keep all.
    pub keep: i64,
    /// RFC3339 UTC timestamp of the last successful backup.
    pub last_run: Option<String>,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            dir: None,
            password: None,
            schedule_enabled: false,
            schedule_mode: "interval".into(),
            interval_hours: 24,
            time: "03:00".into(),
            weekday: 0,
            keep: 5,
            last_run: None,
        }
    }
}

async fn get_setting(db: &Pool<Sqlite>, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}

async fn set_setting(db: &Pool<Sqlite>, key: &str, value: &str) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

async fn load_config(db: &Pool<Sqlite>) -> BackupConfig {
    let d = BackupConfig::default();
    BackupConfig {
        dir: get_setting(db, "backup_dir").await.filter(|s| !s.is_empty()),
        password: get_setting(db, "backup_password").await.filter(|s| !s.is_empty()),
        schedule_enabled: get_setting(db, "backup_schedule_enabled")
            .await
            .map(|v| v == "1")
            .unwrap_or(d.schedule_enabled),
        schedule_mode: get_setting(db, "backup_schedule_mode").await.unwrap_or(d.schedule_mode),
        interval_hours: get_setting(db, "backup_interval_hours")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(d.interval_hours),
        time: get_setting(db, "backup_time").await.unwrap_or(d.time),
        weekday: get_setting(db, "backup_weekday")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(d.weekday),
        keep: get_setting(db, "backup_keep")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(d.keep),
        last_run: get_setting(db, "backup_last_run").await,
    }
}

// ── Commands: config ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn backup_get_config(state: tauri::State<'_, AppState>) -> CmdResult<BackupConfig> {
    Ok(load_config(&state.db).await)
}

#[tauri::command]
pub async fn backup_set_config(
    cfg: BackupConfig,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let db = &state.db;
    match cfg.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => set_setting(db, "backup_dir", v).await?,
        None => set_setting(db, "backup_dir", "").await?,
    }
    match cfg.password.as_deref().filter(|s| !s.is_empty()) {
        Some(v) => set_setting(db, "backup_password", v).await?,
        None => set_setting(db, "backup_password", "").await?,
    }
    set_setting(db, "backup_schedule_enabled", if cfg.schedule_enabled { "1" } else { "0" }).await?;
    set_setting(db, "backup_schedule_mode", &cfg.schedule_mode).await?;
    set_setting(db, "backup_interval_hours", &cfg.interval_hours.to_string()).await?;
    set_setting(db, "backup_time", &cfg.time).await?;
    set_setting(db, "backup_weekday", &cfg.weekday.to_string()).await?;
    set_setting(db, "backup_keep", &cfg.keep.to_string()).await?;
    Ok(())
}

// ── Commands: list ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BackupFileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Modified time, RFC3339 UTC.
    pub modified: String,
}

fn list_backups(dir: &Path) -> Vec<BackupFileInfo> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with(FILE_PREFIX) && name.ends_with(FILE_EXT)) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| DateTime::<Utc>::from(t).to_rfc3339().into())
            .unwrap_or_default();
        out.push(BackupFileInfo {
            name,
            path: entry.path().to_string_lossy().to_string(),
            size: meta.len(),
            modified,
        });
    }
    // Newest first.
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

#[tauri::command]
pub async fn backup_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<BackupFileInfo>> {
    let dir = get_setting(&state.db, "backup_dir")
        .await
        .filter(|s| !s.is_empty());
    match dir {
        Some(d) => Ok(list_backups(Path::new(&d))),
        None => Ok(Vec::new()),
    }
}

// ── Commands: run ───────────────────────────────────────────────────────────

/// Kick off a manual backup. Returns immediately; progress and completion are
/// reported via `backup://progress`, `backup://done`, `backup://error`.
#[tauri::command]
pub async fn backup_run_now(app: AppHandle) -> CmdResult<()> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = perform_backup(&app).await {
            let _ = app.emit("backup://error", e);
        }
    });
    Ok(())
}

#[derive(Serialize, Clone)]
struct Progress {
    phase: String,
    percent: u32,
}

fn emit_progress(app: &AppHandle, phase: &str, percent: u32) {
    let _ = app.emit(
        "backup://progress",
        Progress {
            phase: phase.into(),
            percent,
        },
    );
}

/// The full backup routine. Emits progress events; returns Err(message) on
/// failure. Safe to call from both the command and the scheduler.
async fn perform_backup(app: &AppHandle) -> Result<PathBuf, String> {
    let state = app.state::<AppState>();

    // Guard against overlapping runs.
    if state
        .backup
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A backup is already running".into());
    }
    let _guard = RunningGuard(&state.backup.running);

    let cfg = load_config(&state.db).await;
    let dir = cfg
        .dir
        .clone()
        .ok_or_else(|| "No backup folder configured".to_string())?;
    let password = cfg
        .password
        .clone()
        .ok_or_else(|| "No backup password configured".to_string())?;
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create backup folder: {e}"))?;

    let data_dir = state.app_data_dir.clone();
    let notes_custom_dir = get_setting(&state.db, "notes_custom_dir")
        .await
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let app_version = app.package_info().version.to_string();

    emit_progress(app, "snapshot", 2);

    // 1) Consistent DB snapshot via VACUUM INTO into a staging dir.
    let staging = data_dir.join("backup_tmp").join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let db_snapshot = staging.join("profiles.db");
    let vacuum_sql = format!(
        "VACUUM INTO '{}'",
        db_snapshot.to_string_lossy().replace('\'', "''")
    );
    // Dynamic SQL: VACUUM INTO can't take a bound parameter for the filename,
    // so the path is escaped and embedded. `raw_sql` is sqlx's opt-in for
    // audited dynamic statements.
    let vacuum_res = sqlx::raw_sql(sqlx::AssertSqlSafe(vacuum_sql))
        .execute(&state.db)
        .await;
    if let Err(e) = vacuum_res {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("DB snapshot failed: {e}"));
    }

    // 2) Collect the file list (archive_name, fs_path).
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_files(&data_dir.join("profiles"), "profiles", &mut files);
    collect_files(&data_dir.join("notes"), "notes", &mut files);
    if let Some(ref custom) = notes_custom_dir {
        collect_files(custom, "notes_custom", &mut files);
    }

    // 3) Manifest.
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        app_version,
        created_at: Utc::now().to_rfc3339(),
        includes_camoufox: false,
        notes_custom_dir: notes_custom_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        entries: files.iter().map(|(n, _)| n.clone()).collect(),
    };
    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;

    // 4) Stream the archive (blocking CPU/IO work off the async runtime).
    let out_name = format!(
        "{FILE_PREFIX}{}.{FILE_EXT}",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let out_path = dir.join(&out_name);
    let app2 = app.clone();
    let db_snapshot2 = db_snapshot.clone();
    let out_path2 = out_path.clone();
    let write_res = tauri::async_runtime::spawn_blocking(move || {
        write_archive(Some(&app2), &out_path2, &password, &manifest_json, &db_snapshot2, &files)
    })
    .await
    .map_err(|e| e.to_string())?;

    // Clean staging regardless of outcome.
    let _ = std::fs::remove_dir_all(&staging);
    write_res.map_err(|e| {
        let _ = std::fs::remove_file(&out_path);
        e
    })?;

    // 5) Rotation + bookkeeping.
    if cfg.keep > 0 {
        rotate(&dir, cfg.keep as usize);
    }
    let now = Utc::now().to_rfc3339();
    // If last_run can't be persisted the scheduler would re-run the backup on
    // every tick — the backup itself succeeded, so just log the failure loudly.
    if let Err(e) = set_setting(&state.db, "backup_last_run", &now).await {
        eprintln!("backup: failed to persist backup_last_run: {e}");
    }

    emit_progress(app, "done", 100);
    let _ = app.emit("backup://done", out_path.to_string_lossy().to_string());
    Ok(out_path)
}

/// Recursively gather every file under `base_fs`, mapping it to an archive path
/// rooted at `base_arch`, skipping runtime-cache blacklist names.
fn collect_files(base_fs: &Path, base_arch: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(base_fs) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_blacklisted(&name) {
            continue;
        }
        let path = entry.path();
        let arch = format!("{base_arch}/{name}");
        if path.is_dir() {
            collect_files(&path, &arch, out);
        } else if path.is_file() {
            out.push((arch, path));
        }
    }
}

/// Write manifest + db snapshot + files as tar→zstd→age into `out_path`.
/// `app` is optional so the pipeline can be exercised in unit tests.
fn write_archive(
    app: Option<&AppHandle>,
    out_path: &Path,
    password: &str,
    manifest_json: &[u8],
    db_snapshot: &Path,
    files: &[(String, PathBuf)],
) -> Result<(), String> {
    let file = File::create(out_path).map_err(|e| e.to_string())?;
    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(password.to_owned()));
    let age_writer = encryptor
        .wrap_output(file)
        .map_err(|e| format!("Encryption init failed: {e}"))?;
    let zstd_writer =
        zstd::stream::write::Encoder::new(age_writer, 3).map_err(|e| e.to_string())?;
    let mut tar = tar::Builder::new(zstd_writer);

    append_bytes(&mut tar, "manifest.json", manifest_json).map_err(|e| e.to_string())?;

    let total = files.len() + 1;
    let mut done = 0usize;
    // DB snapshot.
    {
        let mut f = File::open(db_snapshot).map_err(|e| e.to_string())?;
        tar.append_file("profiles.db", &mut f)
            .map_err(|e| e.to_string())?;
        done += 1;
        if let Some(app) = app {
            emit_progress(app, "archiving", pct(done, total));
        }
    }
    for (arch, path) in files {
        // Files can vanish mid-backup (temp caches); skip rather than abort.
        if let Ok(mut f) = File::open(path) {
            tar.append_file(arch, &mut f).map_err(|e| e.to_string())?;
        }
        done += 1;
        if let Some(app) = app {
            if done % 25 == 0 || done == total {
                emit_progress(app, "archiving", pct(done, total));
            }
        }
    }

    // Finish each layer in order: tar → zstd → age.
    let zstd_writer = tar.into_inner().map_err(|e| e.to_string())?;
    let age_writer = zstd_writer.finish().map_err(|e| e.to_string())?;
    age_writer
        .finish()
        .map_err(|e| format!("Encryption finalize failed: {e}"))?;
    Ok(())
}

fn pct(done: usize, total: usize) -> u32 {
    if total == 0 {
        return 100;
    }
    ((done as f64 / total as f64) * 100.0).round() as u32
}

fn append_bytes<W: Write>(
    builder: &mut tar::Builder<W>,
    name: &str,
    data: &[u8],
) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, name, data)
}

/// Delete all but the `keep` newest `*.vbk` files in `dir`.
fn rotate(dir: &Path, keep: usize) {
    let mut backups = list_backups(dir); // already newest-first
    if backups.len() <= keep {
        return;
    }
    for old in backups.split_off(keep) {
        let _ = std::fs::remove_file(&old.path);
    }
}

// ── Commands: restore ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    app_version: String,
    created_at: String,
    includes_camoufox: bool,
    notes_custom_dir: Option<String>,
    entries: Vec<String>,
}

/// Restore from a `.vbk` file, then relaunch the app so the fresh DB/profiles
/// are picked up cleanly. Never returns on success (process restarts).
#[tauri::command]
pub async fn backup_restore(
    path: String,
    password: String,
    app: AppHandle,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let data_dir = state.app_data_dir.clone();

    let staging = data_dir
        .join("restore_tmp")
        .join(uuid::Uuid::new_v4().to_string());

    // Decrypt + decompress + unpack into staging (blocking work).
    emit_restore_progress(&app, "decrypting", 0);
    let staging2 = staging.clone();
    let app2 = app.clone();
    let unpack_res = tauri::async_runtime::spawn_blocking(move || {
        extract_archive(Some(&app2), Path::new(&path), &password, &staging2)
    })
    .await
    .map_err(AppError::other)?;

    if let Err(e) = unpack_res {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(AppError::other(e));
    }

    // Validate manifest.
    let manifest: Manifest = match std::fs::read(staging.join("manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        Some(m) => m,
        None => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(AppError::other("Invalid or corrupt backup (no manifest)"));
        }
    };
    if manifest.format_version > FORMAT_VERSION {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(AppError::other(format!(
            "Backup format v{} is newer than supported (v{})",
            manifest.format_version, FORMAT_VERSION
        )));
    }

    // The source machine's data dir may differ from ours: rewrite stored
    // absolute paths so profiles/notes resolve after the swap.
    emit_restore_progress(&app, "remapping", 95);
    if let Err(e) = remap_paths(&staging, &data_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(AppError::other(e));
    }

    // Release every handle on the live data before renaming it. Windows
    // refuses to rename files/dirs still open by SQLite, a running browser
    // or the notes directory watcher.
    emit_restore_progress(&app, "swapping", 98);
    crate::browser::launch::stop_all(&state.browser).await;
    if let Ok(mut slot) = state.notes_watcher.lock() {
        slot.take();
    }
    state.db.close().await;

    // Swap top-level entries into place. Camoufox and other files stay put.
    // The pool is closed at this point, so we restart even on failure —
    // swap_in has already rolled the live data back.
    let (data_dir2, staging2) = (data_dir.clone(), staging.clone());
    let swap_res = tauri::async_runtime::spawn_blocking(move || {
        swap_in(&data_dir2, &staging2, &manifest)
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    if let Err(e) = swap_res {
        eprintln!("backup restore: swap failed, keeping current data: {e}");
    }
    let _ = std::fs::remove_dir_all(&staging);
    emit_restore_progress(&app, "restarting", 100);

    // Relaunch so the app rebinds to the restored DB/profiles.
    // `restart()` diverges (-> !), so this is the function's tail expression.
    app.restart()
}

/// Rewrite absolute paths inside the staged DB to point at `data_dir`.
/// Profiles always live at `profiles/<id>`; note files are matched by basename
/// against the staged `notes/documents` (custom-dir notes are left untouched).
fn remap_paths(staging: &Path, data_dir: &Path) -> Result<(), String> {
    let conn = rusqlite::Connection::open(staging.join("profiles.db"))
        .map_err(|e| format!("Cannot open restored DB: {e}"))?;

    let profiles_dir = data_dir.join("profiles");
    let mut stmt = conn
        .prepare("SELECT id FROM profiles")
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    for id in ids {
        let path = profiles_dir.join(&id).to_string_lossy().to_string();
        conn.execute(
            "UPDATE profiles SET profile_path = ?1 WHERE id = ?2",
            rusqlite::params![path, id],
        )
        .map_err(|e| e.to_string())?;
    }

    let staged_docs = staging.join("notes").join("documents");
    let docs_dir = data_dir.join("notes").join("documents");
    let mut stmt = conn
        .prepare("SELECT id, file_path FROM notes")
        .map_err(|e| e.to_string())?;
    let notes = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<(String, String)>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    for (id, file_path) in notes {
        // Basename split on both separators: the backup may come from Windows.
        let Some(name) = file_path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()) else {
            continue;
        };
        if !staged_docs.join(name).is_file() {
            continue;
        }
        let path = docs_dir.join(name).to_string_lossy().to_string();
        conn.execute(
            "UPDATE notes SET file_path = ?1 WHERE id = ?2",
            rusqlite::params![path, id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn emit_restore_progress(app: &AppHandle, phase: &str, percent: u32) {
    let _ = app.emit(
        "backup://restore-progress",
        Progress {
            phase: phase.into(),
            percent,
        },
    );
}

/// Decrypt + decompress + unpack `src` into `dest`, reporting extraction
/// progress. `manifest.json` is the first tar entry, so its entry count gives
/// the total. Extraction spans 5..95% (decrypt before, remap/swap after).
fn extract_archive(
    app: Option<&AppHandle>,
    src: &Path,
    password: &str,
    dest: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let file = File::open(src).map_err(|e| e.to_string())?;
    let decryptor = age::Decryptor::new(file).map_err(|e| format!("Cannot read backup: {e}"))?;
    let mut identity = age::scrypt::Identity::new(SecretString::from(password.to_owned()));
    // age caps the accepted work factor relative to *this* machine's speed;
    // a backup made on a faster machine (or read by a debug build) would be
    // rejected as ExcessiveWork. Use a fixed cap instead.
    identity.set_max_work_factor(MAX_SCRYPT_WORK_FACTOR);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| match e {
            age::DecryptError::ExcessiveWork { .. } => {
                "Backup encryption is too expensive to unwrap on this machine".to_string()
            }
            _ => "Wrong password or corrupt backup".to_string(),
        })?;
    let zstd_reader = zstd::stream::read::Decoder::new(reader).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(zstd_reader);

    // manifest.json + profiles.db + entries; refined once the manifest is read.
    let mut total = 2usize;
    let mut done = 0usize;
    let mut last_pct = 0u32;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        // `unpack_in` rejects `..` and absolute paths like `unpack` does.
        let unpacked = entry.unpack_in(dest).map_err(|e| e.to_string())?;
        done += 1;

        if unpacked && done == 1 {
            let is_manifest = entry
                .path()
                .map(|p| p.as_os_str() == "manifest.json")
                .unwrap_or(false);
            if is_manifest {
                if let Some(m) = std::fs::read(dest.join("manifest.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
                {
                    total = m.entries.len() + 2;
                }
            }
        }

        if let Some(app) = app {
            let pct = 5 + pct(done, total.max(done)) * 90 / 100;
            if pct != last_pct {
                last_pct = pct;
                emit_restore_progress(app, "extracting", pct);
            }
        }
    }
    Ok(())
}

/// Replace `profiles.db`, `profiles/`, `notes/` in the live data dir with the
/// staged versions, keeping a temporary backup until the swap succeeds.
fn swap_in(data_dir: &Path, staging: &Path, manifest: &Manifest) -> Result<(), String> {
    let ts = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let old = data_dir.join(format!("restore_backup_{ts}"));
    std::fs::create_dir_all(&old).map_err(|e| e.to_string())?;

    let entries = ["profiles.db", "profiles", "notes"];
    // Move current → old. Also relocate the live DB's WAL/SHM sidecars: leaving
    // a stale `-wal` next to the freshly restored `profiles.db` would let SQLite
    // replay mismatched frames and corrupt it.
    for sidecar in ["profiles.db-wal", "profiles.db-shm"] {
        let live = data_dir.join(sidecar);
        if live.exists() {
            let _ = std::fs::rename(&live, old.join(sidecar));
        }
    }
    // Undo a partial swap: anything saved in `old` goes back to its live path
    // (dropping whatever staged entry may have been placed there meanwhile).
    let rollback = |err: String| {
        let all = ["profiles.db", "profiles", "notes", "profiles.db-wal", "profiles.db-shm"];
        for name in all {
            let saved = old.join(name);
            if !saved.exists() {
                continue;
            }
            let live = data_dir.join(name);
            let _ = std::fs::remove_dir_all(&live);
            let _ = std::fs::remove_file(&live);
            let _ = std::fs::rename(&saved, &live);
        }
        let _ = std::fs::remove_dir_all(&old);
        err
    };
    for name in entries {
        let live = data_dir.join(name);
        if live.exists() {
            rename_retry(&live, &old.join(name))
                .map_err(|e| rollback(format!("Cannot move {name}: {e}")))?;
        }
    }
    // Move staged → live (only what the backup actually contains).
    for name in entries {
        let staged = staging.join(name);
        if staged.exists() {
            rename_retry(&staged, &data_dir.join(name))
                .map_err(|e| rollback(format!("Cannot place {name}: {e}")))?;
        }
    }

    // Restore custom notes dir contents to the recorded path, if any.
    if let Some(ref custom) = manifest.notes_custom_dir {
        let src = staging.join("notes_custom");
        if src.exists() {
            let dst = PathBuf::from(custom);
            if std::fs::create_dir_all(&dst).is_ok() {
                let _ = copy_dir(&src, &dst);
            }
        }
    }

    // Success — drop the safety copy.
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// `fs::rename` with retries: on Windows a just-killed browser or an AV
/// scanner can hold a handle for a moment and fail the rename with a
/// sharing violation.
fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 15;
    const DELAY: std::time::Duration = std::time::Duration::from_millis(200);
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt + 1 < ATTEMPTS => {
                attempt += 1;
                eprintln!("backup restore: rename {} failed ({e}), retry {attempt}", from.display());
                std::thread::sleep(DELAY);
            }
            Err(e) => return Err(e),
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ── Scheduler ───────────────────────────────────────────────────────────────

/// Spawn the background scheduler. The first tick fires immediately so a backup
/// missed while the app was closed is caught up on startup.
pub fn start_backup_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let state = app.state::<AppState>();
            let cfg = load_config(&state.db).await;
            if !cfg.schedule_enabled || cfg.dir.is_none() || cfg.password.is_none() {
                continue;
            }
            if state.backup.running.load(Ordering::SeqCst) {
                continue;
            }
            let last_run = cfg
                .last_run
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            if is_due(&cfg, last_run, Local::now()) {
                if let Err(e) = perform_backup(&app).await {
                    let _ = app.emit("backup://error", e);
                }
            }
        }
    });
}

/// Whether a scheduled backup is due right now given the last run.
fn is_due(cfg: &BackupConfig, last_run: Option<DateTime<Utc>>, now_local: DateTime<Local>) -> bool {
    match cfg.schedule_mode.as_str() {
        "interval" => {
            let hours = cfg.interval_hours.max(1);
            match last_run {
                None => true,
                // Derive "now" from the injected clock so tests control it.
                Some(lr) => now_local.with_timezone(&Utc) - lr >= chrono::Duration::hours(hours),
            }
        }
        "daily" | "weekly" => {
            let (h, m) = parse_hhmm(&cfg.time);
            let scheduled = if cfg.schedule_mode == "weekly" {
                last_scheduled_weekly(now_local, cfg.weekday, h, m)
            } else {
                last_scheduled_daily(now_local, h, m)
            };
            match scheduled {
                None => false,
                Some(sched) => {
                    let sched_utc = sched.with_timezone(&Utc);
                    last_run.map_or(true, |lr| lr < sched_utc)
                }
            }
        }
        _ => false,
    }
}

fn parse_hhmm(s: &str) -> (u32, u32) {
    let mut parts = s.split(':');
    let h = parts.next().and_then(|v| v.parse().ok()).unwrap_or(3);
    let m = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (h.min(23), m.min(59))
}

/// Most recent local datetime at HH:MM that is <= now (today, or yesterday).
fn last_scheduled_daily(now: DateTime<Local>, h: u32, m: u32) -> Option<DateTime<Local>> {
    let today = now.date_naive().and_hms_opt(h, m, 0)?;
    let today_l = Local.from_local_datetime(&today).single()?;
    if today_l <= now {
        Some(today_l)
    } else {
        Local.from_local_datetime(&(today - chrono::Duration::days(1))).single()
    }
}

/// Most recent local datetime on `weekday` (0=Mon..6=Sun) at HH:MM that is <= now.
fn last_scheduled_weekly(
    now: DateTime<Local>,
    weekday: i64,
    h: u32,
    m: u32,
) -> Option<DateTime<Local>> {
    let target = (weekday.rem_euclid(7)) as u32;
    let mut date = now.date_naive();
    for _ in 0..8 {
        if date.weekday().num_days_from_monday() == target {
            if let Some(naive) = date.and_hms_opt(h, m, 0) {
                if let Some(dt) = Local.from_local_datetime(&naive).single() {
                    if dt <= now {
                        return Some(dt);
                    }
                }
            }
        }
        date = date.pred_opt()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn tmp_root(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("vb_backup_test_{tag}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A real tar→zstd→age backup can be decrypted+extracted with the right
    /// password and reproduces the original files byte-for-byte.
    #[test]
    fn archive_roundtrip_reproduces_files() {
        let root = tmp_root("rt");
        let src = root.join("src");
        std::fs::create_dir_all(src.join("profiles/p1/firefox-profile")).unwrap();
        std::fs::write(src.join("profiles/p1/firefox-profile/prefs.js"), b"user_pref(1);").unwrap();
        std::fs::write(src.join("profiles/p1/firefox-profile/startupCache"), b"CACHE").unwrap(); // blacklisted
        std::fs::create_dir_all(src.join("notes")).unwrap();
        std::fs::write(src.join("notes/hello.md"), b"# hello \xE2\x9C\x93").unwrap();

        let db = root.join("db.sqlite");
        std::fs::write(&db, b"SQLITE-SNAPSHOT-BYTES").unwrap();

        let mut files = Vec::new();
        collect_files(&src.join("profiles"), "profiles", &mut files);
        collect_files(&src.join("notes"), "notes", &mut files);

        // Blacklisted cache must be excluded.
        assert!(files.iter().all(|(n, _)| !n.contains("startupCache")));

        let out = root.join("backup.vbk");
        let manifest = br#"{"format_version":1}"#;
        write_archive(None, &out, "s3cret", manifest, &db, &files).expect("write");
        assert!(out.exists() && std::fs::metadata(&out).unwrap().len() > 0);

        let dest = root.join("restored");
        extract_archive(None, &out, "s3cret", &dest).expect("extract");

        assert_eq!(std::fs::read(dest.join("profiles.db")).unwrap(), b"SQLITE-SNAPSHOT-BYTES");
        assert_eq!(
            std::fs::read(dest.join("profiles/p1/firefox-profile/prefs.js")).unwrap(),
            b"user_pref(1);"
        );
        assert_eq!(std::fs::read(dest.join("notes/hello.md")).unwrap(), "# hello ✓".as_bytes());
        assert!(!dest.join("profiles/p1/firefox-profile/startupCache").exists());

        let mut mf = String::new();
        File::open(dest.join("manifest.json")).unwrap().read_to_string(&mut mf).unwrap();
        assert!(mf.contains("format_version"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wrong_password_fails() {
        let root = tmp_root("wp");
        let db = root.join("db");
        std::fs::write(&db, b"x").unwrap();
        let out = root.join("b.vbk");
        write_archive(None, &out, "right", b"{}", &db, &[]).unwrap();

        let dest = root.join("out");
        let err = extract_archive(None, &out, "wrong", &dest);
        assert!(err.is_err(), "wrong password must fail");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn interval_due_logic() {
        let cfg = BackupConfig {
            schedule_mode: "interval".into(),
            interval_hours: 24,
            ..Default::default()
        };
        let now = Local::now();
        assert!(is_due(&cfg, None, now), "never-run is due");
        assert!(is_due(&cfg, Some(Utc::now() - chrono::Duration::hours(25)), now));
        assert!(!is_due(&cfg, Some(Utc::now() - chrono::Duration::hours(1)), now));
    }

    #[test]
    fn daily_catches_up_missed_run() {
        // Daily at 03:00; last run 2 days ago → the most recent 03:00 is later
        // than last_run, so a catch-up is due.
        let cfg = BackupConfig {
            schedule_mode: "daily".into(),
            time: "03:00".into(),
            ..Default::default()
        };
        let now = Local::now();
        assert!(is_due(&cfg, Some(Utc::now() - chrono::Duration::days(2)), now));
        // A run at the current instant is at/after the last scheduled 03:00,
        // so nothing is due.
        assert!(!is_due(&cfg, Some(Utc::now()), now));
    }
}
