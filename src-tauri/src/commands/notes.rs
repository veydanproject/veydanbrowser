// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::Emitter;
use uuid::Uuid;

// zstd compression helpers for history content
fn compress_content(content: &str) -> Result<Vec<u8>, AppError> {
    zstd::encode_all(content.as_bytes(), 3).map_err(AppError::other)
}

fn decompress_content(blob: &[u8]) -> Result<String, AppError> {
    let bytes = zstd::decode_all(blob).map_err(AppError::other)?;
    String::from_utf8(bytes).map_err(AppError::other)
}

// ── Models ────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
struct NoteRow {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub format: String,
    // Legacy columns kept for SELECT * compatibility; bindings is the new source of truth
    pub scope: String,
    pub workspace_id: Option<String>,
    pub profile_id: Option<String>,
    pub pinned: i64,
    pub archived: i64,
    pub deleted: i64,
    pub doc_status: String,
    pub version_base: Option<String>,
    pub fts_rowid: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub file_mtime: Option<String>,
    pub content_hash: Option<String>,
    pub preview: String,
    /// JSON array of binding strings, e.g. ["workspace:id", "profile:id"]
    pub bindings: String,
    pub folder_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub format: String,
    pub bindings: Vec<String>,
    pub tags: Vec<NoteTagInfo>,
    pub pinned: bool,
    pub archived: bool,
    pub doc_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub content_hash: Option<String>,
    pub content: Option<String>,
    pub has_draft: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct NoteListItem {
    pub id: String,
    pub title: String,
    pub format: String,
    pub bindings: Vec<String>,
    pub tags: Vec<NoteTagInfo>,
    pub folder_ids: Vec<String>,
    pub pinned: bool,
    pub archived: bool,
    pub doc_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub has_draft: bool,
    pub preview: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Serialize, Clone, FromRow)]
