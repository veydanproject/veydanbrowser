// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Notes as a sync entity. The note file (frontmatter + body) is the payload;
//! it travels as a blob, the op only references it.
//!
//! Op payload:
//! `{ "blob": <name>, "parents": [<blob>..], "format": "md", "title": ".." }`
//! A delete op carries only `parents`.
//!
//! Apply rules for a remote op against the local head:
//! - op.hlc <= head.hlc: skip only when that blob is already in our head.
//!   A divergent older put still merges, so both devices show the conflict.
//! - delete: apply (fast-forward or newer-wins).
//! - put whose parents include our head: fast-forward.
//! - put that diverged: 3-way merge from the exact common ancestor; a clean
//!   merge is pushed with both parents, overlapping edits become a conflict
//!   that keeps the local file untouched and is resolved through the UI.

use super::state::{load_note_state, save_note_state, NoteSyncState};
use crate::commands::notes::{
    effective_docs_dir, history_content_by_id, history_snapshot_by, merge3, note_delete,
    note_restore, parse_note_file, resolve_note_abs_path, set_note_tag_links, sync_notes_index,
    update_note, write_note_file, MergeResult, NoteRow, NoteUpdateInput,
};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use veydan_sync::{sha256_hex, Engine, Hlc, HlcClock, LocalState, Op};

pub const ENTITY: &str = "note";

#[derive(Debug, Default, Serialize, Deserialize)]
struct NotePayload {
    #[serde(default)]
    blob: String,
    #[serde(default)]
    parents: Vec<String>,
    #[serde(default = "default_format")]
    format: String,
    #[serde(default)]
    title: String,
}

fn default_format() -> String {
    "md".into()
}

#[derive(sqlx::FromRow)]
struct NoteHead {
    id: String,
    title: String,
    file_path: String,
    format: String,
    deleted: i64,
}

/// Note ids are UUIDs; anything else must not become a file name.
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn put_op(id: &str, hlc: Hlc, blob: String, parents: Vec<String>, format: &str, title: &str) -> Op {
    Op {
        entity_type: ENTITY.into(),
        entity_id: id.into(),
        hlc,
        deleted: false,
        payload: serde_json::to_value(NotePayload {
            blob,
            parents,
            format: format.into(),
            title: title.into(),
        })
        .unwrap_or_default(),
    }
}

fn delete_op(id: &str, hlc: Hlc, parents: Vec<String>) -> Op {
    Op {
        entity_type: ENTITY.into(),
        entity_id: id.into(),
        hlc,
        deleted: true,
        payload: serde_json::to_value(NotePayload {
            parents,
            ..Default::default()
        })
        .unwrap_or_default(),
    }
}

pub(super) fn docs_dir(state: &AppState) -> PathBuf {
    let custom = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    effective_docs_dir(&state.app_data_dir, custom.as_ref())
}

