// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Firefox profile directories as sync entities.
//!
//! `profile_lease` (id = profile id): `{ device_id, device_name, since }`; a
//! tombstone releases it. The device holding the lease may run the browser,
//! others see a badge. Two devices leasing at once: the lower HLC wins.
//!
//! `profile_snapshot` (id = profile id): `{ manifest_blob, taken_at, files, bytes }`.
//! The manifest blob lists `[{ path, blob, size, hash }]`; every file is its
//! own content-addressed blob, so unchanged files are never uploaded twice.
//! Binary state is not merged: snapshots are LWW, and when both sides changed
//! the files the profile is flagged `diverged` for the user to pick a side.

use super::fs_hash::HashCache;
use super::notes::write_raw;
use super::state::{
    delete_profile_files_state, load_profile_files_state, load_profile_files_states, save_profile_files_state,
    ProfileFilesState,
};
use crate::commands::profiles::is_blacklisted;
use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use veydan_sync::{sha256_hex, Engine, Hlc, HlcClock, Op};

pub const LEASE_ENTITY: &str = "profile_lease";
pub const SNAPSHOT_ENTITY: &str = "profile_snapshot";

#[derive(Debug, Default, Serialize, Deserialize)]
struct LeasePayload {
    #[serde(default)]
    device_id: String,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    since: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SnapshotPayload {
    #[serde(default)]
    manifest_blob: String,
    #[serde(default)]
    taken_at: String,
    #[serde(default)]
    files: u64,
    #[serde(default)]
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    /// Relative path inside `firefox-profile/`, `/`-separated.
    path: String,
    blob: String,
    size: u64,
    /// SHA-256 of the plaintext; compared against local files without downloading.
    hash: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    files: Vec<ManifestFile>,
}

impl Manifest {
    /// Fails on malformed JSON: an empty manifest would delete every profile file.
    fn parse(json: &str) -> CmdResult<Self> {
        serde_json::from_str(json).map_err(|e| AppError::other(format!("profile manifest: {e}")))
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn blob_by_hash(&self) -> HashMap<&str, &str> {
        self.files.iter().map(|f| (f.hash.as_str(), f.blob.as_str())).collect()
    }
}

// ── Local files ──────────────────────────────────────────────────────────────

/// `firefox-profile/` of a known profile.
async fn profile_dir(state: &AppState, profile_id: &str) -> CmdResult<Option<PathBuf>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT profile_path FROM profiles WHERE id = ?")
        .bind(profile_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(row.map(|(p,)| PathBuf::from(p).join("firefox-profile")))
}

/// Every syncable file below `dir` as (relative path, absolute path).
fn scan(dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if is_blacklisted(&name) || name.ends_with(".tmp") {
                continue;
            }
            let Ok(ty) = e.file_type() else { continue };
            let path = e.path();
            if ty.is_dir() {
                walk(root, &path, out);
            } else if ty.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                    out.push((rel, path));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn has_files(dir: &Path) -> bool {
    dir.is_dir() && !scan(dir).is_empty()
}

/// A manifest path may only descend: no absolute parts, no `..`, no blacklisted names.
fn valid_rel_path(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= 1024
        && rel.split('/').all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\\') && !is_blacklisted(c))
}

/// Timestamps come as RFC 3339 (ours) or SQLite `datetime('now')` (profiles table).
fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok().map(|n| n.and_utc()))
}

fn lease_op(profile_id: &str, hlc: Hlc, payload: Option<LeasePayload>) -> Op {
    Op {
        entity_type: LEASE_ENTITY.into(),
        entity_id: profile_id.into(),
        hlc,
        deleted: payload.is_none(),
        payload: serde_json::to_value(payload.unwrap_or_default()).unwrap_or_default(),
    }
}

fn snapshot_op(profile_id: &str, hlc: Hlc, payload: SnapshotPayload) -> Op {
    Op {
        entity_type: SNAPSHOT_ENTITY.into(),
        entity_id: profile_id.into(),
        hlc,
        deleted: false,
        payload: serde_json::to_value(payload).unwrap_or_default(),
    }
}

// ── Lease bookkeeping (called from launch / stop) ────────────────────────────

/// Another device's lease, if any: (device_id, device_name).
pub async fn foreign_lease(state: &AppState, profile_id: &str) -> CmdResult<Option<(String, String)>> {
    let device = super::config::device_id(&state.db).await?;
    Ok(load_profile_files_state(&state.db, profile_id)
        .await?
        .filter(|s| !s.lease_device.is_empty() && s.lease_device != device)
        .map(|s| (s.lease_device, s.lease_name)))
}

/// Take the lease locally; the next cycle publishes it.
pub async fn acquire_lease(state: &AppState, profile_id: &str) -> CmdResult<()> {
    let db = &state.db;
    let mut st = load_profile_files_state(db, profile_id).await?.unwrap_or_else(|| ProfileFilesState::new(profile_id));
    st.lease_device = super::config::device_id(db).await?;
    st.lease_name = super::config::device_name(db).await;
    st.lease_since = Utc::now().to_rfc3339();
    st.lease_synced = false;
    save_profile_files_state(db, &st).await
}

/// The browser exited: files changed, our lease is released.
pub async fn on_profile_stopped(state: &AppState, profile_id: &str) -> CmdResult<()> {
    let db = &state.db;
    let mut st = load_profile_files_state(db, profile_id).await?.unwrap_or_else(|| ProfileFilesState::new(profile_id));
    st.dirty = true;
    // A remote snapshot arrived while the browser ran: both sides changed now.
    if !st.pending_manifest.is_empty() {
        st.diverged = true;
    }
    if st.lease_device == super::config::device_id(db).await? {
        st.lease_device.clear();
        st.lease_name.clear();
        st.lease_since.clear();
        st.lease_synced = false;
    }
    save_profile_files_state(db, &st).await
}

/// A remote snapshot waits for this profile and can be applied now.
pub async fn has_pending(state: &AppState, profile_id: &str) -> CmdResult<bool> {
    Ok(load_profile_files_state(&state.db, profile_id)
        .await?
        .map(|s| !s.pending_manifest.is_empty() && !s.diverged)
        .unwrap_or(false))
}

// ── Push ─────────────────────────────────────────────────────────────────────

pub struct LocalChanges {
    pub ops: Vec<Op>,
    pub states: Vec<ProfileFilesState>,
}

/// Names uploaded in this cycle; skip a second PUT of the same content.
struct BlobIndex<'a> {
    engine: &'a Engine,
    uploaded: BTreeSet<String>,
}

