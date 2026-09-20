// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Persistence of the engine's `LocalState` and per-note sync positions.

use super::config::{get_setting, set_setting};
use crate::error::{AppError, CmdResult};
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use veydan_sync::{Hlc, LocalState, PeerHead};

/// Where the local note file stands relative to the vault.
#[derive(Debug, Clone, Default)]
pub struct NoteSyncState {
    pub note_id: String,
    /// Blob of the version this device last pushed or applied.
    pub head_blob: String,
    pub head_parents: Vec<String>,
    pub head_hlc: Option<Hlc>,
    /// SHA-256 of the local file when `head_blob` was pushed/applied.
    pub synced_hash: String,
    pub deleted: bool,
    /// A remote version overlaps local edits; the note is not pushed until resolved.
    pub conflict: bool,
    pub conflict_ancestor_id: String,
    pub conflict_local_id: String,
    pub conflict_remote_id: String,
    /// Remote blob to record as second parent once the conflict is resolved.
    pub conflict_remote_blob: String,
}

type NoteStateRow = (String, String, String, String, String, i64, i64, String, String, String, String);

const SELECT_NOTE_STATE: &str = "SELECT note_id, head_blob, head_parents, head_hlc, synced_hash, deleted, conflict,
    conflict_ancestor_id, conflict_local_id, conflict_remote_id, conflict_remote_blob FROM sync_note_state";

fn row_to_state(r: NoteStateRow) -> NoteSyncState {
    NoteSyncState {
        note_id: r.0,
        head_blob: r.1,
        head_parents: serde_json::from_str(&r.2).unwrap_or_default(),
        head_hlc: Hlc::decode(&r.3),
        synced_hash: r.4,
        deleted: r.5 != 0,
        conflict: r.6 != 0,
        conflict_ancestor_id: r.7,
        conflict_local_id: r.8,
        conflict_remote_id: r.9,
        conflict_remote_blob: r.10,
    }
}

pub async fn load_note_states(db: &Pool<Sqlite>) -> CmdResult<HashMap<String, NoteSyncState>> {
    let rows: Vec<NoteStateRow> = sqlx::query_as(SELECT_NOTE_STATE).fetch_all(db).await.map_err(AppError::db)?;
    Ok(rows.into_iter().map(row_to_state).map(|s| (s.note_id.clone(), s)).collect())
}

pub async fn load_note_state(db: &Pool<Sqlite>, note_id: &str) -> CmdResult<Option<NoteSyncState>> {
    let row: Option<NoteStateRow> = sqlx::query_as(
        "SELECT note_id, head_blob, head_parents, head_hlc, synced_hash, deleted, conflict,
         conflict_ancestor_id, conflict_local_id, conflict_remote_id, conflict_remote_blob
         FROM sync_note_state WHERE note_id = ?",
    )
    .bind(note_id)
    .fetch_optional(db)
    .await
    .map_err(AppError::db)?;
    Ok(row.map(row_to_state))
}

pub async fn save_note_state(db: &Pool<Sqlite>, s: &NoteSyncState) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO sync_note_state (note_id, head_blob, head_parents, head_hlc, synced_hash, deleted, conflict,
           conflict_ancestor_id, conflict_local_id, conflict_remote_id, conflict_remote_blob)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(note_id) DO UPDATE SET
           head_blob = excluded.head_blob, head_parents = excluded.head_parents, head_hlc = excluded.head_hlc,
           synced_hash = excluded.synced_hash, deleted = excluded.deleted, conflict = excluded.conflict,
           conflict_ancestor_id = excluded.conflict_ancestor_id, conflict_local_id = excluded.conflict_local_id,
           conflict_remote_id = excluded.conflict_remote_id, conflict_remote_blob = excluded.conflict_remote_blob",
    )
    .bind(&s.note_id)
    .bind(&s.head_blob)
    .bind(serde_json::to_string(&s.head_parents).unwrap_or_else(|_| "[]".into()))
    .bind(s.head_hlc.as_ref().map(Hlc::encode).unwrap_or_default())
    .bind(&s.synced_hash)
    .bind(s.deleted as i64)
    .bind(s.conflict as i64)
    .bind(&s.conflict_ancestor_id)
    .bind(&s.conflict_local_id)
    .bind(&s.conflict_remote_id)
    .bind(&s.conflict_remote_blob)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

