// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Note attachments as a sync entity. Each file under `attachments/<note_id>/`
//! is one entity `<note_id>/<name>`; the bytes travel as an encrypted blob.
//!
//! Op payload: `{ "note_id": .., "name": .., "blob": <name>, "size": n }`.
//! A delete op carries only `note_id` and `name`.
//!
//! Binary files are not merged: a remote op newer than our head wins (LWW).

use super::notes::{docs_dir, valid_id, write_raw};
use super::state::{load_attachment_state, load_attachment_states, save_attachment_state, AttachmentSyncState};
use crate::commands::notes::{attachments_dir_for, resolve_note_abs_path, safe_file_name};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager};
use veydan_sync::{sha256_hex, Engine, Hlc, HlcClock, Op};

pub const ENTITY: &str = "note_attachment";
pub const EVENT_CHANGED: &str = "notes://attachments-changed";

#[derive(Debug, Default, Serialize, Deserialize)]
struct AttachmentPayload {
    #[serde(default)]
    note_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    blob: String,
    #[serde(default)]
    size: u64,
}

/// A name is safe when the sanitizer leaves it untouched.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && safe_file_name(name) == name
}

fn entity_id(note_id: &str, name: &str) -> String {
    format!("{note_id}/{name}")
}

fn make_op(note_id: &str, name: &str, hlc: Hlc, blob: Option<(String, u64)>) -> Op {
    let (blob, size) = blob.unwrap_or_default();
    Op {
        entity_type: ENTITY.into(),
        entity_id: entity_id(note_id, name),
        hlc,
        deleted: false,
        payload: serde_json::to_value(AttachmentPayload { note_id: note_id.into(), name: name.into(), blob, size })
            .unwrap_or_default(),
    }
}

fn delete_op(note_id: &str, name: &str, hlc: Hlc) -> Op {
    let mut op = make_op(note_id, name, hlc, None);
    op.deleted = true;
    op
}

/// Attachment directory: next to the note file, or the default location for a
/// note that has not arrived yet.
async fn attachment_dir(state: &AppState, note_id: &str) -> CmdResult<PathBuf> {
    let row: Option<(String,)> = sqlx::query_as("SELECT file_path FROM notes WHERE id = ?")
        .bind(note_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(match row {
        Some((file_path,)) => attachments_dir_for(&resolve_note_abs_path(&state.app_data_dir, &file_path), note_id),
        None => docs_dir(state).join("attachments").join(note_id),
    })
}

/// Cached hash of a file, valid while its mtime and size are unchanged.
#[derive(Clone)]
pub struct FileStamp {
    mtime: Option<SystemTime>,
    size: u64,
    hash: String,
}

/// Hash without re-reading unchanged files; the cache is per process.
fn file_hash(state: &AppState, path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let (mtime, size) = (meta.modified().ok(), meta.len());
    let mut cache = state.sync.attachment_hashes.lock().ok()?;
    if let Some(s) = cache.get(path) {
        if s.mtime == mtime && s.size == size {
            return Some(s.hash.clone());
        }
    }
    let hash = sha256_hex(&std::fs::read(path).ok()?);
    cache.insert(path.to_path_buf(), FileStamp { mtime, size, hash: hash.clone() });
    Some(hash)
}

/// Attachment names in a directory (sanitizer-safe, no temp files).
fn dir_names(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|name| valid_name(name) && !name.ends_with(".tmp"))
        .collect()
}

// ── Push ─────────────────────────────────────────────────────────────────────

pub struct LocalChanges {
    pub ops: Vec<Op>,
    pub states: Vec<AttachmentSyncState>,
}

