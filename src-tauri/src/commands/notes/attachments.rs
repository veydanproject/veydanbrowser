// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Note attachments: files stored next to the note as `attachments/{note_id}/{name}`
//! and referenced from Markdown by that relative path.

use crate::error::{AppError, CmdResult};
use crate::AppState;
use super::files::*;
use super::models::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];

#[derive(Debug, Clone, Serialize)]
pub struct NoteAttachment {
    pub name: String,
    /// Path relative to the note file, ready for a Markdown link
    pub rel_path: String,
    pub size: u64,
    pub is_image: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrphanAttachment {
    pub note_id: String,
    pub name: String,
    pub size: u64,
}

// ── Paths ─────────────────────────────────────────────────────────────────────

/// `attachments/{note_id}` next to the note file.
pub(crate) fn attachments_dir_for(note_file: &Path, note_id: &str) -> PathBuf {
    note_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
        .join("attachments")
        .join(note_id)
}

pub(crate) fn remove_note_attachments(note_file: &Path, note_id: &str) {
    let dir = attachments_dir_for(note_file, note_id);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Let the webview load files from this documents dir via the asset protocol.
pub fn allow_asset_dir(app: &tauri::AppHandle, docs_dir: &Path) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(docs_dir, true) {
        eprintln!("notes: asset scope for {} failed: {e}", docs_dir.display());
    }
}

async fn note_file_path(note_id: &str, state: &AppState) -> Result<PathBuf, AppError> {
    let (file_path,): (String,) = sqlx::query_as("SELECT file_path FROM notes WHERE id = ?")
        .bind(note_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {note_id}")))?;
    Ok(resolve_note_abs_path(&state.app_data_dir, &file_path))
}

fn is_image(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Strip directory components and characters unsafe for file names.
pub(crate) fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() { "file".to_string() } else { trimmed.to_string() }
}

/// Append `-1`, `-2`, … before the extension until the name is free.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|i| dir.join(format!("{stem}-{i}{ext}")))
        .find(|p| !p.exists())
        .expect("unbounded iterator")
}

fn to_attachment(note_id: &str, path: &Path) -> Option<NoteAttachment> {
    let name = path.file_name()?.to_str()?.to_string();
    let size = std::fs::metadata(path).ok()?.len();
    Some(NoteAttachment {
        rel_path: format!("attachments/{note_id}/{name}"),
        is_image: is_image(&name),
        name,
        size,
    })
}

/// Store bytes as a new attachment of the note.
pub(crate) fn store_attachment(
    note_file: &Path,
    note_id: &str,
    file_name: &str,
    data: &[u8],
) -> Result<NoteAttachment, AppError> {
    let dir = attachments_dir_for(note_file, note_id);
    std::fs::create_dir_all(&dir).map_err(AppError::io)?;
    let dest = unique_path(&dir, &safe_file_name(file_name));
    std::fs::write(&dest, data).map_err(AppError::io)?;
    to_attachment(note_id, &dest).ok_or_else(|| AppError::io("attachment write failed"))
}

/// Decode the percent-encoded ASCII header value used for file names.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Names of attachments referenced from the note body (`attachments/{id}/{name}`).
fn referenced_names(body: &str, note_id: &str) -> Vec<String> {
    let prefix = format!("attachments/{note_id}/");
    let mut names = Vec::new();
    for (idx, _) in body.match_indices(&prefix) {
        let rest = &body[idx + prefix.len()..];
        let end = rest.find(|c: char| c == ')' || c == ' ' || c == '"' || c == '\'' || c == '\n' || c == '>' || c == ']').unwrap_or(rest.len());
        let name = percent_decode(&rest[..end]);
        if !name.is_empty() { names.push(name); }
    }
    names
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Raw-body upload: bytes in the request body, `x-note-id` and `x-file-name`
/// (percent-encoded) in headers.
#[tauri::command]
pub async fn note_attachment_add(
    request: tauri::ipc::Request<'_>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachment> {
    let header = |name: &str| -> Result<String, AppError> {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::other(format!("missing header {name}")))
    };
    let note_id = header("x-note-id")?;
    let file_name = percent_decode(&header("x-file-name")?);
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err(AppError::other("attachment body must be raw bytes"));
    };
    let note_file = note_file_path(&note_id, &state).await?;
    store_attachment(&note_file, &note_id, &file_name, data)
}