/// File stems (= note ids) present in the documents directory.
fn file_stems(dir: &PathBuf) -> HashSet<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| {
                    e.path()
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Extension for a new note file; anything odd falls back to `md`.
fn safe_format(format: &str) -> &str {
    if !format.is_empty() && format.len() <= 8 && format.chars().all(|c| c.is_ascii_alphanumeric())
    {
        format
    } else {
        "md"
    }
}

// ── Push ─────────────────────────────────────────────────────────────────────

/// Local changes not yet in the vault. Blobs are uploaded here; the returned
/// states must be saved only after the ops were pushed.
pub struct LocalChanges {
    pub ops: Vec<Op>,
    pub states: Vec<NoteSyncState>,
}

pub async fn collect_local_changes(
    engine: &Engine,
    state: &AppState,
    clock: &mut HlcClock,
) -> CmdResult<LocalChanges> {
    let db = &state.db;
    let rows: Vec<NoteHead> =
        sqlx::query_as("SELECT id, title, file_path, format, deleted FROM notes")
            .fetch_all(db)
            .await
            .map_err(AppError::db)?;
    let mut states = super::state::load_note_states(db).await?;
    let mut out = LocalChanges {
        ops: Vec::new(),
        states: Vec::new(),
    };

    for row in &rows {
        let prev = states.remove(&row.id);
        if row.deleted != 0 {
            // Tombstone once; a note the vault never saw gets one without parents.
            if prev.as_ref().map(|s| s.deleted).unwrap_or(false) {
                continue;
            }
            let mut st = prev.unwrap_or_else(|| NoteSyncState {
                note_id: row.id.clone(),
                ..Default::default()
            });
            let hlc = clock.now();
            out.ops
                .push(delete_op(&row.id, hlc.clone(), head_parents(&st)));
            st.deleted = true;
            st.head_hlc = Some(hlc);
            out.states.push(st);
            continue;
        }
        // An unresolved conflict holds the note back; resolving clears the flag.
        if prev.as_ref().map(|s| s.conflict).unwrap_or(false) {
            continue;
        }
        let path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let hash = sha256_hex(&raw);
        // A resolved conflict is pushed even when the text stayed local, so the vault learns the merge.
        let unchanged = prev
            .as_ref()
            .map(|s| s.synced_hash == hash && !s.deleted && s.conflict_remote_blob.is_empty());
        if unchanged.unwrap_or(false) {
            continue;
        }
        let blob = engine.put_blob(&raw).await.map_err(AppError::other)?;
        // Parents: our head, plus the remote version a resolved conflict merged in.
        let parents: Vec<String> = prev
            .iter()
            .flat_map(|s| [s.head_blob.clone(), s.conflict_remote_blob.clone()])
            .filter(|b| !b.is_empty())
            .collect();
        let hlc = clock.now();
        out.ops.push(put_op(
            &row.id,
            hlc.clone(),
            blob.clone(),
            parents.clone(),
            &row.format,
            &row.title,
        ));
        out.states.push(NoteSyncState {
            note_id: row.id.clone(),
            head_blob: blob,
            head_parents: parents,
            head_hlc: Some(hlc),
            synced_hash: hash,
            ..Default::default()
        });
    }

    // Rows gone entirely (hard delete) while the vault still has them. A file
    // that is still on disk only means the index has not caught up yet.
    let on_disk = file_stems(&docs_dir(state));
    for (_, mut st) in states
        .into_iter()
        .filter(|(id, s)| !s.deleted && !on_disk.contains(id))
    {
        let hlc = clock.now();
        out.ops.push(delete_op(
            &st.note_id,
            hlc.clone(),
            vec![st.head_blob.clone()],
        ));
        st.deleted = true;
        st.head_hlc = Some(hlc);
        out.states.push(st);
    }
    Ok(out)
}

// ── Pull ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ApplyOutcome {
    /// Notes whose file changed; the index and UI must be refreshed.
    pub changed: Vec<String>,
    /// Notes taken out of the trash during apply; the UI is told to refresh them.
    pub restored: Vec<String>,
    /// Set when an op was skipped on a transient condition; peer heads must not
    /// advance so the skipped ops are delivered again next cycle.
    pub retry: Option<String>,
    /// Blobs collected by GC. Reported as warnings; peer heads still advance.
    pub skipped: Vec<String>,
}

async fn load_head(db: &sqlx::Pool<sqlx::Sqlite>, id: &str) -> CmdResult<Option<NoteHead>> {
    sqlx::query_as("SELECT id, title, file_path, format, deleted FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
        .map_err(AppError::db)
}

/// Atomic write: tmp file next to the target, then rename.
pub(super) fn write_raw(path: &PathBuf, raw: &[u8]) -> CmdResult<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::io)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp).map_err(AppError::io)?;
    f.write_all(raw).map_err(AppError::io)?;
    f.sync_all().map_err(AppError::io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(AppError::io)?;
    Ok(())
}

/// Union keeping local order, then remote extras.
fn merge_tags(local: &[String], remote: &[String]) -> Vec<String> {
    let mut out = local.to_vec();
    for t in remote {
        if !out.contains(t) {
            out.push(t.clone());
        }
    }
    out
}

