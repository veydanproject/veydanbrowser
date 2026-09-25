// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Password vault: one random key wrapped by the lock password and,
//! optionally, by a recovery code.

mod crypto;
mod recovery;
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
/// Wraps the vault while no PIN or password is set. Known from the sources:
/// it only keeps the storage format uniform, it does not protect the data.
pub const DEFAULT_SECRET: &str = "veydan-no-lock";
/// `lock_kind` value marking a row wrapped with `DEFAULT_SECRET`.
const KIND_NONE: &str = "none";

struct VaultRow {
    vault_id: String,
    crypto_version: i64,
    kdf_memory: i64,
    kdf_iterations: i64,
    kdf_parallelism: i64,
    kdf_salt: String,
    wrapped_key: String,
    recovery_salt: Option<String>,
    recovery_wrapped_key: Option<String>,
    lock_hash: Option<String>,
    lock_kind: Option<String>,
    lock_hint: Option<String>,
}

/// Lock settings carried by the vault row so every device checks the same secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockMeta {
    pub hash: String,
    pub kind: Option<String>,
    pub hint: Option<String>,
}

/// What the synced row says about the lock.
pub enum SyncedLock {
    /// No vault row yet.
    NoRow,
    /// Row from before the hash travelled with it.
    Legacy,
    /// Wrapped with `DEFAULT_SECRET`: no PIN on any device.
    Default,
    Meta(LockMeta),
}

fn decode_salt(b64: &str) -> Result<[u8; 16], CryptoError> {
    let raw = B64.decode(b64).map_err(|_| CryptoError::Kdf)?;
    raw.try_into().map_err(|_| CryptoError::Kdf)
}

fn kdf_from_row(row: &VaultRow) -> Result<KdfParams, CryptoError> {
    Ok(KdfParams {
        memory_kib: u32::try_from(row.kdf_memory).unwrap_or(u32::MAX),
        iterations: u32::try_from(row.kdf_iterations).unwrap_or(u32::MAX),
        parallelism: u32::try_from(row.kdf_parallelism).unwrap_or(u32::MAX),
        salt: decode_salt(&row.kdf_salt)?,
    })
}

type RawRow = (
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

async fn load_row(executor: impl sqlx::SqliteExecutor<'_>) -> Result<Option<VaultRow>, AppError> {
    let row = sqlx::query_as::<_, RawRow>(
        "SELECT vault_id, crypto_version, kdf_memory, kdf_iterations, kdf_parallelism, kdf_salt,
                wrapped_key, recovery_salt, recovery_wrapped_key, lock_hash, lock_kind, lock_hint
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
        recovery_salt: r.7,
        recovery_wrapped_key: r.8,
        lock_hash: r.9,
        lock_kind: r.10,
        lock_hint: r.11,
    }))
}

pub async fn synced_lock(db: &Pool<Sqlite>) -> Result<SyncedLock, AppError> {
    let Some(row) = load_row(db).await? else {
        return Ok(SyncedLock::NoRow);
    };
    Ok(match (row.lock_hash, row.lock_kind) {
        (Some(hash), kind) => SyncedLock::Meta(LockMeta {
            hash,
            kind,
            hint: row.lock_hint,
        }),
        (None, Some(kind)) if kind == KIND_NONE => SyncedLock::Default,
        (None, _) => SyncedLock::Legacy,
    })
}

