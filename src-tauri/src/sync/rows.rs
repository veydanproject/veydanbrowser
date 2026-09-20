// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Table rows as sync entities. One implementation serves every table: the
//! synced columns of a row travel inline as a JSON object, local changes are
//! found by hashing that object, and the newer remote row wins (LWW by HLC).
//!
//! Op payload: `{ "<column>": <value>, .., "<link key>": [<id>..] }`.
//! A delete op carries an empty payload.

use super::state::{load_row_state, load_row_states, save_row_state, RowSyncState};
use crate::commands::profiles::profile_delete;
use crate::error::{AppError, CmdResult};
use crate::AppState;
use serde_json::{Map, Value};
use sqlx::sqlite::SqliteArguments;
use sqlx::{query::Query, AssertSqlSafe, Pool, Sqlite};
use std::collections::{BTreeSet, HashMap};
use tauri::{AppHandle, Emitter, Manager};
use veydan_sync::{sha256_hex, Hlc, HlcClock, Op};

pub const EVENT_CHANGED: &str = "sync://data-changed";

/// Link table folded into the parent payload as an id array.
pub struct LinkSpec {
    table: &'static str,
    parent_col: &'static str,
    child_col: &'static str,
    /// Payload key holding the child ids.
    key: &'static str,
}

pub enum Delete {
    /// `DELETE FROM table WHERE pk = ?` plus the listed statements (`?` = id).
    Plain(&'static [&'static str]),
    /// Row and directory through `profile_delete`; deferred while the browser runs.
    Profile,
    /// Deletion belongs to another entity; tombstones are neither sent nor applied.
    Ignore,
}

pub struct TableSpec {
    pub entity: &'static str,
    table: &'static str,
    pk: &'static str,
    columns: &'static [&'static str],
    links: &'static [LinkSpec],
    /// SQL condition selecting the rows to sync.
    filter: Option<&'static str>,
    /// A local row with the same value here is the same thing under another id.
    unique: Option<&'static str>,
    /// Remote rows may create local rows; `false` for tables owned by another entity.
    insert: bool,
    delete: Delete,
}

const NO_LINKS: &[LinkSpec] = &[];

pub const SETTING_ENTITY: &str = "setting";

/// User preferences shared across devices; paths, credentials and sync state stay local.
const SETTING_FILTER: &str = "key IN ('ui_locale', 'minimize_to_tray', 'close_to_tray', 'start_hidden',
    'notes_lock_timeout_min', 'notes_capture_rules', 'quick_capture_shortcut')";

