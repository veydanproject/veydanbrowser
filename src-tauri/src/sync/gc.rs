// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Blob garbage collection. A blob is live when the latest op of any entity on
//! any device references it. Everything else is a candidate; a candidate is
//! deleted only when it was already a candidate one grace period earlier, so a
//! blob uploaded just before its op is pushed is never lost.

use super::config::{get_setting, set_setting};
use super::{attachments, notes, profile_files};
use crate::error::{AppError, CmdResult};
use chrono::Utc;
use sqlx::{Pool, Sqlite};
use std::collections::{BTreeSet, HashMap};
use veydan_sync::{Engine, Op};

const GRACE_MS: i64 = 24 * 60 * 60 * 1000;
const INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

pub const LAST_RUN_KEY: &str = "sync_gc_last";
pub const BLOBS_TOTAL_KEY: &str = "sync_gc_blobs_total";
pub const REMOVED_KEY: &str = "sync_gc_removed";

#[derive(Debug, Default)]
pub struct Outcome {
    pub blobs_total: usize,
    pub removed: usize,
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Once a day.
pub async fn due(db: &Pool<Sqlite>) -> bool {
    let last: i64 = get_setting(db, LAST_RUN_KEY).await.and_then(|v| v.parse().ok()).unwrap_or(0);
    now_ms() - last >= INTERVAL_MS
}

/// Blobs an op keeps alive.
async fn blob_refs(engine: &Engine, op: &Op) -> CmdResult<Vec<String>> {
    if op.deleted {
        return Ok(Vec::new());
    }
    let field = |k: &str| op.payload.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(match op.entity_type.as_str() {
        // The note and its parents: the ancestor stays available for 3-way merges.
        notes::ENTITY => {
            let mut refs: Vec<String> = field("blob").into_iter().collect();
            if let Some(parents) = op.payload.get("parents").and_then(|v| v.as_array()) {
                refs.extend(parents.iter().filter_map(|p| p.as_str()).map(str::to_string));
            }
            refs
        }
        attachments::ENTITY => field("blob").into_iter().collect(),
        profile_files::SNAPSHOT_ENTITY => profile_files::blob_refs(engine, op).await?,
        _ => Vec::new(),
    })
}

/// One GC pass. Fails (and changes nothing) when some device's log is not fully readable.
pub async fn run(engine: &Engine, db: &Pool<Sqlite>) -> CmdResult<Outcome> {
    let ops = engine.all_latest_ops().await.map_err(AppError::other)?;
    let mut live: BTreeSet<String> = BTreeSet::new();
    for op in &ops {
        live.extend(blob_refs(engine, op).await?);
    }
    let all = engine.list_blobs().await.map_err(AppError::other)?;

    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT blob, first_seen FROM sync_gc_candidates").fetch_all(db).await.map_err(AppError::db)?;
    let mut candidates: HashMap<String, i64> = rows.into_iter().collect();
    let now = now_ms();
    let mut outcome = Outcome { blobs_total: all.len(), removed: 0 };

    for blob in &all {
        if live.contains(blob) {
            if candidates.remove(blob).is_some() {
                forget(db, blob).await?;
            }
            continue;
        }
        match candidates.remove(blob) {
            Some(first_seen) if now - first_seen >= GRACE_MS => {
                engine.delete_blob(blob).await.map_err(AppError::other)?;
                forget(db, blob).await?;
                outcome.removed += 1;
            }
            Some(_) => {}
            None => remember(db, blob, now).await?,
        }
    }
    // Whatever is left was deleted by another device.
    for blob in candidates.keys() {
        forget(db, blob).await?;
    }

    outcome.blobs_total -= outcome.removed;
    set_setting(db, LAST_RUN_KEY, &now.to_string()).await?;
    set_setting(db, BLOBS_TOTAL_KEY, &outcome.blobs_total.to_string()).await?;
    set_setting(db, REMOVED_KEY, &outcome.removed.to_string()).await?;
    Ok(outcome)
}

async fn remember(db: &Pool<Sqlite>, blob: &str, first_seen: i64) -> CmdResult<()> {
    sqlx::query("INSERT OR IGNORE INTO sync_gc_candidates (blob, first_seen) VALUES (?, ?)")
        .bind(blob)
        .bind(first_seen)
        .execute(db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

async fn forget(db: &Pool<Sqlite>, blob: &str) -> CmdResult<()> {
    sqlx::query("DELETE FROM sync_gc_candidates WHERE blob = ?").bind(blob).execute(db).await.map_err(AppError::db)?;
    Ok(())
}
