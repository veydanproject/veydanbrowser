// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use super::attachments::remove_note_attachments;
use super::files::*;
use super::filter::FilterContext;
use super::history::*;
use super::index::*;
use super::links::{propagate_rename, reindex_links};
use super::models::*;
use super::tags::*;
use super::templates::{render_template, TemplateVars};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn note_list(
    filter: NoteFilter,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteListItem>> {
    let rows =
        sqlx::query_as::<_, NoteRow>("SELECT * FROM notes ORDER BY pinned DESC, updated_at DESC")
            .fetch_all(&state.db)
            .await
            .map_err(AppError::db)?;

    let ctx = FilterContext::load(&filter, &state).await?;
    Ok(list_items(rows, &filter, &ctx, &state.app_data_dir))
}

/// Apply the filter and build list items from rows.
fn list_items(
    rows: Vec<NoteRow>,
    filter: &NoteFilter,
    ctx: &FilterContext,
    app_data_dir: &std::path::PathBuf,
) -> Vec<NoteListItem> {
    let drafts = drafts_dir(app_data_dir);
    rows.into_iter()
        .filter(|r| ctx.matches(r, filter))
        .map(|r| {
            let tags = ctx.tags.get(&r.id).cloned().unwrap_or_default();
            let folder_ids = ctx.folders.get(&r.id).cloned().unwrap_or_default();
            let has_draft = drafts.join(format!("{}.draft", r.id)).exists();
            row_to_list_item(r, tags, folder_ids, has_draft)
        })
        .collect()
}

#[tauri::command]
pub async fn note_get(id: String, state: tauri::State<'_, AppState>) -> CmdResult<Note> {
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let tags = fetch_note_tags(&id, &state.db).await?;
    let folder_ids = fetch_note_folder_ids(&id, &state.db).await?;
    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);

    // Hash the file body, not the indexed column: an external edit may not be reindexed yet.
    let (content, content_hash) = if file_path.exists() {
        let (_, _, body) = read_note_file(&file_path)?;
        let hash = compute_hash(&body);
        (Some(body), Some(hash))
    } else {
        (None, row.content_hash)
    };

    let has_draft = draft_file_path(&state.app_data_dir, &id).exists();

    let bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();

    Ok(Note {
        id: row.id,
        title: row.title,
        base_dir: note_base_dir(&state.app_data_dir, &row.file_path),
        file_path: row.file_path,
        format: row.format,
        bindings,
        tags,
        folder_ids,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        deleted: row.deleted != 0,
        doc_status: row.doc_status,
        created_at: row.created_at,
        updated_at: row.updated_at,
        content_hash,
        content,
        has_draft,
    })
}