pub const SPECS: &[TableSpec] = &[
    TableSpec {
        entity: "workspace",
        table: "workspaces",
        pk: "id",
        columns: &["name", "description", "color", "icon", "notes", "is_default", "created_at", "updated_at"],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
    TableSpec {
        entity: "workspace_column",
        table: "workspace_columns",
        pk: "id",
        columns: &["workspace_id", "name", "tag_name", "color", "position", "created_at"],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
    TableSpec {
        entity: "proxy",
        table: "proxies",
        pk: "id",
        columns: &[
            "name", "proxy_type", "host", "port", "username", "password", "country", "city", "private_key",
            "server_fingerprint", "tags", "created_at",
        ],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&["UPDATE profiles SET proxy_id = NULL WHERE proxy_id = ?"]),
    },
    TableSpec {
        entity: "ssh_key",
        table: "ssh_keys",
        pk: "id",
        columns: &[
            "name", "algorithm", "bits", "comment", "private_key", "public_key", "passphrase", "fingerprint", "source",
            "created_at", "updated_at",
        ],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&["UPDATE ssh_connections SET ssh_key_id = NULL WHERE ssh_key_id = ?"]),
    },
    TableSpec {
        entity: "profile",
        table: "profiles",
        pk: "id",
        columns: &[
            "name", "browser_type", "proxy_id", "fingerprint_preset", "user_agent", "platform", "timezone", "locale",
            "languages", "screen_width", "screen_height", "webrtc_mode", "geolocation_enabled", "latitude", "longitude",
            "notes", "workspace_id", "kanban_status", "kanban_order", "tags", "webgl_vendor", "webgl_renderer",
            "default_search_engine", "history_enabled", "created_at",
        ],
        // `updated_at` is bumped by launch/stop; syncing it would turn every launch into a row op.
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Profile,
    },
    TableSpec {
        entity: "ssh_connection",
        table: "ssh_connections",
        pk: "id",
        columns: &[
            "name", "host", "port", "username", "auth_type", "password", "private_key", "key_passphrase", "requires_2fa",
            "totp_entry_id", "proxy_id", "ssh_key_id", "connect_timeout_sec", "keepalive_sec", "terminal_theme",
            "default_cols", "default_rows", "server_fingerprint", "created_at", "updated_at",
        ],
        links: &[
            LinkSpec {
                table: "ssh_connection_workspaces",
                parent_col: "connection_id",
                child_col: "workspace_id",
                key: "workspace_ids",
            },
            LinkSpec { table: "ssh_connection_profiles", parent_col: "connection_id", child_col: "profile_id", key: "profile_ids" },
        ],
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
    TableSpec {
        entity: "totp",
        table: "totp_entries",
        pk: "id",
        columns: &["name", "issuer", "secret", "algorithm", "digits", "period", "tags", "created_at", "updated_at"],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
    TableSpec {
        entity: "pw_history",
        table: "password_history",
        pk: "id",
        columns: &["password", "created_at"],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
    TableSpec {
        entity: "note_tag",
        table: "note_tags",
        pk: "id",
        columns: &["name", "color", "created_at", "updated_at"],
        links: NO_LINKS,
        filter: None,
        unique: Some("name"),
        insert: true,
        delete: Delete::Plain(&["DELETE FROM note_tag_links WHERE tag_id = ?"]),
    },
    TableSpec {
        entity: "note_folder",
        table: "note_folders",
        pk: "id",
        columns: &["name", "parent_id", "color", "created_at", "updated_at"],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&["DELETE FROM note_folder_links WHERE folder_id = ?"]),
    },
    TableSpec {
        entity: "note_smart_view",
        table: "note_smart_views",
        pk: "id",
        columns: &["name", "color", "conditions", "sort_order", "created_at", "updated_at"],
        links: NO_LINKS,
        filter: None,
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
    // Note flags and folder membership live only in the DB, not in the note file.
    TableSpec {
        entity: "note_meta",
        table: "notes",
        pk: "id",
        columns: &["pinned", "archived"],
        links: &[LinkSpec { table: "note_folder_links", parent_col: "note_id", child_col: "folder_id", key: "folder_ids" }],
        filter: None,
        unique: None,
        insert: false,
        delete: Delete::Ignore,
    },
    TableSpec {
        entity: SETTING_ENTITY,
        table: "app_settings",
        pk: "key",
        columns: &["value"],
        links: NO_LINKS,
        filter: Some(SETTING_FILTER),
        unique: None,
        insert: true,
        delete: Delete::Plain(&[]),
    },
];

fn spec_for(entity: &str) -> Option<&'static TableSpec> {
    SPECS.iter().find(|s| s.entity == entity)
}

/// Tags, folders, smart views and note flags belong to the notes stream.
#[derive(Clone, Copy)]
pub enum RowScope {
    App,
    Notes,
    NotesCatalog,
    NotesMeta,
}

fn is_note_row(entity: &str) -> bool {
    matches!(entity, "note_tag" | "note_folder" | "note_smart_view" | "note_meta")
}

fn in_scope(entity: &str, scope: RowScope) -> bool {
    match scope {
        RowScope::App => !is_note_row(entity),
        RowScope::Notes => is_note_row(entity),
        RowScope::NotesCatalog => matches!(entity, "note_tag" | "note_folder" | "note_smart_view"),
        RowScope::NotesMeta => entity == "note_meta",
    }
}

/// Ids are UUIDs, setting keys or the literal `default`; nothing else is accepted.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

// ── Reading rows ─────────────────────────────────────────────────────────────

/// `SELECT pk, json_object('c', c, ..) FROM table`: SQLite keeps the column types.
fn select_sql(spec: &TableSpec) -> String {
    let pairs: Vec<String> = spec.columns.iter().map(|c| format!("'{c}', {c}")).collect();
    let mut sql = format!("SELECT {}, json_object({}) FROM {}", spec.pk, pairs.join(", "), spec.table);
    if let Some(f) = spec.filter {
        sql.push_str(" WHERE ");
        sql.push_str(f);
    }
    sql
}

/// Child ids per parent for a link table, one query.
async fn link_map(db: &Pool<Sqlite>, link: &LinkSpec, parent: Option<&str>) -> CmdResult<HashMap<String, Vec<String>>> {
    let mut sql = format!("SELECT {}, {} FROM {}", link.parent_col, link.child_col, link.table);
    if parent.is_some() {
        sql.push_str(&format!(" WHERE {} = ?", link.parent_col));
    }
    sql.push_str(&format!(" ORDER BY {}, {}", link.parent_col, link.child_col));
    let mut q = sqlx::query_as::<_, (String, String)>(AssertSqlSafe(sql));
    if let Some(p) = parent {
        q = q.bind(p.to_string());
    }
    let rows = q.fetch_all(db).await.map_err(AppError::db)?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (p, c) in rows {
        map.entry(p).or_default().push(c);
    }
    Ok(map)
}

fn parse_object(json: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(json).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// Rows matching `select` with their link arrays folded in, as `(id, payload)`.
async fn read_payloads(db: &Pool<Sqlite>, spec: &TableSpec, select: String, id: Option<&str>) -> CmdResult<Vec<(String, Value)>> {
    let mut q = sqlx::query_as::<_, (String, String)>(AssertSqlSafe(select));
    if let Some(id) = id {
        q = q.bind(id.to_string());
    }
    let rows = q.fetch_all(db).await.map_err(AppError::db)?;
    let mut links: Vec<(&LinkSpec, HashMap<String, Vec<String>>)> = Vec::new();
    for link in spec.links {
        links.push((link, link_map(db, link, id).await?));
    }
    Ok(rows
        .into_iter()
        .map(|(row_id, json)| {
            let mut payload = parse_object(&json);
            for (link, map) in &links {
                let ids = map.get(&row_id).cloned().unwrap_or_default();
                payload.insert(link.key.into(), serde_json::to_value(ids).unwrap_or_default());
            }
            (row_id, Value::Object(payload))
        })
        .collect())
}

/// Every synced row of a table.
async fn read_rows(db: &Pool<Sqlite>, spec: &TableSpec) -> CmdResult<Vec<(String, Value)>> {
    read_payloads(db, spec, select_sql(spec), None).await
}

async fn read_row(db: &Pool<Sqlite>, spec: &TableSpec, id: &str) -> CmdResult<Option<Value>> {
    let sql = format!("{} {} {} = ?", select_sql(spec), if spec.filter.is_some() { "AND" } else { "WHERE" }, spec.pk);
    Ok(read_payloads(db, spec, sql, Some(id)).await?.pop().map(|(_, v)| v))
}

/// serde_json sorts object keys, so equal rows hash equal.
fn payload_hash(payload: &Value) -> String {
    sha256_hex(payload.to_string().as_bytes())
}

// ── Push ─────────────────────────────────────────────────────────────────────

pub struct LocalChanges {
    pub ops: Vec<Op>,
    pub states: Vec<RowSyncState>,
}

fn make_op(spec: &TableSpec, id: &str, hlc: Hlc, payload: Value, deleted: bool) -> Op {
    Op { entity_type: spec.entity.into(), entity_id: id.into(), hlc, deleted, payload }
}

/// Rows whose synced columns differ from what the vault has, plus tombstones
/// for rows that disappeared.
pub async fn collect_local_changes(state: &AppState, clock: &mut HlcClock, scope: RowScope) -> CmdResult<LocalChanges> {
    let db = &state.db;
    let mut out = LocalChanges { ops: Vec::new(), states: Vec::new() };

    for spec in SPECS.iter().filter(|s| in_scope(s.entity, scope)) {
        let mut states = load_row_states(db, spec.entity).await?;
        for (id, payload) in read_rows(db, spec).await? {
            let hash = payload_hash(&payload);
            let prev = states.remove(&id);
            if prev.as_ref().map(|s| s.synced_hash == hash && !s.deleted).unwrap_or(false) {
                continue;
            }
            let hlc = clock.now();
            out.ops.push(make_op(spec, &id, hlc.clone(), payload, false));
            out.states.push(RowSyncState {
                entity: spec.entity.into(),
                id,
                head_hlc: Some(hlc),
                synced_hash: hash,
                deleted: false,
            });
        }
        if matches!(spec.delete, Delete::Ignore) {
            continue;
        }
        // Whatever is left in `states` has no row any more.
        for (_, mut st) in states.into_iter().filter(|(_, s)| !s.deleted) {
            let hlc = clock.now();
            out.ops.push(make_op(spec, &st.id, hlc.clone(), Value::Object(Map::new()), true));
            st.deleted = true;
            st.head_hlc = Some(hlc);
            out.states.push(st);
        }
    }
    Ok(out)
}

// ── Pull ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ApplyOutcome {
    /// Entities with at least one applied op; the UI reloads their stores.
    pub changed: BTreeSet<String>,
    /// Set when an op was skipped on a transient condition; peer heads must not advance.
    pub retry: Option<String>,
}

fn bind_json<'q>(q: Query<'q, Sqlite, SqliteArguments>, v: &Value) -> Query<'q, Sqlite, SqliteArguments> {
    match v {
        Value::Null => q.bind(None::<String>),
        Value::Bool(b) => q.bind(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => q.bind(i),
            None => q.bind(n.as_f64()),
        },
        Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    }
}

