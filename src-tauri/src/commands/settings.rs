// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! App-level UI settings: tray behavior + tray menu labels.

use crate::error::{AppError, CmdResult};
use crate::tray::{self, TrayLabels};
use crate::AppState;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::sync::atomic::Ordering;

#[derive(Debug, Serialize, Deserialize)]
pub struct TraySettings {
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub start_hidden: bool,
}

async fn persist_bool(db: &Pool<Sqlite>, key: &str, val: bool) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(if val { "1" } else { "0" })
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn tray_settings_get(state: tauri::State<'_, AppState>) -> CmdResult<TraySettings> {
    Ok(TraySettings {
        minimize_to_tray: state.tray_settings.minimize_to_tray.load(Ordering::Relaxed),
        close_to_tray: state.tray_settings.close_to_tray.load(Ordering::Relaxed),
        start_hidden: state.tray_settings.start_hidden.load(Ordering::Relaxed),
    })
}

#[tauri::command]
pub async fn tray_settings_set(
    minimize_to_tray: bool,
    close_to_tray: bool,
    start_hidden: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    persist_bool(&state.db, "minimize_to_tray", minimize_to_tray).await?;
    persist_bool(&state.db, "close_to_tray", close_to_tray).await?;
    persist_bool(&state.db, "start_hidden", start_hidden).await?;

    state
        .tray_settings
        .minimize_to_tray
        .store(minimize_to_tray, Ordering::Relaxed);
    state
        .tray_settings
        .close_to_tray
        .store(close_to_tray, Ordering::Relaxed);
    state
        .tray_settings
        .start_hidden
        .store(start_hidden, Ordering::Relaxed);

    let want_tray = minimize_to_tray || close_to_tray || start_hidden;
    tray::apply_tray_async(&app, want_tray);
    // Visible window stays in the taskbar; skip only while actually stashed.
    tray::sync_taskbar_to_visibility(&app);
    Ok(())
}

/// Re-read the tray flags from `app_settings` (after sync applied them) and update the tray.
pub async fn reload_tray_settings(app: &tauri::AppHandle, state: &AppState) {
    let mut want_tray = false;
    for (key, flag) in [
        ("minimize_to_tray", &state.tray_settings.minimize_to_tray),
        ("close_to_tray", &state.tray_settings.close_to_tray),
        ("start_hidden", &state.tray_settings.start_hidden),
    ] {
        let on = sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .map(|v| v == "1")
            .unwrap_or(false);
        flag.store(on, Ordering::Relaxed);
        want_tray |= on;
    }
    tray::apply_tray_async(app, want_tray);
    tray::sync_taskbar_to_visibility(app);
}

/// Minimize action for the custom titlebar's "–" button. With minimize-to-tray
/// on, the main window is hidden to the tray (leaves the taskbar) instead of
/// being iconified. Other windows (e.g. notes) always minimize themselves.
#[tauri::command]
pub async fn window_minimize(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let to_tray = state.tray_settings.minimize_to_tray.load(Ordering::Relaxed);
    if to_tray && window.label() == "main" {
        tray::hide_to_tray(&app);
        return Ok(());
    }
    let win = window.clone();
    app.run_on_main_thread(move || {
        let _ = win.minimize();
    })
    .map_err(AppError::other)?;
    Ok(())
}

const UI_LOCALE_KEY: &str = "ui_locale";

/// Persist the UI language so non-frontend consumers (browser extension) can follow it.
#[tauri::command]
pub async fn app_locale_set(locale: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let locale = match locale.as_str() {
        "ru" => "ru",
        _ => "en",
    };
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(UI_LOCALE_KEY)
    .bind(locale)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn app_locale_get(state: tauri::State<'_, AppState>) -> CmdResult<String> {
    Ok(app_locale(&state.db).await)
}

/// Stored UI language, `en` when never set.
pub async fn app_locale(db: &Pool<Sqlite>) -> String {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(UI_LOCALE_KEY)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "en".to_string())
}

/// Hand the tray the active locale's menu strings and refresh it.
#[tauri::command]
pub async fn tray_set_labels(
    labels: TrayLabels,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    // A poisoned lock would otherwise panic every future call; surface it as an error.
    *state
        .tray_labels
        .lock()
        .map_err(|e| AppError::other(e.to_string()))? = labels;
    tray::refresh_tray_async(&app);
    Ok(())
}
