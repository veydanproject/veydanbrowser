// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Password entries. Secrets are encrypted before they touch SQLite.

use crate::commands::notes::require_lock_password;
use crate::error::{AppError, CmdResult};
use crate::vault::{self, SecretKey};
use crate::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(FromRow)]
struct PasswordRow {
    id: String,
    title: String,
    username: Option<String>,
    url: Option<String>,
    password_enc: String,
    note_enc: Option<String>,
    /// JSON list of linked TOTP entry ids
    totp_ids: String,
    tags: String,
    vault_id: String,
    created_at: String,
    updated_at: String,
}

/// Metadata only. Ciphertext never leaves the backend through list/get.
#[derive(Serialize)]
pub struct PasswordPublic {
    pub id: String,
    pub title: String,
    pub username: Option<String>,
    pub url: Option<String>,
    pub has_note: bool,
    pub totp_ids: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct RevealedSecret {
    pub value: String,
}

#[derive(Deserialize)]
pub struct PasswordCreate {
    pub title: String,
    pub username: Option<String>,
    pub url: Option<String>,
    pub password: String,
    pub note: Option<String>,
    pub totp_ids: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct PasswordUpdate {
    pub title: Option<String>,
    pub username: Option<String>,
    pub url: Option<String>,
    pub password: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub clear_note: bool,
    pub totp_ids: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

fn blank(value: Option<String>) -> Option<String> {
    value.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn parse_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Trimmed, non-empty, unique ids in the given order.
fn clean_ids(ids: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let id = id.trim().to_string();
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn to_public(row: &PasswordRow) -> PasswordPublic {
    PasswordPublic {
        id: row.id.clone(),
        title: row.title.clone(),
        username: row.username.clone(),
        url: row.url.clone(),
        has_note: row.note_enc.as_ref().is_some_and(|s| !s.is_empty()),
        totp_ids: parse_list(&row.totp_ids),
        tags: parse_list(&row.tags),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    }
}

async fn load(state: &AppState, id: &str) -> Result<PasswordRow, AppError> {
    sqlx::query_as::<_, PasswordRow>("SELECT * FROM passwords WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("password"))
}

async fn totp_all_exist(state: &AppState, ids: &[String]) -> Result<(), AppError> {
    for id in ids {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM totp_entries WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await
            .map_err(AppError::db)?;
        if n == 0 {
            return Err(AppError::not_found("TOTP"));
        }
    }
    Ok(())
}

fn encrypt_secret(key: &SecretKey, id: &str, field: &str, plaintext: &str) -> Result<String, AppError> {
    vault::encrypt_field(key, id, field, plaintext)
}

async fn open_key(state: &AppState) -> Result<(SecretKey, String), AppError> {
    // SecretKey is crate-private inside vault; require_open returns it.
    vault::require_open(state)
}

#[tauri::command]
pub async fn password_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<PasswordPublic>> {
    let rows = sqlx::query_as::<_, PasswordRow>("SELECT * FROM passwords ORDER BY updated_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(rows.iter().map(to_public).collect())
}

#[tauri::command]
pub async fn password_get(id: String, state: tauri::State<'_, AppState>) -> CmdResult<PasswordPublic> {
    let row = load(&state, &id).await?;
    Ok(to_public(&row))
}

#[tauri::command]
pub async fn password_create(
    req: PasswordCreate,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PasswordPublic> {
    let title = req.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::other("Title is required"));
    }
    if req.password.is_empty() {
        return Err(AppError::other("Password cannot be empty"));
    }
    let totp_ids = clean_ids(req.totp_ids.unwrap_or_default());
    totp_all_exist(&state, &totp_ids).await?;
    let (key, vault_id) = vault::ensure_key(&state).await?;
    // A row created here must tell peers which secret wraps it.
    crate::commands::notes::publish_lock_meta(&state).await?;
    let id = Uuid::new_v4().to_string();
    let password_enc = encrypt_secret(&key, &id, "password", &req.password)?;
    let note_enc = match blank(req.note) {
        Some(note) => Some(encrypt_secret(&key, &id, "note", &note)?),
        None => None,
    };
    let tags = serde_json::to_string(&req.tags.unwrap_or_default())?;
    let totp_json = serde_json::to_string(&totp_ids)?;
    let now = Utc::now().to_rfc3339();
    let username = blank(req.username);
    let url = blank(req.url);
    sqlx::query(
        "INSERT INTO passwords (
            id, title, username, url, password_enc, note_enc, totp_ids, tags, vault_id, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&title)
    .bind(&username)
    .bind(&url)
    .bind(&password_enc)
    .bind(&note_enc)
    .bind(&totp_json)
    .bind(&tags)
    .bind(&vault_id)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(PasswordPublic {
        id,
        title,
        username,
        url,
        has_note: note_enc.is_some(),
        totp_ids,
        tags: parse_list(&tags),
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub async fn password_update(
    id: String,
    req: PasswordUpdate,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PasswordPublic> {
    let (key, vault_id) = open_key(&state).await?;
    let mut row = load(&state, &id).await?;
    if let Some(title) = req.title {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::other("Title is required"));
        }
        row.title = title;
    }
    if let Some(username) = req.username {
        row.username = blank(Some(username));
    }
    if let Some(url) = req.url {
        row.url = blank(Some(url));
    }
    if let Some(ids) = req.totp_ids {
        let ids = clean_ids(ids);
        totp_all_exist(&state, &ids).await?;
        row.totp_ids = serde_json::to_string(&ids)?;
    }
    if let Some(tags) = req.tags {
        row.tags = serde_json::to_string(&tags)?;
    }

    let secrets = req.password.is_some() || req.note.is_some() || req.clear_note;
    if secrets {
        if row.vault_id != vault_id {
            return Err(AppError::DecryptFailed);
        }
        let password = if let Some(password) = req.password {
            if password.is_empty() {
                return Err(AppError::other("Password cannot be empty"));
            }
            password
        } else {
            vault::decrypt_field(&key, &row.id, "password", &row.password_enc)?
        };
        let note = if req.clear_note {
            None
        } else if let Some(note) = req.note {
            blank(Some(note))
        } else if let Some(stored) = &row.note_enc {
            blank(Some(vault::decrypt_field(&key, &row.id, "note", stored)?))
        } else {
            None
        };
        row.password_enc = encrypt_secret(&key, &row.id, "password", &password)?;
        row.note_enc = match note {
            Some(note) => Some(encrypt_secret(&key, &row.id, "note", &note)?),
            None => None,
        };
        row.vault_id = vault_id.clone();
    }

    row.updated_at = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE passwords
         SET title = ?, username = ?, url = ?, password_enc = ?, note_enc = ?,
             totp_ids = ?, tags = ?, vault_id = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&row.title)
    .bind(&row.username)
    .bind(&row.url)
    .bind(&row.password_enc)
    .bind(&row.note_enc)
    .bind(&row.totp_ids)
    .bind(&row.tags)
    .bind(&row.vault_id)
    .bind(&row.updated_at)
    .bind(&row.id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(to_public(&row))
}

/// Metadata only; no vault key needed.
#[tauri::command]
pub async fn password_delete(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let n = sqlx::query("DELETE FROM passwords WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    if n.rows_affected() == 0 {
        return Err(AppError::not_found("password"));
    }
    Ok(())
}

#[tauri::command]
pub async fn password_reveal(
    id: String,
    field: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<RevealedSecret> {
    let (key, vault_id) = open_key(&state).await?;
    let row = load(&state, &id).await?;
    if row.vault_id != vault_id {
        return Err(AppError::DecryptFailed);
    }
    let value = match field.as_str() {
        "password" => vault::decrypt_field(&key, &row.id, "password", &row.password_enc)?,
        "note" => match &row.note_enc {
            Some(stored) => vault::decrypt_field(&key, &row.id, "note", stored)?,
            None => String::new(),
        },
        _ => return Err(AppError::other("Unknown field")),
    };
    Ok(RevealedSecret { value })
}

#[cfg(desktop)]
#[tauri::command]
pub async fn password_copy(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let revealed = password_reveal(id, "password".into(), state).await?;
    let secret = revealed.value;
    let to_write = secret.clone();
    tokio::task::spawn_blocking(move || {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(to_write).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| AppError::other(e.to_string()))?
    .map_err(AppError::other)?;

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let _ = tokio::task::spawn_blocking(move || {
            let mut clipboard = arboard::Clipboard::new().ok()?;
            if clipboard.get_text().ok()? == secret {
                let _ = clipboard.set_text(String::new());
            }
            Some(())
        })
        .await;
    });
    Ok(())
}

/// Delete every entry and the vault row. The lock password stays; a new vault is staged.
#[tauri::command]
pub async fn password_vault_reset(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    require_lock_password(&state, &password).await?;
    let mut tx = state.db.begin().await.map_err(AppError::db)?;
    sqlx::query("DELETE FROM passwords")
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;
    sqlx::query("DELETE FROM password_vault")
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;
    tx.commit().await.map_err(AppError::db)?;
    state.vault.lock();
    vault::open_with_password(&state, &password).await?;
    // Recreate the row at once so peers see a rewrap, not a vanished lock.
    vault::ensure_key(&state).await?;
    crate::commands::notes::publish_lock_meta(&state).await?;
    state.notes_lock.touch();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::SecretKey;

    const SECRET: &str = "VEYDAN_TEST_SECRET_72FC194";
    const SYNC_SECRET: &str = "VEYDAN_SYNC_SECRET_91AB";

    #[tokio::test]
    async fn plaintext_absent_from_db_and_wal() {
        let dir = std::env::temp_dir().join(format!("veydan-pw-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE passwords (
                id TEXT PRIMARY KEY NOT NULL,
                password_enc TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let key = SecretKey::random();
        let id = "entry-1";
        let enc = vault::encrypt_field(&key, id, "password", SECRET).unwrap();
        assert!(!enc.contains(SECRET));
        sqlx::query("INSERT INTO passwords (id, password_enc) VALUES (?, ?)")
            .bind(id)
            .bind(&enc)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA wal_checkpoint(FULL)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let db_bytes = std::fs::read(&db_path).unwrap();
        assert!(!bytes_contain(&db_bytes, SECRET.as_bytes()));
        let wal_path = format!("{}-wal", db_path.display());
        if let Ok(wal) = std::fs::read(&wal_path) {
            assert!(!bytes_contain(&wal, SECRET.as_bytes()));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_row_holds_ciphertext_only() {
        let key = SecretKey::random();
        let enc = vault::encrypt_field(&key, "entry", "password", SYNC_SECRET).unwrap();
        let row = serde_json::json!({
            "id": "entry",
            "title": "GitHub",
            "password_enc": enc,
        });
        let serialized = row.to_string();
        assert!(!serialized.contains(SYNC_SECRET));
        assert!(serialized.contains(&enc));
    }

    fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
