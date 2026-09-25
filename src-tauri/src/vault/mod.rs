// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Password vault: one random key wrapped by the notes-lock password.

mod crypto;
mod state;

use crate::error::AppError;
use crate::AppState;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use crypto::{
    decrypt, derive_kek, encrypt, field_aad, random_salt, unwrap_key, wrap_key, CryptoError,
    KdfParams, KDF_ITERATIONS, KDF_MEMORY_KIB, KDF_PARALLELISM,
};
use sqlx::{Pool, Sqlite, Transaction};
use uuid::Uuid;

pub use crypto::SecretKey;
pub use state::{MemoryUpdate, VaultState};

const ROW_ID: &str = "default";

struct VaultRow {
    vault_id: String,
    crypto_version: i64,
    kdf_memory: i64,
    kdf_iterations: i64,
    kdf_parallelism: i64,
    kdf_salt: String,
    wrapped_key: String,
}

fn kdf_from_row(row: &VaultRow) -> Result<KdfParams, CryptoError> {
    let raw = B64.decode(&row.kdf_salt).map_err(|_| CryptoError::Kdf)?;
    let salt: [u8; 16] = raw.try_into().map_err(|_| CryptoError::Kdf)?;
    Ok(KdfParams {
        memory_kib: u32::try_from(row.kdf_memory).unwrap_or(u32::MAX),
        iterations: u32::try_from(row.kdf_iterations).unwrap_or(u32::MAX),
        parallelism: u32::try_from(row.kdf_parallelism).unwrap_or(u32::MAX),
        salt,
    })
}

async fn load_row(executor: impl sqlx::SqliteExecutor<'_>) -> Result<Option<VaultRow>, AppError> {
    let row = sqlx::query_as::<_, (String, i64, i64, i64, i64, String, String)>(
        "SELECT vault_id, crypto_version, kdf_memory, kdf_iterations, kdf_parallelism, kdf_salt, wrapped_key
         FROM password_vault WHERE id = ?",
    )
    .bind(ROW_ID)
    .fetch_optional(executor)
    .await
    .map_err(AppError::db)?;
    Ok(row.map(|r| VaultRow {
        vault_id: r.0,
        crypto_version: r.1,
        kdf_memory: r.2,
        kdf_iterations: r.3,
        kdf_parallelism: r.4,
        kdf_salt: r.5,
        wrapped_key: r.6,
    }))
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, CryptoError> + Send + 'static,
) -> Result<T, CryptoError> {
    match tokio::task::spawn_blocking(job).await {
        Ok(r) => r,
        Err(_) => Err(CryptoError::Kdf),
    }
}

/// After the notes lock accepts `password`, open or stage the vault.
/// A wrong wrap does not fail the notes unlock; the phase becomes mismatch.
pub async fn open_with_password(state: &AppState, password: &str) -> Result<(), AppError> {
    let row = load_row(&state.db).await?;
    let password = password.to_string();
    match row {
        None => {
            let salt = random_salt();
            let params = KdfParams::production(salt);
            let kek = blocking(move || derive_kek(&password, &params))
                .await
                .map_err(|_| AppError::VaultMismatch)?;
            state.vault.set_pending(kek, salt);
            Ok(())
        }
        Some(row) => {
            if row.crypto_version != 1 {
                state.vault.set_mismatch();
                return Ok(());
            }
            let params = match kdf_from_row(&row) {
                Ok(p) => p,
                Err(_) => {
                    state.vault.set_mismatch();
                    return Ok(());
                }
            };
            let wrapped = row.wrapped_key.clone();
            let vault_id = row.vault_id.clone();
            let opened = blocking(move || {
                let kek = derive_kek(&password, &params)?;
                let key = unwrap_key(&wrapped, &kek, &vault_id)?;
                Ok((kek, key, vault_id))
            })
            .await;
            match opened {
                Ok((kek, key, vault_id)) => state.vault.set_open(kek, key, vault_id),
                Err(_) => state.vault.set_mismatch(),
            }
            Ok(())
        }
    }
}

/// `none` | `ok` | `mismatch` for the lock status payload.
pub async fn has_row(state: &AppState) -> Result<bool, AppError> {
    Ok(load_row(&state.db).await?.is_some())
}

pub async fn status_label(state: &AppState) -> String {
    match state.vault.label() {
        "ok" => "ok".into(),
        "mismatch" => "mismatch".into(),
        "none" => "none".into(),
        _ => match load_row(&state.db).await {
            Ok(Some(_)) => "ok".into(),
            _ => "none".into(),
        },
    }
}