/// Scan every attachment directory: new/changed files become put ops,
/// files that disappeared become tombstones.
pub async fn collect_local_changes(engine: &Engine, state: &AppState, clock: &mut HlcClock) -> CmdResult<LocalChanges> {
    let db = &state.db;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, file_path FROM notes").fetch_all(db).await.map_err(AppError::db)?;
    let mut states = load_attachment_states(db).await?;
    let mut out = LocalChanges { ops: Vec::new(), states: Vec::new() };

    // Directories to scan: next to each known note, plus default-location dirs
    // of notes whose row has not been created yet.
    let mut dirs: Vec<(String, PathBuf)> = rows
        .iter()
        .map(|(id, fp)| (id.clone(), attachments_dir_for(&resolve_note_abs_path(&state.app_data_dir, fp), id)))
        .collect();
    let known: HashSet<&String> = rows.iter().map(|(id, _)| id).collect();
    if let Ok(rd) = std::fs::read_dir(docs_dir(state).join("attachments")) {
        for e in rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
            let id = e.file_name().to_string_lossy().to_string();
            if valid_id(&id) && !known.contains(&id) {
                dirs.push((id, e.path()));
            }
        }
    }

    for (note_id, dir) in &dirs {
        for name in dir_names(dir) {
            let path = dir.join(&name);
            let Some(hash) = file_hash(state, &path) else { continue };
            let prev = states.remove(&(note_id.clone(), name.clone()));
            if prev.as_ref().map(|s| s.synced_hash == hash && !s.deleted).unwrap_or(false) {
                continue;
            }
            let Ok(raw) = std::fs::read(&path) else { continue };
            let blob = engine.put_blob(&raw).await.map_err(AppError::other)?;
            let hlc = clock.now();
            out.ops.push(make_op(note_id, &name, hlc.clone(), Some((blob.clone(), raw.len() as u64))));
            out.states.push(AttachmentSyncState {
                note_id: note_id.clone(),
                name,
                head_blob: blob,
                head_hlc: Some(hlc),
                synced_hash: hash,
                deleted: false,
            });
        }
    }

    // Whatever is left in `states` is no longer on disk.
    for (_, mut st) in states.into_iter().filter(|(_, s)| !s.deleted) {
        let hlc = clock.now();
        out.ops.push(delete_op(&st.note_id, &st.name, hlc.clone()));
        st.deleted = true;
        st.head_hlc = Some(hlc);
        out.states.push(st);
    }
    Ok(out)
}

// ── Pull ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ApplyOutcome {
    /// Notes whose attachment set changed; the editor refreshes its list.
    pub changed_notes: Vec<String>,
    /// Set when a blob was not yet available; peer heads must not advance.
    pub retry: Option<String>,
}

/// Apply remote attachment ops (LWW by HLC).
pub async fn apply_remote(engine: &Engine, app: &AppHandle, ops: &[Op]) -> CmdResult<ApplyOutcome> {
    let state = app.state::<AppState>();
    let db = &state.db;
    let mut outcome = ApplyOutcome::default();

    for op in ops.iter().filter(|op| op.entity_type == ENTITY) {
        let payload: AttachmentPayload = serde_json::from_value(op.payload.clone()).unwrap_or_default();
        if !valid_id(&payload.note_id) || !valid_name(&payload.name) {
            continue;
        }
        let prev = load_attachment_state(db, &payload.note_id, &payload.name).await?;
        if prev.as_ref().and_then(|s| s.head_hlc.as_ref()).map(|h| *h >= op.hlc).unwrap_or(false) {
            continue;
        }
        let path = attachment_dir(&state, &payload.note_id).await?.join(&payload.name);
        let mut st = prev.unwrap_or_else(|| AttachmentSyncState {
            note_id: payload.note_id.clone(),
            name: payload.name.clone(),
            ..Default::default()
        });

        if op.deleted {
            if path.is_file() {
                std::fs::remove_file(&path).map_err(AppError::io)?;
                outcome.changed_notes.push(payload.note_id.clone());
            }
            st.deleted = true;
            st.head_hlc = Some(op.hlc.clone());
            save_attachment_state(db, &st).await?;
            continue;
        }

        let Some(raw) = engine.get_blob(&payload.blob).await.map_err(AppError::other)? else {
            outcome.retry = Some(format!("blob for attachment {} not available yet", op.entity_id));
            continue;
        };
        let hash = sha256_hex(&raw);
        if std::fs::read(&path).ok().as_deref() != Some(raw.as_slice()) {
            write_raw(&path, &raw)?;
            outcome.changed_notes.push(payload.note_id.clone());
        }
        st.head_blob = payload.blob.clone();
        st.head_hlc = Some(op.hlc.clone());
        st.synced_hash = hash;
        st.deleted = false;
        save_attachment_state(db, &st).await?;
    }
    outcome.changed_notes.sort();
    outcome.changed_notes.dedup();
    Ok(outcome)
}

/// Tell the editor which notes got new or removed attachments.
pub fn notify(app: &AppHandle, outcome: &ApplyOutcome) {
    for id in &outcome.changed_notes {
        let _ = app.emit(EVENT_CHANGED, id);
    }
}