/// Apply remote ops. Ops from the same pull are already HLC-sorted.
/// Merge results are pushed immediately so a later failure cannot strand them.
pub async fn apply_remote(
    engine: &Engine,
    app: &AppHandle,
    ops: Vec<Op>,
    clock: &mut HlcClock,
    local: &mut LocalState,
) -> CmdResult<ApplyOutcome> {
    let state = app.state::<AppState>();
    let db = &state.db;
    let mut outcome = ApplyOutcome::default();
    let total = ops
        .iter()
        .filter(|op| op.entity_type == ENTITY && !op.deleted && valid_id(&op.entity_id))
        .count() as u32;
    let mut current = 0u32;
    // Notes trashed on this device that a newer remote put tried to revive.
    let mut reassert: Vec<String> = Vec::new();

    for op in ops {
        if op.entity_type != ENTITY || !valid_id(&op.entity_id) {
            continue;
        }
        let id = op.entity_id.clone();
        let payload: NotePayload = serde_json::from_value(op.payload.clone()).unwrap_or_default();
        let prev = load_note_state(db, &id).await?;
        // Older than our head: drop it only if we already have that version.
        // A fork with an older clock still merges, so the conflict shows here too.
        let stale = prev
            .as_ref()
            .and_then(|s| s.head_hlc.as_ref())
            .is_some_and(|h| *h >= op.hlc);
        let already = prev
            .as_ref()
            .is_some_and(|s| s.head_blob == payload.blob || s.head_parents.contains(&payload.blob));
        if stale && (op.deleted || already) {
            continue;
        }
        let row = load_head(db, &id).await?;

        if op.deleted {
            if let Some(r) = &row {
                if r.deleted == 0 {
                    note_delete(id.clone(), Some(false), app.state()).await?;
                    outcome.changed.push(id.clone());
                }
            }
            let mut st = prev.unwrap_or_else(|| NoteSyncState {
                note_id: id.clone(),
                ..Default::default()
            });
            st.deleted = true;
            st.head_hlc = Some(op.hlc.clone());
            save_note_state(db, &st).await?;
            continue;
        }

        current += 1;
        super::emit_progress(
            app,
            "apply",
            super::progress_pct(48, 52, current, total.max(1)),
            current,
            total,
            &id,
        );
        // Our own delete wins over a newer remote put; a peer's tombstone may be
        // undone by a put, since that is how a restore on another device arrives.
        if deleted_here(engine, prev.as_ref(), row.as_ref()) {
            if !reassert.contains(&id) {
                reassert.push(id);
            }
            continue;
        }
        let Some(remote_raw) = engine
            .get_blob(&payload.blob)
            .await
            .map_err(AppError::other)?
        else {
            let now_ms = Utc::now().timestamp_millis().max(0) as u64;
            if super::blob_gone(&op, now_ms) {
                outcome
                    .skipped
                    .push(format!("blob for note {id} was collected"));
            } else {
                // Chunk arrived before its blob; deliver the rest again next cycle.
                outcome.retry = Some(format!("blob for note {id} not available yet"));
            }
            continue;
        };
        let remote_hash = sha256_hex(&remote_raw);

        let path = match &row {
            Some(r) => resolve_note_abs_path(&state.app_data_dir, &r.file_path),
            None => docs_dir(&state).join(format!("{id}.{}", safe_format(&payload.format))),
        };
        let local_raw = std::fs::read(&path).ok();
        let local_changed = match (&prev, &local_raw) {
            (Some(s), Some(raw)) if !s.deleted && !s.head_blob.is_empty() => {
                sha256_hex(raw) != s.synced_hash
            }
            _ => false,
        };
        let remote_includes_ours = match &prev {
            None => true,
            Some(s) => {
                s.deleted
                    || s.head_blob.is_empty()
                    || s.head_blob == payload.blob
                    || payload.parents.contains(&s.head_blob)
            }
        };

        // No DB row yet: the file came from an earlier op of this pull, nothing local to merge.
        if (remote_includes_ours || row.is_none()) && !local_changed {
            let same_bytes = local_raw.as_deref() == Some(remote_raw.as_slice());
            if !same_bytes {
                write_raw(&path, &remote_raw)?;
            }
            // Un-trash before the state is saved: if this cycle dies later the
            // op is skipped next time (hlc <= head) and the note would stay deleted.
            if row.as_ref().map(|r| r.deleted != 0).unwrap_or(false) {
                note_restore(id.clone(), app.state()).await?;
                outcome.restored.push(id.clone());
            }
            save_note_state(
                db,
                &NoteSyncState {
                    note_id: id.clone(),
                    head_blob: payload.blob.clone(),
                    head_parents: payload.parents.clone(),
                    head_hlc: Some(op.hlc.clone()),
                    synced_hash: remote_hash,
                    ..Default::default()
                },
            )
            .await?;
            if !same_bytes {
                outcome.changed.push(id);
            }
            continue;
        }

        let mut st = prev.expect("diverged implies a known head");
        if remote_includes_ours && !st.conflict {
            // Plain race with the editor: next cycle pushes the edit first, then merges.
            outcome.retry = Some(format!("note {id} changed during sync"));
            continue;
        }
        let local_raw = local_raw.unwrap_or_default();
        let remote_device = op.hlc.device_id.clone();

        // A newer remote version of an already conflicted note replaces the remote side.
        if st.conflict && !remote_includes_ours {
            let (rk, _, rbody) = parse_note_file(&String::from_utf8_lossy(&remote_raw));
            let title = rk.get("title").cloned().unwrap_or_default();
            st.conflict_remote_id = history_snapshot_by(
                &id,
                &title,
                &rbody,
                "conflict",
                None,
                Some(&remote_device),
                db,
            )
            .await?;
            st.conflict_remote_blob = payload.blob.clone();
            st.head_hlc = Some(op.hlc.clone());
            save_note_state(db, &st).await?;
            continue;
        }

        // Exact common ancestor only: our head when the remote builds on it,
        // otherwise a blob both heads list as parent.
        let ancestor_name = if remote_includes_ours {
            Some(st.head_blob.clone())
        } else {
            payload
                .parents
                .iter()
                .find(|p| st.head_parents.contains(p))
                .cloned()
        };
        let ancestor_raw = match &ancestor_name {
            Some(name) => engine.get_blob(name).await.map_err(AppError::other)?,
            None => None,
        };

        let (lk, ltags, lbody) = parse_note_file(&String::from_utf8_lossy(&local_raw));
        let (rk, rtags, rbody) = parse_note_file(&String::from_utf8_lossy(&remote_raw));
        let local_title = lk.get("title").cloned().unwrap_or_default();
        let remote_title = rk.get("title").cloned().unwrap_or_default();

        // Ancestor unknown (compacted away or never shared) and nothing edited
        // here since the last sync: the newer op wins on both devices, the
        // local version stays recoverable from history. A fast-forward would
        // let two real forks swap contents forever.
        let Some(ancestor_raw) = ancestor_raw else {
            if !local_changed {
                history_snapshot_by(&id, &local_title, &lbody, "conflict", None, None, db).await?;
                write_raw(&path, &remote_raw)?;
                save_note_state(
                    db,
                    &NoteSyncState {
                        note_id: id.clone(),
                        head_blob: payload.blob.clone(),
                        head_parents: payload.parents.clone(),
                        head_hlc: Some(op.hlc.clone()),
                        synced_hash: remote_hash,
                        ..Default::default()
                    },
                )
                .await?;
                outcome.changed.push(id);
                continue;
            }
            record_conflict(
                &mut st,
                &id,
                "",
                &local_title,
                &lbody,
                &remote_title,
                &rbody,
                &remote_device,
                &payload.blob,
                &op.hlc,
                db,
            )
            .await?;
            continue;
        };

        let (ak, _, abody) = parse_note_file(&String::from_utf8_lossy(&ancestor_raw));
        let merged = merge3(&abody, &lbody, &rbody);
        if merged.has_conflicts {
            record_conflict(
                &mut st,
                &id,
                &abody,
                &local_title,
                &lbody,
                &remote_title,
                &rbody,
                &remote_device,
                &payload.blob,
                &op.hlc,
                db,
            )
            .await?;
            continue;
        }

        // Independent edits: apply the merge and keep every side restorable.
        let title = if lk.get("title") != ak.get("title") {
            local_title.clone()
        } else {
            remote_title.clone()
        };
        let tags = merge_tags(&ltags, &rtags);
        let mut note_row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
            .bind(&id)
            .fetch_optional(db)
            .await
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;
        note_row.title = title.clone();
        note_row.updated_at = Utc::now().to_rfc3339();

        let local_id =
            history_snapshot_by(&id, &local_title, &lbody, "sync", None, None, db).await?;
        history_snapshot_by(
            &id,
            &remote_title,
            &rbody,
            "sync",
            None,
            Some(&remote_device),
            db,
        )
        .await?;
        history_snapshot_by(
            &id,
            &title,
            &merged.content,
            "merge",
            Some(local_id),
            None,
            db,
        )
        .await?;
        write_note_file(&path, &note_row, &tags, &merged.content)?;

        let merged_raw = std::fs::read(&path).map_err(AppError::io)?;
        let blob = engine
            .put_blob(&merged_raw)
            .await
            .map_err(AppError::other)?;
        let parents = vec![st.head_blob.clone(), payload.blob.clone()];
        let hlc = clock.now();
        engine
            .push(
                local,
                vec![put_op(
                    &id,
                    hlc.clone(),
                    blob.clone(),
                    parents.clone(),
                    &note_row.format,
                    &title,
                )],
            )
            .await
            .map_err(AppError::other)?;
        super::state::save_own_state(db, local, &clock.last()).await?;
        save_note_state(
            db,
            &NoteSyncState {
                note_id: id.clone(),
                head_blob: blob,
                head_parents: parents,
                head_hlc: Some(hlc),
                synced_hash: sha256_hex(&merged_raw),
                ..Default::default()
            },
        )
        .await?;
        outcome.changed.push(id);
    }

    // The clock already observed every pulled op, so these tombstones sort after the puts.
    if !reassert.is_empty() {
        let mut ops = Vec::new();
        let mut states = Vec::new();
        for id in &reassert {
            let mut st = load_note_state(db, id)
                .await?
                .unwrap_or_else(|| NoteSyncState {
                    note_id: id.clone(),
                    ..Default::default()
                });
            let hlc = clock.now();
            ops.push(delete_op(id, hlc.clone(), head_parents(&st)));
            st.deleted = true;
            st.conflict = false;
            st.head_hlc = Some(hlc);
            states.push(st);
        }
        engine.push(local, ops).await.map_err(AppError::other)?;
        super::state::save_own_state(db, local, &clock.last()).await?;
        for st in &states {
            save_note_state(db, st).await?;
        }
    }
    Ok(outcome)
}

