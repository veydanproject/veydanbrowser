// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ── Models ────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub(crate) struct NoteRow {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub format: String,
    // Legacy columns kept for SELECT * compatibility; bindings is the new source of truth
    pub scope: String,
    pub workspace_id: Option<String>,
    pub profile_id: Option<String>,
    pub pinned: i64,
    pub archived: i64,
    pub deleted: i64,
    pub doc_status: String,
    pub version_base: Option<String>,
    pub fts_rowid: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub file_mtime: Option<String>,
    pub content_hash: Option<String>,
    pub preview: String,
    /// JSON array of binding strings, e.g. ["workspace:id", "profile:id"]
    pub bindings: String,
    pub folder_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub format: String,
    pub bindings: Vec<String>,
    pub tags: Vec<NoteTagInfo>,
    pub pinned: bool,
    pub archived: bool,
    pub doc_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub content_hash: Option<String>,
    pub content: Option<String>,
    pub has_draft: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct NoteListItem {
    pub id: String,
    pub title: String,
    pub format: String,
    pub bindings: Vec<String>,
    pub tags: Vec<NoteTagInfo>,
    pub folder_ids: Vec<String>,
    pub pinned: bool,
    pub archived: bool,
    pub doc_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub has_draft: bool,
    pub preview: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Serialize, Clone, FromRow)]
pub struct NoteFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub color: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct NoteTagInfo {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Clone, FromRow)]
pub struct NoteTag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct NoteCreateInput {
    pub title: String,
    pub format: Option<String>,
    /// Context bindings, e.g. ["workspace:id", "profile:id"]
    pub bindings: Option<Vec<String>>,
    pub tag_names: Option<Vec<String>>,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NoteUpdateInput {
    pub title: Option<String>,
    pub content: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct NoteFilter {
    /// Filter notes that contain this binding, e.g. "workspace:id" or "profile:id"
    pub binding: Option<String>,
    pub tag_name: Option<String>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub include_deleted: Option<bool>,
}
