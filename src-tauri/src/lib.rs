// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! One library for every platform. The shared core (notes, TOTP, sync, settings
//! storage) compiles everywhere; browser profiles, proxies, SSH/SFTP, tray,
//! updater and the capture bridge are `cfg(desktop)`.

#[cfg(desktop)]
mod browser;
#[cfg(desktop)]
pub mod capture;
// Public so integration smoke examples (examples/*.rs) can exercise command internals.
pub mod commands;
mod db;
pub mod error;
#[cfg(desktop)]
mod fingerprint;
#[cfg(desktop)]
mod models;
#[cfg(desktop)]
mod proxy;
mod sync;
#[cfg(desktop)]
mod tray;

#[cfg(desktop)]
use browser::launch::BrowserState;
#[cfg(desktop)]
use commands::backup::{
    backup_get_config, backup_list, backup_restore, backup_run_now, backup_set_config,
    start_backup_scheduler, BackupManager,
};
#[cfg(desktop)]
use commands::camoufox::{
    camoufox_download, camoufox_download_cancel, camoufox_download_state, camoufox_latest_version,
    camoufox_status, DownloadManager,
};
use commands::demo::{app_clear_data, demo_seed};
use commands::notes::{
    note_add_binding, note_add_folder, note_archive, note_attachment_add_from_path,
    note_attachment_delete, note_attachment_fetch, note_attachment_list, note_attachment_read,
    note_attachment_save, note_backlinks, note_create, note_delete, note_delete_many,
    note_draft_discard, note_draft_get, note_draft_save, note_folder_create, note_folder_delete,
    note_folder_list, note_folder_update, note_get, note_history_diff, note_history_get,
    note_history_list, note_history_merge, note_history_restore, note_links, note_list, note_nav,
    note_reindex, note_related, note_remove_binding, note_remove_folder, note_resolve_link,
    note_restore, note_search, note_set_folder, note_set_tags, note_smart_view_create,
    note_smart_view_delete, note_smart_view_list, note_smart_view_update, note_sync,
    note_tag_create, note_tag_delete, note_tag_list, note_tag_update, note_trash_empty,
    note_update, notes_attachment_policy_get, notes_attachment_policy_set, notes_lock_lock,
    notes_lock_set, notes_lock_status, notes_lock_timeout_set, notes_lock_touch, notes_lock_unlock,
};
#[cfg(desktop)]
use commands::settings::{
    app_locale_get, app_locale_set, tray_set_labels, tray_settings_get, tray_settings_set,
    window_minimize,
};
// Desktop only: OS integration (open in editor / file manager, clipboard, dialogs, extra windows).
#[cfg(desktop)]
use commands::fs::{
    fs_chmod, fs_create_file, fs_delete, fs_home, fs_list, fs_mkdir, fs_rename, fs_stat,
};
use commands::media::media_grant_access;
#[cfg(desktop)]
use commands::notes::{
    clipboard_file_paths, note_attachment_add, note_attachment_open, note_attachments_gc,
    note_export, note_import, note_open_external, note_open_folder, note_open_window,
    notes_capture_rules_get, notes_capture_rules_set, notes_get_dir, notes_set_dir,
    open_quick_capture, quick_capture_shortcut_get, quick_capture_shortcut_set,
};
use commands::password::{
    pwgen_history_add, pwgen_history_clear, pwgen_history_list, pwgen_history_trim,
};
#[cfg(desktop)]
use commands::profiles::*;
#[cfg(desktop)]
use commands::proxies::*;
#[cfg(desktop)]
use commands::sftp::{
    sftp_chmod, sftp_connect, sftp_create_file, sftp_delete, sftp_disconnect, sftp_home, sftp_list,
    sftp_mkdir, sftp_rename, sftp_respond_prompt, sftp_session_list, sftp_stat, SftpSessions,
};
#[cfg(desktop)]
use commands::ssh::{
    ssh_connect, ssh_connection_create, ssh_connection_delete, ssh_connection_get,
    ssh_connection_list, ssh_connection_trust_fingerprint, ssh_connection_update, ssh_disconnect,
    ssh_resize, ssh_respond_prompt, ssh_send_data, ssh_session_list, ssh_session_remove,
    SshSessions,
};
#[cfg(desktop)]
use commands::ssh_keys::{
    ssh_key_delete, ssh_key_export_private, ssh_key_generate, ssh_key_get, ssh_key_import,
    ssh_key_list, ssh_key_update,
};
use commands::totp::{
    totp_add, totp_delete, totp_generate_code, totp_generate_codes, totp_list, totp_preview_uri,
    totp_update,
};
#[cfg(desktop)]
use commands::transfer::{sftp_transfer_cancel, sftp_transfer_start};
#[cfg(desktop)]
use commands::workspaces::*;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use sync::{
    note_sync_info, start_sync_scheduler, sync_attachment_cancel, sync_change_passphrase,
    sync_conflict_get, sync_conflict_resolve, sync_create_vault, sync_debug_clear, sync_debug_get,
    sync_debug_set, sync_get_config, sync_join_vault, sync_leave, sync_probe, sync_run_now,
    sync_set_config, sync_status, sync_trigger, SyncManager,
};
#[cfg(desktop)]
use sync::{sync_profile_files_push_mine, sync_profile_files_take_remote};
#[cfg(desktop)]
use tauri::tray::TrayIcon;
#[cfg(desktop)]
use tauri::Listener;
use tauri::Manager;
#[cfg(desktop)]
use tray::TrayLabels;