/// Upload with the bytes as base64. Mobile IPC goes through postMessage, so
/// raw request bodies are not available there.
#[tauri::command]
pub async fn note_attachment_add_base64(
    note_id: String,
    name: String,
    data: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachment> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(AppError::other)?;
    let note_file = note_file_path(&note_id, &state).await?;
    store_attachment(&note_file, &note_id, &name, &bytes)
}

/// File bytes as base64 for webviews without the asset protocol.
#[tauri::command]
pub async fn note_attachment_read(
    note_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<String> {
    use base64::Engine;
    let note_file = note_file_path(&note_id, &state).await?;
    let path = attachments_dir_for(&note_file, &note_id).join(safe_file_name(&name));
    if !path.is_file() {
        return Err(AppError::not_found(name));
    }
    let bytes = std::fs::read(&path).map_err(AppError::io)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Local file paths currently held by the OS clipboard (files copied in a file manager).
#[cfg(desktop)]
#[tauri::command]
pub async fn clipboard_file_paths() -> CmdResult<Vec<String>> {
    let paths = tokio::task::spawn_blocking(|| {
        let mut cb = arboard::Clipboard::new().map_err(AppError::io)?;
        Ok::<_, AppError>(cb.get().file_list().unwrap_or_default())
    })
    .await
    .map_err(AppError::io)??;
    // uri-list entries are CRLF-terminated; arboard leaves the '\r' on the path
    Ok(paths
        .into_iter()
        .map(|p| p.to_string_lossy().trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect())
}

/// Copy an existing file (e.g. from OS drag-and-drop) into the note's attachments.
#[tauri::command]
pub async fn note_attachment_add_from_path(
    note_id: String,
    src_path: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachment> {
    let src = PathBuf::from(&src_path);
    let data = std::fs::read(&src).map_err(AppError::io)?;
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let note_file = note_file_path(&note_id, &state).await?;
    store_attachment(&note_file, &note_id, name, &data)
}

#[tauri::command]
pub async fn note_attachment_list(
    note_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteAttachment>> {
    let note_file = note_file_path(&note_id, &state).await?;
    let dir = attachments_dir_for(&note_file, &note_id);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
    let mut list: Vec<NoteAttachment> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| to_attachment(&note_id, &e.path()))
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

#[tauri::command]
pub async fn note_attachment_delete(
    note_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let note_file = note_file_path(&note_id, &state).await?;
    let path = attachments_dir_for(&note_file, &note_id).join(safe_file_name(&name));
    if path.is_file() {
        std::fs::remove_file(&path).map_err(AppError::io)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_attachment_open(
    note_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let note_file = note_file_path(&note_id, &state).await?;
    let path = attachments_dir_for(&note_file, &note_id).join(safe_file_name(&name));
    if !path.is_file() {
        return Err(AppError::not_found(name));
    }
    open_path(&path)
}

/// Copy an attachment to a user-chosen path (Save As).
#[tauri::command]
pub async fn note_attachment_save(
    note_id: String,
    name: String,
    dest: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let note_file = note_file_path(&note_id, &state).await?;
    let path = attachments_dir_for(&note_file, &note_id).join(safe_file_name(&name));
    if !path.is_file() {
        return Err(AppError::not_found(name));
    }
    let dest_path = PathBuf::from(&dest);
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::io)?;
    }
    std::fs::copy(&path, &dest_path).map_err(AppError::io)?;
    Ok(())
}

/// Find attachments no note body references. With `delete`, remove them.
#[tauri::command]
pub async fn note_attachments_gc(
    delete: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<OrphanAttachment>> {
    let rows = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;

    let mut orphans = Vec::new();
    for row in &rows {
        let note_file = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
        let dir = attachments_dir_for(&note_file, &row.id);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let body = read_note_file(&note_file).map(|(_, _, b)| b).unwrap_or_default();
        let used = referenced_names(&body, &row.id);
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else { continue };
            if used.contains(&name) { continue; }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if delete {
                let _ = std::fs::remove_file(&path);
            }
            orphans.push(OrphanAttachment { note_id: row.id.clone(), name, size });
        }
        if delete && std::fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    Ok(orphans)
}