/// Run a statement built from code constants with JSON-typed bind values.
async fn exec(db: &Pool<Sqlite>, sql: String, values: &[Value]) -> CmdResult<u64> {
    let mut q = sqlx::query(AssertSqlSafe(sql));
    for v in values {
        q = bind_json(q, v);
    }
    Ok(q.execute(db).await.map_err(AppError::db)?.rows_affected())
}

/// Columns the remote row carries and we sync; anything else keeps its local value.
fn known_columns<'a>(spec: &'a TableSpec, payload: &Map<String, Value>) -> Vec<(&'a str, Value)> {
    spec.columns.iter().filter_map(|c| payload.get(*c).map(|v| (*c, v.clone()))).collect()
}

/// NOT NULL columns without defaults that are device-local.
fn insert_defaults(spec: &TableSpec, state: &AppState, id: &str) -> CmdResult<Vec<(&'static str, Value)>> {
    if spec.entity != "profile" {
        return Ok(Vec::new());
    }
    let dir = state.app_data_dir.join("profiles").join(id);
    std::fs::create_dir_all(&dir).map_err(AppError::io)?;
    Ok(vec![
        ("profile_path", Value::String(dir.to_string_lossy().into_owned())),
        ("status", Value::String("stopped".into())),
        ("updated_at", Value::String(chrono::Utc::now().to_rfc3339())),
    ])
}