/// Tray-behavior settings, kept as atomics for cheap access from the window
/// event handler. Mirrors the `minimize_to_tray` / `close_to_tray` /
/// `start_hidden` keys in `app_settings`.
#[cfg(desktop)]
#[derive(Default)]
pub struct TraySettings {
    pub minimize_to_tray: AtomicBool,
    pub close_to_tray: AtomicBool,
    pub start_hidden: AtomicBool,
}

pub struct AppState {
    pub db: Pool<Sqlite>,
    pub app_data_dir: PathBuf,
    pub notes_custom_dir: Arc<std::sync::RwLock<Option<PathBuf>>>,
    /// Notes dir watcher; dropped before a backup restore releases its handle.
    pub notes_watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
    pub notes_lock: commands::notes::NotesLock,
    pub sync: Arc<SyncManager>,
    #[cfg(desktop)]
    pub browser: Arc<BrowserState>,
    #[cfg(desktop)]
    pub download: DownloadManager,
    #[cfg(desktop)]
    pub ssh_sessions: SshSessions,
    #[cfg(desktop)]
    pub sftp_sessions: SftpSessions,
    #[cfg(desktop)]
    pub backup: Arc<BackupManager>,
    #[cfg(desktop)]
    pub tray_settings: Arc<TraySettings>,
    #[cfg(desktop)]
    pub tray_labels: Arc<Mutex<TrayLabels>>,
    #[cfg(desktop)]
    pub tray: Arc<Mutex<Option<TrayIcon>>>,
}

/// Runtime facts for the settings screen.
#[derive(serde::Serialize)]
pub struct HostInfo {
    os: &'static str,
    arch: &'static str,
    version: &'static str,
}

#[tauri::command]
fn host_info() -> HostInfo {
    HostInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// Create the notes directory layout under `data_dir`.
fn ensure_notes_dirs(data_dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir.join("notes").join("documents"))?;
    std::fs::create_dir_all(data_dir.join("notes").join("attachments"))?;
    std::fs::create_dir_all(data_dir.join("notes").join("drafts"))
}

/// Read a boolean flag from `app_settings` (stored as "1"/"0"), defaulting to
/// `false` when the key is absent.
#[cfg(desktop)]
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

#[cfg(desktop)]
#[tauri::command]
fn fingerprint_presets() -> Vec<fingerprint::PresetInfo> {
    fingerprint::list_presets()
}

/// Open an http(s) URL in the default browser, or a mailto: link in the mail client.
/// The opener plugin never goes through a shell, so URL contents cannot become commands.
#[cfg(desktop)]
#[tauri::command]
fn open_url(url: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err("only http(s) and mailto URLs are allowed".into());
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Whether the updater can install updates in-place for this install method.
/// On Linux only AppImage is updatable; deb/rpm installs must download manually.
#[cfg(desktop)]
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
    #[cfg(desktop)]
    run_desktop();
    #[cfg(mobile)]
    run_mobile();
}

