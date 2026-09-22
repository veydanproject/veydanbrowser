// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use super::content::{pack, DemoPack, SmartKind};
use crate::commands::notes::{insert_note, NoteFilter, NewNote};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use uuid::Uuid;

pub async fn seed_catalog(state: &AppState, locale: &str) -> CmdResult<()> {
    let pack = pack(locale);
    let now = Utc::now().to_rfc3339();

    seed_workspaces(state, &pack, &now).await?;
    seed_proxies(state, &pack, &now).await?;
    seed_profiles(state, &pack, &now).await?;
    seed_totp(state, &pack, &now).await?;

    #[cfg(desktop)]
    seed_ssh(state, &pack, &now).await?;

    seed_tags(state, &pack, &now).await?;
    seed_folders(state, &pack, &now).await?;
    seed_notes(state, &pack, &now).await?;
    seed_smart_views(state, &pack, &now).await?;

    #[cfg(desktop)]
    seed_capture_rules(state, &pack).await?;

    // Extra volume for a fuller video demo (~5× catalog size)
    super::bulk::seed_bulk(state, locale, &now).await?;

    crate::commands::notes::rebuild_manifest(&state.db, &state.app_data_dir).await?;
    Ok(())
}

async fn seed_workspaces(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    sqlx::query(
        "UPDATE workspaces SET name = ?, description = ?, color = ?, icon = ?, updated_at = ?
         WHERE id = 'default'",
    )
    .bind(pack.default_workspace_name)
    .bind(Some(if pack.default_workspace_name == "Личное" {
        "Личные аккаунты"
    } else {
        "Personal accounts"
    }))
    .bind("#6b7280")
    .bind("home")
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    for (i, col) in pack.default_columns.iter().enumerate() {
        insert_column(state, "default", col.name, col.tag_name, col.color, i as i64, now).await?;
    }

    for ws in &pack.workspaces {
        sqlx::query(
            "INSERT INTO workspaces (id, name, description, color, icon, is_default, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(ws.id)
        .bind(ws.name)
        .bind(ws.description)
        .bind(ws.color)
        .bind(ws.icon)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

        for (i, col) in ws.columns.iter().enumerate() {
            insert_column(state, ws.id, col.name, col.tag_name, col.color, i as i64, now).await?;
        }
    }
    Ok(())
}

async fn insert_column(
    state: &AppState,
    workspace_id: &str,
    name: &str,
    tag_name: &str,
    color: &str,
    position: i64,
    now: &str,
) -> CmdResult<()> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO workspace_columns (id, workspace_id, name, tag_name, color, position, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(workspace_id)
    .bind(name)
    .bind(tag_name)
    .bind(color)
    .bind(position)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

async fn seed_proxies(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    for p in &pack.proxies {
        let tags = serde_json::to_string(p.tags).map_err(AppError::other)?;
        sqlx::query(
            "INSERT INTO proxies
             (id, name, proxy_type, host, port, username, password, country, city, status, tags, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'unknown', ?, ?)",
        )
        .bind(p.id)
        .bind(p.name)
        .bind(p.proxy_type)
        .bind(p.host)
        .bind(p.port)
        .bind(p.username)
        .bind(p.password)
        .bind(p.country)
        .bind(p.city)
        .bind(&tags)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

async fn seed_profiles(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    let profiles_root = state.app_data_dir.join("profiles");
    std::fs::create_dir_all(&profiles_root).map_err(AppError::io)?;

    for p in &pack.profiles {
        let profile_path = profiles_root.join(p.id);
        std::fs::create_dir_all(&profile_path).map_err(AppError::io)?;
        let tags = serde_json::to_string(p.tags).map_err(AppError::other)?;

        sqlx::query(
            "INSERT INTO profiles
             (id, name, status, profile_path, browser_type, proxy_id, fingerprint_preset,
              user_agent, platform, timezone, locale, languages, screen_width, screen_height,
              webrtc_mode, geolocation_enabled, latitude, longitude, webgl_vendor, webgl_renderer,
              notes, workspace_id, kanban_status, kanban_order, tags, default_search_engine,
              history_enabled, created_at, updated_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(p.id)
        .bind(p.name)
        .bind("stopped")
        .bind(profile_path.to_string_lossy().as_ref())
        .bind("camoufox")
        .bind(p.proxy_id)
        .bind(p.fingerprint_preset)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(p.timezone)
        .bind(p.locale)
        .bind(p.languages)
        .bind(1920_i64)
        .bind(1080_i64)
        .bind("disable")
        .bind(0_i64)
        .bind(None::<f64>)
        .bind(None::<f64>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(p.notes)
        .bind(p.workspace_id)
        .bind(p.kanban_status)
        .bind(p.kanban_order)
        .bind(&tags)
        .bind("ddg")
        .bind(1_i64)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

async fn seed_totp(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    for e in &pack.totp {
        let tags = serde_json::to_string(e.tags).map_err(AppError::other)?;
        sqlx::query(
            "INSERT INTO totp_entries (id, name, issuer, secret, algorithm, digits, period, tags, created_at, updated_at)
             VALUES (?, ?, ?, ?, 'SHA1', 6, 30, ?, ?, ?)",
        )
        .bind(e.id)
        .bind(e.name)
        .bind(e.issuer)
        .bind(e.secret)
        .bind(&tags)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

#[cfg(desktop)]
async fn seed_ssh(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    use crate::commands::ssh_keys::generate_key_material;

    for k in &pack.ssh_keys {
        let bits = if k.algorithm == "rsa" { Some(2048) } else { None };
        let material = tokio::task::spawn_blocking({
            let alg = k.algorithm.to_string();
            let comment = format!("demo@{}", k.name);
            move || generate_key_material(&alg, bits, comment, None)
        })
        .await
        .map_err(|e| AppError::other(e.to_string()))??;

        sqlx::query(
            "INSERT INTO ssh_keys (
                id, name, algorithm, bits, comment,
                private_key, public_key, passphrase, fingerprint, source,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, 'generated', ?, ?)",
        )
        .bind(k.id)
        .bind(k.name)
        .bind(&material.algorithm)
        .bind(material.bits)
        .bind(&material.comment)
        .bind(&material.private_pem)
        .bind(&material.public_openssh)
        .bind(&material.fingerprint)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }

    for c in &pack.ssh_connections {
        sqlx::query(
            "INSERT INTO ssh_connections (
                id, name, host, port, username, auth_type,
                password, private_key, key_passphrase, ssh_key_id,
                requires_2fa, totp_entry_id, proxy_id,
                connect_timeout_sec, keepalive_sec, terminal_theme,
                default_cols, default_rows, created_at, updated_at
            ) VALUES (
                ?, ?, ?, ?, ?, ?,
                ?, NULL, NULL, ?,
                ?, ?, ?,
                15, 30, NULL,
                120, 32, ?, ?
            )",
        )
        .bind(c.id)
        .bind(c.name)
        .bind(c.host)
        .bind(c.port)
        .bind(c.username)
        .bind(c.auth_type)
        .bind(c.password)
        .bind(c.ssh_key_id)
        .bind(c.requires_2fa as i64)
        .bind(c.totp_entry_id)
        .bind(c.proxy_id)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

        for wid in c.workspace_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO ssh_connection_workspaces (connection_id, workspace_id) VALUES (?, ?)",
            )
            .bind(c.id)
            .bind(wid)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
        }
        for pid in c.profile_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO ssh_connection_profiles (connection_id, profile_id) VALUES (?, ?)",
            )
            .bind(c.id)
            .bind(pid)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
        }
    }
    Ok(())
}

async fn seed_tags(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    for tag in &pack.tags {
        sqlx::query(
            "INSERT INTO note_tags (id, name, color, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(tag.id)
        .bind(tag.name)
        .bind(tag.color)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

async fn seed_folders(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    for f in pack.folders.iter().filter(|f| f.parent_id.is_none()) {
        insert_folder(state, f.id, f.name, None, f.color, now).await?;
    }
    for f in pack.folders.iter().filter(|f| f.parent_id.is_some()) {
        insert_folder(state, f.id, f.name, f.parent_id, f.color, now).await?;
    }
    Ok(())
}

async fn insert_folder(
    state: &AppState,
    id: &str,
    name: &str,
    parent_id: Option<&str>,
    color: &str,
    now: &str,
) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO note_folders (id, name, parent_id, color, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(parent_id)
    .bind(color)
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

async fn seed_notes(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    // Create notes that are wiki targets first (no @@), then the rest — insert_note reindexes each time
    let mut ordered = pack.notes.clone();
    ordered.sort_by_key(|n| n.content.contains("@@"));

    let mut created: Vec<(String, String, bool, bool, bool)> = Vec::new();

    for n in &ordered {
        let note = insert_note(
            NewNote {
                id: Uuid::new_v4().to_string(),
                title: n.title.to_string(),
                format: "md".into(),
                bindings: n.bindings.iter().map(|s| s.to_string()).collect(),
                tags: n.tags.iter().map(|s| s.to_string()).collect(),
                content: n.content.to_string(),
                created_at: now.to_string(),
                updated_at: now.to_string(),
            },
            state,
        )
        .await?;

        if let Some(fid) = n.folder_id {
            sqlx::query(
                "INSERT OR IGNORE INTO note_folder_links (note_id, folder_id) VALUES (?, ?)",
            )
            .bind(&note.id)
            .bind(fid)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
        }

        created.push((note.id, note.content.unwrap_or_default(), n.pinned, n.archived, n.deleted));
    }

    // Reindex wiki links now that all titles exist
    for (id, content, _, _, _) in &created {
        if content.contains("@@") {
            crate::commands::notes::reindex_links(id, content, &state.db).await?;
        }
    }

    for (id, _, pinned, archived, deleted) in &created {
        if *pinned {
            sqlx::query("UPDATE notes SET pinned = 1 WHERE id = ?")
                .bind(id)
                .execute(&state.db)
                .await
                .map_err(AppError::db)?;
        }
        if *archived {
            sqlx::query("UPDATE notes SET archived = 1 WHERE id = ?")
                .bind(id)
                .execute(&state.db)
                .await
                .map_err(AppError::db)?;
        }
        if *deleted {
            // Match soft_delete: mark trash and drop FTS row
            let fts: Option<(Option<i64>,)> =
                sqlx::query_as("SELECT fts_rowid FROM notes WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await
                    .map_err(AppError::db)?;
            sqlx::query("UPDATE notes SET deleted = 1, updated_at = ? WHERE id = ?")
                .bind(now)
                .bind(id)
                .execute(&state.db)
                .await
                .map_err(AppError::db)?;
            if let Some((Some(rowid),)) = fts {
                let _ = sqlx::query("DELETE FROM notes_fts WHERE rowid = ?")
                    .bind(rowid)
                    .execute(&state.db)
                    .await;
            }
        }
    }

    Ok(())
}

async fn seed_smart_views(state: &AppState, pack: &DemoPack, now: &str) -> CmdResult<()> {
    for (i, v) in pack.smart_views.iter().enumerate() {
        let conditions = match v.kind {
            SmartKind::OpenTasks => NoteFilter {
                has_open_tasks: Some(true),
                archived: Some(false),
                ..Default::default()
            },
            SmartKind::Pinned => NoteFilter {
                pinned: Some(true),
                archived: Some(false),
                ..Default::default()
            },
            SmartKind::Recent7d => NoteFilter {
                updated_within_days: Some(7),
                archived: Some(false),
                ..Default::default()
            },
            SmartKind::HasAttachments => NoteFilter {
                has_attachments: Some(true),
                archived: Some(false),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&conditions).map_err(AppError::other)?;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO note_smart_views (id, name, color, conditions, sort_order, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(v.name)
        .bind(v.color)
        .bind(&json)
        .bind(i as i64)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

#[cfg(desktop)]
async fn seed_capture_rules(state: &AppState, pack: &DemoPack) -> CmdResult<()> {
    use crate::commands::notes::CaptureRule;

    let rules: Vec<CaptureRule> = pack
        .capture_rules
        .iter()
        .map(|r| CaptureRule {
            domain: r.domain.to_string(),
            folder_id: Some(r.folder_id.to_string()),
            tags: r.tags.iter().map(|s| s.to_string()).collect(),
            template_id: None,
        })
        .collect();
    let json = serde_json::to_string(&rules).map_err(AppError::other)?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('notes_capture_rules', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}