/// Forget every sync position; the next cycle starts from scratch.
pub async fn clear_all_states(db: &Pool<Sqlite>) -> CmdResult<()> {
    for table in [
        "sync_note_state",
        "sync_attachment_state",
        "sync_row_state",
        "sync_profile_files_state",
        "sync_gc_candidates",
        "sync_peers",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}"))).execute(db).await.map_err(AppError::db)?;
    }
    Ok(())
}

// ── Table rows ───────────────────────────────────────────────────────────────

/// Where a local table row stands relative to the vault (LWW, no merge).
#[derive(Debug, Clone, Default)]
pub struct RowSyncState {
    pub entity: String,
    pub id: String,
    pub head_hlc: Option<Hlc>,
    /// Hash of the synced columns when the row was last pushed or applied.
    pub synced_hash: String,
    pub deleted: bool,
}

type RowStateRow = (String, String, String, String, i64);

fn row_to_row_state(r: RowStateRow) -> RowSyncState {
    RowSyncState { entity: r.0, id: r.1, head_hlc: Hlc::decode(&r.2), synced_hash: r.3, deleted: r.4 != 0 }
}

pub async fn load_row_states(db: &Pool<Sqlite>, entity: &str) -> CmdResult<HashMap<String, RowSyncState>> {
    let rows: Vec<RowStateRow> = sqlx::query_as(
        "SELECT entity_type, entity_id, head_hlc, synced_hash, deleted FROM sync_row_state WHERE entity_type = ?",
    )
    .bind(entity)
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;
    Ok(rows.into_iter().map(row_to_row_state).map(|s| (s.id.clone(), s)).collect())
}

pub async fn load_row_state(db: &Pool<Sqlite>, entity: &str, id: &str) -> CmdResult<Option<RowSyncState>> {
    let row: Option<RowStateRow> = sqlx::query_as(
        "SELECT entity_type, entity_id, head_hlc, synced_hash, deleted FROM sync_row_state
         WHERE entity_type = ? AND entity_id = ?",
    )
    .bind(entity)
    .bind(id)
    .fetch_optional(db)
    .await
    .map_err(AppError::db)?;
    Ok(row.map(row_to_row_state))
}

pub async fn save_row_state(db: &Pool<Sqlite>, s: &RowSyncState) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO sync_row_state (entity_type, entity_id, head_hlc, synced_hash, deleted)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(entity_type, entity_id) DO UPDATE SET
           head_hlc = excluded.head_hlc, synced_hash = excluded.synced_hash, deleted = excluded.deleted",
    )
    .bind(&s.entity)
    .bind(&s.id)
    .bind(s.head_hlc.as_ref().map(Hlc::encode).unwrap_or_default())
    .bind(&s.synced_hash)
    .bind(s.deleted as i64)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

// ── Attachments ──────────────────────────────────────────────────────────────

/// Where a local attachment file stands relative to the vault (LWW, no merge).
#[derive(Debug, Clone, Default)]
pub struct AttachmentSyncState {
    pub note_id: String,
    pub name: String,
    pub head_blob: String,
    pub head_hlc: Option<Hlc>,
    pub synced_hash: String,
    pub deleted: bool,
}

type AttachmentStateRow = (String, String, String, String, String, i64);

fn row_to_attachment_state(r: AttachmentStateRow) -> AttachmentSyncState {
    AttachmentSyncState {
        note_id: r.0,
        name: r.1,
        head_blob: r.2,
        head_hlc: Hlc::decode(&r.3),
        synced_hash: r.4,
        deleted: r.5 != 0,
    }
}

pub async fn load_attachment_states(db: &Pool<Sqlite>) -> CmdResult<HashMap<(String, String), AttachmentSyncState>> {
    let rows: Vec<AttachmentStateRow> =
        sqlx::query_as("SELECT note_id, name, head_blob, head_hlc, synced_hash, deleted FROM sync_attachment_state")
            .fetch_all(db)
            .await
            .map_err(AppError::db)?;
    Ok(rows
        .into_iter()
        .map(row_to_attachment_state)
        .map(|s| ((s.note_id.clone(), s.name.clone()), s))
        .collect())
}

pub async fn load_attachment_state(db: &Pool<Sqlite>, note_id: &str, name: &str) -> CmdResult<Option<AttachmentSyncState>> {
    let row: Option<AttachmentStateRow> = sqlx::query_as(
        "SELECT note_id, name, head_blob, head_hlc, synced_hash, deleted FROM sync_attachment_state WHERE note_id = ? AND name = ?",
    )
    .bind(note_id)
    .bind(name)
    .fetch_optional(db)
    .await
    .map_err(AppError::db)?;
    Ok(row.map(row_to_attachment_state))
}