impl<'a> BlobIndex<'a> {
    async fn exists(&self, name: &str) -> CmdResult<bool> {
        if self.uploaded.contains(name) {
            return Ok(true);
        }
        self.engine.blob_exists(name).await.map_err(AppError::other)
    }

    async fn put(&mut self, data: &[u8]) -> CmdResult<String> {
        let name = self.engine.blob_name(data);
        if self.uploaded.contains(&name) {
            return Ok(name);
        }
        self.engine.put_blob(data).await.map_err(AppError::other)?;
        self.uploaded.insert(name.clone());
        Ok(name)
    }
}

/// Manifest of the directory; only files whose hash is new are read and uploaded.
async fn build_manifest(
    cache: &HashCache,
    blobs: &mut BlobIndex<'_>,
    dir: &Path,
    prev: &Manifest,
    app: &AppHandle,
) -> CmdResult<Manifest> {
    let known = prev.blob_by_hash();
    let entries = scan(dir);
    let total = entries.len() as u32;
    let mut files = Vec::new();
    for (i, (rel, abs)) in entries.into_iter().enumerate() {
        let current = i as u32 + 1;
        super::emit_progress(app, "profiles_up", super::progress_pct(55, 75, current, total.max(1)), current, total, &rel);
        let Some(hash) = cache.file_hash(&abs) else { continue };
        let size = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
        // A name from the previous manifest is reused only while the blob is
        // still in the vault; GC may have dropped it since.
        let reusable = match known.get(hash.as_str()) {
            Some(b) if blobs.exists(b).await? => Some((*b).to_string()),
            _ => None,
        };
        let blob = match reusable {
            Some(b) => b,
            None => {
                let Ok(data) = std::fs::read(&abs) else { continue };
                blobs.put(&data).await?
            }
        };
        files.push(ManifestFile { path: rel, blob, size, hash });
    }
    Ok(Manifest { files })
}