pub struct NoteFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub color: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct NoteTagInfo {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Clone, FromRow)]
pub struct NoteTag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct NoteCreateInput {
    pub title: String,
    pub format: Option<String>,
    /// Context bindings, e.g. ["workspace:id", "profile:id"]
    pub bindings: Option<Vec<String>>,
    pub tag_names: Option<Vec<String>>,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NoteUpdateInput {
    pub title: Option<String>,
    pub content: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct NoteFilter {
    /// Filter notes that contain this binding, e.g. "workspace:id" or "profile:id"
    pub binding: Option<String>,
    pub tag_name: Option<String>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub include_deleted: Option<bool>,
}

// ── File helpers ──────────────────────────────────────────────────────────────

fn notes_dir(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("notes")
}

fn documents_dir(app_data_dir: &PathBuf) -> PathBuf {
    notes_dir(app_data_dir).join("documents")
}

fn drafts_dir(app_data_dir: &PathBuf) -> PathBuf {
    notes_dir(app_data_dir).join("drafts")
}

fn draft_file_path(app_data_dir: &PathBuf, id: &str) -> PathBuf {
    drafts_dir(app_data_dir).join(format!("{}.draft", id))
}

fn manifest_path(app_data_dir: &PathBuf) -> PathBuf {
    notes_dir(app_data_dir).join("notes_manifest.json")
}

/// Returns the effective documents directory: custom if set, otherwise default.
fn effective_docs_dir(app_data_dir: &PathBuf, custom_dir: Option<&PathBuf>) -> PathBuf {
    custom_dir.cloned().unwrap_or_else(|| documents_dir(app_data_dir))
}

/// Resolves a stored file_path to an absolute path.
/// New notes with custom dir store absolute paths; legacy notes store paths relative to app_data_dir.
fn resolve_note_abs_path(app_data_dir: &PathBuf, file_path: &str) -> PathBuf {
    let p = PathBuf::from(file_path);
    if p.is_absolute() { p } else { app_data_dir.join(file_path) }
}

fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    // sha2 0.11 returns a hybrid_array `Array` which no longer implements
    // LowerHex; encode to a lowercase hex string manually.
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Extract plain-text preview from note content (strips frontmatter + markdown symbols)
fn make_preview(content: &str) -> String {
    // Skip YAML frontmatter block (--- ... ---)
    let body = if content.starts_with("---") {
        if let Some(end) = content[3..].find("\n---") {
            content[3 + end + 4..].trim_start()
        } else {
            content
        }
    } else {
        content
    };
    // Strip common markdown: headers, bold/italic markers, links, code fences
    let mut result = String::new();
    for line in body.lines() {
        let stripped = line
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim_start_matches('-')
            .trim_start_matches('*')
            .replace("**", "")
            .replace('`', "")
            .replace("__", "");
        let stripped = stripped.trim();
        if !stripped.is_empty() {
            if !result.is_empty() { result.push(' '); }
            result.push_str(stripped);
            if result.chars().count() >= 120 { break; }
        }
    }
    result.chars().take(120).collect()
}

/// Write note file with frontmatter — atomic (tmp → fsync → rename)
fn write_note_file(
    path: &PathBuf,
    row: &NoteRow,
    tags: &[String],
    content: &str,
) -> Result<(), AppError> {
    let bindings_vec: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();
    let bindings_json = serde_json::to_string(&bindings_vec).unwrap_or_else(|_| "[]".to_string());

    let tags_yaml = if tags.is_empty() {
        "  []\n".to_string()
    } else {
        tags.iter().map(|t| format!("  - {}\n", t)).collect()
    };

    let frontmatter = format!(
        "---\nid: {}\ntitle: {}\nformat: {}\nbindings: {}\ntags:\n{}created_at: {}\nupdated_at: {}\n---\n",
        row.id, row.title, row.format, bindings_json,
        tags_yaml, row.created_at, row.updated_at
    );

    let full = format!("{}{}", frontmatter, content);
    atomic_write(path, &full)
}

/// Atomic write: write to .tmp, fsync, rename
fn atomic_write(path: &PathBuf, content: &str) -> Result<(), AppError> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp).map_err(AppError::io)?;
    f.write_all(content.as_bytes()).map_err(AppError::io)?;
    f.sync_all().map_err(AppError::io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(AppError::io)?;
    Ok(())
}

/// Parse frontmatter + body from file content.
/// Returns (kv map, tags list, body).
fn parse_note_file(raw: &str) -> (HashMap<String, String>, Vec<String>, String) {
    if !raw.starts_with("---\n") {
        return (HashMap::new(), vec![], raw.to_string());
    }
    let rest = &raw[4..];
    let Some(end_idx) = rest.find("\n---\n") else {
        return (HashMap::new(), vec![], raw.to_string());
    };
    let fm_str = &rest[..end_idx];
    let raw_body = &rest[end_idx + 5..];
    // Strip exactly one leading newline that separates frontmatter from content
    let body = if raw_body.starts_with('\n') {
        raw_body[1..].to_string()
    } else {
        raw_body.to_string()
    };

    let mut kv: HashMap<String, String> = HashMap::new();
    let mut tags: Vec<String> = Vec::new();
    let mut in_tags = false;

    for line in fm_str.lines() {
        if in_tags {
            if line.starts_with("  - ") {
                tags.push(line[4..].trim().to_string());
                continue;
            } else if line.trim() == "[]" {
                in_tags = false;
                continue;
            } else {
                in_tags = false;
            }
        }
        if let Some((key, val)) = line.split_once(": ") {
            if key == "tags" {
                in_tags = true;
            } else {
                kv.insert(key.trim().to_string(), val.trim().to_string());
            }
        }
    }

    (kv, tags, body)
}

/// Read note file: returns (kv, tags, body content)
fn read_note_file(path: &PathBuf) -> Result<(HashMap<String, String>, Vec<String>, String), AppError> {
    let raw = std::fs::read_to_string(path).map_err(AppError::io)?;
    Ok(parse_note_file(&raw))
}

// ── Tag helpers ───────────────────────────────────────────────────────────────

async fn fetch_note_tags(
    note_id: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Vec<NoteTagInfo>, AppError> {
    sqlx::query_as::<_, NoteTagInfo>(
        "SELECT nt.id, nt.name, nt.color FROM note_tags nt
         JOIN note_tag_links ntl ON nt.id = ntl.tag_id
         WHERE ntl.note_id = ?
         ORDER BY nt.name",
    )
    .bind(note_id)
    .fetch_all(db)
    .await
    .map_err(AppError::db)
}

async fn fetch_all_note_tags_map(
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<HashMap<String, Vec<NoteTagInfo>>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT ntl.note_id, nt.id, nt.name, nt.color
         FROM note_tag_links ntl JOIN note_tags nt ON ntl.tag_id = nt.id",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;

    let mut map: HashMap<String, Vec<NoteTagInfo>> = HashMap::new();
    for (note_id, tag_id, tag_name, tag_color) in rows {
        map.entry(note_id).or_default().push(NoteTagInfo {
            id: tag_id,
            name: tag_name,
            color: tag_color,
        });
    }
    Ok(map)
}

async fn fetch_all_note_folder_ids_map(
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<HashMap<String, Vec<String>>, AppError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT note_id, folder_id FROM note_folder_links",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (note_id, folder_id) in rows {
        map.entry(note_id).or_default().push(folder_id);
    }
    Ok(map)
}

/// Find or create a tag by name, return its id.
async fn upsert_tag(
    name: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM note_tags WHERE name = ?")
            .bind(name)
            .fetch_optional(db)
            .await
            .map_err(AppError::db)?;

    if let Some((id,)) = existing {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO note_tags (id, name, color, created_at, updated_at) VALUES (?, ?, '#6366f1', ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await
    .map_err(AppError::db)?;

    Ok(id)
}

async fn set_note_tag_links(
    note_id: &str,
    tag_names: &[String],
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM note_tag_links WHERE note_id = ?")
        .bind(note_id)
        .execute(db)
        .await
        .map_err(AppError::db)?;

    for name in tag_names {
        let tag_id = upsert_tag(name, db).await?;
        sqlx::query("INSERT OR IGNORE INTO note_tag_links (note_id, tag_id) VALUES (?, ?)")
            .bind(note_id)
            .bind(&tag_id)
            .execute(db)
            .await
            .map_err(AppError::db)?;
    }
    Ok(())
}

// ── FTS helpers ───────────────────────────────────────────────────────────────

async fn fts_upsert(
    note_id: &str,
    title: &str,
    content: &str,
    tags: &[String],
    old_fts_rowid: Option<i64>,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<i64, AppError> {
    let mut conn = db.acquire().await.map_err(AppError::db)?;

    if let Some(rowid) = old_fts_rowid {
        sqlx::query("DELETE FROM notes_fts WHERE rowid = ?")
            .bind(rowid)
            .execute(&mut *conn)
            .await
            .map_err(AppError::db)?;
    }
    sqlx::query(
        "INSERT INTO notes_fts(note_id, title, content, tags) VALUES (?, ?, ?, ?)",
    )
    .bind(note_id)
    .bind(title)
    .bind(content)
    .bind(&tags.join(" "))
    .execute(&mut *conn)
    .await
    .map_err(AppError::db)?;

    let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
        .fetch_one(&mut *conn)
        .await
        .map_err(AppError::db)?;

    Ok(rowid)
}

async fn fts_delete(fts_rowid: Option<i64>, db: &sqlx::Pool<sqlx::Sqlite>) -> Result<(), AppError> {
    if let Some(rowid) = fts_rowid {
        sqlx::query("DELETE FROM notes_fts WHERE rowid = ?")
            .bind(rowid)
            .execute(db)
            .await
            .map_err(AppError::db)?;
    }
    Ok(())
}

// ── Manifest ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct ManifestEntry {
    id: String,
    title: String,
    file_path: String,
    format: String,
    bindings: Vec<String>,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
    content_hash: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    updated_at: String,
    notes: Vec<ManifestEntry>,
}

async fn rebuild_manifest(
    db: &sqlx::Pool<sqlx::Sqlite>,
    app_data_dir: &PathBuf,
) -> Result<(), AppError> {
    let rows = sqlx::query_as::<_, NoteRow>(
        "SELECT * FROM notes WHERE deleted = 0 ORDER BY updated_at DESC",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;

    let tags_map = fetch_all_note_tags_map(db).await?;

    let entries: Vec<ManifestEntry> = rows
        .into_iter()
        .map(|r| {
            let tags = tags_map
                .get(&r.id)
                .map(|v| v.iter().map(|t| t.name.clone()).collect())
                .unwrap_or_default();
            let bindings: Vec<String> = serde_json::from_str(&r.bindings).unwrap_or_default();
            ManifestEntry {
                id: r.id,
                title: r.title,
                file_path: r.file_path,
                format: r.format,
                bindings,
                tags,
                created_at: r.created_at,
                updated_at: r.updated_at,
                content_hash: r.content_hash,
            }
        })
        .collect();

    let manifest = Manifest {
        version: 1,
        updated_at: Utc::now().to_rfc3339(),
        notes: entries,
    };

    let json = serde_json::to_string_pretty(&manifest).map_err(AppError::other)?;
    atomic_write(&manifest_path(app_data_dir), &json)?;
    Ok(())
}

// ── Sync ──────────────────────────────────────────────────────────────────────

pub async fn sync_notes_index(
    db: &sqlx::Pool<sqlx::Sqlite>,
    app_data_dir: &PathBuf,
    custom_docs_dir: Option<&PathBuf>,
) -> Result<(), AppError> {
    let docs_dir = effective_docs_dir(app_data_dir, custom_docs_dir);
    if !docs_dir.exists() {
        return Ok(());
    }

    let existing: Vec<NoteRow> = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes")
        .fetch_all(db)
        .await
        .map_err(AppError::db)?;

    let mut indexed: HashMap<String, NoteRow> = existing.into_iter().map(|r| (r.id.clone(), r)).collect();

    let now = Utc::now().to_rfc3339();

    // Scan files
    let entries = std::fs::read_dir(&docs_dir).map_err(AppError::io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
        if ext == "tmp" { continue; }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let note_id = stem.to_string();

        let meta = std::fs::metadata(&path).ok();
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<Utc>::from(t).to_rfc3339());

        if let Some(row) = indexed.remove(&note_id) {
            // File exists, check if changed
            let file_changed = mtime.as_deref() != row.file_mtime.as_deref();
            if file_changed {
                if let Ok((kv, _, _)) = read_note_file(&path) {
                    let title = kv.get("title").cloned().unwrap_or_else(|| row.title.clone());
                    let updated_at = kv.get("updated_at").cloned().unwrap_or_else(|| now.clone());
                    let content_str = std::fs::read_to_string(&path).ok();
                    let content_hash = content_str.as_deref().map(compute_hash);
                    let preview = content_str.as_deref().map(make_preview).unwrap_or_default();

                    sqlx::query(
                        "UPDATE notes SET title=?, updated_at=?, file_mtime=?, content_hash=?, preview=?, doc_status='active' WHERE id=?",
                    )
                    .bind(&title)
                    .bind(&updated_at)
                    .bind(&mtime)
                    .bind(&content_hash)
                    .bind(&preview)
                    .bind(&note_id)
                    .execute(db)
                    .await
                    .map_err(AppError::db)?;
                }
            }
        } else {
            // New file not in DB — read frontmatter and insert
            let Ok((kv, tags, body)) = read_note_file(&path) else { continue };
            let title = kv.get("title").cloned().unwrap_or_else(|| note_id.clone());
            let format = ext.to_string();
            let created_at = kv.get("created_at").cloned().unwrap_or_else(|| now.clone());
            let updated_at = kv.get("updated_at").cloned().unwrap_or_else(|| now.clone());
            let file_path = path.to_string_lossy().to_string();
            let content_hash = compute_hash(&format!(
                "---\n{:?}\n---\n{}",
                kv, body
            ));
            let preview = make_preview(&body);

            // Read bindings from frontmatter; fall back to old scope/workspace_id/profile_id fields
            let bindings_json = if let Some(b) = kv.get("bindings") {
                b.clone()
            } else {
                let scope = kv.get("scope").map(|s| s.as_str()).unwrap_or("global");
                let workspace_id = kv.get("workspace_id").filter(|v| *v != "null").cloned();
                let profile_id = kv.get("profile_id").filter(|v| *v != "null").cloned();
                let mut b: Vec<String> = Vec::new();
                if let Some(ws) = workspace_id { b.push(format!("workspace:{}", ws)); }
                if let Some(pr) = profile_id { b.push(format!("profile:{}", pr)); }
                let _ = scope;
                serde_json::to_string(&b).unwrap_or_else(|_| "[]".to_string())
            };

            sqlx::query(
                "INSERT OR IGNORE INTO notes
                 (id, title, file_path, format, bindings, created_at, updated_at, file_mtime, content_hash, preview, doc_status)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active')",
            )
            .bind(&note_id)
            .bind(&title)
            .bind(&file_path)
            .bind(&format)
            .bind(&bindings_json)
            .bind(&created_at)
            .bind(&updated_at)
            .bind(&mtime)
            .bind(&content_hash)
            .bind(&preview)
            .execute(db)
            .await
            .map_err(AppError::db)?;

            // Insert tags
            set_note_tag_links(&note_id, &tags, db).await?;
        }
    }

    // Remaining in indexed = files missing from disk
    for (id, _) in &indexed {
        sqlx::query("UPDATE notes SET doc_status='missing' WHERE id=?")
            .bind(id)
            .execute(db)
            .await
            .map_err(AppError::db)?;
    }

    // Rebuild FTS
    sqlx::query("DELETE FROM notes_fts")
        .execute(db)
        .await
        .map_err(AppError::db)?;

    let all_notes = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE deleted = 0")
        .fetch_all(db)
        .await
        .map_err(AppError::db)?;

    for row in &all_notes {
        let file_path = resolve_note_abs_path(app_data_dir, &row.file_path);
        let (_, tags_list, body) = read_note_file(&file_path).unwrap_or_default();
        let tags_str = tags_list.join(" ");

        let mut conn = db.acquire().await.map_err(AppError::db)?;
        sqlx::query(
            "INSERT INTO notes_fts(note_id, title, content, tags) VALUES (?, ?, ?, ?)",
        )
        .bind(&row.id)
        .bind(&row.title)
        .bind(&body)
        .bind(&tags_str)
        .execute(&mut *conn)
        .await
        .map_err(AppError::db)?;

        let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
            .fetch_one(&mut *conn)
            .await
            .map_err(AppError::db)?;

        sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
            .bind(rowid)
            .bind(&row.id)
            .execute(db)
            .await
            .map_err(AppError::db)?;
    }

    rebuild_manifest(db, app_data_dir).await?;
    Ok(())
}

// ── File watcher ──────────────────────────────────────────────────────────────

pub fn start_notes_watcher(app_handle: tauri::AppHandle, app_data_dir: PathBuf, custom_dir: Option<PathBuf>) {
    use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
    let docs_dir = effective_docs_dir(&app_data_dir, custom_dir.as_ref());

    let Ok(mut watcher) = RecommendedWatcher::new(tx, Config::default()) else {
        return;
    };

    if docs_dir.exists() {
        let _ = watcher.watch(&docs_dir, RecursiveMode::NonRecursive);
    }

    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive
        for res in rx {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    for path in &event.paths {
                        // Skip tmp files generated by our own atomic write
                        if path.extension().and_then(|e| e.to_str()) == Some("tmp") {
                            continue;
                        }
                        if let Some(stem) = path.file_stem() {
                            let note_id = stem.to_string_lossy().to_string();
                            let _ = app_handle.emit("notes://external-change", &note_id);
                        }
                    }
                }
            }
        }
    });
}