/// Write the lock into the vault row; `None` marks it as wrapped with `DEFAULT_SECRET`.
/// No-op without a row.
pub async fn store_lock_meta(
    executor: impl sqlx::SqliteExecutor<'_>,
    meta: Option<&LockMeta>,
) -> Result<(), AppError> {
    let (hash, kind, hint) = match meta {
        Some(m) => (Some(m.hash.as_str()), m.kind.as_deref(), m.hint.as_deref()),
        None => (None, Some(KIND_NONE), None),
    };
    sqlx::query(
        "UPDATE password_vault
         SET lock_hash = ?, lock_kind = ?, lock_hint = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(hash)
    .bind(kind)
    .bind(hint)
    .bind(Utc::now().to_rfc3339())
    .bind(ROW_ID)
    .execute(executor)
    .await
    .map_err(AppError::db)?;
    Ok(())
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

/// New vault wrap, computed without holding a database lock.
/// Argon2 takes seconds; a transaction open that long makes the next write fail with "database is locked".
pub struct RewrapPrepared {
    pub memory: MemoryUpdate,
    write: Option<RewrapWrite>,
}

struct RewrapWrite {
    salt_b64: String,
    wrapped: String,
}

/// Read the vault row and derive the new wrap. No transaction.
pub async fn prepare_rewrap(
    db: &Pool<Sqlite>,
    current: Option<&str>,
    new_password: &str,
) -> Result<RewrapPrepared, AppError> {
    let new_password = new_password.to_string();
    let Some(row) = load_row(db).await? else {
        let salt = random_salt();
        let params = KdfParams::production(salt);
        let kek = blocking(move || derive_kek(&new_password, &params))
            .await
            .map_err(|_| AppError::VaultMismatch)?;
        return Ok(RewrapPrepared {
            memory: MemoryUpdate::Pending { kek, salt },
            write: None,
        });
    };
    if row.crypto_version != 1 {
        return Err(AppError::VaultMismatch);
    }
    let Some(current) = current.map(str::to_string) else {
        return Ok(RewrapPrepared {
            memory: MemoryUpdate::Mismatch,
            write: None,
        });
    };
    let params = kdf_from_row(&row).map_err(|_| AppError::VaultMismatch)?;
    let wrapped = row.wrapped_key.clone();
    let vault_id = row.vault_id.clone();
    let (key, vault_id) = blocking(move || {
        let kek = derive_kek(&current, &params)?;
        let key = unwrap_key(&wrapped, &kek, &vault_id)?;
        Ok((key, vault_id))
    })
    .await
    .map_err(|_| AppError::VaultMismatch)?;
    prepare_rewrap_key(key, vault_id, new_password).await
}

/// Wrap an already open vault key. No database access.
pub async fn prepare_rewrap_key(
    key: SecretKey,
    vault_id: String,
    new_password: String,
) -> Result<RewrapPrepared, AppError> {
    let salt = random_salt();
    let new_params = KdfParams::production(salt);
    let key_for_wrap = key.clone_key();
    let vault_for_wrap = vault_id.clone();
    let (kek, wrapped) = blocking(move || {
        let kek = derive_kek(&new_password, &new_params)?;
        let wrapped = wrap_key(&key_for_wrap, &kek, &vault_for_wrap)?;
        Ok((kek, wrapped))
    })
    .await
    .map_err(|_| AppError::VaultMismatch)?;
    Ok(RewrapPrepared {
        memory: MemoryUpdate::Open { kek, key, vault_id },
        write: Some(RewrapWrite {
            salt_b64: B64.encode(salt),
            wrapped,
        }),
    })
}

/// Store a prepared wrap. The statement is the whole critical section.
pub async fn apply_rewrap(
    tx: &mut Transaction<'_, Sqlite>,
    prepared: &RewrapPrepared,
) -> Result<(), AppError> {
    let Some(write) = &prepared.write else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE password_vault
         SET kdf_salt = ?, kdf_memory = ?, kdf_iterations = ?, kdf_parallelism = ?,
             wrapped_key = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&write.salt_b64)
    .bind(i64::from(KDF_MEMORY_KIB))
    .bind(i64::from(KDF_ITERATIONS))
    .bind(i64::from(KDF_PARALLELISM))
    .bind(&write.wrapped)
    .bind(Utc::now().to_rfc3339())
    .bind(ROW_ID)
    .execute(&mut **tx)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

/// Recovery wrap ready to store. The code is shown once and never written.
pub struct RecoveryPrepared {
    pub code: String,
    salt_b64: String,
    wrapped: String,
}

/// Derive a recovery wrap. No database access.
pub async fn prepare_recovery(key: &SecretKey, vault_id: &str) -> Result<RecoveryPrepared, AppError> {
    let key = key.clone_key();
    let vault_id = vault_id.to_string();
    let (code, salt, wrapped) = blocking(move || {
        let code = recovery::generate();
        let canonical = recovery::normalize(&code).ok_or(CryptoError::Kdf)?;
        let salt = random_salt();
        let kek = derive_kek(&canonical, &KdfParams::production(salt))?;
        let wrapped = wrap_key(&key, &kek, &vault_id)?;
        Ok((code, salt, wrapped))
    })
    .await
    .map_err(|_| AppError::DecryptFailed)?;
    Ok(RecoveryPrepared {
        code,
        salt_b64: B64.encode(salt),
        wrapped,
    })
}

pub async fn store_recovery(
    executor: impl sqlx::SqliteExecutor<'_>,
    prepared: &RecoveryPrepared,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE password_vault
         SET recovery_salt = ?, recovery_wrapped_key = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&prepared.salt_b64)
    .bind(&prepared.wrapped)
    .bind(Utc::now().to_rfc3339())
    .bind(ROW_ID)
    .execute(executor)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

/// Wrap `key` under a fresh recovery code and store it as the second wrap.
/// Returns the code in display form; it is never stored.
pub async fn install_recovery(
    executor: impl sqlx::SqliteExecutor<'_>,
    key: &SecretKey,
    vault_id: &str,
) -> Result<String, AppError> {
    let prepared = prepare_recovery(key, vault_id).await?;
    store_recovery(executor, &prepared).await?;
    Ok(prepared.code)
}

pub async fn has_recovery(db: &Pool<Sqlite>) -> Result<bool, AppError> {
    Ok(load_row(db)
        .await?
        .is_some_and(|row| row.recovery_wrapped_key.is_some()))
}

/// Unwrap the vault key with a recovery code. Wrong or malformed codes fail alike.
pub async fn open_with_recovery(
    db: &Pool<Sqlite>,
    code: &str,
) -> Result<(SecretKey, String), AppError> {
    let row = load_row(db).await?.ok_or(AppError::RecoveryInvalid)?;
    let (Some(salt_b64), Some(wrapped)) = (row.recovery_salt, row.recovery_wrapped_key) else {
        return Err(AppError::not_found("recovery key"));
    };
    let canonical = recovery::normalize(code).ok_or(AppError::RecoveryInvalid)?;
    let vault_id = row.vault_id;
    let id_job = vault_id.clone();
    let key = blocking(move || {
        let salt = decode_salt(&salt_b64)?;
        let kek = derive_kek(&canonical, &KdfParams::production(salt))?;
        unwrap_key(&wrapped, &kek, &id_job)
    })
    .await
    .map_err(|_| AppError::RecoveryInvalid)?;
    Ok((key, vault_id))
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