/// Lease ops only: a tiny payload, never waits on profile files.
pub async fn collect_leases(state: &AppState, clock: &mut HlcClock) -> CmdResult<LocalChanges> {
    let db = &state.db;
    let device = super::config::device_id(db).await?;
    let mut out = LocalChanges { ops: Vec::new(), states: Vec::new() };
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT id FROM profiles").fetch_all(db).await.map_err(AppError::db)?;
    for (id,) in rows {
        let mut st = load_profile_files_state(db, &id).await?.unwrap_or_else(|| ProfileFilesState::new(&id));
        if st.lease_synced {
            continue;
        }
        let hlc = clock.now();
        let payload = (st.lease_device == device).then(|| LeasePayload {
            device_id: device.clone(),
            device_name: st.lease_name.clone(),
            since: st.lease_since.clone(),
        });
        out.ops.push(lease_op(&id, hlc.clone(), payload));
        st.lease_hlc = Some(hlc);
        st.lease_synced = true;
        out.states.push(st);
    }
    Ok(out)
}

/// Snapshots of profiles that ran since the last one. Leases are collected separately.
pub async fn collect_local_changes(
    engine: &Engine,
    state: &AppState,
    clock: &mut HlcClock,
    app: &AppHandle,
) -> CmdResult<LocalChanges> {
    let db = &state.db;
    let device = super::config::device_id(db).await?;
    let mut out = LocalChanges { ops: Vec::new(), states: Vec::new() };
    let mut states = load_profile_files_states(db).await?;
    let rows: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT id, profile_path, last_launch_at FROM profiles").fetch_all(db).await.map_err(AppError::db)?;
    let mut blobs = BlobIndex { engine, uploaded: BTreeSet::new() };

    for (id, profile_path, last_launch_at) in rows {
        let mut st = states.remove(&id).unwrap_or_else(|| ProfileFilesState::new(&id));

        let dir = PathBuf::from(&profile_path).join("firefox-profile");
        let launched_after = match (last_launch_at.as_deref().and_then(parse_time), parse_time(&st.snapshot_at)) {
            (Some(launch), Some(snap)) => launch > snap,
            (Some(_), None) => true,
            _ => false,
        };
        let candidate = st.dirty || launched_after || (st.head_hlc.is_none() && has_files(&dir));
        let running = state.browser.is_running(&id).await;
        let foreign_lease = !st.lease_device.is_empty() && st.lease_device != device;
        if !candidate || running || st.diverged || foreign_lease {
            // Someone else is running it: our local changes cannot win, let the user decide.
            let now_diverged = foreign_lease && candidate && !running && st.dirty && !st.diverged;
            if now_diverged {
                st.diverged = true;
                save_profile_files_state(db, &st).await?;
            }
            continue;
        }

        // Local bookkeeping only; empty before the first snapshot.
        let prev = Manifest::parse(&st.manifest_json).unwrap_or_default();
        let manifest = build_manifest(&state.sync.file_hashes, &mut blobs, &dir, &prev, app).await?;
        let json = manifest.to_json();
        let hash = sha256_hex(json.as_bytes());
        let now = Utc::now().to_rfc3339();
        if hash == st.synced_hash {
            st.dirty = false;
            st.snapshot_at = now;
            save_profile_files_state(db, &st).await?;
            continue;
        }
        let manifest_blob = blobs.put(json.as_bytes()).await?;
        let hlc = clock.now();
        out.ops.push(snapshot_op(
            &id,
            hlc.clone(),
            SnapshotPayload {
                manifest_blob,
                taken_at: now.clone(),
                files: manifest.files.len() as u64,
                bytes: manifest.files.iter().map(|f| f.size).sum(),
            },
        ));
        st.head_hlc = Some(hlc);
        st.synced_hash = hash;
        st.manifest_json = json;
        st.snapshot_at = now;
        st.dirty = false;
        st.pending_manifest.clear();
        out.states.push(st);
    }
    // Profiles deleted locally leave no state behind.
    for id in states.keys() {
        delete_profile_files_state(db, id).await?;
    }
    Ok(out)
}

// ── Pull ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ApplyOutcome {
    /// Set when an op was skipped on a transient condition; peer heads must not advance.
    pub retry: Option<String>,
}

