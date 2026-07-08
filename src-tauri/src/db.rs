// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::path::Path;

pub async fn init_pool(db_path: &Path) -> Result<Pool<Sqlite>> {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;
    run_migrations(&pool).await?;
    Ok(pool)
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

    // Reset stale running status on startup — no browsers are actually running yet
    sqlx::query("UPDATE profiles SET status = 'stopped' WHERE status = 'running'")
        .execute(pool)
        .await?;

    // Migrate legacy webrtc_mode value: 'default' was renamed to 'real_ip'
    sqlx::query("UPDATE profiles SET webrtc_mode = 'real_ip' WHERE webrtc_mode = 'default'")
        .execute(pool)
        .await?;

    // Create Default workspace if none exist
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM workspaces")
        .fetch_one(pool)
        .await?;

    if count == 0 {
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
