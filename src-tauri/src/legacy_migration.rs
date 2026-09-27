// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Legacy Veydan Browser data migration. Remove in 4.0.
//!
//! Moves `net.veydan.browser/VeydanBrowser` into `net.veydan.space/VeydanSpace`
//! with a full backup and verification. Supported through 3.x; from 4.0 the
//! folder must be moved by hand.
//!
//! Flow: backup (copy) -> verify backup -> move (rename) -> verify new dir +
//! sqlite integrity_check -> init_desktop -> shortcuts (Windows). Progress and
//! phase changes are emitted as `migration://progress` and `migration://state`.
//!
//! The webview profile (localStorage: language, theme, layout) lives under the
//! app identifier too and is copied before Tauri starts, see `migrate_webview_profile`.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

const OLD_IDENTIFIER: &str = "net.veydan.browser";
const NEW_IDENTIFIER: &str = "net.veydan.space";
const OLD_SUBDIR: &str = "VeydanBrowser";
const NEW_SUBDIR: &str = "VeydanSpace";
#[cfg(target_os = "windows")]
const OLD_PRODUCT: &str = "Veydan Browser";
#[cfg(target_os = "windows")]
const NEW_PRODUCT: &str = "Veydan Space";

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
    /// Start menu / desktop / taskbar shortcuts now point at Veydan Space (Windows).
    pub shortcuts_updated: bool,
    /// The old Veydan Browser install was removed (Windows).
    pub old_uninstalled: bool,
    /// Non-fatal problems from the shortcut step; the data is already in place.
    pub warnings: Vec<String>,
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

    // 6) Windows only: point the old shortcuts at this binary, drop the old install.
    // Failures here are warnings; the data is already migrated.
    emit_progress(&app, "shortcuts", 100, total, total);
    let install = windows_install::finish();

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
            shortcuts_updated: install.shortcuts_updated,
            old_uninstalled: install.old_uninstalled,
            warnings: install.warnings,
        }),
    );
}

// ── Webview profile ──────────────────────────────────────────────────────────

/// Platform app data root, the same one Tauri's `app_data_dir()` is built on.
fn data_root() -> Option<PathBuf> {
    dirs::data_dir()
}

/// True while the legacy folder exists and the new one does not (same rule as `detect`).
fn legacy_pending() -> bool {
    let Some(root) = data_root() else { return false };
    let old = root.join(OLD_IDENTIFIER).join(OLD_SUBDIR);
    let new = root.join(NEW_IDENTIFIER).join(NEW_SUBDIR);
    old.is_dir() && !new.exists()
}

/// Where the webview keeps localStorage for one identifier.
fn webview_storage_dir(identifier: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Some(
            dirs::data_local_dir()?
                .join(identifier)
                .join("EBWebView")
                .join("Default")
                .join("Local Storage"),
        )
    }
    #[cfg(target_os = "macos")]
    {
        Some(
            dirs::home_dir()?
                .join("Library")
                .join("WebKit")
                .join(identifier)
                .join("WebsiteData")
                .join("LocalStorage"),
        )
    }
    #[cfg(target_os = "linux")]
    {
        Some(dirs::data_dir()?.join(identifier).join("localstorage"))
    }
}

/// Copy the old webview localStorage into the new identifier's profile.
/// Must run before Tauri creates a window: the new profile is still empty then.
pub fn migrate_webview_profile() {
    if !legacy_pending() {
        return;
    }
    let (Some(old), Some(new)) = (
        webview_storage_dir(OLD_IDENTIFIER),
        webview_storage_dir(NEW_IDENTIFIER),
    ) else {
        return;
    };
    if !old.is_dir() || new.exists() {
        return;
    }
    let tree = match walk(&old) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("legacy migration: webview profile scan failed: {e}");
            return;
        }
    };
    if let Err(e) = copy_tree(&old, &new, &tree, &mut |_, _| {}) {
        eprintln!("legacy migration: webview profile copy failed: {e}");
        let _ = std::fs::remove_dir_all(&new);
    }
}

// ── Old install (Windows) ────────────────────────────────────────────────────

pub struct InstallOutcome {
    pub shortcuts_updated: bool,
    pub old_uninstalled: bool,
    pub warnings: Vec<String>,
}

