// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::models::{CreateProxyRequest, Proxy, ProxyCheckResult};
use crate::proxy::check;
use crate::AppState;
use chrono::Utc;
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
            username = ?, password = ?, country = ?, city = ?, private_key = ?, tags = ?
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

    sqlx::query(
        "UPDATE proxies SET
            status = 'active',
            last_ip = ?,
            last_check_at = datetime('now'),
            country = CASE WHEN (country IS NULL OR country = '') THEN ? ELSE country END,
            city    = CASE WHEN (city IS NULL OR city = '')       THEN ? ELSE city    END
        WHERE id = ?",
    )
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