/// Keep the local file readable; ancestor and both sides go to history for the UI.
#[allow(clippy::too_many_arguments)]
async fn record_conflict(
    st: &mut NoteSyncState,
    id: &str,
    abody: &str,
    local_title: &str,
    lbody: &str,
    remote_title: &str,
    rbody: &str,
    remote_device: &str,
    remote_blob: &str,
    hlc: &Hlc,
    db: &sqlx::SqlitePool,
) -> CmdResult<()> {
    let ancestor_id =
        history_snapshot_by(id, local_title, abody, "conflict", None, None, db).await?;
    let local_id = history_snapshot_by(
        id,
        local_title,
        lbody,
        "conflict",
        Some(ancestor_id.clone()),
        None,
        db,
    )
    .await?;
    let remote_id = history_snapshot_by(
        id,
        remote_title,
        rbody,
        "conflict",
        Some(ancestor_id.clone()),
        Some(remote_device),
        db,
    )
    .await?;
    st.conflict = true;
    st.conflict_ancestor_id = ancestor_id;
    st.conflict_local_id = local_id;
    st.conflict_remote_id = remote_id;
    st.conflict_remote_blob = remote_blob.to_string();
    st.head_hlc = Some(hlc.clone());
    save_note_state(db, st).await
}

/// Tombstone parents: the last known head, if any.
fn head_parents(st: &NoteSyncState) -> Vec<String> {
    std::iter::once(st.head_blob.clone())
        .filter(|b| !b.is_empty())
        .collect()
}

