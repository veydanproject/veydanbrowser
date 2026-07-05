// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct TotpEntry {
    pub id: String,
    pub name: String,
    pub issuer: Option<String>,
    // secret is never returned to frontend — see TotpEntryPublic
    #[serde(skip_serializing)]
    pub secret: String,
    pub algorithm: String,
    pub digits: i64,
    pub period: i64,
    pub tags: String, // JSON array
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

/// Public representation — no secret field
#[derive(Debug, Serialize, Clone)]
pub struct TotpEntryPublic {
    pub id: String,
    pub name: String,
    pub issuer: Option<String>,
    pub algorithm: String,
    pub digits: i64,
    pub period: i64,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

impl TotpEntry {
    fn to_public(&self) -> Result<TotpEntryPublic, serde_json::Error> {
        let tags: Vec<String> = serde_json::from_str(&self.tags)?;
        Ok(TotpEntryPublic {
            id: self.id.clone(),
            name: self.name.clone(),
            issuer: self.issuer.clone(),
            algorithm: self.algorithm.clone(),
            digits: self.digits,
            period: self.period,
            tags,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            last_used_at: self.last_used_at.clone(),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct TotpCode {
    pub id: String,
    pub code: String,
    pub seconds_left: u64,
}

#[derive(Debug, Serialize)]
pub struct TotpPreview {
    pub name: String,
    pub issuer: Option<String>,
    pub secret_masked: String,
    pub algorithm: String,
    pub digits: u32,
    pub period: u64,
}

fn parse_algorithm(alg: &str) -> Algorithm {
    match alg.to_uppercase().as_str() {
        "SHA256" => Algorithm::SHA256,
        "SHA512" => Algorithm::SHA512,
        _ => Algorithm::SHA1,
    }
}

/// Normalise a user-supplied secret for storage.
/// Strips spaces, dashes, colons; uppercases.
/// Tries Base32 decode first, then hex — stores the raw cleaned string.
/// build_totp later decodes the same way via decode_secret.
fn normalize_secret(raw: &str) -> Result<String, AppError> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != ':')
        .collect::<String>()
        .to_uppercase();

    if cleaned.is_empty() {
        return Err(AppError::other("Secret cannot be empty"));
    }

    // Try Base32 — works if all chars are A-Z / 2-7
    if Secret::Encoded(cleaned.clone()).to_bytes().is_ok() {
        return Ok(cleaned);
    }

    // Try hex — even length, all 0-9 / A-F
    if cleaned.len() % 2 == 0 {
        let hex_ok = (0..cleaned.len())
            .step_by(2)
            .all(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).is_ok());
        if hex_ok {
            return Ok(cleaned);
        }
    }

    Err(AppError::other(
        "Invalid secret: not recognised as Base32 or hex. Check the key and try again.",
    ))
}

/// Decode a stored secret (Base32 or HEX) to raw bytes.
fn decode_secret(s: &str) -> Result<Vec<u8>, AppError> {
    // Try Base32 first
    if let Ok(bytes) = Secret::Encoded(s.to_string()).to_bytes() {
        return Ok(bytes);
    }
    // Try hex
    if s.len() % 2 == 0 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes: Result<Vec<u8>, _> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
            .collect();
        if let Ok(b) = bytes {
            return Ok(b);
        }
    }
    Err(AppError::other("Could not decode TOTP secret"))
}

pub(crate) fn build_totp(entry: &TotpEntry) -> Result<TOTP, AppError> {
    let secret = decode_secret(&entry.secret)?;

    TOTP::new(
        parse_algorithm(&entry.algorithm),
        entry.digits as usize,
        1,
        entry.period as u64,
        secret,
        entry.issuer.clone(),
        entry.name.clone(),
    )
    .map_err(|e| AppError::other(format!("TOTP build error: {e}")))
}

fn generate_code_for(entry: &TotpEntry) -> Result<TotpCode, AppError> {
    let totp = build_totp(entry)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::other(e.to_string()))?;

    let code = totp.generate(now.as_secs());

    let step = entry.period as u64;
    let elapsed = now.as_secs() % step;
    let seconds_left = step - elapsed;

    Ok(TotpCode {
        id: entry.id.clone(),
        code,
        seconds_left,
    })
}

