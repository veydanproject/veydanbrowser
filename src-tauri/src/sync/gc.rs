// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Blob garbage collection. A blob is live when the latest op of any entity on
//! any device references it. Everything else is a candidate; a candidate is
//! deleted only when it was already a candidate one grace period earlier, so a
//! blob uploaded just before its op is pushed is never lost.
//!
//! Large files v2 are swept by the same pass: the roots are the `lf_refs` of
//! the latest op of every entity type, known or not, so a client that does not
//! understand a future entity still keeps its files. Any unreadable op or
//! manifest aborts the whole pass (fail-closed). Ledger rows for v2 objects
//! store the full storage key, which tells them apart from v1 blob names.
//!
//! Runs on desktop only: mobile does not know profile-file snapshots and would
//! treat their blobs as garbage. Status keys are still read there.
#![cfg_attr(mobile, allow(dead_code, unused_imports))]

use super::config::{get_setting, set_setting};
#[cfg(desktop)]
use super::profile_files;
use super::{attachments, notes};
use crate::error::{AppError, CmdResult};
use chrono::Utc;
use sqlx::{Pool, Sqlite};
use std::collections::{BTreeSet, HashMap};
use veydan_sync::{refs_from_payload, Engine, LargeFileStore, Op};

const GRACE_MS: i64 = 24 * 60 * 60 * 1000;
const INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

pub const LAST_RUN_KEY: &str = "sync_gc_last";
pub const BLOBS_TOTAL_KEY: &str = "sync_gc_blobs_total";
pub const REMOVED_KEY: &str = "sync_gc_removed";
pub const LF_TOTAL_KEY: &str = "sync_gc_lf_total";
pub const LF_REMOVED_KEY: &str = "sync_gc_lf_removed";

#[derive(Debug, Default)]
pub struct Outcome {
    pub blobs_total: usize,
    pub removed: usize,
    /// v2 manifests + chunks after the pass.
    pub lf_total: usize,
    pub lf_removed: usize,
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Once a day.
pub async fn due(db: &Pool<Sqlite>) -> bool {
    let last: i64 = get_setting(db, LAST_RUN_KEY)
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    now_ms() - last >= INTERVAL_MS
}

/// Blobs an op keeps alive.
async fn blob_refs(engine: &Engine, op: &Op) -> CmdResult<Vec<String>> {
    if op.deleted {
        return Ok(Vec::new());
    }
    let field = |k: &str| {
        op.payload
            .get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    #[cfg(mobile)]
    let _ = engine;
    Ok(match op.entity_type.as_str() {
        // The note and its parents: the ancestor stays available for 3-way merges.
        notes::ENTITY => {
            let mut refs: Vec<String> = field("blob").into_iter().collect();
            if let Some(parents) = op.payload.get("parents").and_then(|v| v.as_array()) {
                refs.extend(
                    parents
                        .iter()
                        .filter_map(|p| p.as_str())
                        .map(str::to_string),
                );
            }
            refs
        }
        attachments::ENTITY => field("blob").into_iter().collect(),
        #[cfg(desktop)]
        profile_files::SNAPSHOT_ENTITY => profile_files::blob_refs(engine, op).await?,
        _ => Vec::new(),
    })
}

/// Manifest ids every non-deleted op keeps alive, regardless of entity type.
fn large_file_roots(ops: &[Op]) -> BTreeSet<String> {
    ops.iter()
        .filter(|op| !op.deleted)
        .flat_map(|op| refs_from_payload(&op.payload))
        .collect()
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
        sqlx::query_as("SELECT blob, first_seen FROM sync_gc_candidates")
            .fetch_all(db)
            .await
            .map_err(AppError::db)?;
    let (lf_candidates, candidates): (HashMap<String, i64>, HashMap<String, i64>) = rows
        .into_iter()
        .partition(|(k, _)| LargeFileStore::is_v2_key(k));
    let mut candidates = candidates;
    let now = now_ms();
    let mut outcome = Outcome {
        blobs_total: all.len(),
        ..Default::default()
    };

    // v2 first: it is fail-closed and must not run after v1 already deleted objects.
    let store = engine
        .large_files(Default::default())
        .map_err(AppError::other)?;
    let lf = store
        .gc_with_complete_root_set(&large_file_roots(&ops), &lf_candidates, now, GRACE_MS)
        .await
        .map_err(AppError::other)?;
    for (key, first_seen) in &lf.remembered {
        remember(db, key, *first_seen).await?;
    }
    for key in &lf.forgotten {
        forget(db, key).await?;
    }
    outcome.lf_removed = lf.removed.len();
    outcome.lf_total = lf.manifests_total + lf.chunks_total - lf.removed.len();

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
    set_setting(db, LF_TOTAL_KEY, &outcome.lf_total.to_string()).await?;
    set_setting(db, LF_REMOVED_KEY, &outcome.lf_removed.to_string()).await?;
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
    sqlx::query("DELETE FROM sync_gc_candidates WHERE blob = ?")
        .bind(blob)
        .execute(db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}
