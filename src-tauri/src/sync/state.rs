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
    pub conflict: bool,
}

type NoteStateRow = (String, String, String, String, String, i64, i64);

fn row_to_state(r: NoteStateRow) -> NoteSyncState {
    NoteSyncState {
        note_id: r.0,
        head_blob: r.1,
        head_parents: serde_json::from_str(&r.2).unwrap_or_default(),
        head_hlc: Hlc::decode(&r.3),
        synced_hash: r.4,
        deleted: r.5 != 0,
        conflict: r.6 != 0,
    }
}

pub async fn load_note_states(db: &Pool<Sqlite>) -> CmdResult<HashMap<String, NoteSyncState>> {
    let rows: Vec<NoteStateRow> =
        sqlx::query_as("SELECT note_id, head_blob, head_parents, head_hlc, synced_hash, deleted, conflict FROM sync_note_state")
            .fetch_all(db)
            .await
            .map_err(AppError::db)?;
    Ok(rows.into_iter().map(row_to_state).map(|s| (s.note_id.clone(), s)).collect())
}

pub async fn load_note_state(db: &Pool<Sqlite>, note_id: &str) -> CmdResult<Option<NoteSyncState>> {
    let row: Option<NoteStateRow> = sqlx::query_as(
        "SELECT note_id, head_blob, head_parents, head_hlc, synced_hash, deleted, conflict FROM sync_note_state WHERE note_id = ?",
    )
    .bind(note_id)
        .fetch_optional(db)
        .await
        .map_err(AppError::db)?;
    Ok(row.map(row_to_state))
}

pub async fn save_note_state(db: &Pool<Sqlite>, s: &NoteSyncState) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO sync_note_state (note_id, head_blob, head_parents, head_hlc, synced_hash, deleted, conflict)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(note_id) DO UPDATE SET
           head_blob = excluded.head_blob, head_parents = excluded.head_parents, head_hlc = excluded.head_hlc,
           synced_hash = excluded.synced_hash, deleted = excluded.deleted, conflict = excluded.conflict",
    )
    .bind(&s.note_id)
    .bind(&s.head_blob)
    .bind(serde_json::to_string(&s.head_parents).unwrap_or_else(|_| "[]".into()))
    .bind(s.head_hlc.as_ref().map(Hlc::encode).unwrap_or_default())
    .bind(&s.synced_hash)
    .bind(s.deleted as i64)
    .bind(s.conflict as i64)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

pub async fn clear_note_states(db: &Pool<Sqlite>) -> CmdResult<()> {
    sqlx::query("DELETE FROM sync_note_state").execute(db).await.map_err(AppError::db)?;
    sqlx::query("DELETE FROM sync_peers").execute(db).await.map_err(AppError::db)?;
    Ok(())
}

/// Notes currently carrying conflict markers: (id, title).
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