/// Rewrap the vault key (or stage a new KEK) inside `tx`. Caller writes the lock hash.
pub async fn rewrap_in(
    tx: &mut Transaction<'_, Sqlite>,
    current: Option<&str>,
    new_password: &str,
) -> Result<MemoryUpdate, AppError> {
    let row = load_row(&mut **tx).await?;
    let new_password = new_password.to_string();
    let Some(row) = row else {
        let salt = random_salt();
        let params = KdfParams::production(salt);
        let kek = blocking(move || derive_kek(&new_password, &params))
            .await
            .map_err(|_| AppError::VaultMismatch)?;
        return Ok(MemoryUpdate::Pending { kek, salt });
    };
    if row.crypto_version != 1 {
        return Err(AppError::VaultMismatch);
    }

    let Some(current) = current.map(str::to_string) else {
        return Ok(MemoryUpdate::Mismatch);
    };
    let params = kdf_from_row(&row).map_err(|_| AppError::VaultMismatch)?;
    let wrapped = row.wrapped_key.clone();
    let vault_id = row.vault_id.clone();
    let opened = blocking(move || {
        let kek = derive_kek(&current, &params)?;
        let key = unwrap_key(&wrapped, &kek, &vault_id)?;
        Ok((key, vault_id))
    })
    .await
    .map_err(|_| AppError::VaultMismatch)?;
    let (key, vault_id) = opened;

    let salt = random_salt();
    let new_params = KdfParams::production(salt);
    let new_password2 = new_password.clone();
    let key_for_wrap = key.clone_key();
    let vault_for_wrap = vault_id.clone();
    let (kek, wrapped_new) = blocking(move || {
        let kek = derive_kek(&new_password2, &new_params)?;
        let wrapped = wrap_key(&key_for_wrap, &kek, &vault_for_wrap)?;
        Ok((kek, wrapped))
    })
    .await
    .map_err(|_| AppError::VaultMismatch)?;

    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE password_vault
         SET kdf_salt = ?, kdf_memory = ?, kdf_iterations = ?, kdf_parallelism = ?,
             wrapped_key = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(B64.encode(salt))
    .bind(i64::from(KDF_MEMORY_KIB))
    .bind(i64::from(KDF_ITERATIONS))
    .bind(i64::from(KDF_PARALLELISM))
    .bind(&wrapped_new)
    .bind(&now)
    .bind(ROW_ID)
    .execute(&mut **tx)
    .await
    .map_err(AppError::db)?;

    Ok(MemoryUpdate::Open { kek, key, vault_id })
}

pub async fn entry_count(db: &Pool<Sqlite>) -> Result<i64, AppError> {
    sqlx::query_scalar("SELECT COUNT(*) FROM passwords")
        .fetch_one(db)
        .await
        .map_err(AppError::db)
}

