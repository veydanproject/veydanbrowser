// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use super::attachments::allow_asset_dir;
use super::files::*;
use super::index::{start_notes_watcher, sync_notes_index};
use crate::error::{AppError, CmdResult};
use crate::AppState;
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
pub async fn notes_capture_rules_get(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<CaptureRule>> {
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
            tags: r
                .tags
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
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

// ── Attachment policy: when a note attachment becomes a large file ───────────

const POLICY_KEY: &str = "notes_attachment_policy";
const MAX_THRESHOLD_MIB: u64 = 4096;
const MAX_FILE_GIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Notes-domain policy; the large-files module never reads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteAttachmentPolicy {
    /// Off: every attachment stays a v1 blob, whatever its size.
    pub large_files_enabled: bool,
    /// Files at or above this size sync as v2 large files.
    pub threshold_mib: u64,
    /// Product limit for one attachment; 0 = no limit (backend quota still applies).
    pub max_file_gib: u64,
    /// Off: chunked attachments at or above `ask_above_mib` stay in the vault until requested.
    #[serde(default = "default_true")]
    pub download_on_sync: bool,
    /// Size from which a missing attachment prompts for download when the note opens.
    #[serde(default = "default_ask_above_mib")]
    pub ask_above_mib: u64,
}

fn default_true() -> bool {
    true
}

fn default_ask_above_mib() -> u64 {
    16
}

impl Default for NoteAttachmentPolicy {
    fn default() -> Self {
        Self {
            large_files_enabled: true,
            threshold_mib: 16,
            max_file_gib: 10,
            download_on_sync: true,
            ask_above_mib: default_ask_above_mib(),
        }
    }
}

impl NoteAttachmentPolicy {
    pub fn validate(&self) -> CmdResult<()> {
        if !(1..=MAX_THRESHOLD_MIB).contains(&self.threshold_mib) {
            return Err(AppError::other(format!(
                "threshold must be 1..{MAX_THRESHOLD_MIB} MiB"
            )));
        }
        if self.max_file_gib > MAX_FILE_GIB {
            return Err(AppError::other(format!(
                "max file size must be 0..{MAX_FILE_GIB} GiB"
            )));
        }
        if !(1..=MAX_THRESHOLD_MIB).contains(&self.ask_above_mib) {
            return Err(AppError::other(format!(
                "download prompt threshold must be 1..{MAX_THRESHOLD_MIB} MiB"
            )));
        }
        Ok(())
    }

    pub fn uses_large_files(&self, size: u64) -> bool {
        self.large_files_enabled && size >= self.threshold_mib * MIB
    }

    /// Whether a remote chunked attachment of `size` is fetched during sync.
    pub fn downloads_on_sync(&self, size: u64) -> bool {
        self.download_on_sync || size < self.ask_above_mib * MIB
    }

    pub fn ask_above_bytes(&self) -> u64 {
        self.ask_above_mib * MIB
    }

    /// Product limit in bytes; `None` when unlimited.
    pub fn max_file_bytes(&self) -> Option<u64> {
        (self.max_file_gib > 0).then(|| self.max_file_gib * GIB)
    }

    /// Product limit check; `None` size (unknown source length) passes.
    pub fn check_size(&self, size: Option<u64>) -> CmdResult<()> {
        match (size, self.max_file_bytes()) {
            (Some(n), Some(max)) if n > max => Err(AppError::other(format!(
                "attachment exceeds the {} GiB limit",
                self.max_file_gib
            ))),
            _ => Ok(()),
        }
    }
}

pub(crate) async fn load_attachment_policy(db: &sqlx::Pool<sqlx::Sqlite>) -> NoteAttachmentPolicy {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(POLICY_KEY)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn notes_attachment_policy_get(
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachmentPolicy> {
    Ok(load_attachment_policy(&state.db).await)
}

#[tauri::command]
pub async fn notes_attachment_policy_set(
    policy: NoteAttachmentPolicy,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachmentPolicy> {
    policy.validate()?;
    let json = serde_json::to_string(&policy).map_err(AppError::other)?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(POLICY_KEY)
    .bind(&json)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(policy)
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
        (
            documents_dir(&state.app_data_dir)
                .to_string_lossy()
                .to_string(),
            false,
        )
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
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
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
        (
            documents_dir(&state.app_data_dir)
                .to_string_lossy()
                .to_string(),
            false,
        )
    };
    Ok(NotesDirInfo { current, is_custom })
}
