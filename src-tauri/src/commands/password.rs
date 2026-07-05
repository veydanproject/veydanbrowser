// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct PwEntry {
    pub id: String,
    pub password: String,
    pub created_at: String,
}

#[tauri::command]
pub async fn pwgen_history_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<PwEntry>> {
    sqlx::query_as::<_, PwEntry>("SELECT * FROM password_history ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)
}

#[tauri::command]
pub async fn pwgen_history_add(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PwEntry> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    sqlx::query("INSERT INTO password_history (id, password, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(&password)
        .bind(&created_at)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

    Ok(PwEntry {
        id,
        password,
        created_at,
    })
}

#[tauri::command]
pub async fn pwgen_history_clear(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    sqlx::query("DELETE FROM password_history")
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn pwgen_history_trim(limit: i64, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    sqlx::query(
        "DELETE FROM password_history WHERE id NOT IN (
            SELECT id FROM password_history ORDER BY created_at DESC LIMIT ?
        )",
    )
    .bind(limit)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}
