// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Wiki links `@@Title@@` / `@@Title|alias@@` / `@@id@@` between notes.
//! `note_links(from_id, to_id)` is rebuilt from the body on every save;
//! renaming a note rewrites `@@Old title@@` in the notes that link to it.

use super::files::{read_note_file, resolve_note_abs_path, row_to_list_item, write_note_file};
use super::models::{NoteListItem, NoteRow};
use super::tags::{fetch_all_note_folder_ids_map, fetch_all_note_tags_map};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use std::collections::HashSet;

type Db = sqlx::Pool<sqlx::Sqlite>;

/// Link targets in the body (text before `|`, trimmed), deduplicated.
pub(crate) fn extract_targets(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("@@") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("@@") else { break };
        let inner = &after[..end];
        if !inner.contains('\n') {
            let target = inner.split('|').next().unwrap_or("").trim();
            if !target.is_empty() && seen.insert(target.to_lowercase()) {
                out.push(target.to_string());
            }
        }
        rest = &after[end + 2..];
    }
    out
}

/// Resolve a target to a live note id: exact id first, then case-insensitive title.
pub(crate) async fn resolve_target(target: &str, db: &Db) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM notes WHERE deleted = 0 AND (id = ? OR lower(title) = lower(?))
         ORDER BY CASE WHEN id = ? THEN 0 ELSE 1 END, updated_at DESC LIMIT 1",
    )
    .bind(target)
    .bind(target)
    .bind(target)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// Replace outgoing links of a note with those found in `content`.
pub(crate) async fn reindex_links(note_id: &str, content: &str, db: &Db) -> Result<(), AppError> {
    sqlx::query("DELETE FROM note_links WHERE from_id = ?")
        .bind(note_id)
        .execute(db)
        .await
        .map_err(AppError::db)?;
    for target in extract_targets(content) {
        let Some(to_id) = resolve_target(&target, db).await else {
            continue;
        };
        if to_id == note_id {
            continue;
        }
        sqlx::query("INSERT OR IGNORE INTO note_links (from_id, to_id) VALUES (?, ?)")
            .bind(note_id)
            .bind(&to_id)
            .execute(db)
            .await
            .map_err(AppError::db)?;
    }
    Ok(())
}

