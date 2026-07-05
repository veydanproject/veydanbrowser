// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

mod browser;
mod commands;
mod db;
pub mod error;
mod fingerprint;
mod models;
mod proxy;

use browser::launch::BrowserState;
use commands::camoufox::{
    camoufox_download, camoufox_download_cancel, camoufox_download_state, camoufox_latest_version,
    camoufox_status, DownloadManager,
};
use commands::notes::{
    note_archive, note_create, note_delete, note_draft_discard, note_draft_save, note_get,
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
use commands::ssh::{
    ssh_connect, ssh_connection_create, ssh_connection_delete, ssh_connection_get,
    ssh_connection_list, ssh_connection_update, ssh_disconnect, ssh_resize, ssh_send_data,
    ssh_session_list, ssh_session_remove, ssh_respond_prompt, SshSessions,
};
use commands::workspaces::*;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

pub struct AppState {
    pub db: Pool<Sqlite>,
    pub browser: Arc<BrowserState>,
    pub app_data_dir: PathBuf,
    pub download: DownloadManager,
    pub notes_custom_dir: Arc<std::sync::RwLock<Option<PathBuf>>>,
    pub ssh_sessions: SshSessions,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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

            app.manage(AppState {
                db,
                browser: Arc::new(BrowserState::default()),
                app_data_dir: data_dir.clone(),
                download: DownloadManager::default(),
                notes_custom_dir: Arc::new(std::sync::RwLock::new(notes_custom_dir)),
                ssh_sessions: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            });

            commands::notes::start_notes_watcher(app.handle().clone(), data_dir, {
                let state = app.state::<AppState>();
                state.notes_custom_dir.read().ok().and_then(|g| g.clone())
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            fingerprint_presets,
            open_url,
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
            ssh_connect,
            ssh_disconnect,
            ssh_send_data,
            ssh_resize,
            ssh_session_list,
            ssh_session_remove,
            ssh_respond_prompt,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(browser::launch::stop_all(&state.browser));
            }
        });
}
