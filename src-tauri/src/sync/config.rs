// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Sync settings in `app_settings` and the storage adapter they describe.

use crate::error::{AppError, CmdResult};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use veydan_sync::{LargeFileConfig, LocalDir, S3Config, S3Storage, Storage, WebDavConfig, WebDavStorage};

pub const DEFAULT_INTERVAL_SEC: u64 = 60;
const MIN_INTERVAL_SEC: u64 = 1;
const MAX_INTERVAL_SEC: u64 = 86400;
const MIB: u64 = 1024 * 1024;

/// Large-file transfer settings, device-local. Says nothing about when to use v2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFileSettings {
    /// Chunk size for new files, 1..=64.
    pub chunk_mib: u32,
    /// Chunks in flight, 1..=8.
    pub parallelism: u32,
    /// Continue interrupted downloads from the staged prefix.
    pub resume: bool,
}

impl Default for LargeFileSettings {
    fn default() -> Self {
        let d = LargeFileConfig::default();
        Self { chunk_mib: (d.chunk_size / MIB) as u32, parallelism: d.parallelism as u32, resume: d.resume }
    }
}

impl LargeFileSettings {
    /// Rust is the source of truth: reject out-of-range values instead of clamping silently.
    pub fn to_config(&self) -> CmdResult<LargeFileConfig> {
        let cfg = LargeFileConfig {
            chunk_size: self.chunk_mib as u64 * MIB,
            parallelism: self.parallelism as usize,
            resume: self.resume,
        };
        cfg.validate().map_err(AppError::other)?;
        Ok(cfg)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct S3Settings {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    /// Never sent to the UI. On save: `None` keeps the stored key, `Some("")` clears it.
    #[serde(skip_serializing, default)]
    pub secret_key: Option<String>,
    #[serde(default)]
    pub has_secret_key: bool,
    pub path_style: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebDavSettings {
    pub url: String,
    pub username: String,
    /// Same three-state rule as `S3Settings::secret_key`.
    #[serde(skip_serializing, default)]
    pub password: Option<String>,
    #[serde(default)]
    pub has_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    /// "folder" | "s3" | "webdav"
    pub backend: String,
    pub folder_path: String,
    pub s3: S3Settings,
    pub webdav: WebDavSettings,
    pub interval_sec: u64,
    /// Replicate `firefox-profile/` directories (cookies, sessions, history).
    #[serde(default = "default_true")]
    pub profile_files: bool,
    /// How this device is shown to others (lease badge).
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub large_files: LargeFileSettings,
}

fn default_true() -> bool {
    true
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // Mobile has no shared folder and no Firefox profiles to replicate.
            backend: if cfg!(mobile) { "s3" } else { "folder" }.into(),
            folder_path: String::new(),
            s3: S3Settings::default(),
            webdav: WebDavSettings::default(),
            interval_sec: DEFAULT_INTERVAL_SEC,
            profile_files: cfg!(desktop),
            device_name: String::new(),
            large_files: LargeFileSettings::default(),
        }
    }
}

/// Hostname, or the device id when the OS gives nothing usable.
fn default_device_name() -> String {
    gethostname::gethostname().to_string_lossy().trim().to_string()
}

/// Identity of the vault this device joined. Absent until create/join.
#[derive(Debug, Clone)]
pub struct VaultBinding {
    pub vault_id: String,
    pub vmk_b64: String,
}

pub async fn get_setting(db: &Pool<Sqlite>, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}

pub async fn set_setting(db: &Pool<Sqlite>, key: &str, value: &str) -> CmdResult<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await
    .map_err(AppError::db)?;
    Ok(())
}