/// Fully specified note to persist (used by create and import).
pub(crate) struct NewNote {
    pub id: String,
    pub title: String,
    pub format: String,
    pub bindings: Vec<String>,
    pub tags: Vec<String>,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Current documents directory (custom or default).
pub(crate) fn current_docs_dir(state: &AppState) -> std::path::PathBuf {
    let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    effective_docs_dir(&state.app_data_dir, custom_dir.as_ref())
}

/// Write the note file, insert the DB row, index FTS and refresh the manifest.
pub(crate) async fn insert_note(new: NewNote, state: &AppState) -> Result<Note, AppError> {
    let bindings_json = serde_json::to_string(&new.bindings).map_err(AppError::other)?;
    let abs_path = current_docs_dir(state).join(format!("{}.{}", new.id, new.format));
    let stored_path = abs_path.to_string_lossy().to_string();

    let row = NoteRow {
        id: new.id.clone(),
        title: new.title.clone(),
        file_path: stored_path.clone(),
        format: new.format.clone(),
        scope: "global".to_string(),
        workspace_id: None,
        profile_id: None,
        pinned: 0,
        archived: 0,
        deleted: 0,
        doc_status: "active".to_string(),
        version_base: None,
        fts_rowid: None,
        created_at: new.created_at.clone(),
        updated_at: new.updated_at.clone(),
        file_mtime: None,
        content_hash: None,
        preview: String::new(),
        bindings: bindings_json.clone(),
        folder_id: None,
    };

    write_note_file(&abs_path, &row, &new.tags, &new.content)?;

    let content_hash = compute_hash(&new.content);
    let preview = make_preview(&new.content);

    sqlx::query(
        "INSERT INTO notes (id, title, file_path, format, bindings, pinned, archived, deleted, doc_status, created_at, updated_at, content_hash, preview)
         VALUES (?, ?, ?, ?, ?, 0, 0, 0, 'active', ?, ?, ?, ?)",
    )
    .bind(&new.id)
    .bind(&new.title)
    .bind(&stored_path)
    .bind(&new.format)
    .bind(&bindings_json)
    .bind(&new.created_at)
    .bind(&new.updated_at)
    .bind(&content_hash)
    .bind(&preview)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    set_note_tag_links(&new.id, &new.tags, &state.db).await?;
    reindex_links(&new.id, &new.content, &state.db).await?;

    let fts_rowid = fts_upsert(
        &new.id,
        &new.title,
        &new.content,
        &new.tags,
        None,
        &state.db,
    )
    .await?;
    sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
        .bind(fts_rowid)
        .bind(&new.id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    let tags = fetch_note_tags(&new.id, &state.db).await?;
    rebuild_manifest(&state.db, &state.app_data_dir).await?;

    Ok(Note {
        id: new.id,
        title: new.title,
        base_dir: note_base_dir(&state.app_data_dir, &stored_path),
        file_path: stored_path,
        format: new.format,
        bindings: new.bindings,
        tags,
        folder_ids: Vec::new(),
        pinned: false,
        archived: false,
        deleted: false,
        doc_status: "active".to_string(),
        created_at: new.created_at,
        updated_at: new.updated_at,
        content_hash: Some(content_hash),
        content: Some(new.content),
        has_draft: false,
    })
}

#[tauri::command]
pub async fn note_create(
    input: NoteCreateInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Note> {
    let now = Utc::now().to_rfc3339();
    let bindings = input.bindings.unwrap_or_default();
    let mut content = input.content.unwrap_or_default();
    if let Some(template_id) = input.template_id.filter(|t| !t.is_empty()) {
        let vars = TemplateVars::from_bindings(&input.title, &bindings, &state).await;
        let rendered = render_template(&template_id, &vars, &state).await?;
        content = if content.is_empty() {
            rendered
        } else {
            format!("{rendered}\n{content}")
        };
    }
    let new = NewNote {
        id: Uuid::new_v4().to_string(),
        title: input.title,
        format: input.format.unwrap_or_else(|| "md".to_string()),
        bindings,
        tags: input.tag_names.unwrap_or_default(),
        content,
        created_at: now.clone(),
        updated_at: now,
    };
    Ok(insert_note(new, &state).await?)
}

#[tauri::command]
pub async fn note_update(
    id: String,
    input: NoteUpdateInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Note> {
    Ok(update_note(&id, input, &state).await?)
}

/// Apply title/pinned/content changes: snapshot, rewrite file, reindex.
pub(crate) async fn update_note(
    id: &str,
    input: NoteUpdateInput,
    state: &AppState,
) -> Result<Note, AppError> {
    let id = id.to_string();
    let now = Utc::now().to_rfc3339();

    let mut row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let old_title = row.title.clone();
    if let Some(title) = input.title {
        row.title = title;
    }
    if let Some(pinned) = input.pinned {
        row.pinned = if pinned { 1 } else { 0 };
    }
    row.updated_at = now.clone();

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    let (_, old_tags_list, old_body) = if file_path.exists() {
        read_note_file(&file_path)?
    } else {
        (HashMap::new(), vec![], String::new())
    };

    let old_content = old_body.clone();
    // Reject a stale editor buffer. Callers that omit base_hash (capture, pin) still overwrite.
    if input.content.is_some() {
        if let Some(base) = input.base_hash.as_deref() {
            if compute_hash(&old_content) != base {
                return Err(AppError::conflict_changed(format!("note {id}")));
            }
        }
    }
    let content = input.content.unwrap_or(old_body);
    let content_hash = compute_hash(&content);
    let preview = make_preview(&content);

    let tag_names = fetch_note_tags(&id, &state.db)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect::<Vec<_>>();

    let _ = old_tags_list;

    // Snapshot the previous content before overwriting (if content actually changed).
    // A failed snapshot must not block the save itself, but don't lose it silently.
    if content != old_content {
        if let Err(e) = maybe_snapshot(&id, &row.title, &old_content, "save", &state.db).await {
            eprintln!("notes: version snapshot failed for note {id}: {e}");
        }
    }

    write_note_file(&file_path, &row, &tag_names, &content)?;
    reindex_links(&id, &content, &state.db).await?;
    if old_title != row.title {
        propagate_rename(&id, &old_title, &row.title, state).await?;
    }

    let fts_rowid = fts_upsert(
        &id,
        &row.title,
        &content,
        &tag_names,
        row.fts_rowid,
        &state.db,
    )
    .await?;

    sqlx::query(
        "UPDATE notes SET title=?, pinned=?, updated_at=?, content_hash=?, preview=?, fts_rowid=? WHERE id=?",
    )
    .bind(&row.title)
    .bind(row.pinned)
    .bind(&now)
    .bind(&content_hash)
    .bind(&preview)
    .bind(fts_rowid)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    // Delete draft after successful save
    let draft = draft_file_path(&state.app_data_dir, &id);
    if draft.exists() {
        let _ = std::fs::remove_file(&draft);
    }

    rebuild_manifest(&state.db, &state.app_data_dir).await?;

    let tags = fetch_note_tags(&id, &state.db).await?;
    let folder_ids = fetch_note_folder_ids(&id, &state.db).await?;

    let bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();

    Ok(Note {
        id: row.id,
        title: row.title,
        base_dir: note_base_dir(&state.app_data_dir, &row.file_path),
        file_path: row.file_path,
        format: row.format,
        bindings,
        tags,
        folder_ids,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        deleted: row.deleted != 0,
        doc_status: row.doc_status,
        created_at: row.created_at,
        updated_at: now,
        content_hash: Some(content_hash),
        content: Some(content),
        has_draft: false,
    })
}

/// Hard delete: file, attachments, draft, history and every index row.
async fn hard_delete(id: &str, state: &AppState) -> Result<(), AppError> {
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;

    if let Some(r) = row {
        fts_delete(r.fts_rowid, &state.db).await?;
        let file_path = resolve_note_abs_path(&state.app_data_dir, &r.file_path);
        if file_path.exists() {
            let _ = std::fs::remove_file(&file_path);
        }
        remove_note_attachments(&file_path, id);
    }

    // Sensitive leftovers: unsaved draft and the version history
    let _ = std::fs::remove_file(draft_file_path(&state.app_data_dir, id));
    sqlx::query("UPDATE note_history SET parent_id = NULL WHERE note_id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    sqlx::query("DELETE FROM note_history WHERE note_id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    sqlx::query("DELETE FROM note_tag_links WHERE note_id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    sqlx::query("DELETE FROM note_folder_links WHERE note_id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    sqlx::query("DELETE FROM note_links WHERE from_id = ? OR to_id = ?")
        .bind(id)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    sqlx::query("DELETE FROM note_mentions WHERE note_id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    // Soft `note:{id}` links stored on passwords
    let needle = format!("\"note:{id}\"");
    let linked: Vec<(String, String)> =
        sqlx::query_as("SELECT id, tags FROM passwords WHERE instr(tags, ?) > 0")
            .bind(&needle)
            .fetch_all(&state.db)
            .await
            .map_err(AppError::db)?;
    let tag = format!("note:{id}");
    for (pw_id, raw) in linked {
        let mut tags: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
        tags.retain(|t| t != &tag);
        sqlx::query("UPDATE passwords SET tags = ?, updated_at = ? WHERE id = ?")
            .bind(serde_json::to_string(&tags).map_err(AppError::other)?)
            .bind(Utc::now().to_rfc3339())
            .bind(&pw_id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

/// Soft delete: the row stays for sync, only the search index entry goes.
async fn soft_delete(id: &str, state: &AppState) -> Result<(), AppError> {
    sqlx::query("UPDATE notes SET deleted=1, updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;
    if let Some(r) = row {
        fts_delete(r.fts_rowid, &state.db).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_delete(
    id: String,
    hard: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    if hard.unwrap_or(false) {
        hard_delete(&id, &state).await?;
    } else {
        soft_delete(&id, &state).await?;
    }
    rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(())
}

/// Move several notes to the trash with a single manifest rebuild.
#[tauri::command]
pub async fn note_delete_many(
    ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    for id in &ids {
        soft_delete(id, &state).await?;
    }
    rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(())
}

/// Hard-delete everything in the trash; returns how many notes were removed.
#[tauri::command]
pub async fn note_trash_empty(state: tauri::State<'_, AppState>) -> CmdResult<usize> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM notes WHERE deleted = 1")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;
    for id in &ids {
        hard_delete(id, &state).await?;
    }
    rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(ids.len())
}

#[tauri::command]
pub async fn note_archive(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    sqlx::query("UPDATE notes SET archived=1, updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_restore(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    sqlx::query(
        "UPDATE notes SET archived=0, deleted=0, doc_status='active', updated_at=? WHERE id=?",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    // Soft delete drops the FTS row; put it back when leaving the trash.
    if row.deleted != 0 {
        let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
        let (_, tags_list, body) = read_note_file(&file_path).unwrap_or_default();
        let fts_rowid =
            fts_upsert(&id, &row.title, &body, &tags_list, row.fts_rowid, &state.db).await?;
        sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
            .bind(fts_rowid)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
        rebuild_manifest(&state.db, &state.app_data_dir).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_set_tags(
    id: String,
    tag_names: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let now = Utc::now().to_rfc3339();
    set_note_tag_links(&id, &tag_names, &state.db).await?;

    sqlx::query("UPDATE notes SET updated_at=? WHERE id=?")
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    // Rewrite file frontmatter with new tags
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    if file_path.exists() {
        let (_, _, body) = read_note_file(&file_path)?;
        let mut updated_row = row.clone();
        updated_row.updated_at = now.clone();
        write_note_file(&file_path, &updated_row, &tag_names, &body)?;

        let fts_rowid =
            fts_upsert(&id, &row.title, &body, &tag_names, row.fts_rowid, &state.db).await?;

        sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
            .bind(fts_rowid)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(())
}

/// FTS5 MATCH treats quotes, parens, NEAR/AND/OR etc. as query syntax, so raw
/// user input can produce SQL errors. Wrap each whitespace-separated term in
/// double quotes (doubling embedded quotes) so it matches literally; the
/// trailing `*` keeps the last term a prefix search. `None` for blank input.
pub(crate) fn fts_match_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut fts_query = trimmed
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");
    fts_query.push('*');
    Some(fts_query)
}

/// Escape a raw FTS snippet and turn the \u{1}/\u{2} markers into <mark> tags.
pub(crate) fn snippet_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    for c in raw.chars() {
        match c {
            '\u{1}' => out.push_str("<mark>"),
            '\u{2}' => out.push_str("</mark>"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[tauri::command]
pub async fn note_search(
    query: String,
    filter: NoteFilter,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteListItem>> {
    let Some(fts_query) = fts_match_query(&query) else {
        return Ok(vec![]);
    };

    // Control chars mark the match; the raw text is HTML-escaped before the
    // markers become <mark> tags, so note content never reaches the UI as HTML.
    let matched: Vec<(String, String)> =
        sqlx::query_as("SELECT note_id, snippet(notes_fts, 2, char(1), char(2), '…', 12) FROM notes_fts WHERE notes_fts MATCH ? ORDER BY rank LIMIT 100")
            .bind(&fts_query)
            .fetch_all(&state.db)
            .await
            .map_err(AppError::db)?;

    if matched.is_empty() {
        return Ok(vec![]);
    }

    let snippets_map: std::collections::HashMap<String, String> = matched
        .iter()
        .map(|(id, snip)| (id.clone(), snippet_html(snip)))
        .collect();
    let ids: Vec<String> = matched.into_iter().map(|(id, _)| id).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT * FROM notes WHERE id IN ({}) ORDER BY pinned DESC, updated_at DESC",
        placeholders
    );

    // Safe: only `?` placeholders are interpolated; ids are bound below.
    let mut q = sqlx::query_as::<_, NoteRow>(sqlx::AssertSqlSafe(sql));
    for id in &ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(&state.db).await.map_err(AppError::db)?;

    // Archived notes are searchable only when the archive filter is active.
    let mut filter = filter;
    if filter.archived.is_none() {
        filter.archived = Some(false);
    }
    let ctx = FilterContext::load(&filter, &state).await?;
    let mut items = list_items(rows, &filter, &ctx, &state.app_data_dir);
    for item in &mut items {
        item.snippet = snippets_map.get(&item.id).cloned();
    }
    Ok(items)
}

#[tauri::command]
pub async fn note_sync(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    sync_notes_index(&state.db, &state.app_data_dir, custom_dir.as_ref()).await
}

#[tauri::command]
pub async fn note_reindex(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    // Full FTS rebuild
    sqlx::query("DELETE FROM notes_fts")
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    let rows = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE deleted = 0")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;

    for row in rows {
        let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
        let (_, tags_list, body) = read_note_file(&file_path).unwrap_or_default();
        let tags_str = tags_list.join(" ");

        let mut conn = state.db.acquire().await.map_err(AppError::db)?;
        sqlx::query("INSERT INTO notes_fts(note_id, title, content, tags) VALUES (?, ?, ?, ?)")
            .bind(&row.id)
            .bind(&row.title)
            .bind(&body)
            .bind(&tags_str)
            .execute(&mut *conn)
            .await
            .map_err(AppError::db)?;

        let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
            .fetch_one(&mut *conn)
            .await
            .map_err(AppError::db)?;

        sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
            .bind(rowid)
            .bind(&row.id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn note_open_folder(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    let dir = effective_docs_dir(&state.app_data_dir, custom_dir.as_ref());
    open_path(&dir)
}

#[tauri::command]
pub async fn note_open_external(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    open_path(&file_path)
}

#[tauri::command]
pub async fn note_draft_save(
    id: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let draft_path = draft_file_path(&state.app_data_dir, &id);
    std::fs::write(&draft_path, &content).map_err(AppError::io)?;
    Ok(())
}

#[tauri::command]
pub async fn note_draft_get(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<String>> {
    let draft_path = draft_file_path(&state.app_data_dir, &id);
    if draft_path.exists() {
        let content = std::fs::read_to_string(&draft_path).map_err(AppError::io)?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn note_draft_discard(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let draft_path = draft_file_path(&state.app_data_dir, &id);
    if draft_path.exists() {
        std::fs::remove_file(&draft_path).map_err(AppError::io)?;
    }
    Ok(())
}
