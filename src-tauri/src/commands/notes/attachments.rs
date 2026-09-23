// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Note attachments: files stored next to the note as `attachments/{note_id}/{name}`
//! and referenced from Markdown by that relative path.

use super::files::*;
use super::models::*;
use super::settings::load_attachment_policy;
use crate::error::{AppError, CmdResult};
use crate::AppState;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;
use veydan_sync::{available_space, Opener, PathSink};

#[cfg(target_os = "android")]
mod content_uri;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];
/// In-progress local copy; never listed as an attachment.
const PART_SUFFIX: &str = ".part";
/// Free space that must remain after a copy.
const DISK_RESERVE: u64 = 64 * 1024 * 1024;

/// Files the attachment directory holds that are not attachments.
pub(crate) fn is_staging_name(name: &str) -> bool {
    name.ends_with(PART_SUFFIX) || name.ends_with(".tmp") || PathSink::is_staging_file(name)
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteAttachment {
    pub name: String,
    /// Path relative to the note file, ready for a Markdown link
    pub rel_path: String,
    pub size: u64,
    pub is_image: bool,
    /// False for a chunked file that is still only in the vault.
    pub present: bool,
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
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.to_string()
    }
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
        present: true,
    })
}

fn deferred_attachment(note_id: &str, name: String, size: u64) -> NoteAttachment {
    NoteAttachment {
        rel_path: format!("attachments/{note_id}/{name}"),
        is_image: is_image(&name),
        name,
        size,
        present: false,
    }
}

/// Store bytes as a new attachment of the note (small clipboard / capture payloads).
pub(crate) fn store_attachment(
    note_file: &Path,
    note_id: &str,
    file_name: &str,
    data: &[u8],
) -> Result<NoteAttachment, AppError> {
    store_attachment_stream(
        note_file,
        note_id,
        file_name,
        Some(data.len() as u64),
        None,
        &mut &data[..],
    )
}

/// Stream `reader` into `attachments/{note_id}/{name}` through a `.part` file
/// that is renamed only when the copy completed. `max` is the product limit.
pub(crate) fn store_attachment_stream(
    note_file: &Path,
    note_id: &str,
    file_name: &str,
    len_hint: Option<u64>,
    max: Option<u64>,
    reader: &mut dyn Read,
) -> Result<NoteAttachment, AppError> {
    let dir = attachments_dir_for(note_file, note_id);
    std::fs::create_dir_all(&dir).map_err(AppError::io)?;
    if let (Some(need), Some(free)) = (len_hint, available_space(&dir)) {
        if free < need.saturating_add(DISK_RESERVE) {
            return Err(AppError::io(format!(
                "not enough free space: need {need} bytes, {free} available"
            )));
        }
    }
    let dest = unique_path(&dir, &safe_file_name(file_name));
    let part = PathBuf::from(format!("{}{PART_SUFFIX}", dest.display()));
    let copied = (|| -> Result<u64, AppError> {
        let mut out = std::io::BufWriter::with_capacity(
            1 << 20,
            std::fs::File::create(&part).map_err(AppError::io)?,
        );
        let limit = max.map(|m| m + 1).unwrap_or(u64::MAX);
        let n = std::io::copy(&mut reader.take(limit), &mut out).map_err(AppError::io)?;
        if max.is_some_and(|m| n > m) {
            return Err(AppError::other(format!(
                "attachment exceeds the {} byte limit",
                max.unwrap_or(0)
            )));
        }
        out.into_inner()
            .map_err(|e| AppError::io(e.into_error()))?
            .sync_all()
            .map_err(AppError::io)?;
        Ok(n)
    })();
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &dest).map_err(AppError::io)?;
    to_attachment(note_id, &dest).ok_or_else(|| AppError::io("attachment write failed"))
}

/// Streaming input for an attachment: a local path, or an Android `content://` URI.
pub(crate) struct AttachmentSource {
    pub name: String,
    pub len: Option<u64>,
    pub open: Opener,
}