/// Mobile: shared core only, data in `app_data_dir/veydan.db` (same path as the
/// standalone mobile app, so existing installs keep their notes and vault).
#[cfg(mobile)]
fn run_mobile() {
    tauri::Builder::default()
        .plugin(tauri_plugin_barcode_scanner::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            ensure_notes_dirs(&data_dir)?;

            let (db, legacy) =
                tauri::async_runtime::block_on(db::init_pool_mobile(&data_dir.join("veydan.db")))
                    .expect("Failed to initialize database");
            if let Err(e) =
                tauri::async_runtime::block_on(sync::check_install_marker(&db, &data_dir))
            {
                eprintln!("sync: install marker check failed: {e}");
            }
            if legacy {
                // Old index has no FTS row ids or link table; rebuild both from the note files.
                if let Err(e) = tauri::async_runtime::block_on(commands::notes::sync_notes_index(
                    &db, &data_dir, None,
                )) {
                    eprintln!("notes: legacy reindex failed: {e}");
                }
            }

            app.manage(AppState {
                db,
                app_data_dir: data_dir.clone(),
                notes_custom_dir: Arc::new(std::sync::RwLock::new(None)),
                notes_watcher: Arc::new(Mutex::new(None)),
                notes_lock: commands::notes::NotesLock::default(),
                sync: Arc::new(SyncManager::default()),
            });

            {
                let state = app.state::<AppState>();
                let watcher =
                    commands::notes::start_notes_watcher(app.handle().clone(), data_dir, None);
                if let Ok(mut slot) = state.notes_watcher.lock() {
                    *slot = watcher;
                };
            }

            start_sync_scheduler(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            host_info,
            // Demo / clear
            demo_seed,
            app_clear_data,
            // Sync
            sync_get_config,
            sync_set_config,
            sync_probe,
            sync_create_vault,
            sync_join_vault,
            sync_leave,
            sync_change_passphrase,
            sync_status,
            sync_debug_get,
            sync_debug_set,
            sync_debug_clear,
            sync_run_now,
            sync_trigger,
            sync_conflict_get,
            sync_conflict_resolve,
            note_sync_info,
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
            // Media capture
            media_grant_access,
            // Notes
            note_list,
            note_nav,
            note_get,
            note_create,
            note_update,
            note_delete,
            note_delete_many,
            note_trash_empty,
            note_archive,
            note_restore,
            note_set_tags,
            note_search,
            note_sync,
            note_reindex,
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
            note_set_folder,
            note_add_binding,
            note_remove_binding,
            note_smart_view_list,
            note_smart_view_create,
            note_smart_view_update,
            note_smart_view_delete,
            note_backlinks,
            note_links,
            note_related,
            note_resolve_link,
            notes_lock_status,
            notes_lock_set,
            notes_lock_timeout_set,
            notes_lock_unlock,
            notes_lock_lock,
            notes_lock_touch,
            note_history_list,
            note_history_get,
            note_history_diff,
            note_history_restore,
            note_history_merge,
            note_attachment_add_from_path,
            note_attachment_read,
            note_attachment_list,
            note_attachment_fetch,
            note_attachment_delete,
            note_attachment_save,
            notes_attachment_policy_get,
            notes_attachment_policy_set,
            sync_attachment_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running veydan");
}

#[cfg(desktop)]
fn run_desktop() {
    // Started by the browser as the extension's native messaging host: no UI, just relay.
    if capture::is_host_invocation() {
        capture::host::run();
        return;
    }
    tauri::Builder::default()
        // Must be first: second launch is closed here before other plugins run.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_from_tray(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let data_dir = app_data_dir.join("VeydanBrowser");
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(data_dir.join("profiles"))?;
            ensure_notes_dirs(&data_dir)?;

            let db_path = data_dir.join("profiles.db");
            let db = tauri::async_runtime::block_on(db::init_pool(&db_path))
                .expect("Failed to initialize database");
            if let Err(e) =
                tauri::async_runtime::block_on(sync::check_install_marker(&db, &data_dir))
            {
                eprintln!("sync: install marker check failed: {e}");
            }

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
                tray_settings.minimize_to_tray.store(
                    read_bool_setting(&db, "minimize_to_tray").await,
                    Ordering::Relaxed,
                );
                tray_settings.close_to_tray.store(
                    read_bool_setting(&db, "close_to_tray").await,
                    Ordering::Relaxed,
                );
                tray_settings.start_hidden.store(
                    read_bool_setting(&db, "start_hidden").await,
                    Ordering::Relaxed,
                );
            });

            app.manage(AppState {
                db,
                app_data_dir: data_dir.clone(),
                notes_custom_dir: Arc::new(std::sync::RwLock::new(notes_custom_dir)),
                notes_watcher: Arc::new(Mutex::new(None)),
                notes_lock: commands::notes::NotesLock::default(),
                sync: Arc::new(SyncManager::default()),
                browser: Arc::new(BrowserState::default()),
                download: DownloadManager::default(),
                ssh_sessions: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
                sftp_sessions: Arc::new(commands::sftp::SftpState::default()),
                backup: Arc::new(BackupManager::default()),
                tray_settings: tray_settings.clone(),
                tray_labels: Arc::new(Mutex::new(TrayLabels::default())),
                tray: Arc::new(Mutex::new(None)),
            });
            commands::notes::start_auto_lock(app.handle().clone());

            // Browser capture bridge: local socket + native messaging manifest
            capture::server::start(app.handle().clone());
            if let Err(e) = capture::extension::register_native_host(&data_dir) {
                eprintln!("capture: native host registration failed: {e}");
            }

            {
                let state = app.state::<AppState>();
                let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
                // Attachments are served through the asset protocol from both dirs.
                commands::notes::allow_asset_dir(
                    app.handle(),
                    &data_dir.join("notes").join("documents"),
                );
                if let Some(ref custom) = custom_dir {
                    commands::notes::allow_asset_dir(app.handle(), custom);
                }
                let watcher = commands::notes::start_notes_watcher(
                    app.handle().clone(),
                    data_dir,
                    custom_dir,
                );
                if let Ok(mut slot) = state.notes_watcher.lock() {
                    *slot = watcher;
                };
            }

            commands::notes::register_quick_capture_shortcut(app.handle());

            // Scheduled backups: ticks every 60s, catches up missed runs on start.
            start_backup_scheduler(app.handle().clone());

            // Sync (beta): idle until enabled and a vault is joined.
            start_sync_scheduler(app.handle().clone());

            // ── System tray ──
            let want_tray = tray_settings.minimize_to_tray.load(Ordering::Relaxed)
                || tray_settings.close_to_tray.load(Ordering::Relaxed)
                || tray_settings.start_hidden.load(Ordering::Relaxed);
            if want_tray {
                tray::apply_tray_async(app.handle(), true);
            }
            if tray_settings.start_hidden.load(Ordering::Relaxed) {
                tray::hide_to_tray(app.handle());
            }

            // Hide-on-close / minimize-to-tray. skip_taskbar only while stashed.
            // On Linux, OS minimize is handled by the custom titlebar button
            // (window_minimize); WindowEvent::Resized / is_minimized() are
            // unreliable on GTK/Wayland.
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
                                        let _ = w.set_skip_taskbar(true);
                                        let _ = w.hide();
                                    }
                                }
                            }
                        }
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            if state.tray_settings.close_to_tray.load(Ordering::Relaxed) {
                                api.prevent_close();
                                tray::hide_to_tray(&handle);
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
            host_info,
            fingerprint_presets,
            open_url,
            update_supported,
            // Demo / clear
            demo_seed,
            app_clear_data,
            // Tray / UI settings
            tray_settings_get,
            tray_settings_set,
            tray_set_labels,
            app_locale_set,
            app_locale_get,
            window_minimize,
            // Backup
            backup_get_config,
            backup_set_config,
            backup_list,
            backup_run_now,
            backup_restore,
            // Sync (beta)
            sync_get_config,
            sync_set_config,
            sync_probe,
            sync_create_vault,
            sync_join_vault,
            sync_leave,
            sync_change_passphrase,
            sync_status,
            sync_debug_get,
            sync_debug_set,
            sync_debug_clear,
            sync_run_now,
            sync_trigger,
            sync_conflict_get,
            sync_conflict_resolve,
            sync_profile_files_take_remote,
            sync_profile_files_push_mine,
            note_sync_info,
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
            // Media capture
            media_grant_access,
            // Notes
            note_list,
            note_nav,
            note_get,
            note_create,
            note_update,
            note_delete,
            note_delete_many,
            note_trash_empty,
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
            note_set_folder,
            note_add_binding,
            note_remove_binding,
            notes_get_dir,
            notes_set_dir,
            notes_capture_rules_get,
            notes_capture_rules_set,
            note_smart_view_list,
            note_smart_view_create,
            note_smart_view_update,
            note_smart_view_delete,
            open_quick_capture,
            quick_capture_shortcut_get,
            quick_capture_shortcut_set,
            note_backlinks,
            note_links,
            note_related,
            note_resolve_link,
            notes_lock_status,
            notes_lock_set,
            notes_lock_timeout_set,
            notes_lock_unlock,
            notes_lock_lock,
            notes_lock_touch,
            note_open_window,
            note_history_list,
            note_history_get,
            note_history_diff,
            note_history_restore,
            note_history_merge,
            note_attachment_add,
            note_attachment_add_from_path,
            note_attachment_list,
            note_attachment_fetch,
            note_attachment_read,
            note_attachment_delete,
            note_attachment_open,
            note_attachment_save,
            note_attachments_gc,
            notes_attachment_policy_get,
            notes_attachment_policy_set,
            sync_attachment_cancel,
            clipboard_file_paths,
            note_export,
            note_import,
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
            ssh_key_export_private,
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
