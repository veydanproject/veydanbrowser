// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer};

fn serialize_tags_as_vec<S>(tags: &str, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let vec: Vec<String> = serde_json::from_str(tags).unwrap_or_default();
    vec.serialize(s)
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WorkspaceColumn {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub tag_name: String,
    pub color: String,
    pub position: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceColumnRequest {
    pub name: String,
    pub tag_name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkspaceColumnRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub position: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub icon: String,
    pub notes: Option<String>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStats {
    pub id: String,
    pub profile_count: i64,
    pub proxy_count: i64,
    pub active_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub status: String,
    pub profile_path: String,
    pub browser_type: String,
    pub proxy_id: Option<String>,
    pub fingerprint_preset: String,
    pub user_agent: Option<String>,
    pub platform: Option<String>,
    pub timezone: Option<String>,
    pub locale: String,
    pub languages: String,
    pub screen_width: i64,
    pub screen_height: i64,
    pub webrtc_mode: String,
    pub geolocation_enabled: bool,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub webgl_vendor: Option<String>,
    pub webgl_renderer: Option<String>,
    pub notes: Option<String>,
    pub workspace_id: Option<String>,
    pub kanban_status: String,
    pub kanban_order: i64,
    #[serde(serialize_with = "serialize_tags_as_vec")]
    pub tags: String,
    pub default_search_engine: String,
    pub history_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_launch_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProfileRequest {
    pub name: String,
    pub workspace_id: Option<String>,
    pub browser_type: Option<String>,
    pub proxy_id: Option<String>,
    pub fingerprint_preset: Option<String>,
    pub user_agent: Option<String>,
    pub platform: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub languages: Option<String>,
    pub screen_width: Option<i64>,
    pub screen_height: Option<i64>,
    pub webrtc_mode: Option<String>,
    pub geolocation_enabled: Option<bool>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub webgl_vendor: Option<String>,
    pub webgl_renderer: Option<String>,
    pub notes: Option<String>,
    pub default_search_engine: Option<String>,
    pub history_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub name: Option<String>,
    pub browser_type: Option<String>,
    pub proxy_id: Option<String>,
    pub fingerprint_preset: Option<String>,
    pub user_agent: Option<String>,
    pub platform: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub languages: Option<String>,
    pub screen_width: Option<i64>,
    pub screen_height: Option<i64>,
    pub webrtc_mode: Option<String>,
    pub geolocation_enabled: Option<bool>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub webgl_vendor: Option<String>,
    pub webgl_renderer: Option<String>,
    pub notes: Option<String>,
    pub default_search_engine: Option<String>,
    pub history_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Proxy {
    pub id: String,
    pub name: String,
    pub proxy_type: String,
    pub host: String,
    pub port: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub status: String,
    pub last_ip: Option<String>,
    pub last_check_at: Option<DateTime<Utc>>,
    pub private_key: Option<String>,
    /// SHA256 fingerprint of the SSH server's host key, saved on first successful connection (TOFU).
    /// None = never connected. On mismatch — connection is blocked.
    pub server_fingerprint: Option<String>,
    #[serde(serialize_with = "serialize_tags_as_vec")]
    pub tags: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProxyRequest {
    pub name: String,
    pub proxy_type: String,
    pub host: String,
    pub port: i64,
    pub tags: Option<Vec<String>>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub private_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub host: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub expiry: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileRawData {
    pub user_agent: String,
    pub platform: String,
    pub locale: String,
    pub languages: String,
    pub timezone: Option<String>,
    pub screen_width: i64,
    pub screen_height: i64,
    pub webrtc_mode: String,
    pub webgl_vendor: Option<String>,
    pub webgl_renderer: Option<String>,
    pub canvas_seed: u32,
    pub audio_seed: u32,
    pub fonts_seed: u32,
    pub geolocation_enabled: bool,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub camoufox_config: String,
    pub user_js: String,
    pub cookies: Vec<CookieEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyCheckResult {
    pub ip: String,
    pub country: Option<String>,
    pub city: Option<String>,
    pub ok: bool,
    /// For SSH proxies: fingerprint received from server. None for non-SSH.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_fingerprint: Option<String>,
    /// For SSH proxies: true if this was the first connection (fingerprint not yet saved).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_fingerprint_is_new: Option<bool>,
}

// ── Cookie Import ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CookieImportResult {
    pub count: usize,
    /// Unique domains, up to 20, sorted alphabetically
    pub domains: Vec<String>,
}

/// Cookie in EditThisCookie / antidetect browser export format
#[derive(Debug, Serialize)]
pub struct ExportCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(rename = "expirationDate", skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<f64>,
    #[serde(rename = "hostOnly")]
    pub host_only: bool,
    pub session: bool,
    #[serde(rename = "httpOnly")]
    pub http_only: bool,
    pub secure: bool,
    #[serde(rename = "sameSite")]
    pub same_site: String,
}

/// Cookie format exported by antidetect browsers (EditThisCookie compatible).
/// Supports both `[{...}]` and `{"cookies":[...]}` JSON shapes.
#[derive(Debug, Deserialize)]
pub struct AntidetectCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(rename = "expirationDate")]
    pub expiration_date: Option<f64>,
    pub session: Option<bool>,
    #[serde(rename = "httpOnly")]
    pub http_only: Option<bool>,
    pub secure: Option<bool>,
    #[serde(rename = "sameSite")]
    pub same_site: Option<String>,
}

// ── Export / Import ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportOptions {
    pub include_proxy: bool,
    pub include_proxy_password: bool,
    pub include_files: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileExportData {
    pub name: String,
    pub browser_type: String,
    pub fingerprint_preset: String,
    pub user_agent: Option<String>,
    pub platform: Option<String>,
    pub timezone: Option<String>,
    pub locale: String,
    pub languages: String,
    pub screen_width: i64,
    pub screen_height: i64,
    pub webrtc_mode: String,
    pub geolocation_enabled: bool,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub webgl_vendor: Option<String>,
    pub webgl_renderer: Option<String>,
    pub notes: Option<String>,
    pub kanban_status: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyExportData {
    pub name: String,
    pub proxy_type: String,
    pub host: String,
    pub port: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileExport {
    pub version: String,
    pub exported_at: String,
    pub profile: ProfileExportData,
    pub proxy: Option<ProxyExportData>,
}
