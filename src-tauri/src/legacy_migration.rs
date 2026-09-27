// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Legacy Veydan Browser data migration. Remove in 4.0.
//!
//! Moves `net.veydan.browser/VeydanBrowser` into `net.veydan.space/VeydanSpace`
//! with a full backup and verification. Supported through 3.x; from 4.0 the
//! folder must be moved by hand.
//!
//! Flow: backup (copy) -> verify backup -> move (rename) -> verify new dir +
//! sqlite integrity_check -> init_desktop. Progress and phase changes are
//! emitted as `migration://progress` and `migration://state`.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

const OLD_IDENTIFIER: &str = "net.veydan.browser";
const OLD_SUBDIR: &str = "VeydanBrowser";
const NEW_SUBDIR: &str = "VeydanSpace";

struct Dirs {
    old: PathBuf,
    new: PathBuf,
    backup: PathBuf,
}

#[derive(Serialize, Clone)]
pub struct Pending {
    pub old_dir: String,
    pub new_dir: String,
    pub backup_dir: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Serialize, Clone)]
pub struct Report {
    pub old_dir: String,
    pub new_dir: String,
    pub backup_dir: String,
    pub files: u64,
    pub bytes: u64,
    pub integrity_ok: bool,
    pub duration_ms: u64,
}

#[derive(Serialize, Clone)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    None,
    Pending(Pending),
    Running,
    Done(Report),
    Failed {
        message: String,
        backup_dir: Option<String>,
        /// True while the old folder is untouched (failure before the move).
        data_intact: bool,
    },
}

#[derive(Serialize, Clone)]
struct Progress {
    step: &'static str,
    percent: u32,
    done_bytes: u64,
    total_bytes: u64,
}

pub struct MigrationState {
    phase: Mutex<Phase>,
    dirs: Option<Dirs>,
}

impl MigrationState {
    pub fn none() -> Self {
        Self {
            phase: Mutex::new(Phase::None),
            dirs: None,
        }
    }

    pub fn pending(detected: Detected) -> Self {
        Self {
            phase: Mutex::new(Phase::Pending(detected.pending)),
            dirs: Some(detected.dirs),
        }
    }

    fn set(&self, app: &AppHandle, phase: Phase) {
        if let Ok(mut p) = self.phase.lock() {
            *p = phase.clone();
        }
        let _ = app.emit("migration://state", phase);
    }
}

pub struct Detected {
    dirs: Dirs,
    pending: Pending,
}

/// Legacy data present and the new location still empty.
pub fn detect(app_data_dir: &Path) -> Option<Detected> {
    let parent = app_data_dir.parent()?;
    let old = parent.join(OLD_IDENTIFIER).join(OLD_SUBDIR);
    let new = app_data_dir.join(NEW_SUBDIR);
    if !old.is_dir() || new.exists() {
        return None;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup = parent.join(format!("{OLD_IDENTIFIER}-backup-{stamp}"));
    let tree = walk(&old).ok()?;
    let pending = Pending {
        old_dir: old.display().to_string(),
        new_dir: new.display().to_string(),
        backup_dir: backup.display().to_string(),
        files: tree.len() as u64,
        bytes: tree.values().sum(),
    };
    Some(Detected {
        dirs: Dirs { old, new, backup },
        pending,
    })
}

#[tauri::command]
pub fn migration_status(state: tauri::State<'_, MigrationState>) -> Phase {
    state
        .phase
        .lock()
        .map(|p| p.clone())
        .unwrap_or(Phase::None)
}

/// Start the migration on a background thread. Result arrives via `migration://state`.
#[tauri::command]
pub fn migration_start(app: AppHandle) -> Result<(), String> {
    let state = app.state::<MigrationState>();
    {
        let mut phase = state.phase.lock().map_err(|e| e.to_string())?;
        if !matches!(*phase, Phase::Pending(_)) {
            return Err("migration is not pending".into());
        }
        *phase = Phase::Running;
    }
    let _ = app.emit("migration://state", Phase::Running);
    std::thread::spawn(move || run(app));
    Ok(())
}

#[tauri::command]
pub fn migration_quit(app: AppHandle) {
    app.exit(0);
}

fn run(app: AppHandle) {
    let state = app.state::<MigrationState>();
    let Some(dirs) = state.dirs.as_ref() else {
        state.set(&app, fail("no legacy directories resolved", None, true));
        return;
    };
    let started = Instant::now();
    let backup_str = dirs.backup.display().to_string();

    // 1) Full copy of the old folder. On error the partial backup is removed.
    let tree = match walk(&dirs.old) {
        Ok(t) => t,
        Err(e) => {
            state.set(&app, fail(&format!("scan failed: {e}"), None, true));
            return;
        }
    };
    let total: u64 = tree.values().sum();
    emit_progress(&app, "backup", 0, 0, total);
    let copied = copy_tree(&dirs.old, &dirs.backup, &tree, &mut |percent, done| {
        emit_progress(&app, "backup", percent, done, total)
    });
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&dirs.backup);
        state.set(&app, fail(&format!("backup failed: {e}"), None, true));
        return;
    }

    // 2) Backup must match the source exactly.
    emit_progress(&app, "verify_backup", 100, total, total);
    if let Err(e) = compare_trees(&dirs.old, &dirs.backup) {
        let _ = std::fs::remove_dir_all(&dirs.backup);
        state.set(&app, fail(&format!("backup verification failed: {e}"), None, true));
        return;
    }

    // 3) Atomic move within the same parent directory.
    emit_progress(&app, "move", 100, total, total);
    let moved = dirs
        .new
        .parent()
        .ok_or_else(|| "new dir has no parent".to_string())
        .and_then(|p| std::fs::create_dir_all(p).map_err(|e| e.to_string()))
        .and_then(|_| std::fs::rename(&dirs.old, &dirs.new).map_err(|e| e.to_string()));
    if let Err(e) = moved {
        state.set(
            &app,
            fail(&format!("move failed: {e}"), Some(backup_str), true),
        );
        return;
    }

    // 4) New location must match the backup; database must be consistent.
    emit_progress(&app, "verify", 100, total, total);
    if let Err(e) = compare_trees(&dirs.backup, &dirs.new) {
        state.set(
            &app,
            fail(&format!("verification failed: {e}"), Some(backup_str), false),
        );
        return;
    }
    if let Err(e) = integrity_check(&dirs.new.join("profiles.db")) {
        state.set(
            &app,
            fail(&format!("database check failed: {e}"), Some(backup_str), false),
        );
        return;
    }

    // 5) Regular startup on the new folder.
    emit_progress(&app, "starting", 100, total, total);
    if let Err(e) = crate::init_desktop(&app, dirs.new.clone()) {
        state.set(
            &app,
            fail(&format!("startup failed: {e}"), Some(backup_str), false),
        );
        return;
    }

    state.set(
        &app,
        Phase::Done(Report {
            old_dir: dirs.old.display().to_string(),
            new_dir: dirs.new.display().to_string(),
            backup_dir: backup_str,
            files: tree.len() as u64,
            bytes: total,
            integrity_ok: true,
            duration_ms: started.elapsed().as_millis() as u64,
        }),
    );
}

