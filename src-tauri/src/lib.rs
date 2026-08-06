// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

mod browser;
// Public so integration smoke examples (examples/*.rs) can exercise command internals.
pub mod commands;
mod db;
pub mod error;
mod fingerprint;
mod models;
mod proxy;
mod tray;

use browser::launch::BrowserState;
use commands::backup::{
    backup_get_config, backup_list, backup_restore, backup_run_now, backup_set_config,
    start_backup_scheduler, BackupManager,
};
use commands::settings::{tray_set_labels, tray_settings_get, tray_settings_set, window_minimize};
use commands::camoufox::{
    camoufox_download, camoufox_download_cancel, camoufox_download_state, camoufox_latest_version,
    camoufox_status, DownloadManager,
};
use commands::notes::{
    note_archive, note_create, note_delete, note_draft_discard, note_draft_get, note_draft_save, note_get,
    note_list, note_open_external, note_open_folder, note_reindex, note_restore, note_search,
    note_set_tags, note_sync, note_tag_list, note_tag_create, note_tag_delete, note_tag_update,
    note_folder_list, note_folder_create, note_folder_update, note_folder_delete,
    note_add_folder, note_remove_folder, note_add_binding, note_remove_binding,
    note_update, notes_get_dir, notes_set_dir,
    note_history_list, note_history_get, note_history_diff,
    note_history_restore, note_history_merge,
};
use commands::password::{
    pwgen_history_add, pwgen_history_clear, pwgen_history_list, pwgen_history_trim,
};
use commands::totp::{
    totp_add, totp_delete, totp_generate_code, totp_generate_codes, totp_list, totp_preview_uri,
    totp_update,
};
use commands::profiles::*;
use commands::proxies::*;
use commands::sftp::{
    sftp_chmod, sftp_connect, sftp_create_file, sftp_delete, sftp_disconnect, sftp_home,
    sftp_list, sftp_mkdir, sftp_rename, sftp_respond_prompt, sftp_session_list, sftp_stat,
    SftpSessions,
};
use commands::transfer::{sftp_transfer_cancel, sftp_transfer_start};
use commands::fs::{
    fs_chmod, fs_create_file, fs_delete, fs_home, fs_list, fs_mkdir, fs_rename, fs_stat,
};
use commands::ssh::{
    ssh_connect, ssh_connection_create, ssh_connection_delete, ssh_connection_get,
    ssh_connection_list, ssh_connection_trust_fingerprint, ssh_connection_update,
    ssh_disconnect, ssh_resize, ssh_send_data, ssh_session_list, ssh_session_remove,
    ssh_respond_prompt, SshSessions,
};
use commands::ssh_keys::{
    ssh_key_delete, ssh_key_generate, ssh_key_get, ssh_key_import, ssh_key_list, ssh_key_update,
};
use commands::workspaces::*;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::tray::TrayIcon;
use tauri::{Listener, Manager};
use tray::TrayLabels;

/// Tray-behavior settings, kept as atomics for cheap access from the window
/// event handler. Mirrors the `minimize_to_tray` / `close_to_tray` /
/// `start_hidden` keys in `app_settings`.
#[derive(Default)]
pub struct TraySettings {
    pub minimize_to_tray: AtomicBool,
    pub close_to_tray: AtomicBool,
    pub start_hidden: AtomicBool,
}

pub struct AppState {
    pub db: Pool<Sqlite>,
    pub browser: Arc<BrowserState>,
    pub app_data_dir: PathBuf,
    pub download: DownloadManager,
    pub notes_custom_dir: Arc<std::sync::RwLock<Option<PathBuf>>>,
    pub ssh_sessions: SshSessions,
    pub sftp_sessions: SftpSessions,
    pub backup: Arc<BackupManager>,
    pub tray_settings: Arc<TraySettings>,
    pub tray_labels: Arc<Mutex<TrayLabels>>,
    pub tray: Arc<Mutex<Option<TrayIcon>>>,
}