/// Write the row. Returns `false` when the table forbids inserts and the row is absent.
async fn upsert(state: &AppState, spec: &TableSpec, id: &str, payload: &Map<String, Value>) -> CmdResult<bool> {
    let db = &state.db;
    let cols = known_columns(spec, payload);

    // Same value in the unique column under another id: update that row instead.
    if let Some(u) = spec.unique {
        if let Some(uv) = payload.get(u).and_then(Value::as_str) {
            let sql = format!("SELECT {} FROM {} WHERE {} = ? AND {} != ?", spec.pk, spec.table, u, spec.pk);
            let other: Option<(String,)> =
                sqlx::query_as(AssertSqlSafe(sql)).bind(uv).bind(id).fetch_optional(db).await.map_err(AppError::db)?;
            if let Some((other_id,)) = other {
                let rest: Vec<(&str, Value)> = cols.into_iter().filter(|(c, _)| *c != u).collect();
                return update_row(db, spec, &other_id, &rest).await.map(|_| true);
            }
        }
    }

    if !spec.insert {
        return update_row(db, spec, id, &cols).await;
    }

    let extra = insert_defaults(spec, state, id)?;
    let mut names = vec![spec.pk.to_string()];
    let mut values = vec![Value::String(id.into())];
    for (c, v) in cols.iter().chain(extra.iter()) {
        names.push((*c).into());
        values.push(v.clone());
    }
    let placeholders = vec!["?"; names.len()].join(", ");
    let set: Vec<String> = cols.iter().map(|(c, _)| format!("{c} = excluded.{c}")).collect();
    let on_conflict = if set.is_empty() { "DO NOTHING".to_string() } else { format!("DO UPDATE SET {}", set.join(", ")) };
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT({}) {}",
        spec.table,
        names.join(", "),
        placeholders,
        spec.pk,
        on_conflict
    );
    exec(db, sql, &values).await?;
    Ok(true)
}

async fn update_row(db: &Pool<Sqlite>, spec: &TableSpec, id: &str, cols: &[(&str, Value)]) -> CmdResult<bool> {
    if cols.is_empty() {
        let sql = format!("SELECT 1 FROM {} WHERE {} = ?", spec.table, spec.pk);
        let exists: Option<(i64,)> =
            sqlx::query_as(AssertSqlSafe(sql)).bind(id).fetch_optional(db).await.map_err(AppError::db)?;
        return Ok(exists.is_some());
    }
    let set: Vec<String> = cols.iter().map(|(c, _)| format!("{c} = ?")).collect();
    let sql = format!("UPDATE {} SET {} WHERE {} = ?", spec.table, set.join(", "), spec.pk);
    let mut values: Vec<Value> = cols.iter().map(|(_, v)| v.clone()).collect();
    values.push(Value::String(id.into()));
    Ok(exec(db, sql, &values).await? > 0)
}