/// Rewrite `@@old@@` -> `@@new@@` (any case, with or without alias) in one body.
pub(crate) fn rewrite_title_links(body: &str, old_title: &str, new_title: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("@@") {
        out.push_str(&rest[..start + 2]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("@@") else {
            out.push_str(after);
            return out;
        };
        let inner = &after[..end];
        let (target, alias) = match inner.find('|') {
            Some(p) => (&inner[..p], Some(&inner[p..])),
            None => (inner, None),
        };
        if target.trim().eq_ignore_ascii_case(old_title.trim()) {
            out.push_str(new_title);
            if let Some(a) = alias {
                out.push_str(a);
            }
        } else {
            out.push_str(inner);
        }
        out.push_str("@@");
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// After a title change: update files of all notes linking to `note_id`.
pub(crate) async fn propagate_rename(
    note_id: &str,
    old_title: &str,
    new_title: &str,
    state: &AppState,
) -> Result<(), AppError> {
    if old_title.trim().eq_ignore_ascii_case(new_title.trim()) {
        return Ok(());
    }
    let sources = sqlx::query_as::<_, NoteRow>(
        "SELECT n.* FROM notes n JOIN note_links l ON l.from_id = n.id WHERE l.to_id = ? AND n.deleted = 0",
    )
    .bind(note_id)
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;

    for row in sources {
        let path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
        let Ok((_, tags, body)) = read_note_file(&path) else {
            continue;
        };
        let rewritten = rewrite_title_links(&body, old_title, new_title);
        if rewritten == body {
            continue;
        }
        write_note_file(&path, &row, &tags, &rewritten)?;
        let hash = super::files::compute_hash(&rewritten);
        sqlx::query("UPDATE notes SET content_hash = ?, preview = ? WHERE id = ?")
            .bind(&hash)
            .bind(super::files::make_preview(&rewritten))
            .bind(&row.id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
        let fts_rowid = super::index::fts_upsert(
            &row.id,
            &row.title,
            &rewritten,
            &tags,
            row.fts_rowid,
            &state.db,
        )
        .await?;
        sqlx::query("UPDATE notes SET fts_rowid = ? WHERE id = ?")
            .bind(fts_rowid)
            .bind(&row.id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }
    Ok(())
}

async fn rows_to_items(
    rows: Vec<NoteRow>,
    state: &AppState,
) -> Result<Vec<NoteListItem>, AppError> {
    let tags = fetch_all_note_tags_map(&state.db).await?;
    let folders = fetch_all_note_folder_ids_map(&state.db).await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let t = tags.get(&r.id).cloned().unwrap_or_default();
            let f = folders.get(&r.id).cloned().unwrap_or_default();
            row_to_list_item(r, t, f, false)
        })
        .collect())
}

#[derive(Debug, serde::Serialize)]
pub struct NoteLinks {
    /// Notes this note links to.
    pub outgoing: Vec<NoteListItem>,
    /// Notes linking to this note.
    pub backlinks: Vec<NoteListItem>,
}

/// Outgoing links and backlinks in one call.
#[tauri::command]
pub async fn note_links(id: String, state: tauri::State<'_, AppState>) -> CmdResult<NoteLinks> {
    let outgoing = sqlx::query_as::<_, NoteRow>(
        "SELECT n.* FROM notes n JOIN note_links l ON l.to_id = n.id
         WHERE l.from_id = ? AND n.deleted = 0 ORDER BY n.updated_at DESC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;
    let backlinks = sqlx::query_as::<_, NoteRow>(
        "SELECT n.* FROM notes n JOIN note_links l ON l.from_id = n.id
         WHERE l.to_id = ? AND n.deleted = 0 ORDER BY n.updated_at DESC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(NoteLinks {
        outgoing: rows_to_items(outgoing, &state).await?,
        backlinks: rows_to_items(backlinks, &state).await?,
    })
}

/// Id of the note a `@@target@@` points to, if it exists.
#[tauri::command]
pub async fn note_resolve_link(
    target: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<String>> {
    Ok(resolve_target(target.trim(), &state.db).await)
}

/// Notes that link to this note.
#[tauri::command]
pub async fn note_backlinks(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteListItem>> {
    let rows = sqlx::query_as::<_, NoteRow>(
        "SELECT n.* FROM notes n JOIN note_links l ON l.from_id = n.id
         WHERE l.to_id = ? AND n.deleted = 0 ORDER BY n.updated_at DESC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;
    rows_to_items(rows, &state).await
}

/// Notes sharing a domain / profile / workspace binding, most specific first.
#[tauri::command]
pub async fn note_related(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteListItem>> {
    let Some(bindings_json) =
        sqlx::query_scalar::<_, String>("SELECT bindings FROM notes WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?
    else {
        return Ok(vec![]);
    };
    let bindings: Vec<String> = serde_json::from_str(&bindings_json).unwrap_or_default();
    let mut ordered: Vec<&String> = Vec::new();
    for prefix in ["url:", "domain:", "profile:", "workspace:"] {
        ordered.extend(bindings.iter().filter(|b| b.starts_with(prefix)));
    }
    if ordered.is_empty() {
        return Ok(vec![]);
    }

    let mut seen = HashSet::from([id.clone()]);
    let mut rows = Vec::new();
    for b in ordered {
        let pattern = format!("%{}%", serde_json::to_string(b).unwrap_or_default());
        let found = sqlx::query_as::<_, NoteRow>(
            "SELECT * FROM notes WHERE deleted = 0 AND archived = 0 AND bindings LIKE ? ORDER BY updated_at DESC LIMIT 20",
        )
        .bind(pattern)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;
        for r in found {
            if seen.insert(r.id.clone()) {
                rows.push(r);
            }
        }
        if rows.len() >= 10 {
            break;
        }
    }
    rows.truncate(10);
    rows_to_items(rows, &state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_targets_without_alias_and_dedups() {
        let t = extract_targets("see @@Alpha@@ and @@alpha|the alias@@ plus @@Beta@@");
        assert_eq!(t, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn rewrites_only_matching_links() {
        let body = "@@Old@@ @@old|alias@@ @@Other@@ @@";
        assert_eq!(
            rewrite_title_links(body, "Old", "New"),
            "@@New@@ @@New|alias@@ @@Other@@ @@"
        );
    }
}