/// Create the vault row on first save. Returns the key and vault id.
pub async fn ensure_key(state: &AppState) -> Result<(SecretKey, String), AppError> {
    if let Some(open) = state.vault.open_key() {
        return Ok(open);
    }
    if state.vault.is_mismatch() {
        return Err(AppError::VaultMismatch);
    }
    let Some((kek, salt)) = state.vault.pending() else {
        return Err(AppError::VaultLocked);
    };
    if let Some(row) = load_row(&state.db).await? {
        return adopt_existing(state, kek, row).await;
    }

    let vault_id = Uuid::new_v4().to_string();
    let key = SecretKey::random();
    let wrapped = wrap_key(&key, &kek, &vault_id).map_err(|_| AppError::DecryptFailed)?;
    let now = Utc::now().to_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO password_vault (
            id, vault_id, crypto_version, kdf_algorithm, kdf_salt,
            kdf_memory, kdf_iterations, kdf_parallelism, wrapped_key, created_at, updated_at
         ) VALUES (?, ?, 1, 'argon2id', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(ROW_ID)
    .bind(&vault_id)
    .bind(B64.encode(salt))
    .bind(i64::from(KDF_MEMORY_KIB))
    .bind(i64::from(KDF_ITERATIONS))
    .bind(i64::from(KDF_PARALLELISM))
    .bind(&wrapped)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await;

    if let Err(e) = inserted {
        if e.to_string().contains("UNIQUE") {
            if let Some(row) = load_row(&state.db).await? {
                return adopt_existing(state, kek, row).await;
            }
        }
        return Err(AppError::db(e));
    }
    state.vault.set_open(kek, key.clone_key(), vault_id.clone());
    Ok((key, vault_id))
}

async fn adopt_existing(
    state: &AppState,
    kek: SecretKey,
    row: VaultRow,
) -> Result<(SecretKey, String), AppError> {
    let vault_id = row.vault_id;
    let wrapped = row.wrapped_key;
    let kek_job = kek.clone_key();
    let id_job = vault_id.clone();
    let key = match blocking(move || unwrap_key(&wrapped, &kek_job, &id_job)).await {
        Ok(key) => key,
        Err(_) => {
            state.vault.set_mismatch();
            return Err(AppError::VaultMismatch);
        }
    };
    state
        .vault
        .set_open(kek, key.clone_key(), vault_id.clone());
    Ok((key, vault_id))
}

pub fn require_open(state: &AppState) -> Result<(SecretKey, String), AppError> {
    if let Some(open) = state.vault.open_key() {
        return Ok(open);
    }
    if state.vault.is_mismatch() {
        Err(AppError::VaultMismatch)
    } else {
        Err(AppError::VaultLocked)
    }
}

pub fn encrypt_field(key: &SecretKey, id: &str, field: &str, plaintext: &str) -> Result<String, AppError> {
    encrypt(key, &field_aad(id, field), plaintext.as_bytes()).map_err(|_| AppError::DecryptFailed)
}

/// Fresh vault wrapped by `password`, left open. Demo seed calls this after wiping the row.
pub async fn install_for_seed(state: &AppState, password: &str) -> Result<(SecretKey, String), AppError> {
    let password = password.to_string();
    let (kek, salt, key, vault_id, wrapped) = blocking(move || {
        let salt = random_salt();
        let params = KdfParams {
            memory_kib: KDF_MEMORY_KIB,
            iterations: KDF_ITERATIONS,
            parallelism: KDF_PARALLELISM,
            salt,
        };
        let kek = derive_kek(&password, &params)?;
        let key = SecretKey::random();
        let vault_id = Uuid::new_v4().to_string();
        let wrapped = wrap_key(&key, &kek, &vault_id)?;
        Ok((kek, salt, key, vault_id, wrapped))
    })
    .await
    .map_err(|_| AppError::DecryptFailed)?;

    let now = Utc::now().to_rfc3339();
    sqlx::query("DELETE FROM password_vault WHERE id = ?")
        .bind(ROW_ID)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    sqlx::query(
        "INSERT INTO password_vault (
            id, vault_id, crypto_version, kdf_algorithm, kdf_salt,
            kdf_memory, kdf_iterations, kdf_parallelism, wrapped_key, created_at, updated_at
         ) VALUES (?, ?, 1, 'argon2id', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(ROW_ID)
    .bind(&vault_id)
    .bind(B64.encode(salt))
    .bind(i64::from(KDF_MEMORY_KIB))
    .bind(i64::from(KDF_ITERATIONS))
    .bind(i64::from(KDF_PARALLELISM))
    .bind(&wrapped)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;
    state.vault.set_open(kek, key.clone_key(), vault_id.clone());
    Ok((key, vault_id))
}

pub fn decrypt_field(key: &SecretKey, id: &str, field: &str, stored: &str) -> Result<String, AppError> {
    let bytes = decrypt(key, &field_aad(id, field), stored).map_err(|_| AppError::DecryptFailed)?;
    String::from_utf8(bytes).map_err(|_| AppError::DecryptFailed)
}

/// Drop or refresh the in-memory key after a synced vault row arrives.
pub async fn refresh_after_sync(state: &AppState) {
    let row = match load_row(&state.db).await {
        Ok(row) => row,
        Err(_) => return,
    };
    let Some(kek) = state.vault.kek() else { return };
    let Some(row) = row else {
        state.vault.set_pending(kek, random_salt());
        return;
    };
    if row.crypto_version != 1 {
        state.vault.set_mismatch();
        return;
    }
    let wrapped = row.wrapped_key;
    let vault_id = row.vault_id;
    let kek_job = kek.clone_key();
    let id_job = vault_id.clone();
    match blocking(move || unwrap_key(&wrapped, &kek_job, &id_job)).await {
        Ok(key) => state.vault.set_open(kek, key, vault_id),
        Err(_) => state.vault.set_mismatch(),
    }
}