pub async fn delete_setting(db: &Pool<Sqlite>, key: &str) -> CmdResult<()> {
    sqlx::query("DELETE FROM app_settings WHERE key = ?")
        .bind(key)
        .execute(db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

async fn get_or_empty(db: &Pool<Sqlite>, key: &str) -> String {
    get_setting(db, key).await.unwrap_or_default()
}

pub async fn load_config(db: &Pool<Sqlite>) -> SyncConfig {
    let d = SyncConfig::default();
    let s3_secret = get_or_empty(db, "sync_s3_secret_key").await;
    let webdav_password = get_or_empty(db, "sync_webdav_password").await;
    SyncConfig {
        enabled: get_setting(db, "sync_enabled").await.as_deref() == Some("1"),
        backend: get_setting(db, "sync_backend").await.unwrap_or(d.backend),
        folder_path: get_or_empty(db, "sync_folder_path").await,
        s3: S3Settings {
            endpoint: get_or_empty(db, "sync_s3_endpoint").await,
            region: get_or_empty(db, "sync_s3_region").await,
            bucket: get_or_empty(db, "sync_s3_bucket").await,
            prefix: get_or_empty(db, "sync_s3_prefix").await,
            access_key: get_or_empty(db, "sync_s3_access_key").await,
            has_secret_key: !s3_secret.is_empty(),
            secret_key: Some(s3_secret),
            path_style: get_setting(db, "sync_s3_path_style").await.as_deref() == Some("1"),
        },
        webdav: WebDavSettings {
            url: get_or_empty(db, "sync_webdav_url").await,
            username: get_or_empty(db, "sync_webdav_username").await,
            has_password: !webdav_password.is_empty(),
            password: Some(webdav_password),
        },
        interval_sec: get_setting(db, "sync_interval_sec")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(d.interval_sec),
        profile_files: cfg!(desktop) && get_setting(db, "sync_profile_files").await.as_deref() != Some("0"),
        device_name: device_name(db).await,
        large_files: LargeFileSettings {
            chunk_mib: get_setting(db, "sync_lf_chunk_mib").await.and_then(|v| v.parse().ok()).unwrap_or(d.large_files.chunk_mib),
            parallelism: get_setting(db, "sync_lf_parallelism")
                .await
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.large_files.parallelism),
            resume: get_setting(db, "sync_lf_resume").await.as_deref() != Some("0"),
        },
    }
}

/// Name shown to other devices; falls back to the hostname, then the device id.
pub async fn device_name(db: &Pool<Sqlite>) -> String {
    if let Some(n) = get_setting(db, "sync_device_name").await.filter(|s| !s.trim().is_empty()) {
        return n;
    }
    let host = default_device_name();
    if !host.is_empty() {
        return host;
    }
    device_id(db).await.unwrap_or_default()
}

pub async fn save_config(db: &Pool<Sqlite>, cfg: &SyncConfig) -> CmdResult<()> {
    cfg.large_files.to_config()?;
    set_setting(db, "sync_enabled", if cfg.enabled { "1" } else { "0" }).await?;
    set_setting(db, "sync_backend", &cfg.backend).await?;
    set_setting(db, "sync_folder_path", &cfg.folder_path).await?;
    set_setting(db, "sync_s3_endpoint", &cfg.s3.endpoint).await?;
    set_setting(db, "sync_s3_region", &cfg.s3.region).await?;
    set_setting(db, "sync_s3_bucket", &cfg.s3.bucket).await?;
    set_setting(db, "sync_s3_prefix", &cfg.s3.prefix).await?;
    set_setting(db, "sync_s3_access_key", &cfg.s3.access_key).await?;
    if let Some(v) = cfg.s3.secret_key.as_deref() {
        set_setting(db, "sync_s3_secret_key", v).await?;
    }
    set_setting(db, "sync_s3_path_style", if cfg.s3.path_style { "1" } else { "0" }).await?;
    set_setting(db, "sync_webdav_url", &cfg.webdav.url).await?;
    set_setting(db, "sync_webdav_username", &cfg.webdav.username).await?;
    if let Some(v) = cfg.webdav.password.as_deref() {
        set_setting(db, "sync_webdav_password", v).await?;
    }
    let interval = cfg.interval_sec.clamp(MIN_INTERVAL_SEC, MAX_INTERVAL_SEC);
    set_setting(db, "sync_interval_sec", &interval.to_string()).await?;
    set_setting(db, "sync_profile_files", if cfg.profile_files { "1" } else { "0" }).await?;
    set_setting(db, "sync_device_name", cfg.device_name.trim()).await?;
    set_setting(db, "sync_lf_chunk_mib", &cfg.large_files.chunk_mib.to_string()).await?;
    set_setting(db, "sync_lf_parallelism", &cfg.large_files.parallelism.to_string()).await?;
    set_setting(db, "sync_lf_resume", if cfg.large_files.resume { "1" } else { "0" }).await?;
    Ok(())
}

/// Stable per-install device id, created on first use.
pub async fn device_id(db: &Pool<Sqlite>) -> CmdResult<String> {
    if let Some(id) = get_setting(db, "sync_device_id").await.filter(|s| !s.is_empty()) {
        return Ok(id);
    }
    let id = veydan_sync::random_hex(8);
    set_setting(db, "sync_device_id", &id).await?;
    Ok(id)
}

pub async fn load_binding(db: &Pool<Sqlite>) -> Option<VaultBinding> {
    let vault_id = get_setting(db, "sync_vault_id").await.filter(|s| !s.is_empty())?;
    let vmk_b64 = get_setting(db, "sync_vault_key").await.filter(|s| !s.is_empty())?;
    Some(VaultBinding { vault_id, vmk_b64 })
}

pub async fn save_binding(db: &Pool<Sqlite>, b: &VaultBinding) -> CmdResult<()> {
    set_setting(db, "sync_vault_id", &b.vault_id).await?;
    set_setting(db, "sync_vault_key", &b.vmk_b64).await
}

/// Start a fresh device log: new id, empty own chain. Peer heads, entity
/// states and the HLC stay so remote ops are still applied idempotently.
pub async fn rotate_device_id(db: &Pool<Sqlite>) -> CmdResult<()> {
    for key in ["sync_device_id", "sync_own_seq", "sync_own_head"] {
        delete_setting(db, key).await?;
    }
    Ok(())
}

/// Detect a database that was restored or copied from another install and
/// rotate the device id so two installs never write to the same log.
/// `data_dir/install.id` is outside the backup set; `sync_install_id` travels with the DB.
pub async fn check_install_marker(db: &Pool<Sqlite>, data_dir: &std::path::Path) -> CmdResult<()> {
    let marker_path = data_dir.join("install.id");
    let marker = match std::fs::read_to_string(&marker_path) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            let id = veydan_sync::random_hex(16);
            std::fs::write(&marker_path, &id).map_err(AppError::io)?;
            id
        }
    };
    match get_setting(db, "sync_install_id").await {
        Some(stored) if stored == marker => Ok(()),
        Some(_) => {
            rotate_device_id(db).await?;
            set_setting(db, "sync_install_id", &marker).await
        }
        None => set_setting(db, "sync_install_id", &marker).await,
    }
}