/// Bring `dir` to the manifest: download changed files, drop files it does not list.
/// Returns the reason when a blob is not available yet.
async fn apply_manifest(
    engine: &Engine,
    cache: &HashCache,
    dir: &Path,
    manifest: &Manifest,
    app: &AppHandle,
) -> CmdResult<Option<String>> {
    std::fs::create_dir_all(dir).map_err(AppError::io)?;
    let mut keep: HashSet<&str> = HashSet::new();
    let listed: Vec<&ManifestFile> = manifest.files.iter().filter(|f| valid_rel_path(&f.path)).collect();
    let total = listed.len() as u32;
    for (i, f) in listed.into_iter().enumerate() {
        let current = i as u32 + 1;
        super::emit_progress(
            app,
            "profiles_down",
            super::progress_pct(78, 93, current, total.max(1)),
            current,
            total,
            &f.path,
        );
        keep.insert(f.path.as_str());
        let dest = dir.join(&f.path);
        if cache.file_hash(&dest).as_deref() == Some(f.hash.as_str()) {
            continue;
        }
        let Some(data) = engine.get_blob(&f.blob).await.map_err(AppError::other)? else {
            return Ok(Some(format!("blob for profile file {} not available yet", f.path)));
        };
        write_raw(&dest, &data)?;
    }
    for (rel, abs) in scan(dir) {
        if !keep.contains(rel.as_str()) {
            let _ = std::fs::remove_file(abs);
        }
    }
    Ok(None)
}

/// Download and apply a manifest blob; `Ok(None)` means it is not available yet.
async fn fetch_and_apply(
    engine: &Engine,
    state: &AppState,
    app: &AppHandle,
    dir: &Path,
    manifest_blob: &str,
) -> CmdResult<Option<Manifest>> {
    let Some(raw) = engine.get_blob(manifest_blob).await.map_err(AppError::other)? else { return Ok(None) };
    let manifest = Manifest::parse(&String::from_utf8_lossy(&raw))?;
    match apply_manifest(engine, &state.sync.file_hashes, dir, &manifest, app).await? {
        None => Ok(Some(manifest)),
        Some(_) => Ok(None),
    }
}

fn record_applied(st: &mut ProfileFilesState, manifest: &Manifest) {
    let json = manifest.to_json();
    st.synced_hash = sha256_hex(json.as_bytes());
    st.manifest_json = json;
    st.snapshot_at = Utc::now().to_rfc3339();
    st.dirty = false;
    st.pending_manifest.clear();
    st.diverged = false;
}

/// Apply remote lease ops. Independent of profile file snapshots.
pub async fn apply_leases(app: &AppHandle, ops: &[Op]) -> CmdResult<ApplyOutcome> {
    let state = app.state::<AppState>();
    let db = &state.db;
    let device = super::config::device_id(db).await?;
    let outcome = ApplyOutcome::default();

    for op in ops {
        if op.entity_type != LEASE_ENTITY {
            continue;
        }
        let id = op.entity_id.as_str();
        let mut st = load_profile_files_state(db, id).await?.unwrap_or_else(|| ProfileFilesState::new(id));
        let stale = st.lease_hlc.as_ref().map(|h| *h >= op.hlc).unwrap_or(false);
        if op.deleted {
            if stale {
                continue;
            }
            if st.lease_device == op.hlc.device_id {
                st.lease_device.clear();
                st.lease_name.clear();
                st.lease_since.clear();
            }
        } else {
            if st.lease_device == device {
                // Concurrent launches: the earlier one (lower HLC) keeps the profile.
                if st.lease_hlc.as_ref().map(|h| *h < op.hlc).unwrap_or(false) {
                    continue;
                }
            } else if stale {
                continue;
            }
            let p: LeasePayload = serde_json::from_value(op.payload.clone()).unwrap_or_default();
            st.lease_device = p.device_id;
            st.lease_name = p.device_name;
            st.lease_since = p.since;
            st.lease_synced = true;
        }
        st.lease_hlc = Some(op.hlc.clone());
        save_profile_files_state(db, &st).await?;
    }
    Ok(outcome)
}

