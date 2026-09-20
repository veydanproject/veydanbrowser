// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::browser::launch as browser_launch;
use crate::browser::profile_launcher;
use crate::error::{AppError, CmdResult};
use crate::models::Profile;
use crate::AppState;

pub async fn emit_running_ids(app_handle: &tauri::AppHandle, state: &AppState) {
    let ids = state.browser.running_ids().await;
    browser_launch::emit_running_changed(app_handle, ids);
}

/// `force` launches even when another device holds the sync lease.
#[tauri::command]
pub async fn profile_launch(
    id: String,
    force: Option<bool>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> CmdResult<u32> {
    if state.browser.is_running(&id).await {
        return Err(AppError::other("Profile is already running"));
    }
    crate::sync::before_profile_launch(&app_handle, &id, force.unwrap_or(false)).await?;

    let profile = sqlx::query_as::<_, Profile>("SELECT * FROM profiles WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Profile not found"))?;

    let proxy = if let Some(proxy_id) = &profile.proxy_id {
        sqlx::query_as::<_, crate::models::Proxy>("SELECT * FROM proxies WHERE id = ?")
            .bind(proxy_id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?
    } else {
        None
    };

    sqlx::query(
        "UPDATE profiles SET status = 'running', last_launch_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    let result =
        profile_launcher::launch_profile(&profile, proxy.as_ref(), &state, app_handle.clone())
            .await;

    if result.is_err() {
        // Rollback status if launch failed
        let _ = sqlx::query(
            "UPDATE profiles SET status = 'stopped', updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&id)
        .execute(&state.db)
        .await;
        crate::sync::on_profile_stopped(&app_handle, &id).await;
    }

    // Always emit so frontend reflects actual running state (empty on failure)
    emit_running_ids(&app_handle, &state).await;

    Ok(result.map_err(AppError::browser)?.pid)
}

#[tauri::command]
pub async fn profile_stop(
    id: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> CmdResult<()> {
    browser_launch::stop(&id, &state.browser)
        .await
        .map_err(AppError::browser)?;
    // Write stopped status immediately so workspace_stats reflects reality
    sqlx::query(
        "UPDATE profiles SET status = 'stopped', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    emit_running_ids(&app_handle, &state).await;
    Ok(())
}

#[tauri::command]
pub async fn profile_is_running(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<bool> {
    Ok(state.browser.is_running(&id).await)
}

#[tauri::command]
pub async fn profiles_running_ids(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    Ok(state.browser.running_ids().await)
}
