// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use super::files::*;
use super::attachments::allow_asset_dir;
use super::index::{start_notes_watcher, sync_notes_index};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Capture rules: domain -> folder / tags ────────────────────────────────────

const CAPTURE_RULES_KEY: &str = "notes_capture_rules";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRule {
    /// Host or `*.host`; matches the host and its subdomains
    pub domain: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Template note rendered above the captured content
    #[serde(default)]
    pub template_id: Option<String>,
}

pub(crate) async fn load_capture_rules(state: &AppState) -> Vec<CaptureRule> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(CAPTURE_RULES_KEY)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn notes_capture_rules_get(state: tauri::State<'_, AppState>) -> CmdResult<Vec<CaptureRule>> {
    Ok(load_capture_rules(&state).await)
}

#[tauri::command]
pub async fn notes_capture_rules_set(
    rules: Vec<CaptureRule>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<CaptureRule>> {
    let rules: Vec<CaptureRule> = rules
        .into_iter()
        .filter(|r| !r.domain.trim().is_empty())
        .map(|r| CaptureRule {
            domain: r.domain.trim().to_lowercase(),
            folder_id: r.folder_id.filter(|f| !f.is_empty()),
            tags: r.tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
            template_id: r.template_id.filter(|t| !t.is_empty()),
        })
        .collect();
    let json = serde_json::to_string(&rules).map_err(AppError::other)?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(CAPTURE_RULES_KEY)
    .bind(&json)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(rules)
}

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
    app: tauri::AppHandle,
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

    // Re-point the file watcher and index whatever already lives in the new dir.
    if let Some(ref custom) = new_custom {
        allow_asset_dir(&app, custom);
    }
    let watcher = start_notes_watcher(app, state.app_data_dir.clone(), new_custom.clone());
    if let Ok(mut slot) = state.notes_watcher.lock() {
        *slot = watcher;
    }
    sync_notes_index(&state.db, &state.app_data_dir, new_custom.as_ref()).await?;

    let (current, is_custom) = if let Some(ref p) = new_custom {
        (p.to_string_lossy().to_string(), true)
    } else {
        (documents_dir(&state.app_data_dir).to_string_lossy().to_string(), false)
    };
    Ok(NotesDirInfo { current, is_custom })
}