// ── Helpers for row → public structs ─────────────────────────────────────────

fn row_to_list_item(row: NoteRow, tags: Vec<NoteTagInfo>, folder_ids: Vec<String>, has_draft: bool) -> NoteListItem {
    let bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();
    NoteListItem {
        id: row.id,
        title: row.title,
        format: row.format,
        bindings,
        tags,
        folder_ids,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        doc_status: row.doc_status,
        created_at: row.created_at,
        updated_at: row.updated_at,
        has_draft,
        preview: row.preview,
        snippet: None,
    }
}

fn open_path(path: &std::path::Path) -> Result<(), AppError> {
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(AppError::io)?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map_err(AppError::io)?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map_err(AppError::io)?;
    Ok(())
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn note_list(
    filter: NoteFilter,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteListItem>> {
    let include_deleted = filter.include_deleted.unwrap_or(false);
    let include_archived = filter.archived.unwrap_or(false);

    let rows = sqlx::query_as::<_, NoteRow>(
        "SELECT * FROM notes ORDER BY pinned DESC, updated_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;

    let tags_map = fetch_all_note_tags_map(&state.db).await?;
    let folder_ids_map = fetch_all_note_folder_ids_map(&state.db).await?;
    let drafts_dir = drafts_dir(&state.app_data_dir);

    let items: Vec<NoteListItem> = rows
        .into_iter()
        .filter(|r| {
            if !include_deleted && r.deleted != 0 { return false; }
            if !include_archived && r.archived != 0 { return false; }
            if let Some(ref binding) = filter.binding {
                let bindings: Vec<String> = serde_json::from_str(&r.bindings).unwrap_or_default();
                if !bindings.contains(binding) { return false; }
            }
            if let Some(pinned) = filter.pinned {
                if (r.pinned != 0) != pinned { return false; }
            }
            true
        })
        .filter(|r| {
            if let Some(ref tag_name) = filter.tag_name {
                let tags = tags_map.get(&r.id);
                return tags.map(|v| v.iter().any(|t| &t.name == tag_name)).unwrap_or(false);
            }
            true
        })
        .map(|r| {
            let tags = tags_map.get(&r.id).cloned().unwrap_or_default();
            let folder_ids = folder_ids_map.get(&r.id).cloned().unwrap_or_default();
            let has_draft = drafts_dir.join(format!("{}.draft", r.id)).exists();
            row_to_list_item(r, tags, folder_ids, has_draft)
        })
        .collect();

    Ok(items)
}

#[tauri::command]
pub async fn note_get(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Note> {
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let tags = fetch_note_tags(&id, &state.db).await?;
    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);

    let content = if file_path.exists() {
        let (_, _, body) = read_note_file(&file_path)?;
        Some(body)
    } else {
        None
    };

    let has_draft = draft_file_path(&state.app_data_dir, &id).exists();

    let bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();

    Ok(Note {
        id: row.id,
        title: row.title,
        file_path: row.file_path,
        format: row.format,
        bindings,
        tags,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        doc_status: row.doc_status,
        created_at: row.created_at,
        updated_at: row.updated_at,
        content_hash: row.content_hash,
        content,
        has_draft,
    })
}

#[tauri::command]
pub async fn note_create(
    input: NoteCreateInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Note> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    let format = input.format.as_deref().unwrap_or("md").to_string();
    let content = input.content.as_deref().unwrap_or("").to_string();
    let bindings_vec = input.bindings.clone().unwrap_or_default();
    let bindings_json = serde_json::to_string(&bindings_vec).map_err(AppError::other)?;

    let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    let docs_dir = effective_docs_dir(&state.app_data_dir, custom_dir.as_ref());
    let abs_path = docs_dir.join(format!("{}.txt", id));
    let stored_path = abs_path.to_string_lossy().to_string();

    let row = NoteRow {
        id: id.clone(),
        title: input.title.clone(),
        file_path: stored_path.clone(),
        format: format.clone(),
        scope: "global".to_string(),
        workspace_id: None,
        profile_id: None,
        pinned: 0,
        archived: 0,
        deleted: 0,
        doc_status: "active".to_string(),
        version_base: None,
        fts_rowid: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        file_mtime: None,
        content_hash: None,
        preview: String::new(),
        bindings: bindings_json.clone(),
        folder_id: None,
    };

    let tag_names = input.tag_names.clone().unwrap_or_default();
    write_note_file(&abs_path, &row, &tag_names, &content)?;

    let content_hash = compute_hash(&content);
    let preview = make_preview(&content);

    sqlx::query(
        "INSERT INTO notes (id, title, file_path, format, bindings, pinned, archived, deleted, doc_status, created_at, updated_at, content_hash, preview)
         VALUES (?, ?, ?, ?, ?, 0, 0, 0, 'active', ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.title)
    .bind(&stored_path)
    .bind(&format)
    .bind(&bindings_json)
    .bind(&now)
    .bind(&now)
    .bind(&content_hash)
    .bind(&preview)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    set_note_tag_links(&id, &tag_names, &state.db).await?;

    // FTS
    let tags_str = tag_names.join(" ");
    let fts_rowid = fts_upsert(&id, &input.title, &content, &tag_names, None, &state.db).await?;
    sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
        .bind(fts_rowid)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    let _ = tags_str;

    let tags = fetch_note_tags(&id, &state.db).await?;

    rebuild_manifest(&state.db, &state.app_data_dir).await?;

    Ok(Note {
        id,
        title: input.title,
        file_path: stored_path,
        format,
        bindings: bindings_vec,
        tags,
        pinned: false,
        archived: false,
        doc_status: "active".to_string(),
        created_at: now.clone(),
        updated_at: now,
        content_hash: Some(content_hash),
        content: Some(content),
        has_draft: false,
    })
}

#[tauri::command]
pub async fn note_update(
    id: String,
    input: NoteUpdateInput,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Note> {
    let now = Utc::now().to_rfc3339();

    let mut row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    if let Some(title) = input.title {
        row.title = title;
    }
    if let Some(pinned) = input.pinned {
        row.pinned = if pinned { 1 } else { 0 };
    }
    row.updated_at = now.clone();

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    let (_, old_tags_list, old_body) = if file_path.exists() {
        read_note_file(&file_path)?
    } else {
        (HashMap::new(), vec![], String::new())
    };

    let old_content = old_body.clone();
    let content = input.content.unwrap_or(old_body);
    let content_hash = compute_hash(&content);
    let preview = make_preview(&content);

    let tag_names = fetch_note_tags(&id, &state.db)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect::<Vec<_>>();

    let _ = old_tags_list;

    // Snapshot the previous content before overwriting (if content actually changed)
    if content != old_content {
        let _ = maybe_snapshot(&id, &row.title, &old_content, "save", &state.db).await;
    }

    write_note_file(&file_path, &row, &tag_names, &content)?;

    let fts_rowid = fts_upsert(&id, &row.title, &content, &tag_names, row.fts_rowid, &state.db).await?;

    sqlx::query(
        "UPDATE notes SET title=?, pinned=?, updated_at=?, content_hash=?, preview=?, fts_rowid=? WHERE id=?",
    )
    .bind(&row.title)
    .bind(row.pinned)
    .bind(&now)
    .bind(&content_hash)
    .bind(&preview)
    .bind(fts_rowid)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    // Delete draft after successful save
    let draft = draft_file_path(&state.app_data_dir, &id);
    if draft.exists() {
        let _ = std::fs::remove_file(&draft);
    }

    rebuild_manifest(&state.db, &state.app_data_dir).await?;

    let tags = fetch_note_tags(&id, &state.db).await?;

    let bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();

    Ok(Note {
        id: row.id,
        title: row.title,
        file_path: row.file_path,
        format: row.format,
        bindings,
        tags,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        doc_status: row.doc_status,
        created_at: row.created_at,
        updated_at: now,
        content_hash: Some(content_hash),
        content: Some(content),
        has_draft: false,
    })
}

#[tauri::command]
pub async fn note_delete(
    id: String,
    hard: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    if hard.unwrap_or(false) {
        // Hard delete: remove file + DB row
        let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?;

        if let Some(r) = row {
            fts_delete(r.fts_rowid, &state.db).await?;
            let file_path = resolve_note_abs_path(&state.app_data_dir, &r.file_path);
            if file_path.exists() {
                let _ = std::fs::remove_file(&file_path);
            }
        }

        sqlx::query("DELETE FROM note_tag_links WHERE note_id = ?")
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;

        sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    } else {
        // Soft delete
        sqlx::query("UPDATE notes SET deleted=1, updated_at=? WHERE id=?")
            .bind(Utc::now().to_rfc3339())
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;

        let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?;
        if let Some(r) = row {
            fts_delete(r.fts_rowid, &state.db).await?;
        }
    }

    rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(())
}

#[tauri::command]
pub async fn note_archive(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query("UPDATE notes SET archived=1, updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_restore(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query("UPDATE notes SET archived=0, deleted=0, doc_status='active', updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_set_tags(
    id: String,
    tag_names: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let now = Utc::now().to_rfc3339();
    set_note_tag_links(&id, &tag_names, &state.db).await?;

    sqlx::query("UPDATE notes SET updated_at=? WHERE id=?")
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    // Rewrite file frontmatter with new tags
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    if file_path.exists() {
        let (_, _, body) = read_note_file(&file_path)?;
        let mut updated_row = row.clone();
        updated_row.updated_at = now.clone();
        write_note_file(&file_path, &updated_row, &tag_names, &body)?;

        let fts_rowid = fts_upsert(
            &id,
            &row.title,
            &body,
            &tag_names,
            row.fts_rowid,
            &state.db,
        )
        .await?;

        sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
            .bind(fts_rowid)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(())
}

#[tauri::command]
pub async fn note_search(
    query: String,
    filter: NoteFilter,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteListItem>> {
    let fts_query = format!("{}*", query.trim());

    let matched: Vec<(String, String)> =
        sqlx::query_as("SELECT note_id, snippet(notes_fts, 2, '<mark>', '</mark>', '…', 12) FROM notes_fts WHERE notes_fts MATCH ? ORDER BY rank LIMIT 100")
            .bind(&fts_query)
            .fetch_all(&state.db)
            .await
            .map_err(AppError::db)?;

    if matched.is_empty() {
        return Ok(vec![]);
    }

    let snippets_map: std::collections::HashMap<String, String> =
        matched.iter().map(|(id, snip)| (id.clone(), snip.clone())).collect();
    let ids: Vec<String> = matched.into_iter().map(|(id, _)| id).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT * FROM notes WHERE id IN ({}) AND deleted = 0 ORDER BY pinned DESC, updated_at DESC",
        placeholders
    );

    // Safe: only `?` placeholders are interpolated; ids are bound below.
    let mut q = sqlx::query_as::<_, NoteRow>(sqlx::AssertSqlSafe(sql));
    for id in &ids {
        q = q.bind(id);
    }

    let rows = q.fetch_all(&state.db).await.map_err(AppError::db)?;
    let tags_map = fetch_all_note_tags_map(&state.db).await?;
    let folder_ids_map = fetch_all_note_folder_ids_map(&state.db).await?;
    let drafts_dir = drafts_dir(&state.app_data_dir);

    let items = rows
        .into_iter()
        .filter(|r| {
            if let Some(ref binding) = filter.binding {
                let bindings: Vec<String> = serde_json::from_str(&r.bindings).unwrap_or_default();
                if !bindings.contains(binding) { return false; }
            }
            true
        })
        .map(|r| {
            let tags = tags_map.get(&r.id).cloned().unwrap_or_default();
            let folder_ids = folder_ids_map.get(&r.id).cloned().unwrap_or_default();
            let has_draft = drafts_dir.join(format!("{}.draft", r.id)).exists();
            let mut item = row_to_list_item(r, tags, folder_ids, has_draft);
            item.snippet = snippets_map.get(&item.id).cloned();
            item
        })
        .collect();

    Ok(items)
}

#[tauri::command]
pub async fn note_sync(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    sync_notes_index(&state.db, &state.app_data_dir, custom_dir.as_ref()).await
}

#[tauri::command]
pub async fn note_reindex(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    // Full FTS rebuild
    sqlx::query("DELETE FROM notes_fts")
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    let rows = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE deleted = 0")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;

    for row in rows {
        let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
        let (_, tags_list, body) = read_note_file(&file_path).unwrap_or_default();
        let tags_str = tags_list.join(" ");

        let mut conn = state.db.acquire().await.map_err(AppError::db)?;
        sqlx::query(
            "INSERT INTO notes_fts(note_id, title, content, tags) VALUES (?, ?, ?, ?)",
        )
        .bind(&row.id)
        .bind(&row.title)
        .bind(&body)
        .bind(&tags_str)
        .execute(&mut *conn)
        .await
        .map_err(AppError::db)?;

        let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
            .fetch_one(&mut *conn)
            .await
            .map_err(AppError::db)?;

        sqlx::query("UPDATE notes SET fts_rowid=? WHERE id=?")
            .bind(rowid)
            .bind(&row.id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn note_open_folder(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let custom_dir = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    let dir = effective_docs_dir(&state.app_data_dir, custom_dir.as_ref());
    open_path(&dir)
}

#[tauri::command]
pub async fn note_open_external(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {id}")))?;

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    open_path(&file_path)
}

#[tauri::command]
pub async fn note_draft_save(
    id: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let draft_path = draft_file_path(&state.app_data_dir, &id);
    std::fs::write(&draft_path, &content).map_err(AppError::io)?;
    Ok(())
}

#[tauri::command]
pub async fn note_draft_get(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<String>> {
    let draft_path = draft_file_path(&state.app_data_dir, &id);
    if draft_path.exists() {
        let content = std::fs::read_to_string(&draft_path).map_err(AppError::io)?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn note_draft_discard(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let draft_path = draft_file_path(&state.app_data_dir, &id);
    if draft_path.exists() {
        std::fs::remove_file(&draft_path).map_err(AppError::io)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_tag_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<NoteTag>> {
    sqlx::query_as::<_, NoteTag>(
        "SELECT id, name, color, created_at, updated_at FROM note_tags ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn note_tag_create(
    name: String,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteTag> {
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return Err(AppError::io("Tag name cannot be empty"));
    }
    let color = color.unwrap_or_else(|| "#6366f1".to_string());
    let now = Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO note_tags (id, name, color, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET color = excluded.color, updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(&name)
    .bind(&color)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    let tag = sqlx::query_as::<_, NoteTag>(
        "SELECT id, name, color, created_at, updated_at FROM note_tags WHERE name = ?",
    )
    .bind(&name)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(tag)
}

#[tauri::command]
pub async fn note_tag_delete(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query("DELETE FROM note_tag_links WHERE tag_id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    sqlx::query("DELETE FROM note_tags WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_tag_update(
    id: String,
    name: Option<String>,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteTag> {
    let now = Utc::now().to_rfc3339();
    let name = name.map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty());
    sqlx::query(
        "UPDATE note_tags SET
            name       = COALESCE(?, name),
            color      = COALESCE(?, color),
            updated_at = ?
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&color)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    let tag = sqlx::query_as::<_, NoteTag>(
        "SELECT id, name, color, created_at, updated_at FROM note_tags WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(tag)
}

// ── Folder commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn note_folder_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<NoteFolder>> {
    sqlx::query_as::<_, NoteFolder>(
        "SELECT id, name, parent_id, color, created_at, updated_at FROM note_folders ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn note_folder_create(
    name: String,
    parent_id: Option<String>,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteFolder> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::io("Folder name cannot be empty"));
    }
    let id = Uuid::new_v4().to_string();
    let color = color.unwrap_or_else(|| "#6366f1".to_string());
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO note_folders (id, name, parent_id, color, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&parent_id)
    .bind(&color)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    sqlx::query_as::<_, NoteFolder>(
        "SELECT id, name, parent_id, color, created_at, updated_at FROM note_folders WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn note_folder_update(
    id: String,
    name: Option<String>,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteFolder> {
    let now = Utc::now().to_rfc3339();
    let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    sqlx::query(
        "UPDATE note_folders SET
            name       = COALESCE(?, name),
            color      = COALESCE(?, color),
            updated_at = ?
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&color)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    sqlx::query_as::<_, NoteFolder>(
        "SELECT id, name, parent_id, color, created_at, updated_at FROM note_folders WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn note_folder_delete(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    // Collect all descendant folder IDs recursively using CTE
    let descendants: Vec<(String,)> = sqlx::query_as(
        "WITH RECURSIVE sub(id) AS (
            SELECT id FROM note_folders WHERE id = ?
            UNION ALL
            SELECT f.id FROM note_folders f JOIN sub ON f.parent_id = sub.id
         )
         SELECT id FROM sub",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;

    for (fid,) in &descendants {
        sqlx::query("DELETE FROM note_folder_links WHERE folder_id = ?")
            .bind(fid)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    // Delete all descendant folders (children first via CTE ordering handled by FK cascade
    // but SQLite may not enforce it, so delete in reverse BFS order via delete by ids)
    for (fid,) in descendants.iter().rev() {
        sqlx::query("DELETE FROM note_folders WHERE id = ?")
            .bind(fid)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn note_add_binding(
    note_id: String,
    binding: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let row: Option<(String,)> = sqlx::query_as("SELECT bindings FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;

    let Some((bindings_json,)) = row else { return Ok(()); };
    let mut bindings: Vec<String> = serde_json::from_str(&bindings_json).unwrap_or_default();
    if !bindings.contains(&binding) {
        bindings.push(binding);
        let new_json = serde_json::to_string(&bindings).map_err(AppError::other)?;
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE notes SET bindings = ?, updated_at = ? WHERE id = ?")
            .bind(&new_json)
            .bind(&now)
            .bind(&note_id)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_remove_binding(
    note_id: String,
    binding: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let row: Option<(String,)> = sqlx::query_as("SELECT bindings FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;

    let Some((bindings_json,)) = row else { return Ok(()); };
    let mut bindings: Vec<String> = serde_json::from_str(&bindings_json).unwrap_or_default();
    bindings.retain(|b| b != &binding);
    let new_json = serde_json::to_string(&bindings).map_err(AppError::other)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE notes SET bindings = ?, updated_at = ? WHERE id = ?")
        .bind(&new_json)
        .bind(&now)
        .bind(&note_id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_add_folder(
    note_id: String,
    folder_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO note_folder_links (note_id, folder_id) VALUES (?, ?)",
    )
    .bind(&note_id)
    .bind(&folder_id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn note_remove_folder(
    note_id: String,
    folder_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query("DELETE FROM note_folder_links WHERE note_id = ? AND folder_id = ?")
        .bind(&note_id)
        .bind(&folder_id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

// ── Notes directory settings ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct NotesDirInfo {
    pub current: String,
    pub is_custom: bool,
}

#[tauri::command]
pub async fn notes_get_dir(state: tauri::State<'_, AppState>) -> CmdResult<NotesDirInfo> {
    let custom = state.notes_custom_dir.read().ok().and_then(|g| g.clone());
    let (current, is_custom) = if let Some(ref p) = custom {
        (p.to_string_lossy().to_string(), true)
    } else {
        (documents_dir(&state.app_data_dir).to_string_lossy().to_string(), false)
    };
    Ok(NotesDirInfo { current, is_custom })
}

#[tauri::command]
pub async fn notes_set_dir(
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NotesDirInfo> {
    let new_custom: Option<PathBuf> = path.as_deref().and_then(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() { None } else { Some(PathBuf::from(trimmed)) }
    });

    if let Some(ref p) = new_custom {
        std::fs::create_dir_all(p).map_err(AppError::io)?;
    }

    // Persist to DB
    if let Some(ref p) = new_custom {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('notes_custom_dir', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(p.to_string_lossy().as_ref())
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    } else {
        sqlx::query("DELETE FROM app_settings WHERE key = 'notes_custom_dir'")
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
    }

    // Update in-memory state
    if let Ok(mut lock) = state.notes_custom_dir.write() {
        *lock = new_custom.clone();
    }

    let (current, is_custom) = if let Some(ref p) = new_custom {
        (p.to_string_lossy().to_string(), true)
    } else {
        (documents_dir(&state.app_data_dir).to_string_lossy().to_string(), false)
    };
    Ok(NotesDirInfo { current, is_custom })
}

// ── Note History ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct NoteHistoryEntry {
    pub id: String,
    pub note_id: String,
    pub parent_id: Option<String>,
    pub revision: i64,
    pub version_type: String,
    pub title: String,
    pub content: Option<String>,
    pub content_hash: String,
    pub author: Option<String>,
    pub device: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MergeResult {
    pub content: String,
    pub has_conflicts: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct DiffLine {
    pub kind: String, // "context" | "added" | "removed"
    pub content: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct DiffResult {
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Deserialize, Default)]
pub struct HistoryFilter {
    pub version_type: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

/// Fetch the last history entry metadata (id, created_at, revision) for a note.
async fn history_last_meta(
    note_id: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Option<(String, String, i64)>, AppError> {
    sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, created_at, revision FROM note_history WHERE note_id = ? ORDER BY revision DESC LIMIT 1",
    )
    .bind(note_id)
    .fetch_optional(db)
    .await
    .map_err(AppError::db)
}

/// Create a snapshot of the given content. Inserts into note_history.
async fn history_snapshot(
    note_id: &str,
    title: &str,
    content: &str,
    version_type: &str,
    parent_id: Option<String>,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    let content_hash = compute_hash(content);
    let compressed = compress_content(content)?;

    let (next_revision,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(revision), 0) + 1 FROM note_history WHERE note_id = ?",
    )
    .bind(note_id)
    .fetch_one(db)
    .await
    .map_err(AppError::db)?;

    sqlx::query(
        "INSERT INTO note_history
         (id, note_id, parent_id, revision, version_type, title, content, content_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(note_id)
    .bind(&parent_id)
    .bind(next_revision)
    .bind(version_type)
    .bind(title)
    .bind(&compressed)
    .bind(&content_hash)
    .bind(&now)
    .execute(db)
    .await
    .map_err(AppError::db)?;

    history_cleanup(note_id, db).await?;

    Ok(id)
}

/// Keep last 100 entries; delete entries older than 90 days beyond the first 50.
async fn history_cleanup(note_id: &str, db: &sqlx::Pool<sqlx::Sqlite>) -> Result<(), AppError> {
    let cutoff = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
    sqlx::query(
        "DELETE FROM note_history
         WHERE note_id = ?
           AND created_at < ?
           AND id NOT IN (
               SELECT id FROM note_history WHERE note_id = ? ORDER BY revision DESC LIMIT 100
           )",
    )
    .bind(note_id)
    .bind(&cutoff)
    .bind(note_id)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

/// Called from note_update: apply snapshot policy and conditionally snapshot old content.
async fn maybe_snapshot(
    note_id: &str,
    title: &str,
    old_content: &str,
    version_type: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<(), AppError> {
    let last = history_last_meta(note_id, db).await?;

    let should_snapshot: bool;
    let parent_id: Option<String>;

    match last {
        None => {
            // First snapshot ever for this note
            should_snapshot = true;
            parent_id = None;
        }
        Some((last_id, last_created_at, _)) => {
            let last_dt = chrono::DateTime::parse_from_rfc3339(&last_created_at)
                .ok()
                .map(|dt| dt.with_timezone(&Utc));
            let seconds_since = last_dt
                .map(|ldt| (Utc::now() - ldt).num_seconds())
                .unwrap_or(i64::MAX);

            // Skip if last snapshot was made less than 60 seconds ago
            should_snapshot = seconds_since >= 60;
            parent_id = Some(last_id);
        }
    }

    if should_snapshot {
        history_snapshot(note_id, title, old_content, version_type, parent_id, db).await?;
    }

    Ok(())
}

/// Decompress and return content for a history entry by its id.
async fn history_content_by_id(
    history_id: &str,
    db: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<String, AppError> {
    let row: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT content FROM note_history WHERE id = ?")
            .bind(history_id)
            .fetch_optional(db)
            .await
            .map_err(AppError::db)?;

    let (blob,) = row.ok_or_else(|| AppError::not_found(format!("History {history_id}")))?;
    decompress_content(&blob)
}

fn compute_diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let patch = diffy::create_patch(old, new);
    let mut lines = Vec::new();
    for hunk in patch.hunks() {
        for line in hunk.lines() {
            let (kind, raw): (&str, String) = match line {
                diffy::Line::Context(s) => ("context", s.to_string()),
                diffy::Line::Delete(s) => ("removed", s.to_string()),
                diffy::Line::Insert(s) => ("added", s.to_string()),
            };
            lines.push(DiffLine {
                kind: kind.to_string(),
                content: raw.trim_end_matches('\n').to_string(),
            });
        }
    }
    lines
}

// ── History commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn note_history_list(
    note_id: String,
    filter: Option<HistoryFilter>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<NoteHistoryEntry>> {
    let filter = filter.unwrap_or_default();

    let rows: Vec<(String, String, Option<String>, i64, String, String, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, note_id, parent_id, revision, version_type, title, content_hash, author, device, created_at
             FROM note_history
             WHERE note_id = ?
               AND (? IS NULL OR version_type = ?)
               AND (? IS NULL OR created_at >= ?)
               AND (? IS NULL OR created_at <= ?)
             ORDER BY revision DESC",
        )
        .bind(&note_id)
        .bind(&filter.version_type).bind(&filter.version_type)
        .bind(&filter.date_from).bind(&filter.date_from)
        .bind(&filter.date_to).bind(&filter.date_to)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;

    let entries = rows
        .into_iter()
        .map(|(id, note_id, parent_id, revision, version_type, title, content_hash, author, device, created_at)| {
            NoteHistoryEntry {
                id,
                note_id,
                parent_id,
                revision,
                version_type,
                title,
                content: None,
                content_hash,
                author,
                device,
                created_at,
            }
        })
        .collect();

    Ok(entries)
}

#[tauri::command]
pub async fn note_history_get(
    history_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<NoteHistoryEntry> {
    let row: Option<(String, String, Option<String>, i64, String, String, Vec<u8>, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, note_id, parent_id, revision, version_type, title, content, content_hash, author, device, created_at
             FROM note_history WHERE id = ?",
        )
        .bind(&history_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?;

    let (id, note_id, parent_id, revision, version_type, title, blob, content_hash, author, device, created_at) =
        row.ok_or_else(|| AppError::not_found(format!("History {history_id}")))?;

    let content = decompress_content(&blob)?;

    Ok(NoteHistoryEntry {
        id,
        note_id,
        parent_id,
        revision,
        version_type,
        title,
        content: Some(content),
        content_hash,
        author,
        device,
        created_at,
    })
}

#[tauri::command]
pub async fn note_history_diff(
    note_id: String,
    from_id: String,
    to_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<DiffResult> {
    async fn resolve_content(
        entry_id: &str,
        note_id: &str,
        state: &tauri::State<'_, AppState>,
    ) -> Result<String, AppError> {
        if entry_id == "current" {
            let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
                .bind(note_id)
                .fetch_optional(&state.db)
                .await
                .map_err(AppError::db)?
                .ok_or_else(|| AppError::not_found(format!("Note {note_id}")))?;
            let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
            let (_, _, body) = read_note_file(&file_path).unwrap_or_default();
            Ok(body)
        } else {
            history_content_by_id(entry_id, &state.db).await
        }
    }

    let old_content = resolve_content(&from_id, &note_id, &state).await?;
    let new_content = resolve_content(&to_id, &note_id, &state).await?;

    Ok(DiffResult {
        lines: compute_diff_lines(&old_content, &new_content),
    })
}

#[tauri::command]
pub async fn note_history_restore(
    note_id: String,
    history_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Note> {
    // Get history content to restore
    let history_content = history_content_by_id(&history_id, &state.db).await?;

    // Get history title
    let (hist_title,): (String,) =
        sqlx::query_as("SELECT title FROM note_history WHERE id = ?")
            .bind(&history_id)
            .fetch_one(&state.db)
            .await
            .map_err(AppError::db)?;

    // Get current note
    let mut row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {note_id}")))?;

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    let (_, _, current_content) = if file_path.exists() {
        read_note_file(&file_path)?
    } else {
        (HashMap::new(), vec![], String::new())
    };

    // Snapshot current state before restoring (type = "restore", parent = last history id)
    let parent_id = history_last_meta(&note_id, &state.db).await?.map(|(pid, _, _)| pid);
    let _ = history_snapshot(
        &note_id,
        &row.title,
        &current_content,
        "restore",
        parent_id,
        &state.db,
    ).await;

    // Apply restored content
    let now = Utc::now().to_rfc3339();
    row.title = hist_title.clone();
    row.updated_at = now.clone();

    let content_hash = compute_hash(&history_content);
    let preview = make_preview(&history_content);

    let tag_names = fetch_note_tags(&note_id, &state.db)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect::<Vec<_>>();

    write_note_file(&file_path, &row, &tag_names, &history_content)?;

    let fts_rowid = fts_upsert(
        &note_id,
        &hist_title,
        &history_content,
        &tag_names,
        row.fts_rowid,
        &state.db,
    )
    .await?;

    sqlx::query(
        "UPDATE notes SET title=?, updated_at=?, content_hash=?, preview=?, fts_rowid=? WHERE id=?",
    )
    .bind(&hist_title)
    .bind(&now)
    .bind(&content_hash)
    .bind(&preview)
    .bind(fts_rowid)
    .bind(&note_id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    rebuild_manifest(&state.db, &state.app_data_dir).await?;

    let tags = fetch_note_tags(&note_id, &state.db).await?;
    let bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();

    Ok(Note {
        id: row.id,
        title: hist_title,
        file_path: row.file_path,
        format: row.format,
        bindings,
        tags,
        pinned: row.pinned != 0,
        archived: row.archived != 0,
        doc_status: row.doc_status,
        created_at: row.created_at,
        updated_at: now,
        content_hash: Some(content_hash),
        content: Some(history_content),
        has_draft: false,
    })
}

#[tauri::command]
pub async fn note_history_merge(
    note_id: String,
    history_id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<MergeResult> {
    // Get current note content
    let row = sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Note {note_id}")))?;

    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    let (_, _, current_content) = if file_path.exists() {
        read_note_file(&file_path)?
    } else {
        (HashMap::new(), vec![], String::new())
    };

    // Get history content and its parent (ancestor for 3-way merge)
    let (history_content, parent_id) = {
        let r: Option<(Vec<u8>, Option<String>)> =
            sqlx::query_as("SELECT content, parent_id FROM note_history WHERE id = ?")
                .bind(&history_id)
                .fetch_optional(&state.db)
                .await
                .map_err(AppError::db)?;
        let (blob, pid) = r.ok_or_else(|| AppError::not_found(format!("History {history_id}")))?;
        (decompress_content(&blob)?, pid)
    };

    // Ancestor = parent of history entry (or empty string if none)
    let ancestor = if let Some(pid) = parent_id {
        history_content_by_id(&pid, &state.db).await.unwrap_or_default()
    } else {
        String::new()
    };

    // 3-way merge: original=ancestor, ours=current, theirs=history
    let (merged_content, has_conflicts) =
        match diffy::merge(&ancestor, &current_content, &history_content) {
            Ok(merged) => (merged, false),
            Err(merged_with_conflicts) => (merged_with_conflicts, true),
        };

    Ok(MergeResult {
        content: merged_content,
        has_conflicts,
    })
}
