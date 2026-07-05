// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use super::files::*;
use serde::Serialize;
use std::path::PathBuf;

// ── Notes directory settings ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct NotesDirInfo {
    pub current: String,
    pub is_custom: bool,
}

#[tauri::command]
pub async fn notes_get_dir(state: tauri::State<'_, AppState>) -> CmdResult<NotesDirInfo> {
    let custom = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    let (current, is_custom) = if let Some(ref p) = custom {
        (p.to_string_lossy().to_string(), true)
    } else {
        (documents_dir(&state.app_data_dir).to_string_lossy().to_string(), false)
    };
    Ok(NotesDirInfo { current, is_custom })
}

#[tauri::command]
pub async fn notes_set_dir(
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NotesDirInfo> {
    let new_custom: Option<PathBuf> = path.as_deref().and_then(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() { None } else { Some(PathBuf::from(trimmed)) }
    });

    if let Some(ref p) = new_custom {
        std::fs::create_dir_all(p).map_err(AppError::io)?;
    }

    // Persist to DB
    if let Some(ref p) = new_custom {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('notes_custom_dir', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(p.to_string_lossy().as_ref())
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    } else {
        sqlx::query("DELETE FROM app_settings WHERE key = 'notes_custom_dir'")
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    // Update in-memory state
    if let Ok(mut lock) = state.notes_custom_dir.write() {
        *lock = new_custom.clone();
    }

    let (current, is_custom) = if let Some(ref p) = new_custom {
        (p.to_string_lossy().to_string(), true)
    } else {
        (documents_dir(&state.app_data_dir).to_string_lossy().to_string(), false)
    };
    Ok(NotesDirInfo { current, is_custom })
}