#[tauri::command]
pub async fn totp_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<TotpEntryPublic>> {
    let rows = sqlx::query_as::<_, TotpEntry>(
        "SELECT * FROM totp_entries ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;

    rows.iter()
        .map(|r| r.to_public().map_err(|e| AppError::other(e)))
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct TotpAddRequest {
    pub name: String,
    pub issuer: Option<String>,
    /// Raw base32 secret OR full otpauth:// URI (uri takes priority)
    pub secret: Option<String>,
    pub uri: Option<String>,
    pub algorithm: Option<String>,
    pub digits: Option<i64>,
    pub period: Option<i64>,
    pub tags: Vec<String>,
}

#[tauri::command]
pub async fn totp_add(
    req: TotpAddRequest,
    state: tauri::State<'_, AppState>,
) -> CmdResult<TotpEntryPublic> {
    let (name, issuer, secret, algorithm, digits, period) = if let Some(uri) = &req.uri {
        let totp = TOTP::from_url(uri).map_err(|e| AppError::other(format!("Invalid URI: {e}")))?;
        let secret_b32 = totp.get_secret_base32();
        let alg = match totp.algorithm {
            Algorithm::SHA256 => "SHA256",
            Algorithm::SHA512 => "SHA512",
            _ => "SHA1",
        };
        let issuer = totp.issuer.clone();
        let account = totp.account_name.clone();
        (
            if req.name.is_empty() { account } else { req.name.clone() },
            issuer,
            secret_b32,
            alg.to_string(),
            totp.digits as i64,
            totp.step as i64,
        )
    } else {
        let raw = req.secret.ok_or_else(|| AppError::other("secret or uri required"))?;
        let normalized = normalize_secret(&raw)?;
        (
            req.name.clone(),
            req.issuer.clone(),
            normalized,
            req.algorithm.unwrap_or_else(|| "SHA1".into()),
            req.digits.unwrap_or(6),
            req.period.unwrap_or(30),
        )
    };

    // Validate secret can build a valid TOTP
    let test_entry = TotpEntry {
        id: String::new(),
        name: name.clone(),
        issuer: issuer.clone(),
        secret: secret.clone(),
        algorithm: algorithm.clone(),
        digits,
        period,
        tags: "[]".into(),
        created_at: String::new(),
        updated_at: String::new(),
        last_used_at: None,
    };
    build_totp(&test_entry)?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let tags_json =
        serde_json::to_string(&req.tags).map_err(|e| AppError::other(e))?;

    sqlx::query(
        "INSERT INTO totp_entries (id, name, issuer, secret, algorithm, digits, period, tags, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&issuer)
    .bind(&secret)
    .bind(&algorithm)
    .bind(digits)
    .bind(period)
    .bind(&tags_json)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    Ok(TotpEntryPublic {
        id,
        name,
        issuer,
        algorithm,
        digits,
        period,
        tags: req.tags,
        created_at: now.clone(),
        updated_at: now,
        last_used_at: None,
    })
}

#[derive(Debug, Deserialize)]
pub struct TotpUpdateRequest {
    pub name: Option<String>,
    pub issuer: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[tauri::command]
pub async fn totp_update(
    id: String,
    req: TotpUpdateRequest,
    state: tauri::State<'_, AppState>,
) -> CmdResult<TotpEntryPublic> {
    let now = Utc::now().to_rfc3339();

    let mut entry = sqlx::query_as::<_, TotpEntry>(
        "SELECT * FROM totp_entries WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found(format!("TOTP entry {id}")))?;

    if let Some(name) = req.name {
        entry.name = name;
    }
    if let Some(issuer) = req.issuer {
        entry.issuer = if issuer.is_empty() { None } else { Some(issuer) };
    }
    if let Some(tags) = &req.tags {
        entry.tags = serde_json::to_string(tags).map_err(|e| AppError::other(e))?;
    }
    entry.updated_at = now;

    sqlx::query(
        "UPDATE totp_entries SET name=?, issuer=?, tags=?, updated_at=? WHERE id=?",
    )
    .bind(&entry.name)
    .bind(&entry.issuer)
    .bind(&entry.tags)
    .bind(&entry.updated_at)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    entry.to_public().map_err(|e| AppError::other(e))
}

#[tauri::command]
pub async fn totp_delete(id: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    sqlx::query("DELETE FROM totp_entries WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

#[tauri::command]
pub async fn totp_generate_code(
    id: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<TotpCode> {
    let entry = sqlx::query_as::<_, TotpEntry>(
        "SELECT * FROM totp_entries WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found(format!("TOTP entry {id}")))?;

    let code = generate_code_for(&entry)?;

    // Update last_used_at
    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query("UPDATE totp_entries SET last_used_at=? WHERE id=?")
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await;

    Ok(code)
}

#[tauri::command]
pub async fn totp_generate_codes(
    ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<TotpCode>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }

    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT * FROM totp_entries WHERE id IN ({})",
        placeholders
    );

    // Safe: only `?` placeholders are interpolated; ids are bound below.
    let mut q = sqlx::query_as::<_, TotpEntry>(sqlx::AssertSqlSafe(sql));
    for id in &ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(&state.db).await.map_err(AppError::db)?;

    rows.iter().map(generate_code_for).collect()
}

#[tauri::command]
pub async fn totp_preview_uri(uri: String) -> CmdResult<TotpPreview> {
    let totp = TOTP::from_url(&uri)
        .map_err(|e| AppError::other(format!("Invalid otpauth URI: {e}")))?;

    let secret_b32 = totp.get_secret_base32();
    let masked = if secret_b32.len() > 4 {
        format!("{}…{}", &secret_b32[..2], &secret_b32[secret_b32.len() - 2..])
    } else {
        "••••".to_string()
    };

    let alg = match totp.algorithm {
        Algorithm::SHA256 => "SHA256",
        Algorithm::SHA512 => "SHA512",
        _ => "SHA1",
    };

    Ok(TotpPreview {
        name: totp.account_name,
        issuer: totp.issuer,
        secret_masked: masked,
        algorithm: alg.to_string(),
        digits: totp.digits as u32,
        period: totp.step,
    })
}