pub async fn save_attachment_state(db: &Pool<Sqlite>, s: &AttachmentSyncState) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO sync_attachment_state (note_id, name, head_blob, head_hlc, synced_hash, deleted)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(note_id, name) DO UPDATE SET
           head_blob = excluded.head_blob, head_hlc = excluded.head_hlc,
           synced_hash = excluded.synced_hash, deleted = excluded.deleted",
    )
    .bind(&s.note_id)
    .bind(&s.name)
    .bind(&s.head_blob)
    .bind(s.head_hlc.as_ref().map(Hlc::encode).unwrap_or_default())
    .bind(&s.synced_hash)
    .bind(s.deleted as i64)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

// ── Profile files ────────────────────────────────────────────────────────────

/// Where the local `firefox-profile/` stands relative to the vault, plus the
/// lease telling which device may run the profile right now.
#[derive(Debug, Clone, Default)]
pub struct ProfileFilesState {
    pub profile_id: String,
    /// HLC of the last snapshot op pushed or applied.
    pub head_hlc: Option<Hlc>,
    /// Hash of the manifest JSON at that point.
    pub synced_hash: String,
    /// That manifest; lets unchanged files reuse their blob without re-reading.
    pub manifest_json: String,
    pub snapshot_at: String,
    /// Local files changed since the last snapshot (browser ran).
    pub dirty: bool,
    /// Remote manifest blob that could not be applied while the browser ran.
    pub pending_manifest: String,
    pub lease_device: String,
    pub lease_name: String,
    pub lease_since: String,
    pub lease_hlc: Option<Hlc>,
    /// Our own lease change was pushed.
    pub lease_synced: bool,
    /// Both sides changed the files; the user picks a side.
    pub diverged: bool,
}

impl ProfileFilesState {
    pub fn new(profile_id: &str) -> Self {
        Self { profile_id: profile_id.into(), lease_synced: true, ..Default::default() }
    }
}

type ProfileFilesRow = (String, String, String, String, String, i64, String, String, String, String, String, i64, i64);

const SELECT_PROFILE_FILES: &str = "SELECT profile_id, head_hlc, synced_hash, manifest_json, snapshot_at, dirty,
    pending_manifest, lease_device, lease_name, lease_since, lease_hlc, lease_synced, diverged FROM sync_profile_files_state";

fn row_to_profile_files(r: ProfileFilesRow) -> ProfileFilesState {
    ProfileFilesState {
        profile_id: r.0,
        head_hlc: Hlc::decode(&r.1),
        synced_hash: r.2,
        manifest_json: r.3,
        snapshot_at: r.4,
        dirty: r.5 != 0,
        pending_manifest: r.6,
        lease_device: r.7,
        lease_name: r.8,
        lease_since: r.9,
        lease_hlc: Hlc::decode(&r.10),
        lease_synced: r.11 != 0,
        diverged: r.12 != 0,
    }
}

pub async fn load_profile_files_states(db: &Pool<Sqlite>) -> CmdResult<HashMap<String, ProfileFilesState>> {
    let rows: Vec<ProfileFilesRow> = sqlx::query_as(SELECT_PROFILE_FILES).fetch_all(db).await.map_err(AppError::db)?;
    Ok(rows.into_iter().map(row_to_profile_files).map(|s| (s.profile_id.clone(), s)).collect())
}

pub async fn load_profile_files_state(db: &Pool<Sqlite>, profile_id: &str) -> CmdResult<Option<ProfileFilesState>> {
    let row: Option<ProfileFilesRow> = sqlx::query_as(
        "SELECT profile_id, head_hlc, synced_hash, manifest_json, snapshot_at, dirty,
         pending_manifest, lease_device, lease_name, lease_since, lease_hlc, lease_synced, diverged
         FROM sync_profile_files_state WHERE profile_id = ?",
    )
    .bind(profile_id)
    .fetch_optional(db)
    .await
    .map_err(AppError::db)?;
    Ok(row.map(row_to_profile_files))
}