#[cfg(target_os = "windows")]
mod windows_install {
    use super::{InstallOutcome, NEW_PRODUCT, OLD_IDENTIFIER, OLD_PRODUCT};
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Retarget Veydan Browser shortcuts to this exe, then run the old uninstaller.
    /// One PowerShell script; each line of its output is a status flag or a warning.
    const SCRIPT: &str = r#"
param([string]$Exe, [string]$OldName, [string]$NewName, [string]$OldId)
$ErrorActionPreference = 'Continue'
$dir = Split-Path -Parent $Exe
$ws = New-Object -ComObject WScript.Shell
$updated = $false
function Retarget([string]$Folder) {
  $old = Join-Path $Folder "$OldName.lnk"
  if (-not (Test-Path -LiteralPath $old)) { return $false }
  $new = Join-Path $Folder "$NewName.lnk"
  try {
    $s = $ws.CreateShortcut($old)
    $s.TargetPath = $Exe
    $s.WorkingDirectory = $dir
    $s.IconLocation = "$Exe,0"
    $s.Save()
    if (Test-Path -LiteralPath $new) { Remove-Item -LiteralPath $old -Force }
    else { Rename-Item -LiteralPath $old -NewName "$NewName.lnk" }
    return $true
  } catch {
    Write-Output "WARN shortcut $old`: $($_.Exception.Message)"
    return $false
  }
}
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$desktop = [Environment]::GetFolderPath('Desktop')
$taskbar = Join-Path $env:APPDATA 'Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar'
foreach ($f in @($startMenu, $desktop, $taskbar)) {
  if (Retarget $f) { $updated = $true }
}
if (-not (Test-Path -LiteralPath (Join-Path $startMenu "$NewName.lnk"))) {
  try {
    $s = $ws.CreateShortcut((Join-Path $startMenu "$NewName.lnk"))
    $s.TargetPath = $Exe
    $s.WorkingDirectory = $dir
    $s.IconLocation = "$Exe,0"
    $s.Save()
    $updated = $true
  } catch { Write-Output "WARN start menu: $($_.Exception.Message)" }
}
if ($updated) { Write-Output 'SHORTCUTS_OK' }
$keys = @(
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$OldId",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$OldId"
)
foreach ($k in $keys) {
  if (-not (Test-Path -LiteralPath $k)) { continue }
  $cmd = (Get-ItemProperty -LiteralPath $k -ErrorAction SilentlyContinue).UninstallString
  if (-not $cmd) { continue }
  $uninst = $cmd.Trim('"')
  if (-not (Test-Path -LiteralPath $uninst)) { Write-Output "WARN uninstaller missing: $uninst"; continue }
  try {
    $p = Start-Process -FilePath $uninst -ArgumentList '/S' -Wait -PassThru
    if ($p.ExitCode -eq 0) { Write-Output 'UNINSTALL_OK' }
    else { Write-Output "WARN uninstaller exit code $($p.ExitCode)" }
  } catch { Write-Output "WARN uninstall: $($_.Exception.Message)" }
  break
}
"#;

    pub fn finish() -> InstallOutcome {
        let mut out = InstallOutcome {
            shortcuts_updated: false,
            old_uninstalled: false,
            warnings: Vec::new(),
        };
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                out.warnings.push(format!("current exe: {e}"));
                return out;
            }
        };
        let script = std::env::temp_dir().join("veydan-space-migrate.ps1");
        if let Err(e) = std::fs::write(&script, SCRIPT) {
            out.warnings.push(format!("script write: {e}"));
            return out;
        }
        let result = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .arg("-Exe")
            .arg(&exe)
            .arg("-OldName")
            .arg(OLD_PRODUCT)
            .arg("-NewName")
            .arg(NEW_PRODUCT)
            .arg("-OldId")
            .arg(OLD_IDENTIFIER)
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let _ = std::fs::remove_file(&script);
        let output = match result {
            Ok(o) => o,
            Err(e) => {
                out.warnings.push(format!("powershell: {e}"));
                return out;
            }
        };
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            match line.trim() {
                "SHORTCUTS_OK" => out.shortcuts_updated = true,
                "UNINSTALL_OK" => out.old_uninstalled = true,
                l if l.starts_with("WARN ") => out.warnings.push(l[5..].to_string()),
                _ => {}
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() && !stderr.trim().is_empty() {
            out.warnings.push(stderr.trim().to_string());
        }
        out
    }
}

#[cfg(not(target_os = "windows"))]
mod windows_install {
    use super::InstallOutcome;

    /// Linux and macOS keep their launchers: the updater replaces the binary in place.
    pub fn finish() -> InstallOutcome {
        InstallOutcome {
            shortcuts_updated: false,
            old_uninstalled: false,
            warnings: Vec::new(),
        }
    }
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
