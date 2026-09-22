// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Demo catalog seed + full data wipe for video recordings.

mod bulk;
mod clear;
mod content;
mod seed;

use crate::error::CmdResult;
use crate::AppState;

/// Wipe user catalog (profiles, notes, totp, ssh, proxies, extra workspaces).
#[tauri::command]
pub async fn app_clear_data(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    clear::clear_catalog(&state).await
}

/// Wipe catalog then load bilingual demo data (`ru` or `en`).
#[tauri::command]
pub async fn demo_seed(locale: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    clear::clear_catalog(&state).await?;
    seed::seed_catalog(&state, &locale).await
}