async fn apply_links(db: &Pool<Sqlite>, spec: &TableSpec, id: &str, payload: &Map<String, Value>) -> CmdResult<()> {
    for link in spec.links {
        let Some(ids) = payload.get(link.key).and_then(Value::as_array) else { continue };
        let del = format!("DELETE FROM {} WHERE {} = ?", link.table, link.parent_col);
        exec(db, del, &[Value::String(id.into())]).await?;
        let ins = format!("INSERT OR IGNORE INTO {} ({}, {}) VALUES (?, ?)", link.table, link.parent_col, link.child_col);
        for child in ids.iter().filter_map(Value::as_str).filter(|c| valid_id(c)) {
            exec(db, ins.clone(), &[Value::String(id.into()), Value::String(child.into())]).await?;
        }
    }
    Ok(())
}

/// Delete a row the vault deleted. `Ok(false)` means "not now" (browser running).
async fn delete_row(app: &AppHandle, spec: &TableSpec, id: &str) -> CmdResult<bool> {
    let state = app.state::<AppState>();
    let db = &state.db;
    match &spec.delete {
        Delete::Ignore => Ok(true),
        Delete::Profile => {
            if state.browser.is_running(id).await {
                return Ok(false);
            }
            profile_delete(id.to_string(), app.state(), app.clone()).await?;
            Ok(true)
        }
        Delete::Plain(extra) => {
            let id_arg = [Value::String(id.into())];
            for sql in extra.iter() {
                exec(db, sql.to_string(), &id_arg).await?;
            }
            for link in spec.links {
                exec(db, format!("DELETE FROM {} WHERE {} = ?", link.table, link.parent_col), &id_arg).await?;
            }
            exec(db, format!("DELETE FROM {} WHERE {} = ?", spec.table, spec.pk), &id_arg).await?;
            Ok(true)
        }
    }
}

/// Apply remote row ops (LWW by HLC). Ops are already HLC-sorted.
pub async fn apply_remote(app: &AppHandle, ops: &[Op], scope: RowScope) -> CmdResult<ApplyOutcome> {
    let state = app.state::<AppState>();
    let db = &state.db;
    let mut outcome = ApplyOutcome::default();

    for op in ops {
        if !in_scope(&op.entity_type, scope) {
            continue;
        }
        let Some(spec) = spec_for(&op.entity_type) else { continue };
        if !valid_id(&op.entity_id) {
            continue;
        }
        let id = op.entity_id.as_str();
        let prev = load_row_state(db, spec.entity, id).await?;
        if prev.as_ref().and_then(|s| s.head_hlc.as_ref()).map(|h| *h >= op.hlc).unwrap_or(false) {
            continue;
        }
        let mut st = RowSyncState { entity: spec.entity.into(), id: id.into(), head_hlc: Some(op.hlc.clone()), ..Default::default() };

        if op.deleted {
            if matches!(spec.delete, Delete::Ignore) {
                continue;
            }
            if !delete_row(app, spec, id).await? {
                outcome.retry = Some(format!("{} {id} is in use", spec.entity));
                continue;
            }
            st.deleted = true;
            save_row_state(db, &st).await?;
            outcome.changed.insert(spec.entity.into());
            continue;
        }

        let payload = op.payload.as_object().cloned().unwrap_or_default();
        if !upsert(&state, spec, id, &payload).await? {
            // Row owned by another entity has not arrived; a retry batch brings it back.
            continue;
        }
        apply_links(db, spec, id, &payload).await?;
        // Hash what the table now holds, so the next collect sees no change.
        st.synced_hash = match read_row(db, spec, id).await? {
            Some(local) => payload_hash(&local),
            None => payload_hash(&Value::Object(payload)),
        };
        save_row_state(db, &st).await?;
        outcome.changed.insert(spec.entity.into());
    }
    Ok(outcome)
}

/// Re-apply settings that live in process state, then tell the UI which stores to reload.
pub async fn finish_apply(app: &AppHandle, outcome: &ApplyOutcome) {
    if outcome.changed.contains(SETTING_ENTITY) {
        let state = app.state::<AppState>();
        crate::commands::settings::reload_tray_settings(app, &state).await;
        crate::commands::notes::reapply_quick_capture_shortcut(app).await;
    }
    if !outcome.changed.is_empty() {
        let _ = app.emit(EVENT_CHANGED, outcome.changed.iter().cloned().collect::<Vec<_>>());
    }
}
