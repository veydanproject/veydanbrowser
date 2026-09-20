// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Quick capture: a small always-on-top window opened from a global shortcut or the tray.

use crate::error::{AppError, CmdResult};
use crate::AppState;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const WINDOW_LABEL: &str = "quick-capture";
const SETTING_KEY: &str = "quick_capture_shortcut";
pub const DEFAULT_SHORTCUT: &str = "CmdOrCtrl+Shift+N";

/// Show the quick capture window, creating it on first use. Safe from any thread.
pub fn show_quick_capture(app: &tauri::AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window(WINDOW_LABEL) {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
            let _ = w.emit("quick-capture://shown", ());
            return;
        }
        let mut builder = WebviewWindowBuilder::new(&app2, WINDOW_LABEL, WebviewUrl::App("/notes/quick".into()))
            .title("Quick capture")
            .inner_size(480.0, 340.0)
            .min_inner_size(360.0, 240.0)
            .always_on_top(true)
            .skip_taskbar(true)
            .center();
        #[cfg(target_os = "linux")]
        {
            builder = builder.decorations(false).transparent(true);
        }
        if let Err(e) = builder.build() {
            eprintln!("[quick-capture] failed to open window: {e}");
        }
    });
}

#[tauri::command]
pub async fn open_quick_capture(app: tauri::AppHandle) -> CmdResult<()> {
    show_quick_capture(&app);
    Ok(())
}

async fn load_shortcut(state: &AppState) -> String {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(SETTING_KEY)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_SHORTCUT.to_string())
}

/// Replace the registered global shortcut; empty string disables it.
fn apply_shortcut(app: &tauri::AppHandle, accelerator: &str) -> Result<(), AppError> {
    let gs = app.global_shortcut();
    gs.unregister_all().map_err(AppError::other)?;
    let accelerator = accelerator.trim();
    if accelerator.is_empty() {
        return Ok(());
    }
    let shortcut: Shortcut = accelerator.parse().map_err(|e| AppError::other(format!("Invalid shortcut: {e}")))?;
    gs.on_shortcut(shortcut, |app, _sc, event| {
        if event.state == ShortcutState::Pressed {
            show_quick_capture(app);
        }
    })
    .map_err(AppError::other)
}

/// Called once at startup with the persisted accelerator.
pub fn register_quick_capture_shortcut(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let accelerator = tauri::async_runtime::block_on(load_shortcut(&state));
    if let Err(e) = apply_shortcut(app, &accelerator) {
        eprintln!("[quick-capture] shortcut registration failed: {e}");
    }
}

/// Re-register after the persisted accelerator changed outside this process (sync).
pub async fn reapply_quick_capture_shortcut(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let accelerator = load_shortcut(&state).await;
    if let Err(e) = apply_shortcut(app, &accelerator) {
        eprintln!("[quick-capture] shortcut registration failed: {e}");
    }
}

#[tauri::command]
pub async fn quick_capture_shortcut_get(state: tauri::State<'_, AppState>) -> CmdResult<String> {
    Ok(load_shortcut(&state).await)
}

#[tauri::command]
pub async fn quick_capture_shortcut_set(
    accelerator: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<String> {
    let accelerator = accelerator.trim().to_string();
    apply_shortcut(&app, &accelerator)?;
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(SETTING_KEY)
    .bind(&accelerator)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(accelerator)
}