pub async fn save_profile_files_state(db: &Pool<Sqlite>, s: &ProfileFilesState) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO sync_profile_files_state (profile_id, head_hlc, synced_hash, manifest_json, snapshot_at, dirty,
           pending_manifest, lease_device, lease_name, lease_since, lease_hlc, lease_synced, diverged)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(profile_id) DO UPDATE SET
           head_hlc = excluded.head_hlc, synced_hash = excluded.synced_hash, manifest_json = excluded.manifest_json,
           snapshot_at = excluded.snapshot_at, dirty = excluded.dirty, pending_manifest = excluded.pending_manifest,
           lease_device = excluded.lease_device, lease_name = excluded.lease_name, lease_since = excluded.lease_since,
           lease_hlc = excluded.lease_hlc, lease_synced = excluded.lease_synced, diverged = excluded.diverged",
    )
    .bind(&s.profile_id)
    .bind(s.head_hlc.as_ref().map(Hlc::encode).unwrap_or_default())
    .bind(&s.synced_hash)
    .bind(&s.manifest_json)
    .bind(&s.snapshot_at)
    .bind(s.dirty as i64)
    .bind(&s.pending_manifest)
    .bind(&s.lease_device)
    .bind(&s.lease_name)
    .bind(&s.lease_since)
    .bind(s.lease_hlc.as_ref().map(Hlc::encode).unwrap_or_default())
    .bind(s.lease_synced as i64)
    .bind(s.diverged as i64)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

pub async fn delete_profile_files_state(db: &Pool<Sqlite>, profile_id: &str) -> CmdResult<()> {
    sqlx::query("DELETE FROM sync_profile_files_state WHERE profile_id = ?")
        .bind(profile_id)
        .execute(db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

/// Leased profiles: (profile_id, lease_device, lease_name).
pub async fn profile_leases(db: &Pool<Sqlite>) -> CmdResult<Vec<(String, String, String)>> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT profile_id, lease_device, lease_name FROM sync_profile_files_state WHERE lease_device != ''",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)
}

/// Profiles whose files diverged: (id, name).
pub async fn profile_conflicts(db: &Pool<Sqlite>) -> CmdResult<Vec<(String, String)>> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT s.profile_id, COALESCE(p.name, s.profile_id) FROM sync_profile_files_state s
         LEFT JOIN profiles p ON p.id = s.profile_id WHERE s.diverged = 1",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)
}

/// Notes with an unresolved sync conflict: (id, title).
pub async fn conflicts(db: &Pool<Sqlite>) -> CmdResult<Vec<(String, String)>> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT s.note_id, COALESCE(n.title, s.note_id) FROM sync_note_state s
         LEFT JOIN notes n ON n.id = s.note_id
         WHERE s.conflict = 1 AND s.deleted = 0",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)
}

pub async fn load_local_state(db: &Pool<Sqlite>) -> CmdResult<(LocalState, Option<Hlc>)> {
    let own_seq = get_setting(db, "sync_own_seq").await.and_then(|v| v.parse().ok()).unwrap_or(0);
    let own_head_hash = get_setting(db, "sync_own_head").await.unwrap_or_default();
    let hlc = get_setting(db, "sync_hlc").await.and_then(|v| Hlc::decode(&v));
    let rows: Vec<(String, i64, String)> = sqlx::query_as("SELECT device_id, seq, head_hash FROM sync_peers")
        .fetch_all(db)
        .await
        .map_err(AppError::db)?;
    let peers = rows.into_iter().map(|(d, seq, hash)| (d, PeerHead { seq: seq as u64, hash })).collect();
    Ok((LocalState { own_seq, own_head_hash, peers }, hlc))
}

/// Own log position and clock. Called right after every push so a crash can
/// never reuse a sequence number.
pub async fn save_own_state(db: &Pool<Sqlite>, s: &LocalState, hlc: &Hlc) -> CmdResult<()> {
    set_setting(db, "sync_own_seq", &s.own_seq.to_string()).await?;
    set_setting(db, "sync_own_head", &s.own_head_hash).await?;
    set_setting(db, "sync_hlc", &hlc.encode()).await
}

/// Peer heads. Called only after their ops were fully applied.
pub async fn save_peer_heads(db: &Pool<Sqlite>, s: &LocalState) -> CmdResult<()> {
    for (device_id, head) in &s.peers {
        sqlx::query(
            "INSERT INTO sync_peers (device_id, seq, head_hash) VALUES (?, ?, ?)
             ON CONFLICT(device_id) DO UPDATE SET seq = excluded.seq, head_hash = excluded.head_hash",
        )
        .bind(device_id)
        .bind(head.seq as i64)
        .bind(&head.hash)
        .execute(db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}
