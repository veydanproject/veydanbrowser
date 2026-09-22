// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::models::{
    BulkImportResult, BulkImportRowResult, BulkProxyItem, CreateProxyRequest, Proxy,
    ProxyCheckResult,
};
use crate::proxy::check;
use crate::AppState;
use chrono::Utc;
use std::collections::HashSet;
use uuid::Uuid;

#[tauri::command]
pub async fn proxies_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Proxy>> {
    sqlx::query_as::<_, Proxy>("SELECT * FROM proxies ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)
}

#[tauri::command]
pub async fn proxy_get(id: String, state: tauri::State<'_, AppState>) -> CmdResult<Option<Proxy>> {
    sqlx::query_as::<_, Proxy>("SELECT * FROM proxies WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)
}

#[tauri::command]
pub async fn proxy_create(
    req: CreateProxyRequest,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Proxy> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let tags_json = serde_json::to_string(&req.tags.unwrap_or_default()).map_err(AppError::other)?;

    sqlx::query(
        "INSERT INTO proxies
        (id, name, proxy_type, host, port, username, password, country, city, status, tags, private_key, created_at)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&req.name)
    .bind(&req.proxy_type)
    .bind(&req.host)
    .bind(req.port)
    .bind(&req.username)
    .bind(&req.password)
    .bind(&req.country)
    .bind(&req.city)
    .bind("unknown")
    .bind(&tags_json)
    .bind(&req.private_key)
    .bind(now.to_rfc3339())
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    proxy_get(id, state)
        .await
        .and_then(|p| p.ok_or_else(|| AppError::not_found("Proxy not found after create")))
}

const BULK_ALLOWED_TYPES: &[&str] = &["http", "https", "socks5"];

fn validate_bulk_item(item: &BulkProxyItem) -> Result<(), String> {
    if !BULK_ALLOWED_TYPES.contains(&item.proxy_type.as_str()) {
        return Err(format!("Unsupported proxy type: {}", item.proxy_type));
    }
    let host = item.host.trim();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err("Invalid host".into());
    }
    if !(1..=65535).contains(&item.port) {
        return Err(format!("Invalid port: {}", item.port));
    }
    Ok(())
}

fn dedup_key(proxy_type: &str, host: &str, port: i64, username: Option<&str>) -> (String, String, i64, String) {
    (
        proxy_type.to_string(),
        host.trim().to_lowercase(),
        port,
        username.unwrap_or("").to_string(),
    )
}

#[tauri::command]
pub async fn proxies_bulk_create(
    items: Vec<BulkProxyItem>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<BulkImportResult> {
    bulk_create_impl(&state.db, items).await
}

async fn bulk_create_impl(
    db: &sqlx::Pool<sqlx::Sqlite>,
    items: Vec<BulkProxyItem>,
) -> CmdResult<BulkImportResult> {
    let existing: Vec<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT proxy_type, LOWER(host), port, username FROM proxies",
    )
    .fetch_all(db)
    .await
    .map_err(AppError::db)?;

    let mut seen: HashSet<(String, String, i64, String)> = existing
        .into_iter()
        .map(|(t, h, p, u)| dedup_key(&t, &h, p, u.as_deref()))
        .collect();

    let mut tx = db.begin().await.map_err(AppError::db)?;
    let mut rows: Vec<BulkImportRowResult> = Vec::with_capacity(items.len());
    let mut imported_ids: Vec<String> = Vec::new();
    let now = Utc::now().to_rfc3339();

    for item in &items {
        if let Err(msg) = validate_bulk_item(item) {
            rows.push(BulkImportRowResult {
                line_number: item.line_number,
                status: "error".into(),
                message: Some(msg),
                id: None,
            });
            continue;
        }

        let key = dedup_key(&item.proxy_type, &item.host, item.port, item.username.as_deref());
        if seen.contains(&key) {
            rows.push(BulkImportRowResult {
                line_number: item.line_number,
                status: "duplicate".into(),
                message: None,
                id: None,
            });
            continue;
        }
        seen.insert(key);

        let id = Uuid::new_v4().to_string();
        let host = item.host.trim();
        sqlx::query(
            "INSERT INTO proxies
            (id, name, proxy_type, host, port, username, password, country, city, status, tags, private_key, created_at)
            VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(format!("{}:{}", host, item.port))
        .bind(&item.proxy_type)
        .bind(host)
        .bind(item.port)
        .bind(&item.username)
        .bind(&item.password)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind("unknown")
        .bind("[]")
        .bind(None::<String>)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;

        rows.push(BulkImportRowResult {
            line_number: item.line_number,
            status: "imported".into(),
            message: None,
            id: Some(id.clone()),
        });
        imported_ids.push(id);
    }

    tx.commit().await.map_err(AppError::db)?;

    let mut imported: Vec<Proxy> = Vec::with_capacity(imported_ids.len());
    for id in &imported_ids {
        let proxy = sqlx::query_as::<_, Proxy>("SELECT * FROM proxies WHERE id = ?")
            .bind(id)
            .fetch_optional(db)
            .await
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Proxy not found after import"))?;
        imported.push(proxy);
    }

    Ok(BulkImportResult { rows, imported })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(proxy_type: &str, host: &str, port: i64) -> BulkProxyItem {
        BulkProxyItem {
            line_number: 1,
            proxy_type: proxy_type.into(),
            host: host.into(),
            port,
            username: None,
            password: None,
        }
    }

    #[test]
    fn validate_accepts_supported_types() {
        for t in ["http", "https", "socks5"] {
            assert!(validate_bulk_item(&item(t, "1.2.3.4", 8080)).is_ok());
        }
    }

    #[test]
    fn validate_rejects_ssh_and_unknown_types() {
        assert!(validate_bulk_item(&item("ssh", "1.2.3.4", 22)).is_err());
        assert!(validate_bulk_item(&item("socks4", "1.2.3.4", 1080)).is_err());
    }

    #[test]
    fn validate_rejects_bad_port() {
        assert!(validate_bulk_item(&item("http", "1.2.3.4", 0)).is_err());
        assert!(validate_bulk_item(&item("http", "1.2.3.4", 65536)).is_err());
        assert!(validate_bulk_item(&item("http", "1.2.3.4", 65535)).is_ok());
    }

    #[test]
    fn validate_rejects_bad_host() {
        assert!(validate_bulk_item(&item("http", "", 8080)).is_err());
        assert!(validate_bulk_item(&item("http", "ho st", 8080)).is_err());
    }

    #[test]
    fn dedup_key_is_case_insensitive_on_host() {
        assert_eq!(
            dedup_key("http", "Example.COM", 8080, Some("u")),
            dedup_key("http", "example.com", 8080, Some("u"))
        );
        assert_ne!(
            dedup_key("http", "example.com", 8080, None),
            dedup_key("socks5", "example.com", 8080, None)
        );
    }

    async fn test_pool() -> sqlx::Pool<sqlx::Sqlite> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE proxies (
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
                workspace_id TEXT,
                private_key TEXT,
                server_fingerprint TEXT,
                tags TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn full_item(line: i64, host: &str, port: i64, user: Option<&str>) -> BulkProxyItem {
        BulkProxyItem {
            line_number: line,
            proxy_type: "http".into(),
            host: host.into(),
            port,
            username: user.map(String::from),
            password: user.map(|_| "pw".to_string()),
        }
    }

    #[tokio::test]
    async fn bulk_create_imports_dedups_and_reports_errors() {
        let pool = test_pool().await;

        let result = bulk_create_impl(
            &pool,
            vec![
                full_item(1, "1.2.3.4", 8080, None),
                full_item(2, "1.2.3.4", 8080, None),        // dup within batch
                full_item(3, "1.2.3.4", 8080, Some("user")), // different username -> new
                full_item(4, "bad host", 8080, None),        // error
                full_item(5, "5.6.7.8", 0, None),            // error: port
            ],
        )
        .await
        .unwrap();

        let statuses: Vec<&str> = result.rows.iter().map(|r| r.status.as_str()).collect();
        assert_eq!(statuses, ["imported", "duplicate", "imported", "error", "error"]);
        assert_eq!(result.imported.len(), 2);
        assert_eq!(result.imported[0].name, "1.2.3.4:8080");
        assert_eq!(result.imported[0].status, "unknown");

        // Re-import: everything already in DB → duplicates (case-insensitive host)
        let again = bulk_create_impl(
            &pool,
            vec![
                full_item(1, "1.2.3.4", 8080, None),
                full_item(2, "1.2.3.4", 8080, Some("user")),
            ],
        )
        .await
        .unwrap();
        assert!(again.rows.iter().all(|r| r.status == "duplicate"));
        assert!(again.imported.is_empty());

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM proxies")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 2);
    }
}

/// Update a proxy. Field contract (matches ProxyPanel.svelte, which pre-fills
/// the edit form from the stored proxy and submits the complete state):
///
/// * `name`, `proxy_type`, `host`, `port`, `tags` are required and replaced.
/// * `username`, `country`, `city` are bound raw: null clears the column
///   (the UI sends the full state each save, so null means "no value").
/// * Credentials (`password`, `private_key`) use COALESCE — null means "keep
///   the stored secret", so a caller omitting them can never wipe credentials
///   by accident. Clearing them intentionally still works: an empty string
///   from the UI is stored as NULL.
#[tauri::command]
pub async fn proxy_update(
    id: String,
    req: CreateProxyRequest,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Proxy> {
    let tags_json = serde_json::to_string(&req.tags.unwrap_or_default()).map_err(AppError::other)?;

    sqlx::query(
        "UPDATE proxies SET
            name = ?, proxy_type = ?, host = ?, port = ?,
            username = ?, password = NULLIF(COALESCE(?, password), ''),
            country = ?, city = ?,
            private_key = NULLIF(COALESCE(?, private_key), ''), tags = ?
        WHERE id = ?",
    )
    .bind(&req.name)
    .bind(&req.proxy_type)
    .bind(&req.host)
    .bind(req.port)
    .bind(&req.username)
    .bind(&req.password)
    .bind(&req.country)
    .bind(&req.city)
    .bind(&req.private_key)
    .bind(&tags_json)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    proxy_get(id, state)
        .await
        .and_then(|p| p.ok_or_else(|| AppError::not_found("Proxy not found after update")))
}

/// TOFU pin for an SSH proxy after a successful connect made outside `proxy_check`
/// (profile launch, jump host). Only fills an empty slot; never overwrites.
pub async fn pin_ssh_fingerprint(db: &sqlx::SqlitePool, proxy: &Proxy, result: &crate::proxy::ssh::SshConnectResult) {
    if !result.is_new || proxy.server_fingerprint.is_some() {
        return;
    }
    if let Err(e) = sqlx::query("UPDATE proxies SET server_fingerprint = ? WHERE id = ? AND server_fingerprint IS NULL")
        .bind(&result.fingerprint)
        .bind(&proxy.id)
        .execute(db)
        .await
    {
        eprintln!("[proxy] failed to pin host key for {}: {e}", proxy.id);
    }
}

#[tauri::command]
pub async fn proxy_delete(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    sqlx::query("UPDATE profiles SET proxy_id = NULL WHERE proxy_id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    sqlx::query("DELETE FROM proxies WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    Ok(())
}

#[tauri::command]
pub async fn proxy_check(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<ProxyCheckResult> {
    let proxy = sqlx::query_as::<_, Proxy>("SELECT * FROM proxies WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Proxy not found"))?;

    let result = check::check_proxy(&proxy).await.map_err(AppError::proxy)?;

    // For SSH proxies with a new fingerprint — do NOT save status yet,
    // the frontend must prompt the user to confirm the fingerprint first.
    if proxy.proxy_type == "ssh" && result.ssh_fingerprint_is_new == Some(true) {
        return Ok(result);
    }

    let status = if result.ok { "active" } else { "error" };
    sqlx::query(
        "UPDATE proxies SET
            status = ?,
            last_ip = ?,
            last_check_at = datetime('now'),
            country = CASE WHEN (country IS NULL OR country = '') THEN ? ELSE country END,
            city    = CASE WHEN (city IS NULL OR city = '')       THEN ? ELSE city    END
        WHERE id = ?",
    )
    .bind(status)
    .bind(&result.ip)
    .bind(&result.country)
    .bind(&result.city)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(result)
}

/// Save a trusted SSH server fingerprint (TOFU: user confirmed the fingerprint in UI).
/// Also marks the proxy as active and stores IP/country/city from the check result.
#[tauri::command]
pub async fn proxy_trust_fingerprint(
    id: String,
    fingerprint: String,
    ip: String,
    country: Option<String>,
    city: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    sqlx::query(
        "UPDATE proxies SET
            server_fingerprint = ?,
            status = 'active',
            last_ip = ?,
            last_check_at = datetime('now'),
            country = CASE WHEN (country IS NULL OR country = '') THEN ? ELSE country END,
            city    = CASE WHEN (city IS NULL OR city = '')       THEN ? ELSE city    END
        WHERE id = ?",
    )
    .bind(&fingerprint)
    .bind(&ip)
    .bind(&country)
    .bind(&city)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}