/// Read a boolean flag from `app_settings` (stored as "1"/"0"), defaulting to
/// `false` when the key is absent.
async fn read_bool_setting(db: &Pool<Sqlite>, key: &str) -> bool {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[tauri::command]
fn fingerprint_presets() -> Vec<fingerprint::PresetInfo> {
    fingerprint::list_presets()
}

/// Open an external URL in the user's default browser.
/// Only http(s) URLs are accepted, so this can never launch an arbitrary
/// program or open a local file.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Whether the updater can install updates in-place for this install method.
/// On Linux only AppImage is updatable; deb/rpm installs must download manually.
#[tauri::command]
fn update_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let data_dir = app_data_dir.join("VeydanBrowser");
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(data_dir.join("profiles"))?;
            std::fs::create_dir_all(data_dir.join("notes").join("documents"))?;
            std::fs::create_dir_all(data_dir.join("notes").join("attachments"))?;
            std::fs::create_dir_all(data_dir.join("notes").join("drafts"))?;

            let db_path = data_dir.join("profiles.db");
            let db = tauri::async_runtime::block_on(db::init_pool(&db_path))
                .expect("Failed to initialize database");

            // Load custom notes dir from settings (if set)
            let notes_custom_dir = tauri::async_runtime::block_on(
                sqlx::query_scalar::<_, String>(
                    "SELECT value FROM app_settings WHERE key = 'notes_custom_dir'",
                )
                .fetch_optional(&db),
            )
            .ok()
            .flatten()
            .map(PathBuf::from);

            // Load tray-behavior settings
            let tray_settings = Arc::new(TraySettings::default());
            tauri::async_runtime::block_on(async {
                tray_settings
                    .minimize_to_tray
                    .store(read_bool_setting(&db, "minimize_to_tray").await, Ordering::Relaxed);
                tray_settings
                    .close_to_tray
                    .store(read_bool_setting(&db, "close_to_tray").await, Ordering::Relaxed);
                tray_settings
                    .start_hidden
                    .store(read_bool_setting(&db, "start_hidden").await, Ordering::Relaxed);
            });

            app.manage(AppState {
                db,
                browser: Arc::new(BrowserState::default()),
                app_data_dir: data_dir.clone(),
                download: DownloadManager::default(),
                notes_custom_dir: Arc::new(std::sync::RwLock::new(notes_custom_dir)),
                ssh_sessions: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
                sftp_sessions: Arc::new(commands::sftp::SftpState::default()),
                backup: Arc::new(BackupManager::default()),
                tray_settings: tray_settings.clone(),
                tray_labels: Arc::new(Mutex::new(TrayLabels::default())),
                tray: Arc::new(Mutex::new(None)),
            });

            commands::notes::start_notes_watcher(app.handle().clone(), data_dir, {
                let state = app.state::<AppState>();
                state.notes_custom_dir.read().ok().and_then(|g| g.clone())
            });

            // Scheduled backups: ticks every 60s, catches up missed runs on start.
            start_backup_scheduler(app.handle().clone());

            // ── System tray ──
            let want_tray = tray_settings.minimize_to_tray.load(Ordering::Relaxed)
                || tray_settings.close_to_tray.load(Ordering::Relaxed)
                || tray_settings.start_hidden.load(Ordering::Relaxed);
            if want_tray {
                tray::apply_tray_async(app.handle(), true);
            }
            if tray_settings.start_hidden.load(Ordering::Relaxed) {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            // In minimize-to-tray mode the window is represented only by the
            // tray icon — keep it out of the taskbar.
            if tray_settings.minimize_to_tray.load(Ordering::Relaxed) {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_skip_taskbar(true);
                }
            }

            // Hide-on-close, gated by the live setting (cross-platform). Also
            // handles minimize-to-tray on Windows/macOS via Resized. On Linux
            // the minimize detection uses a native GTK signal instead (below),
            // because WindowEvent::Resized / is_minimized() are unreliable there.
            if let Some(win) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                win.on_window_event(move |event| {
                    let state = handle.state::<AppState>();
                    match event {
                        #[cfg(not(target_os = "linux"))]
                        tauri::WindowEvent::Resized(_) => {
                            if state.tray_settings.minimize_to_tray.load(Ordering::Relaxed) {
                                if let Some(w) = handle.get_webview_window("main") {
                                    if w.is_minimized().unwrap_or(false) {
                                        let _ = w.unminimize();
                                        let _ = w.hide();
                                    }
                                }
                            }
                        }
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            if state.tray_settings.close_to_tray.load(Ordering::Relaxed) {
                                api.prevent_close();
                                if let Some(w) = handle.get_webview_window("main") {
                                    // Hide → the window fully leaves the taskbar
                                    // (tray-only). The dead-decoration-on-restore
                                    // Wayland quirk is handled on show() by the
                                    // tray's schedule_decoration_fix.
                                    let _ = w.hide();
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }

            // Keep the tray's running-profiles submenu + tooltip in sync.
            let tray_handle = app.handle().clone();
            app.listen("profiles://running-changed", move |_| {
                tray::refresh_tray_async(&tray_handle);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            fingerprint_presets,
            open_url,
            update_supported,
            // Tray / UI settings
            tray_settings_get,
            tray_settings_set,
            tray_set_labels,
            window_minimize,
            // Backup
            backup_get_config,
            backup_set_config,
            backup_list,
            backup_run_now,
            backup_restore,
            // Profiles
            profiles_list,
            profile_get,
            profile_create,
            profile_update,
            profile_delete,
            profile_clone,
            profile_launch,
            profile_stop,
            profile_is_running,
            profiles_running_ids,
            // Proxies
            proxies_list,
            proxy_get,
            proxy_create,
            proxies_bulk_create,
            proxy_update,
            proxy_delete,
            proxy_check,
            proxy_trust_fingerprint,
            // Workspaces
            workspace_list,
            workspace_get,
            workspace_create,
            workspace_update,
            workspace_delete,
            workspace_stats,
            profiles_list_by_workspace,
            workspace_column_list,
            workspace_column_create,
            workspace_column_update,
            workspace_column_delete,
            profile_set_tags,
            profile_move_to_kanban_column,
            profile_raw_data,
            // Export / Import
            profile_export_json,
            profile_export_zip,
            profile_import_json,
            profile_import_zip,
            profile_import_zip_data,
            profile_import_cookies,
            profile_export_cookies,
            profile_export_cookies_to_file,
            profile_export_json_to_file,
            // Camoufox
            camoufox_status,
            camoufox_download,
            camoufox_download_state,
            camoufox_download_cancel,
            camoufox_latest_version,
            // Password generator
            pwgen_history_list,
            pwgen_history_add,
            pwgen_history_clear,
            pwgen_history_trim,
            // TOTP
            totp_list,
            totp_add,
            totp_update,
            totp_delete,
            totp_generate_code,
            totp_generate_codes,
            totp_preview_uri,
            // Notes
            note_list,
            note_get,
            note_create,
            note_update,
            note_delete,
            note_archive,
            note_restore,
            note_set_tags,
            note_search,
            note_sync,
            note_reindex,
            note_open_folder,
            note_open_external,
            note_draft_save,
            note_draft_get,
            note_draft_discard,
            note_tag_list,
            note_tag_create,
            note_tag_delete,
            note_tag_update,
            note_folder_list,
            note_folder_create,
            note_folder_update,
            note_folder_delete,
            note_add_folder,
            note_remove_folder,
            note_add_binding,
            note_remove_binding,
            notes_get_dir,
            notes_set_dir,
            note_history_list,
            note_history_get,
            note_history_diff,
            note_history_restore,
            note_history_merge,
            // SSH
            ssh_connection_list,
            ssh_connection_get,
            ssh_connection_create,
            ssh_connection_update,
            ssh_connection_delete,
            ssh_connection_trust_fingerprint,
            ssh_connect,
            ssh_disconnect,
            ssh_send_data,
            ssh_resize,
            ssh_session_list,
            ssh_session_remove,
            ssh_respond_prompt,
            // SSH keys store
            ssh_key_list,
            ssh_key_get,
            ssh_key_import,
            ssh_key_generate,
            ssh_key_update,
            ssh_key_delete,
            // SFTP file browser
            sftp_connect,
            sftp_disconnect,
            sftp_session_list,
            sftp_home,
            sftp_list,
            sftp_stat,
            sftp_respond_prompt,
            sftp_transfer_start,
            sftp_transfer_cancel,
            sftp_mkdir,
            sftp_create_file,
            sftp_rename,
            sftp_delete,
            sftp_chmod,
            // Local filesystem (file browser)
            fs_home,
            fs_list,
            fs_stat,
            fs_mkdir,
            fs_create_file,
            fs_rename,
            fs_delete,
            fs_chmod,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                // stop_all signals every browser and then awaits the monitor
                // tasks (bounded) so the kills actually complete before the
                // process exits — otherwise children would be orphaned.
                tauri::async_runtime::block_on(browser::launch::stop_all(&state.browser));
            }
        });
}