/// True when this device trashed the note: its own tombstone is the head, or
/// the row went to the trash after the tombstone was collected.
fn deleted_here(engine: &Engine, prev: Option<&NoteSyncState>, row: Option<&NoteHead>) -> bool {
    match prev {
        Some(s) if s.deleted => s
            .head_hlc
            .as_ref()
            .map(|h| h.device_id == engine.device_id())
            .unwrap_or(false),
        _ => row.map(|r| r.deleted != 0).unwrap_or(false),
    }
}

/// Conflict snapshot for the UI; `token` ties a resolution to this exact state.
#[derive(Debug, Serialize)]
pub struct ConflictView {
    pub token: String,
    pub merge: MergeResult,
    /// Name from sync settings. Empty when the user has not set one.
    pub local_device: String,
    /// Name of the device that wrote the remote side. Empty when unknown.
    pub remote_device: String,
}

/// Unresolved conflict state plus the current local body.
async fn load_conflict(state: &AppState, note_id: &str) -> CmdResult<(NoteSyncState, String)> {
    let db = &state.db;
    let st = load_note_state(db, note_id)
        .await?
        .filter(|s| s.conflict)
        .ok_or_else(|| AppError::not_found(format!("conflict for note {note_id}")))?;
    let row = load_head(db, note_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("Note {note_id}")))?;
    let path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    let (_, _, local) = parse_note_file(&std::fs::read_to_string(&path).unwrap_or_default());
    Ok((st, local))
}

