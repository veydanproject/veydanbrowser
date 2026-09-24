// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use std::path::Path;

/// Wipe user catalog data (profiles, proxies, notes, totp, ssh, extra workspaces).
/// Keeps Default workspace row, sync/backup config, Camoufox runtime, UI prefs.
pub async fn clear_catalog(state: &AppState) -> CmdResult<()> {
    #[cfg(desktop)]
    {
        // Stop running browsers before deleting profile dirs
        for id in state.browser.running_ids().await {
            let _ = crate::browser::launch::stop(&id, &state.browser).await;
        }
    }

    // SSH links first
    let _ = sqlx::query("DELETE FROM ssh_connection_profiles")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM ssh_connection_workspaces")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM ssh_connections")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM ssh_keys").execute(&state.db).await;

    // Notes graph
    let _ = sqlx::query("DELETE FROM note_links")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM note_mentions")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM note_tag_links")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM note_folder_links")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("UPDATE note_history SET parent_id = NULL")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM note_history")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM notes_fts")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM notes").execute(&state.db).await;
    let _ = sqlx::query("DELETE FROM note_smart_views")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM note_tags")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM note_folders")
        .execute(&state.db)
        .await;

    let _ = sqlx::query("DELETE FROM totp_entries")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM password_history")
        .execute(&state.db)
        .await;

    // Profiles then proxies (proxy_id FK is soft)
    let profile_paths: Vec<(String,)> = sqlx::query_as("SELECT profile_path FROM profiles")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let _ = sqlx::query("DELETE FROM profiles").execute(&state.db).await;
    let _ = sqlx::query("DELETE FROM proxies").execute(&state.db).await;

    let _ = sqlx::query("DELETE FROM workspace_columns")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM workspaces WHERE id != 'default'")
        .execute(&state.db)
        .await;

    // Reset Default workspace
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE workspaces SET name = 'Default', description = NULL, color = '#6366f1',
         icon = 'folder', notes = NULL, updated_at = ? WHERE id = 'default'",
    )
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    // Sync positions, so the next cycle re-reads the vault. Binding stays.
    for key in ["sync_own_seq", "sync_own_head", "sync_hlc"] {
        let _ = sqlx::query("DELETE FROM app_settings WHERE key = ?")
            .bind(key)
            .execute(&state.db)
            .await;
    }

    // Sync entity state (not vault config)
    let _ = sqlx::query("DELETE FROM sync_note_state")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM sync_attachment_state")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM sync_row_state")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM sync_profile_files_state")
        .execute(&state.db)
        .await;
    let _ = sqlx::query("DELETE FROM sync_gc_candidates")
        .execute(&state.db)
        .await;

    // Capture rules
    let _ = sqlx::query("DELETE FROM app_settings WHERE key = 'notes_capture_rules'")
        .execute(&state.db)
        .await;

    // Disk: profile dirs + notes files
    for (path,) in profile_paths {
        let p = Path::new(&path);
        if p.exists() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
    let profiles_root = state.app_data_dir.join("profiles");
    if profiles_root.exists() {
        let _ = std::fs::remove_dir_all(&profiles_root);
        let _ = std::fs::create_dir_all(&profiles_root);
    }

    clear_notes_dirs(&state.app_data_dir)?;

    crate::commands::notes::rebuild_manifest(&state.db, &state.app_data_dir)
        .await
        .ok();

    Ok(())
}

fn clear_notes_dirs(app_data_dir: &Path) -> CmdResult<()> {
    let notes = app_data_dir.join("notes");
    for sub in ["documents", "attachments", "drafts"] {
        let dir = notes.join(sub);
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(&dir).map_err(AppError::io)?;
    }
    let manifest = notes.join("notes_manifest.json");
    if manifest.exists() {
        let _ = std::fs::remove_file(manifest);
    }
    Ok(())
}