pub async fn clear_binding(db: &Pool<Sqlite>) -> CmdResult<()> {
    for key in [
        "sync_vault_id",
        "sync_vault_key",
        "sync_device_id",
        "sync_own_seq",
        "sync_own_head",
        // `sync_hlc` is kept: a clock reset would break LWW against existing ops.
        "sync_last_run",
        "sync_last_started",
        "sync_last_error",
        "sync_last_warning",
        "sync_last_applied",
        "sync_devices",
        "sync_gc_last",
        "sync_gc_blobs_total",
        "sync_gc_removed",
        "sync_gc_lf_total",
        "sync_gc_lf_removed",
    ] {
        delete_setting(db, key).await?;
    }
    Ok(())
}

/// Storage adapter for the configured backend.
pub fn build_storage(cfg: &SyncConfig) -> CmdResult<Box<dyn Storage>> {
    match cfg.backend.as_str() {
        "folder" => {
            if cfg.folder_path.trim().is_empty() {
                return Err(AppError::other("sync folder is not set"));
            }
            Ok(Box::new(LocalDir::new(cfg.folder_path.trim())))
        }
        "s3" => {
            let s = &cfg.s3;
            let secret_key = s.secret_key.clone().unwrap_or_default();
            if s.endpoint.is_empty() || s.bucket.is_empty() || s.access_key.is_empty() || secret_key.is_empty() {
                return Err(AppError::other("S3 endpoint, bucket and keys are required"));
            }
            let storage = S3Storage::new(S3Config {
                endpoint: s.endpoint.clone(),
                region: if s.region.is_empty() { "us-east-1".into() } else { s.region.clone() },
                bucket: s.bucket.clone(),
                prefix: s.prefix.clone(),
                access_key: s.access_key.clone(),
                secret_key,
                path_style: s.path_style,
            })
            .map_err(AppError::other)?;
            Ok(Box::new(storage))
        }
        "webdav" => {
            let w = &cfg.webdav;
            if w.url.is_empty() {
                return Err(AppError::other("WebDAV URL is required"));
            }
            let storage = WebDavStorage::new(WebDavConfig {
                url: w.url.clone(),
                username: w.username.clone(),
                password: w.password.clone().unwrap_or_default(),
            })
            .map_err(AppError::other)?;
            Ok(Box::new(storage))
        }
        other => Err(AppError::other(format!("unknown sync backend {other}"))),
    }
}