/// Changes when either side of the conflict or the local text changes.
fn conflict_token(st: &NoteSyncState, local: &str) -> String {
    let mut seed = String::new();
    for part in [
        &st.conflict_ancestor_id,
        &st.conflict_local_id,
        &st.conflict_remote_id,
        &st.conflict_remote_blob,
    ] {
        seed.push_str(part);
        seed.push('\n');
    }
    seed.push_str(&sha256_hex(local.as_bytes()));
    sha256_hex(seed.as_bytes())
}

/// Device id stored on the remote conflict snapshot.
async fn history_device(db: &sqlx::SqlitePool, history_id: &str) -> String {
    if history_id.is_empty() {
        return String::new();
    }
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT device FROM note_history WHERE id = ?")
            .bind(history_id)
            .fetch_optional(db)
            .await
            .unwrap_or(None);
    row.and_then(|(id,)| id).unwrap_or_default()
}

/// Human name for a device id from the last sync device list. Never the raw id.
fn cached_device_name(raw: &str, device_id: &str) -> String {
    if device_id.is_empty() {
        return String::new();
    }
    #[derive(Deserialize)]
    struct Named {
        id: String,
        #[serde(default)]
        name: String,
    }
    let list: Vec<Named> = serde_json::from_str(raw).unwrap_or_default();
    list.into_iter()
        .find(|d| d.id == device_id)
        .map(|d| d.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_default()
}

/// Merge data for the conflict UI: ancestor and remote from history, local from the file.
pub async fn conflict_merge(state: &AppState, note_id: &str) -> CmdResult<ConflictView> {
    let db = &state.db;
    let (st, local) = load_conflict(state, note_id).await?;
    let ancestor = history_content_by_id(&st.conflict_ancestor_id, db)
        .await
        .unwrap_or_default();
    let remote = history_content_by_id(&st.conflict_remote_id, db).await?;
    let local_device = super::config::get_setting(db, "sync_device_name")
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    let remote_id = history_device(db, &st.conflict_remote_id).await;
    let devices = super::config::get_setting(db, "sync_devices")
        .await
        .unwrap_or_default();
    Ok(ConflictView {
        token: conflict_token(&st, &local),
        merge: merge3(&ancestor, &local, &remote).or_whole_texts(&local, &remote),
        local_device,
        remote_device: cached_device_name(&devices, &remote_id),
    })
}

/// Write the user's resolution and release the note for the next push,
/// which records the remote version as second parent. Refuses a stale token.
pub async fn resolve_conflict(
    state: &AppState,
    note_id: &str,
    token: &str,
    content: String,
) -> CmdResult<()> {
    let db = &state.db;
    let (mut st, local) = load_conflict(state, note_id).await?;
    if conflict_token(&st, &local) != token {
        return Err(AppError::conflict_changed(format!("note {note_id}")));
    }
    update_note(
        note_id,
        NoteUpdateInput {
            title: None,
            content: Some(content),
            pinned: None,
            base_hash: None,
        },
        state,
    )
    .await?;
    st.conflict = false;
    st.conflict_ancestor_id.clear();
    st.conflict_local_id.clear();
    st.conflict_remote_id.clear();
    save_note_state(db, &st).await
}

/// Reindex changed files, relink tags from frontmatter, notify UI.
pub async fn finish_apply(app: &AppHandle, outcome: &ApplyOutcome) -> CmdResult<()> {
    if outcome.changed.is_empty() {
        for id in &outcome.restored {
            let _ = app.emit("notes://external-change", id);
        }
        return Ok(());
    }
    let state = app.state::<AppState>();
    let custom = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    sync_notes_index(&state.db, &state.app_data_dir, custom.as_ref()).await?;

    let mut tags_by_note: HashMap<String, Vec<String>> = HashMap::new();
    for id in &outcome.changed {
        if let Some(row) = load_head(&state.db, id).await? {
            let path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
            if let Ok(raw) = std::fs::read_to_string(&path) {
                let (kv, tags, _) = parse_note_file(&raw);
                // Skip files we could not parse so we do not wipe existing links.
                if kv.contains_key("id") {
                    tags_by_note.insert(id.clone(), tags);
                }
            }
        }
    }
    for (id, tags) in &tags_by_note {
        set_note_tag_links(id, tags, &state.db).await?;
    }
    for id in outcome.changed.iter().chain(
        outcome
            .restored
            .iter()
            .filter(|r| !outcome.changed.contains(r)),
    ) {
        let _ = app.emit("notes://external-change", id);
    }
    Ok(())
}
