// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! App-level UI settings: tray behavior + tray menu labels.

use crate::error::{AppError, CmdResult};
use crate::tray::{self, TrayLabels};
use crate::AppState;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::sync::atomic::Ordering;
use tauri::Manager;

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
    // In minimize-to-tray mode the window lives only in the tray, so drop its
    // taskbar entry; restore it when the mode is off.
    tray::apply_taskbar_async(&app, minimize_to_tray);
    Ok(())
}

/// Minimize action for the custom titlebar's "–" button. With minimize-to-tray
/// on, the window is hidden to the tray (leaves the taskbar) instead of being
/// iconified — now possible because it's our button, not the OS decoration.
#[tauri::command]
pub async fn window_minimize(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let to_tray = state.tray_settings.minimize_to_tray.load(Ordering::Relaxed);
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window("main") {
            if to_tray {
                let _ = w.hide();
            } else {
                let _ = w.minimize();
            }
        }
    })
    .map_err(AppError::other)?;
    Ok(())
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