fn fail(message: &str, backup_dir: Option<String>, data_intact: bool) -> Phase {
    eprintln!("legacy migration: {message}");
    Phase::Failed {
        message: message.to_string(),
        backup_dir,
        data_intact,
    }
}

fn emit_progress(app: &AppHandle, step: &'static str, percent: u32, done: u64, total: u64) {
    let _ = app.emit(
        "migration://progress",
        Progress {
            step,
            percent,
            done_bytes: done,
            total_bytes: total,
        },
    );
}

/// Relative file path -> size for every regular file under `root`.
/// Symlinks are followed; dangling ones are skipped.
pub fn walk(root: &Path) -> std::io::Result<BTreeMap<PathBuf, u64>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, u64>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) if path.symlink_metadata().is_ok() => continue,
                Err(e) => return Err(e),
            };
            if meta.is_dir() {
                visit(root, &path, out)?;
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(rel, meta.len());
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out)?;
    Ok(out)
}

/// Copy every file from `tree`; `on_progress(percent, done_bytes)` fires on each whole percent.
pub fn copy_tree(
    src: &Path,
    dst: &Path,
    tree: &BTreeMap<PathBuf, u64>,
    on_progress: &mut dyn FnMut(u32, u64),
) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let total: u64 = tree.values().sum();
    let mut done: u64 = 0;
    let mut last_percent: u32 = 0;
    for (rel, _) in tree {
        let to = dst.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        done += std::fs::copy(src.join(rel), &to)?;
        let percent = if total == 0 {
            100
        } else {
            ((done * 100) / total) as u32
        };
        if percent != last_percent {
            last_percent = percent;
            on_progress(percent, done);
        }
    }
    Ok(())
}

/// Both trees must contain the same relative paths with the same sizes.
pub fn compare_trees(a: &Path, b: &Path) -> Result<(), String> {
    let ta = walk(a).map_err(|e| format!("scan {}: {e}", a.display()))?;
    let tb = walk(b).map_err(|e| format!("scan {}: {e}", b.display()))?;
    if ta.len() != tb.len() {
        return Err(format!("file count differs: {} vs {}", ta.len(), tb.len()));
    }
    for (rel, size) in &ta {
        match tb.get(rel) {
            Some(s) if s == size => {}
            Some(s) => {
                return Err(format!(
                    "size differs for {}: {size} vs {s}",
                    rel.display()
                ))
            }
            None => return Err(format!("missing {}", rel.display())),
        }
    }
    Ok(())
}

pub fn integrity_check(db_path: &Path) -> Result<(), String> {
    if !db_path.is_file() {
        return Err(format!("{} not found", db_path.display()));
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if result == "ok" {
        Ok(())
    } else {
        Err(result)
    }
}
