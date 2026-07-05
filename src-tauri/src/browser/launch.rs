// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(Clone, Serialize)]
pub struct RunningChangedPayload {
    pub running_ids: Vec<String>,
}

pub fn emit_running_changed(app_handle: &tauri::AppHandle, ids: Vec<String>) {
    app_handle
        .emit("profiles://running-changed", RunningChangedPayload { running_ids: ids })
        .ok();
}

pub struct BrowserProcess {
    pub pid: u32,
    stop_tx: tokio::sync::oneshot::Sender<()>,
    local_proxy_stop: Option<tokio::sync::oneshot::Sender<()>>,
}

pub struct BrowserState {
    pub processes: Arc<Mutex<HashMap<String, BrowserProcess>>>,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl BrowserState {
    pub async fn is_running(&self, profile_id: &str) -> bool {
        self.processes.lock().await.contains_key(profile_id)
    }

    pub async fn get_pid(&self, profile_id: &str) -> Option<u32> {
        self.processes.lock().await.get(profile_id).map(|p| p.pid)
    }

    pub async fn running_ids(&self) -> Vec<String> {
        self.processes.lock().await.keys().cloned().collect()
    }
}

pub async fn launch(
    profile_id: String,
    profile_path: PathBuf,
    browser_binary: PathBuf,
    timezone: Option<String>,
    camoufox_config: Option<serde_json::Value>,
    local_proxy_stop: Option<tokio::sync::oneshot::Sender<()>>,
    state: Arc<BrowserState>,
    db: sqlx::Pool<sqlx::Sqlite>,
    app_handle: tauri::AppHandle,
) -> Result<u32> {
    let firefox_profile_dir = profile_path.join("firefox-profile");
    std::fs::create_dir_all(&firefox_profile_dir)
        .context("Failed to create firefox profile dir")?;

    let mut cmd = tokio::process::Command::new(&browser_binary);
    cmd.arg("--profile")
        .arg(&firefox_profile_dir)
        .arg("--no-remote")
        .arg("--new-instance");

    if let Some(tz) = timezone {
        cmd.env("TZ", tz);
    }

    if let Some(cfg) = camoufox_config {
        cmd.env("CAMOU_CONFIG_1", cfg.to_string());
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn {}", browser_binary.display()))?;

    let pid = child.id().unwrap_or(0);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    {
        let mut procs = state.processes.lock().await;
        if procs.contains_key(&profile_id) {
            // Concurrent launch race: kill the duplicate process and bail
            child.kill().await.ok();
            return Err(anyhow::anyhow!("Profile already running (concurrent launch)"));
        }
        procs.insert(
            profile_id.clone(),
            BrowserProcess { pid, stop_tx, local_proxy_stop },
        );
    } // lock released before tokio::spawn

    let state_clone = Arc::clone(&state);
    let profile_id_clone = profile_id.clone();
    let db_clone = db.clone();
    let app_handle_clone = app_handle.clone();
    #[cfg(target_os = "windows")]
    let firefox_profile_dir_clone = firefox_profile_dir.clone();

    tokio::spawn(async move {
        tokio::select! {
            _ = child.wait() => {
                // On Windows, camoufox.exe is a launcher that exits immediately after
                // spawning the real browser process. Use the Firefox profile lock file
                // to detect when the browser actually exits.
                #[cfg(target_os = "windows")]
                {
                    let lock_file = firefox_profile_dir_clone.join("parent.lock");
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    if lock_file.exists() {
                        while lock_file.exists() {
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        }
                    }
                }
            }
            _ = stop_rx => {
                child.kill().await.ok();
                child.wait().await.ok();
            }
        }
        // Stop local proxy (if any) when browser exits
        if let Some(proc) = state_clone.processes.lock().await.remove(&profile_id_clone) {
            if let Some(ps) = proc.local_proxy_stop {
                ps.send(()).ok();
            }
        }
        sqlx::query(
            "UPDATE profiles SET status = 'stopped', updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&profile_id_clone)
        .execute(&db_clone)
        .await
        .ok();
        let ids = state_clone.running_ids().await;
        emit_running_changed(&app_handle_clone, ids);
    });

    Ok(pid)
}

pub async fn stop(profile_id: &str, state: &BrowserState) -> Result<()> {
    let process = state.processes.lock().await.remove(profile_id);
    if let Some(p) = process {
        if let Some(ps) = p.local_proxy_stop {
            ps.send(()).ok();
        }
        p.stop_tx.send(()).ok();
    }
    Ok(())
}

pub async fn stop_all(state: &BrowserState) {
    let processes: Vec<_> = state.processes.lock().await.drain().collect();
    for (_, p) in processes {
        if let Some(ps) = p.local_proxy_stop {
            ps.send(()).ok();
        }
        p.stop_tx.send(()).ok();
    }
}
