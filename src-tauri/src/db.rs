// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::path::Path;

#[cfg(desktop)]
pub async fn init_pool(db_path: &Path) -> Result<Pool<Sqlite>> {
    let pool = connect(db_path).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

async fn connect(db_path: &Path) -> Result<Pool<Sqlite>> {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    Ok(SqlitePoolOptions::new().max_connections(5).connect(&url).await?)
}

/// Mobile: also upgrades a database written by the standalone mobile app.
/// Returns `true` when that happened and the notes index must be rebuilt.
#[cfg(mobile)]
pub async fn init_pool_mobile(db_path: &Path) -> Result<(Pool<Sqlite>, bool)> {
    let pool = connect(db_path).await?;
    let legacy = legacy_mobile::detect(&pool).await?;
    if legacy {
        legacy_mobile::prepare(&pool).await?;
    }
    run_migrations(&pool).await?;
    if legacy {
        legacy_mobile::finish(&pool).await?;
    }
    Ok((pool, legacy))
}

/// Schema of the standalone mobile app (`veydan.db`, user_version <= 5): fewer
/// columns on `notes`, a 4-column `profiles` catalog and plain-text history.
#[cfg(mobile)]
mod legacy_mobile {
    use super::add_column_if_not_exists;
    use crate::commands::notes::{compress_content, compute_hash};
    use anyhow::Result;
    use sqlx::{Pool, Sqlite};

    pub async fn detect(pool: &Pool<Sqlite>) -> Result<bool> {
        let (has_notes,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'notes'")
                .fetch_one(pool)
                .await?;
        if has_notes == 0 {
            return Ok(false);
        }
        let (has_fts_rowid,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'fts_rowid'")
                .fetch_one(pool)
                .await?;
        Ok(has_fts_rowid == 0)
    }

    /// Before the shared migrations: widen `notes`, park tables whose shape differs.
    pub async fn prepare(pool: &Pool<Sqlite>) -> Result<()> {
        for (col, def) in [
            ("scope", "TEXT NOT NULL DEFAULT 'global'"),
            ("workspace_id", "TEXT NULL"),
            ("profile_id", "TEXT NULL"),
            ("version_base", "TEXT NULL"),
            ("fts_rowid", "INTEGER NULL"),
            ("file_mtime", "TEXT NULL"),
        ] {
            add_column_if_not_exists(pool, "notes", col, def).await?;
        }
        add_column_if_not_exists(pool, "sync_note_state", "conflict", "INTEGER NOT NULL DEFAULT 0").await?;
        sqlx::query("ALTER TABLE profiles RENAME TO legacy_profiles").execute(pool).await?;
        sqlx::query("ALTER TABLE note_history RENAME TO legacy_note_history").execute(pool).await?;
        Ok(())
    }

    /// After the shared migrations: move parked rows into the new tables.
    pub async fn finish(pool: &Pool<Sqlite>) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO profiles (id, name, profile_path, workspace_id, created_at, updated_at)
             SELECT id, name, '', workspace_id, created_at, created_at FROM legacy_profiles",
        )
        .execute(pool)
        .await?;
        sqlx::query("DROP TABLE legacy_profiles").execute(pool).await?;

        let rows: Vec<(String, String, i64, String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT id, note_id, revision, version_type, title, content, device, created_at
             FROM legacy_note_history ORDER BY note_id, revision",
        )
        .fetch_all(pool)
        .await?;
        for (id, note_id, revision, version_type, title, content, device, created_at) in rows {
            let blob = compress_content(&content).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            sqlx::query(
                "INSERT OR IGNORE INTO note_history
                 (id, note_id, revision, version_type, title, content, content_hash, device, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&note_id)
            .bind(revision)
            .bind(&version_type)
            .bind(&title)
            .bind(&blob)
            .bind(compute_hash(&content))
            .bind(&device)
            .bind(&created_at)
            .execute(pool)
            .await?;
        }
        sqlx::query("DROP TABLE legacy_note_history").execute(pool).await?;
        Ok(())
    }
}

async fn add_column_if_not_exists(
    pool: &Pool<Sqlite>,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let q = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition);
    // Safe: table/column/definition are hardcoded schema identifiers from the
    // migration list, never user input (sqlx 0.9 SqlSafeStr audit).
    match sqlx::query(sqlx::AssertSqlSafe(q)).execute(pool).await {
        Ok(_) => {}
        Err(e) if e.to_string().contains("duplicate column name") => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            color TEXT NOT NULL DEFAULT '#6366f1',
            icon TEXT NOT NULL DEFAULT 'folder',
            notes TEXT,
            is_default INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS profiles (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'stopped',
            profile_path TEXT NOT NULL,
            browser_type TEXT NOT NULL DEFAULT 'camoufox',
            proxy_id TEXT,
            fingerprint_preset TEXT NOT NULL DEFAULT 'linux',
            user_agent TEXT,
            platform TEXT,
            timezone TEXT,
            locale TEXT NOT NULL DEFAULT 'en-US',
            languages TEXT NOT NULL DEFAULT 'en-US,en',
            screen_width INTEGER NOT NULL DEFAULT 1920,
            screen_height INTEGER NOT NULL DEFAULT 1080,
            webrtc_mode TEXT NOT NULL DEFAULT 'disable',
            geolocation_enabled INTEGER NOT NULL DEFAULT 0,
            latitude REAL,
            longitude REAL,
            notes TEXT,
            workspace_id TEXT REFERENCES workspaces(id),
            kanban_status TEXT NOT NULL DEFAULT 'new',
            kanban_order INTEGER NOT NULL DEFAULT 0,
            tags TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_launch_at TEXT
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS proxies (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            proxy_type TEXT NOT NULL DEFAULT 'socks5',
            host TEXT NOT NULL,
            port INTEGER NOT NULL,
            username TEXT,
            password TEXT,
            country TEXT,
            city TEXT,
            status TEXT NOT NULL DEFAULT 'unknown',
            last_ip TEXT,
            last_check_at TEXT,
            workspace_id TEXT REFERENCES workspaces(id),
            created_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS password_history (
            id TEXT PRIMARY KEY NOT NULL,
            password TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS totp_entries (
            id          TEXT PRIMARY KEY NOT NULL,
            name        TEXT NOT NULL,
            issuer      TEXT,
            secret      TEXT NOT NULL,
            algorithm   TEXT NOT NULL DEFAULT 'SHA1',
            digits      INTEGER NOT NULL DEFAULT 6,
            period      INTEGER NOT NULL DEFAULT 30,
            tags        TEXT NOT NULL DEFAULT '[]',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            last_used_at TEXT
        )",
    )
    .execute(pool)
    .await?;

    // workspace_columns table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS workspace_columns (
            id TEXT PRIMARY KEY NOT NULL,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id),
            name TEXT NOT NULL,
            tag_name TEXT NOT NULL,
            color TEXT NOT NULL DEFAULT '#6366f1',
            position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // ── Notes Layer ────────────────────────────────────────────────────────────
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS notes (
            id           TEXT PRIMARY KEY NOT NULL,
            title        TEXT NOT NULL,
            file_path    TEXT NOT NULL,
            format       TEXT NOT NULL DEFAULT 'md',
            scope        TEXT NOT NULL DEFAULT 'global',
            workspace_id TEXT NULL,
            profile_id   TEXT NULL,
            pinned       INTEGER NOT NULL DEFAULT 0,
            archived     INTEGER NOT NULL DEFAULT 0,
            deleted      INTEGER NOT NULL DEFAULT 0,
            doc_status   TEXT NOT NULL DEFAULT 'active',
            version_base TEXT NULL,
            fts_rowid    INTEGER NULL,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL,
            file_mtime   TEXT NULL,
            content_hash TEXT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_tags (
            id         TEXT PRIMARY KEY NOT NULL,
            name       TEXT NOT NULL UNIQUE,
            color      TEXT NOT NULL DEFAULT '#6366f1',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_tag_links (
            note_id TEXT NOT NULL,
            tag_id  TEXT NOT NULL,
            PRIMARY KEY (note_id, tag_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts
         USING fts5(note_id UNINDEXED, title, content, tags)",
    )
    .execute(pool)
    .await?;

    // Migrations for existing databases (idempotent)
    add_column_if_not_exists(
        pool,
        "profiles",
        "workspace_id",
        "TEXT REFERENCES workspaces(id)",
    )
    .await?;
    add_column_if_not_exists(
        pool,
        "profiles",
        "kanban_status",
        "TEXT NOT NULL DEFAULT 'new'",
    )
    .await?;
    add_column_if_not_exists(
        pool,
        "profiles",
        "kanban_order",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    add_column_if_not_exists(pool, "profiles", "tags", "TEXT NOT NULL DEFAULT '[]'").await?;
    add_column_if_not_exists(pool, "profiles", "webgl_vendor", "TEXT").await?;
    add_column_if_not_exists(pool, "profiles", "webgl_renderer", "TEXT").await?;
    add_column_if_not_exists(
        pool,
        "proxies",
        "workspace_id",
        "TEXT REFERENCES workspaces(id)",
    )
    .await?;
    add_column_if_not_exists(pool, "proxies", "private_key", "TEXT").await?;
    add_column_if_not_exists(pool, "proxies", "server_fingerprint", "TEXT").await?;
    add_column_if_not_exists(pool, "proxies", "tags", "TEXT NOT NULL DEFAULT '[]'").await?;
    add_column_if_not_exists(pool, "notes", "preview", "TEXT NOT NULL DEFAULT ''").await?;
    add_column_if_not_exists(pool, "notes", "bindings", "TEXT NOT NULL DEFAULT '[]'").await?;
    add_column_if_not_exists(pool, "notes", "folder_id", "TEXT NULL").await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_folders (
            id         TEXT PRIMARY KEY NOT NULL,
            name       TEXT NOT NULL,
            parent_id  TEXT NULL REFERENCES note_folders(id),
            color      TEXT NOT NULL DEFAULT '#6366f1',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // note_folder_links: many-to-many notes ↔ folders
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_folder_links (
            note_id   TEXT NOT NULL,
            folder_id TEXT NOT NULL,
            PRIMARY KEY (note_id, folder_id)
        )",
    )
    .execute(pool)
    .await?;

    // Wiki links between notes, rebuilt from the body on save
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_links (
            from_id TEXT NOT NULL,
            to_id   TEXT NOT NULL,
            PRIMARY KEY (from_id, to_id)
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_note_links_to ON note_links(to_id)")
        .execute(pool)
        .await?;

    // Saved filters: conditions is a NoteFilter JSON
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_smart_views (
            id         TEXT PRIMARY KEY NOT NULL,
            name       TEXT NOT NULL,
            color      TEXT NOT NULL DEFAULT '#8b7bff',
            conditions TEXT NOT NULL DEFAULT '{}',
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // Note version history (DAG: parent_id links versions into a tree)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS note_history (
            id           TEXT PRIMARY KEY,
            note_id      TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            parent_id    TEXT NULL REFERENCES note_history(id),
            revision     INTEGER NOT NULL,
            version_type TEXT NOT NULL DEFAULT 'save',
            title        TEXT NOT NULL,
            content      BLOB NOT NULL,
            content_hash TEXT NOT NULL,
            author       TEXT NULL,
            device       TEXT NULL,
            created_at   TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_nh_note_created ON note_history(note_id, created_at DESC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_nh_note_revision ON note_history(note_id, revision DESC)",
    )
    .execute(pool)
    .await?;

    // Migrate existing folder_id column → note_folder_links (idempotent)
    sqlx::query(
        "INSERT OR IGNORE INTO note_folder_links (note_id, folder_id)
         SELECT id, folder_id FROM notes WHERE folder_id IS NOT NULL",
    )
    .execute(pool)
    .await?;

    // ── SSH Layer ──────────────────────────────────────────────────────────────
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ssh_connections (
            id                  TEXT PRIMARY KEY NOT NULL,
            name                TEXT NOT NULL,
            host                TEXT NOT NULL,
            port                INTEGER NOT NULL DEFAULT 22,
            username            TEXT NOT NULL,
            auth_type           TEXT NOT NULL DEFAULT 'password',
            password            TEXT,
            private_key         TEXT,
            key_passphrase      TEXT,
            requires_2fa        INTEGER NOT NULL DEFAULT 0,
            totp_entry_id       TEXT,
            proxy_id            TEXT REFERENCES proxies(id) ON DELETE SET NULL,
            connect_timeout_sec INTEGER NOT NULL DEFAULT 15,
            keepalive_sec       INTEGER NOT NULL DEFAULT 30,
            terminal_theme      TEXT,
            default_cols        INTEGER NOT NULL DEFAULT 120,
            default_rows        INTEGER NOT NULL DEFAULT 32,
            last_connected_at   TEXT,
            created_at          TEXT NOT NULL,
            updated_at          TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // Many-to-many: SSH connection ↔ workspace
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ssh_connection_workspaces (
            connection_id TEXT NOT NULL REFERENCES ssh_connections(id) ON DELETE CASCADE,
            workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            PRIMARY KEY (connection_id, workspace_id)
        )",
    )
    .execute(pool)
    .await?;

    // Many-to-many: SSH connection ↔ profile
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ssh_connection_profiles (
            connection_id TEXT NOT NULL REFERENCES ssh_connections(id) ON DELETE CASCADE,
            profile_id    TEXT NOT NULL,
            PRIMARY KEY (connection_id, profile_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ssh_conn_ws ON ssh_connection_workspaces(workspace_id)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ssh_conn_pr ON ssh_connection_profiles(profile_id)",
    )
    .execute(pool)
    .await?;

    // Stored SSH keys (referenced by ssh_connections.ssh_key_id)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ssh_keys (
            id          TEXT PRIMARY KEY NOT NULL,
            name        TEXT NOT NULL,
            algorithm   TEXT NOT NULL,
            bits        INTEGER,
            comment     TEXT,
            private_key TEXT NOT NULL,
            public_key  TEXT NOT NULL,
            passphrase  TEXT,
            fingerprint TEXT,
            source      TEXT NOT NULL DEFAULT 'imported',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // NOTE: PRAGMA foreign_keys is not enabled, so ON DELETE SET NULL is
    // documentation only — ssh_key_delete clears references explicitly.
    add_column_if_not_exists(
        pool,
        "ssh_connections",
        "ssh_key_id",
        "TEXT REFERENCES ssh_keys(id) ON DELETE SET NULL",
    )
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_ssh_conn_key ON ssh_connections(ssh_key_id)")
        .execute(pool)
        .await?;

    // SHA256 fingerprint of the server host key, pinned on first successful
    // connection (TOFU) — same scheme as proxies.server_fingerprint.
    add_column_if_not_exists(pool, "ssh_connections", "server_fingerprint", "TEXT").await?;

    // Migrate existing rows: if old columns still exist, drop them gracefully
    // (SQLite doesn't support DROP COLUMN before 3.35, so we leave them; they just won't be used)

    // Generic key-value settings table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_settings (
            key   TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    add_column_if_not_exists(
        pool,
        "profiles",
        "default_search_engine",
        "TEXT NOT NULL DEFAULT 'ddg'",
    )
    .await?;
    add_column_if_not_exists(
        pool,
        "profiles",
        "history_enabled",
        "INTEGER NOT NULL DEFAULT 1",
    )
    .await?;

    // ── Sync Layer (additive; unused while sync is disabled) ─────────────────
    // Last verified position in each peer device's log.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_peers (
            device_id TEXT PRIMARY KEY NOT NULL,
            seq       INTEGER NOT NULL DEFAULT 0,
            head_hash TEXT NOT NULL DEFAULT ''
        )",
    )
    .execute(pool)
    .await?;

    // Per-note sync position: which vault version the local file corresponds to.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_note_state (
            note_id      TEXT PRIMARY KEY NOT NULL,
            head_blob    TEXT NOT NULL DEFAULT '',
            head_parents TEXT NOT NULL DEFAULT '[]',
            head_hlc     TEXT NOT NULL DEFAULT '',
            synced_hash  TEXT NOT NULL DEFAULT '',
            deleted      INTEGER NOT NULL DEFAULT 0,
            conflict     INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;

    // Pending sync conflict: history snapshots of both sides and the remote blob to merge with.
    for col in ["conflict_ancestor_id", "conflict_local_id", "conflict_remote_id", "conflict_remote_blob"] {
        add_column_if_not_exists(pool, "sync_note_state", col, "TEXT NOT NULL DEFAULT ''").await?;
    }

    // Per-attachment sync position: which vault blob the local file corresponds to.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_attachment_state (
            note_id     TEXT NOT NULL,
            name        TEXT NOT NULL,
            head_blob   TEXT NOT NULL DEFAULT '',
            head_hlc    TEXT NOT NULL DEFAULT '',
            synced_hash TEXT NOT NULL DEFAULT '',
            deleted     INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (note_id, name)
        )",
    )
    .execute(pool)
    .await?;
    // Chunked attachment accepted from the vault but not downloaded yet (LargeFileRef JSON).
    add_column_if_not_exists(pool, "sync_attachment_state", "deferred_ref", "TEXT NOT NULL DEFAULT ''").await?;

    // Per-row sync position for table entities (profiles, proxies, ...).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_row_state (
            entity_type TEXT NOT NULL,
            entity_id   TEXT NOT NULL,
            head_hlc    TEXT NOT NULL DEFAULT '',
            synced_hash TEXT NOT NULL DEFAULT '',
            deleted     INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (entity_type, entity_id)
        )",
    )
    .execute(pool)
    .await?;

    // Firefox profile files: last pushed/applied snapshot, lease holder, pending work.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_profile_files_state (
            profile_id       TEXT PRIMARY KEY NOT NULL,
            head_hlc         TEXT NOT NULL DEFAULT '',
            synced_hash      TEXT NOT NULL DEFAULT '',
            manifest_json    TEXT NOT NULL DEFAULT '',
            snapshot_at      TEXT NOT NULL DEFAULT '',
            dirty            INTEGER NOT NULL DEFAULT 0,
            pending_manifest TEXT NOT NULL DEFAULT '',
            lease_device     TEXT NOT NULL DEFAULT '',
            lease_name       TEXT NOT NULL DEFAULT '',
            lease_since      TEXT NOT NULL DEFAULT '',
            lease_hlc        TEXT NOT NULL DEFAULT '',
            lease_synced     INTEGER NOT NULL DEFAULT 1,
            diverged         INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;

    // Unreferenced vault blobs and when they were first seen; deleted after a grace period.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_gc_candidates (
            blob       TEXT PRIMARY KEY NOT NULL,
            first_seen INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    // Reset stale running status on startup — no browsers are actually running yet
    sqlx::query("UPDATE profiles SET status = 'stopped' WHERE status = 'running'")
        .execute(pool)
        .await?;

    // Migrate legacy webrtc_mode value: 'default' was renamed to 'real_ip'
    sqlx::query("UPDATE profiles SET webrtc_mode = 'real_ip' WHERE webrtc_mode = 'default'")
        .execute(pool)
        .await?;

    // Create Default workspace if none exist. Mobile only mirrors the desktop
    // catalog through sync and must not invent a workspace row of its own.
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM workspaces")
        .fetch_one(pool)
        .await?;

    if count == 0 && cfg!(desktop) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO workspaces (id, name, description, color, icon, is_default, created_at, updated_at)
             VALUES ('default', 'Default', NULL, '#6366f1', 'folder', 1, ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    // Assign orphaned profile records to Default workspace
    sqlx::query("UPDATE profiles SET workspace_id = 'default' WHERE workspace_id IS NULL")
        .execute(pool)
        .await?;
    // Keep workspace_id on proxies for the migration step below; orphans get 'default'
    sqlx::query("UPDATE proxies SET workspace_id = 'default' WHERE workspace_id IS NULL")
        .execute(pool)
        .await?;

    // Migrate proxy workspace_id → tags (idempotent: only proxies with empty tags)
    sqlx::query(
        "UPDATE proxies SET tags = json_array('workspace:' || workspace_id)
         WHERE workspace_id IS NOT NULL AND tags = '[]'",
    )
    .execute(pool)
    .await?;

    // Migrate notes scope/workspace_id/profile_id → bindings (idempotent)
    sqlx::query(
        "UPDATE notes SET bindings = json_array('workspace:' || workspace_id)
         WHERE scope = 'workspace' AND workspace_id IS NOT NULL AND bindings = '[]'",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE notes SET bindings = json_array('workspace:' || workspace_id, 'profile:' || profile_id)
         WHERE scope = 'profile' AND workspace_id IS NOT NULL AND profile_id IS NOT NULL AND bindings = '[]'",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE notes SET bindings = json_array('profile:' || profile_id)
         WHERE scope = 'profile' AND workspace_id IS NULL AND profile_id IS NOT NULL AND bindings = '[]'",
    )
    .execute(pool)
    .await?;

    // Fix workspace_columns with empty tag_name (caused by Cyrillic-only names being slugified to "")
    sqlx::query(
        "UPDATE workspace_columns SET tag_name = 'col-' || substr(replace(id, '-', ''), 1, 12) WHERE tag_name = ''"
    )
    .execute(pool)
    .await?;

    // Remove empty string from profile tags (side-effect of empty tag_name columns)
    // Affected profiles become "unassigned" and should be re-assigned manually
    sqlx::query(
        "UPDATE profiles
         SET tags = COALESCE(
             (SELECT json_group_array(value) FROM json_each(tags) WHERE value != ''),
             '[]'
         )
         WHERE EXISTS (SELECT 1 FROM json_each(tags) WHERE value = '')"
    )
    .execute(pool)
    .await?;

    Ok(())
}