pub(crate) fn open_source(app: &tauri::AppHandle, src: &str) -> CmdResult<AttachmentSource> {
    #[cfg(target_os = "android")]
    if src.starts_with("content://") {
        return content_uri::source(app, src);
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
    let path = PathBuf::from(src);
    let len = std::fs::metadata(&path).map_err(AppError::io)?.len();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    Ok(AttachmentSource {
        name,
        len: Some(len),
        open: Arc::new(move || std::fs::File::open(&path)),
    })
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

fn is_link_boundary(c: char) -> bool {
    matches!(c, ')' | ' ' | '"' | '\'' | '\n' | '>' | ']')
}

/// `encodeURIComponent`, which leaves `'` `(` `)` raw.
fn encode_uri_component(name: &str) -> String {
    encode_path_name(name, true)
}

/// Same encoding, but `'` `(` `)` are percent-encoded so the link can end after the name.
fn encode_attachment_name(name: &str) -> String {
    encode_path_name(name, false)
}

fn encode_path_name(name: &str, keep_parens: bool) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        let plain = ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*')
            || (keep_parens && matches!(ch, '\'' | '(' | ')'));
        if plain {
            out.push(ch);
            continue;
        }
        let mut buf = [0; 4];
        for b in ch.encode_utf8(&mut buf).bytes() {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// True when `name` appears as a whole path segment after `attachments/{id}/`.
fn body_references(body: &str, note_id: &str, name: &str) -> bool {
    let prefix = format!("attachments/{note_id}/");
    let needles = [
        format!("{prefix}{name}"),
        format!("{prefix}{}", encode_uri_component(name)),
        format!("{prefix}{}", encode_attachment_name(name)),
    ];
    needles.iter().any(|needle| contains_bounded(body, needle))
}

fn contains_bounded(body: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(rel) = body[start..].find(needle) {
        let at = start + rel;
        let after = at + needle.len();
        let boundary = body[after..].chars().next();
        if boundary.map(is_link_boundary).unwrap_or(true) {
            return true;
        }
        start = at + 1;
    }
    false
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

/// Copy an existing file (picker, drag-and-drop, Android content URI) into the
/// note's attachments without loading it into memory.
#[tauri::command]
pub async fn note_attachment_add_from_path(
    note_id: String,
    src_path: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachment> {
    let note_file = note_file_path(&note_id, &state).await?;
    let policy = load_attachment_policy(&state.db).await;
    let source = open_source(&app, &src_path)?;
    policy.check_size(source.len)?;
    let max = policy.max_file_bytes();
    tokio::task::spawn_blocking(move || {
        let mut reader = (source.open)().map_err(AppError::io)?;
        store_attachment_stream(
            &note_file,
            &note_id,
            &source.name,
            source.len,
            max,
            &mut reader,
        )
    })
    .await
    .map_err(AppError::io)?
}

#[tauri::command]
pub async fn note_attachment_list(
    note_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteAttachment>> {
    let note_file = note_file_path(&note_id, &state).await?;
    let dir = attachments_dir_for(&note_file, &note_id);
    let mut list: Vec<NoteAttachment> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file() && !is_staging_name(&e.file_name().to_string_lossy()))
        .filter_map(|e| to_attachment(&note_id, &e.path()))
        .collect();
    // Vault-only files the policy did not download; a local copy wins.
    for d in crate::sync::attachments::deferred_for_note(&state, &note_id).await? {
        if !list.iter().any(|a| a.name == d.name) {
            list.push(deferred_attachment(&note_id, d.name, d.size));
        }
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

/// Download a vault-only attachment to the note's directory (progress via transfer events).
#[tauri::command]
pub async fn note_attachment_fetch(
    note_id: String,
    name: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteAttachment> {
    let note_file = note_file_path(&note_id, &state).await?;
    let name = safe_file_name(&name);
    crate::sync::attachments::fetch_deferred(&app, &state, &note_id, &name).await?;
    let path = attachments_dir_for(&note_file, &note_id).join(&name);
    to_attachment(&note_id, &path).ok_or_else(|| AppError::io("attachment download failed"))
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

/// Copy an attachment to a user-chosen destination (Save As): a path, or an
/// Android `content://` URI from the system save dialog.
#[tauri::command]
pub async fn note_attachment_save(
    note_id: String,
    name: String,
    dest: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let note_file = note_file_path(&note_id, &state).await?;
    let path = attachments_dir_for(&note_file, &note_id).join(safe_file_name(&name));
    if !path.is_file() {
        return Err(AppError::not_found(name));
    }
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "android")]
        if dest.starts_with("content://") {
            let mut out = content_uri::writer(&app, &dest)?;
            std::io::copy(
                &mut std::fs::File::open(&path).map_err(AppError::io)?,
                &mut out,
            )
            .map_err(AppError::io)?;
            return Ok(());
        }
        #[cfg(not(target_os = "android"))]
        let _ = app;
        let dest_path = PathBuf::from(&dest);
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::io)?;
        }
        std::fs::copy(&path, &dest_path).map_err(AppError::io)?;
        Ok(())
    })
    .await
    .map_err(AppError::io)?
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
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let body = read_note_file(&note_file)
            .map(|(_, _, b)| b)
            .unwrap_or_default();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
            else {
                continue;
            };
            if body_references(&body, &row.id, &name) || is_staging_name(&name) {
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if delete {
                let _ = std::fs::remove_file(&path);
            }
            orphans.push(OrphanAttachment {
                note_id: row.id.clone(),
                name,
                size,
            });
        }
        if delete
            && std::fs::read_dir(&dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    Ok(orphans)
}
