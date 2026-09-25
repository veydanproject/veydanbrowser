// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Demo catalog seed + full data wipe for video recordings.

mod bulk;
mod clear;
mod content;
mod seed;

use crate::error::{AppError, CmdResult};
use crate::AppState;
use std::time::Duration;

const SYNC_WAIT: Duration = Duration::from_secs(30);

/// Wipe user catalog (profiles, notes, totp, ssh, proxies, extra workspaces).
#[tauri::command]
pub async fn app_clear_data(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let guard = state.sync.pause(SYNC_WAIT).await;
    if guard.is_none() {
        return Err(AppError::other("sync is running, try again"));
    }
    let result = clear::clear_catalog(&state).await;
    drop(guard);
    crate::sync::trigger_cycle(&app, "clear_data");
    result
}

/// Wipe catalog then load bilingual demo data (`ru` or `en`).
/// Runs with the sync slot held, then pushes right away so the fresh rows
/// carry a newer HLC than any tombstone another device may still publish.
#[tauri::command]
pub async fn demo_seed(
    locale: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    let guard = state.sync.pause(SYNC_WAIT).await;
    if guard.is_none() {
        return Err(AppError::other("sync is running, try again"));
    }
    let result = async {
        clear::clear_catalog(&state).await?;
        seed::seed_catalog(&state, &locale).await
    }
    .await;
    drop(guard);
    crate::sync::trigger_cycle(&app, "demo_seed");
    result
}
