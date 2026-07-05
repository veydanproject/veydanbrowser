// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use std::collections::HashMap;
use super::models::*;
use chrono::Utc;
use uuid::Uuid;

// ── Tag helpers ───────────────────────────────────────────────────────────────

pub(crate) async fn fetch_note_tags(
    note_id: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Vec<NoteTagInfo>, AppError> {
    sqlx::query_as::<_, NoteTagInfo>(
        "SELECT nt.id, nt.name, nt.color FROM note_tags nt
         JOIN note_tag_links ntl ON nt.id = ntl.tag_id
         WHERE ntl.note_id = ?
         ORDER BY nt.name",
    )
    .bind(note_id)
    .fetch_all(db)
    .await
    .map_err(AppError::db)
}

pub(crate) async fn fetch_all_note_tags_map(
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<HashMap<String, Vec<NoteTagInfo>>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT ntl.note_id, nt.id, nt.name, nt.color
         FROM note_tag_links ntl JOIN note_tags nt ON ntl.tag_id = nt.id",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;

    let mut map: HashMap<String, Vec<NoteTagInfo>> = HashMap::new();
    for (note_id, tag_id, tag_name, tag_color) in rows {
        map.entry(note_id).or_default().push(NoteTagInfo {
            id: tag_id,
            name: tag_name,
            color: tag_color,
        });
    }
    Ok(map)
}

pub(crate) async fn fetch_all_note_folder_ids_map(
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<HashMap<String, Vec<String>>, AppError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT note_id, folder_id FROM note_folder_links",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (note_id, folder_id) in rows {
        map.entry(note_id).or_default().push(folder_id);
    }
    Ok(map)
}

/// Find or create a tag by name, return its id.
pub(crate) async fn upsert_tag(
    name: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM note_tags WHERE name = ?")
            .bind(name)
            .fetch_optional(db)
            .await
            .map_err(AppError::db)?;

    if let Some((id,)) = existing {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO note_tags (id, name, color, created_at, updated_at) VALUES (?, ?, '#6366f1', ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await
    .map_err(AppError::db)?;

    Ok(id)
}

pub(crate) async fn set_note_tag_links(
    note_id: &str,
    tag_names: &[String],
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM note_tag_links WHERE note_id = ?")
        .bind(note_id)
        .execute(db)
        .await
        .map_err(AppError::db)?;

    for name in tag_names {
        let tag_id = upsert_tag(name, db).await?;
        sqlx::query("INSERT OR IGNORE INTO note_tag_links (note_id, tag_id) VALUES (?, ?)")
            .bind(note_id)
            .bind(&tag_id)
            .execute(db)
            .await
            .map_err(AppError::db)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_tag_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<NoteTag>> {
    sqlx::query_as::<_, NoteTag>(
        "SELECT id, name, color, created_at, updated_at FROM note_tags ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn note_tag_create(
    name: String,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteTag> {
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return Err(AppError::io("Tag name cannot be empty"));
    }
    let color = color.unwrap_or_else(|| "#6366f1".to_string());
    let now = Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO note_tags (id, name, color, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET color = excluded.color, updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(&name)
    .bind(&color)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    let tag = sqlx::query_as::<_, NoteTag>(
        "SELECT id, name, color, created_at, updated_at FROM note_tags WHERE name = ?",
    )
    .bind(&name)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(tag)
}

#[tauri::command]
pub async fn note_tag_delete(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query("DELETE FROM note_tag_links WHERE tag_id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    sqlx::query("DELETE FROM note_tags WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_tag_update(
    id: String,
    name: Option<String>,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteTag> {
    let now = Utc::now().to_rfc3339();
    let name = name.map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty());
    sqlx::query(
        "UPDATE note_tags SET
            name       = COALESCE(?, name),
            color      = COALESCE(?, color),
            updated_at = ?
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&color)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    let tag = sqlx::query_as::<_, NoteTag>(
        "SELECT id, name, color, created_at, updated_at FROM note_tags WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(tag)
}