/// Apply remote snapshot ops. Ops are already HLC-sorted.
pub async fn apply_remote(engine: &Engine, app: &AppHandle, ops: &[Op]) -> CmdResult<ApplyOutcome> {
    let state = app.state::<AppState>();
    let db = &state.db;
    let mut outcome = ApplyOutcome::default();

    for op in ops {
        let id = op.entity_id.as_str();
        match op.entity_type.as_str() {
            SNAPSHOT_ENTITY => {
                if op.deleted {
                    continue;
                }
                let mut st = load_profile_files_state(db, id).await?.unwrap_or_else(|| ProfileFilesState::new(id));
                if st.head_hlc.as_ref().map(|h| *h >= op.hlc).unwrap_or(false) {
                    continue;
                }
                let p: SnapshotPayload = serde_json::from_value(op.payload.clone()).unwrap_or_default();
                let Some(dir) = profile_dir(&state, id).await? else {
                    // Profile row not here yet; the retry batch brings both.
                    outcome.retry = Some(format!("profile {id} not known yet"));
                    continue;
                };
                let running = state.browser.is_running(id).await;
                let both_changed = st.dirty || (st.head_hlc.is_none() && has_files(&dir));
                if running || both_changed {
                    st.pending_manifest = p.manifest_blob;
                    st.diverged = both_changed;
                    st.head_hlc = Some(op.hlc.clone());
                    save_profile_files_state(db, &st).await?;
                    continue;
                }
                match fetch_and_apply(engine, &state, app, &dir, &p.manifest_blob).await? {
                    Some(manifest) => {
                        record_applied(&mut st, &manifest);
                        st.head_hlc = Some(op.hlc.clone());
                        save_profile_files_state(db, &st).await?;
                    }
                    None => outcome.retry = Some(format!("profile files for {id} not available yet")),
                }
            }
            _ => {}
        }
    }
    Ok(outcome)
}

// ── Explicit actions (launch, conflict UI) ───────────────────────────────────

/// Apply the snapshot that waited while the browser ran. No-op without one.
pub async fn apply_pending(engine: &Engine, state: &AppState, app: &AppHandle, profile_id: &str) -> CmdResult<()> {
    let db = &state.db;
    let Some(mut st) = load_profile_files_state(db, profile_id).await? else { return Ok(()) };
    if st.pending_manifest.is_empty() {
        return Ok(());
    }
    let dir = profile_dir(state, profile_id).await?.ok_or_else(|| AppError::not_found("Profile not found"))?;
    match fetch_and_apply(engine, state, app, &dir, &st.pending_manifest).await? {
        Some(manifest) => {
            record_applied(&mut st, &manifest);
            save_profile_files_state(db, &st).await
        }
        None => Err(AppError::other("profile files are not fully available in the vault yet")),
    }
}

/// Conflict choice: replace local files with the remote snapshot.
pub async fn take_remote(engine: &Engine, state: &AppState, app: &AppHandle, profile_id: &str) -> CmdResult<()> {
    if state.browser.is_running(profile_id).await {
        return Err(AppError::other("Stop the profile first"));
    }
    apply_pending(engine, state, app, profile_id).await
}

/// Conflict choice: keep local files and publish them as the new snapshot.
pub async fn push_mine(state: &AppState, profile_id: &str) -> CmdResult<()> {
    let db = &state.db;
    let Some(mut st) = load_profile_files_state(db, profile_id).await? else { return Ok(()) };
    st.diverged = false;
    st.dirty = true;
    st.pending_manifest.clear();
    save_profile_files_state(db, &st).await
}

/// Blobs a snapshot op keeps alive: the manifest and every file in it.
pub async fn blob_refs(engine: &Engine, op: &Op) -> CmdResult<Vec<String>> {
    if op.entity_type != SNAPSHOT_ENTITY || op.deleted {
        return Ok(Vec::new());
    }
    let p: SnapshotPayload = serde_json::from_value(op.payload.clone()).unwrap_or_default();
    if p.manifest_blob.is_empty() {
        return Ok(Vec::new());
    }
    let raw = engine
        .get_blob(&p.manifest_blob)
        .await
        .map_err(AppError::other)?
        .ok_or_else(|| AppError::other(format!("manifest {} missing", p.manifest_blob)))?;
    let mut refs = vec![p.manifest_blob];
    refs.extend(Manifest::parse(&String::from_utf8_lossy(&raw))?.files.into_iter().map(|f| f.blob));
    Ok(refs)
}
