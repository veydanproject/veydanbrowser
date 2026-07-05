// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use crate::AppState;
use base64::Engine as _;
use chrono::Utc;
use russh::{client, ChannelMsg};
use russh::keys::{decode_secret_key, HashAlg, PrivateKeyWithHashAlg};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

// ── DB row ─────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
struct SshConnectionRow {
    id: String,
    name: String,
    host: String,
    port: i64,
    username: String,
    auth_type: String,
    password: Option<String>,
    private_key: Option<String>,
    key_passphrase: Option<String>,
    requires_2fa: i64,
    totp_entry_id: Option<String>,
    proxy_id: Option<String>,
    connect_timeout_sec: i64,
    keepalive_sec: i64,
    terminal_theme: Option<String>,
    default_cols: i64,
    default_rows: i64,
    last_connected_at: Option<String>,
    created_at: String,
    updated_at: String,
}

// ── Output model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SshConnection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub key_passphrase: Option<String>,
    pub requires_2fa: bool,
    pub totp_entry_id: Option<String>,
    pub proxy_id: Option<String>,
    pub workspace_ids: Vec<String>,
    pub profile_ids: Vec<String>,
    pub connect_timeout_sec: i64,
    pub keepalive_sec: i64,
    pub terminal_theme: Option<String>,
    pub default_cols: i64,
    pub default_rows: i64,
    pub last_connected_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<SshConnectionRow> for SshConnection {
    fn from(r: SshConnectionRow) -> Self {
        SshConnection {
            id: r.id,
            name: r.name,
            host: r.host,
            port: r.port,
            username: r.username,
            auth_type: r.auth_type,
            password: r.password,
            private_key: r.private_key,
            key_passphrase: r.key_passphrase,
            requires_2fa: r.requires_2fa != 0,
            totp_entry_id: r.totp_entry_id,
            proxy_id: r.proxy_id,
            workspace_ids: vec![],
            profile_ids: vec![],
            connect_timeout_sec: r.connect_timeout_sec,
            keepalive_sec: r.keepalive_sec,
            terminal_theme: r.terminal_theme,
            default_cols: r.default_cols,
            default_rows: r.default_rows,
            last_connected_at: r.last_connected_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ── Create / Update inputs ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SshConnectionCreateInput {
    pub name: String,
    pub host: String,
    pub port: Option<i64>,
    pub username: String,
    pub auth_type: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub key_passphrase: Option<String>,
    pub requires_2fa: Option<bool>,
    pub totp_entry_id: Option<String>,
    pub proxy_id: Option<String>,
    pub workspace_ids: Option<Vec<String>>,
    pub profile_ids: Option<Vec<String>>,
    pub connect_timeout_sec: Option<i64>,
    pub keepalive_sec: Option<i64>,
    pub terminal_theme: Option<String>,
    pub default_cols: Option<i64>,
    pub default_rows: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SshConnectionUpdateInput {
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub auth_type: Option<String>,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub key_passphrase: Option<String>,
    pub requires_2fa: Option<bool>,
    pub totp_entry_id: Option<String>,
    pub proxy_id: Option<String>,
    pub workspace_ids: Option<Vec<String>>,
    pub profile_ids: Option<Vec<String>>,
    pub connect_timeout_sec: Option<i64>,
    pub keepalive_sec: Option<i64>,
    pub terminal_theme: Option<String>,
    pub default_cols: Option<i64>,
    pub default_rows: Option<i64>,
}

// ── Session types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SshStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshSessionInfo {
    pub session_id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub host: String,
    pub port: i64,
    pub status: SshStatus,
    pub error: Option<String>,
    pub connected_at: Option<String>,
}

pub enum SshInputCommand {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Disconnect,
    /// Response to a keyboard-interactive prompt relayed by the frontend
    PromptResponse(String),
}

pub struct SshSessionState {
    pub info: SshSessionInfo,
    pub writer: mpsc::Sender<SshInputCommand>,
}

pub type SshSessions = Arc<std::sync::RwLock<HashMap<String, SshSessionState>>>;

// ── Event payloads ─────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct StatusChangedPayload {
    session_id: String,
    connection_id: String,
    status: SshStatus,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
struct DataPayload {
    session_id: String,
    data_base64: String,
}

#[derive(Serialize, Clone)]
struct KeyboardPromptPayload {
    session_id: String,
    name: String,
    instructions: String,
    prompts: Vec<KeyboardPromptItem>,
}

#[derive(Serialize, Clone)]
struct KeyboardPromptItem {
    prompt: String,
    echo: bool,
}

// ── russh Handler ──────────────────────────────────────────────────────────────

struct TerminalHandler;

impl client::Handler for TerminalHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        std::future::ready(Ok(true))
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn emit_status(
    app: &AppHandle,
    session_id: &str,
    connection_id: &str,
    status: SshStatus,
    error: Option<String>,
) {
    let _ = app.emit(
        "ssh://status-changed",
        StatusChangedPayload {
            session_id: session_id.to_string(),
            connection_id: connection_id.to_string(),
            status,
            error,
        },
    );
}

fn emit_data(app: &AppHandle, session_id: &str, data: &[u8]) {
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(data);
    let _ = app.emit(
        "ssh://data",
        DataPayload {
            session_id: session_id.to_string(),
            data_base64,
        },
    );
}

/// Authenticate with the SSH server based on auth_type.
async fn do_authenticate(
    handle: &mut client::Handle<TerminalHandler>,
    app: &AppHandle,
    session_id: &str,
    username: &str,
    auth_type: &str,
    password: Option<&str>,
    private_key: Option<&str>,
    key_passphrase: Option<&str>,
    requires_2fa: bool,
    totp_entry_id: Option<&str>,
    db: &sqlx::SqlitePool,
    rx: &mut mpsc::Receiver<SshInputCommand>,
) -> anyhow::Result<()> {
    match auth_type {
        "password" => {
            if requires_2fa {
                do_keyboard_interactive(handle, app, session_id, username, password, totp_entry_id, db, rx).await
            } else {
                let pw = password.unwrap_or("");
                let result = handle.authenticate_password(username, pw).await?;
                if matches!(result, russh::client::AuthResult::Success) {
                    Ok(())
                } else {
                    anyhow::bail!("Password authentication failed")
                }
            }
        }
        "key" | "key_password" => {
            let pem = private_key.ok_or_else(|| anyhow::anyhow!("No private key provided"))?;
            let pem = pem.trim();

            let key = if auth_type == "key_password" {
                let passphrase = key_passphrase.unwrap_or("");
                russh::keys::PrivateKey::from_openssh(pem)
                    .or_else(|_| decode_secret_key(pem, Some(passphrase)))
                    .map_err(|e| anyhow::anyhow!("Failed to parse private key: {e}"))?
            } else {
                russh::keys::PrivateKey::from_openssh(pem)
                    .or_else(|_| decode_secret_key(pem, None))
                    .map_err(|e| anyhow::anyhow!("Failed to parse private key: {e}"))?
            };

            let algo = key.algorithm();
            let algo_str = format!("{algo}");
            let fingerprint = key.fingerprint(HashAlg::Sha256);
            let hash_alg = if algo.is_rsa() { Some(HashAlg::Sha256) } else { None };
            let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            let auth_algo = key_with_alg.algorithm();

            let result = handle.authenticate_publickey(username, key_with_alg).await?;
            match result {
                russh::client::AuthResult::Success => Ok(()),
                russh::client::AuthResult::Failure { partial_success: true, ref remaining_methods }
                    if remaining_methods.iter().any(|m| matches!(m, russh::MethodKind::KeyboardInteractive)) =>
                {
                    do_keyboard_interactive(handle, app, session_id, username, password, totp_entry_id, db, rx).await
                }
                russh::client::AuthResult::Failure { remaining_methods, .. } => {
                    let methods: Vec<String> = remaining_methods.iter().map(|m| format!("{m:?}")).collect();
                    anyhow::bail!(
                        "Public key authentication failed.\n\
                         Key: {algo_str} / {auth_algo}, Fingerprint: {fingerprint}\n\
                         Server remaining: [{}]\n\
                         Check that the public key is in ~/.ssh/authorized_keys on the server.",
                        methods.join(", ")
                    )
                }
            }
        }
        other => anyhow::bail!("Unknown auth_type: {other}"),
    }
}

/// Keyboard-interactive auth.
/// Prompts are auto-answered with:
///   1. Stored password (if prompt contains "password")
///   2. TOTP code generated from linked profile (if prompt contains "verification" / "code" / etc.)
///   3. Otherwise relayed to the frontend overlay for interactive input.
async fn do_keyboard_interactive(
    handle: &mut client::Handle<TerminalHandler>,
    app: &AppHandle,
    session_id: &str,
    username: &str,
    auto_password: Option<&str>,
    totp_entry_id: Option<&str>,
    db: &sqlx::SqlitePool,
    rx: &mut mpsc::Receiver<SshInputCommand>,
) -> anyhow::Result<()> {
    let mut result = handle
        .authenticate_keyboard_interactive_start(username, None)
        .await?;

    loop {
        match result {
            russh::client::KeyboardInteractiveAuthResponse::Success => return Ok(()),
            russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => {
                anyhow::bail!("Keyboard-interactive authentication failed")
            }
            russh::client::KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                let mut answers: Vec<String> = Vec::with_capacity(prompts.len());

                for (i, prompt) in prompts.iter().enumerate() {
                    let prompt_lower = prompt.prompt.to_lowercase();

                    let is_password_prompt = prompt_lower.contains("password")
                        || prompt_lower.contains("пароль")
                        || prompt_lower.contains("passwd");

                    let is_totp_prompt = prompt_lower.contains("verification")
                        || prompt_lower.contains("code")
                        || prompt_lower.contains("totp")
                        || prompt_lower.contains("2fa")
                        || prompt_lower.contains("authenticator")
                        || prompt_lower.contains("otp");

                    let auto = if is_password_prompt {
                        auto_password.map(|s| s.to_string())
                    } else if is_totp_prompt {
                        // Try to generate TOTP code from linked profile
                        if let Some(entry_id) = totp_entry_id {
                            generate_totp_code(db, entry_id).await.ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let answer = if let Some(a) = auto {
                        a
                    } else {
                        // Relay the whole InfoRequest to frontend — frontend sends one response per prompt
                        let _ = app.emit(
                            "ssh://keyboard-prompt",
                            KeyboardPromptPayload {
                                session_id: session_id.to_string(),
                                name: name.clone(),
                                instructions: instructions.clone(),
                                // Only send remaining prompts starting from i
                                prompts: prompts[i..].iter().map(|p| KeyboardPromptItem {
                                    prompt: p.prompt.clone(),
                                    echo: p.echo,
                                }).collect(),
                            },
                        );

                        loop {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(120),
                                rx.recv(),
                            ).await {
                                Ok(Some(SshInputCommand::PromptResponse(r))) => break r,
                                Ok(Some(SshInputCommand::Disconnect)) | Ok(None) => {
                                    anyhow::bail!("Disconnected during authentication");
                                }
                                Err(_) => anyhow::bail!("Authentication timeout (120s)"),
                                Ok(Some(_)) => {}
                            }
                        }
                    };

                    answers.push(answer);
                }

                result = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await?;
            }
        }
    }
}

/// Generate a TOTP code for the given entry_id from the database.
async fn generate_totp_code(db: &sqlx::SqlitePool, entry_id: &str) -> anyhow::Result<String> {
    use crate::commands::totp::{TotpEntry, build_totp};
    let entry = sqlx::query_as::<_, TotpEntry>(
        "SELECT * FROM totp_entries WHERE id = ?",
    )
    .bind(entry_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("TOTP entry {entry_id} not found"))?;

    let totp = build_totp(&entry).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    Ok(totp.generate(now))
}

// ── CRUD Commands ──────────────────────────────────────────────────────────────

/// Load workspace_ids and profile_ids for a list of connections in one query each.
async fn load_links(
    pool: &sqlx::SqlitePool,
    connections: &mut Vec<SshConnection>,
) -> anyhow::Result<()> {
    if connections.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = connections.iter().map(|c| c.id.clone()).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    // workspace links
    let ws_query = format!(
        "SELECT connection_id, workspace_id FROM ssh_connection_workspaces WHERE connection_id IN ({})",
        placeholders
    );
    let mut q = sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(ws_query));
    for id in &ids { q = q.bind(id); }
    let ws_rows = q.fetch_all(pool).await?;

    // profile links
    let pr_query = format!(
        "SELECT connection_id, profile_id FROM ssh_connection_profiles WHERE connection_id IN ({})",
        placeholders
    );
    let mut q = sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(pr_query));
    for id in &ids { q = q.bind(id); }
    let pr_rows = q.fetch_all(pool).await?;

    for conn in connections.iter_mut() {
        conn.workspace_ids = ws_rows.iter()
            .filter(|(cid, _)| cid == &conn.id)
            .map(|(_, wid)| wid.clone())
            .collect();
        conn.profile_ids = pr_rows.iter()
            .filter(|(cid, _)| cid == &conn.id)
            .map(|(_, pid)| pid.clone())
            .collect();
    }
    Ok(())
}

/// Replace all workspace links for a connection.
async fn sync_workspace_links(
    pool: &sqlx::SqlitePool,
    connection_id: &str,
    workspace_ids: &[String],
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM ssh_connection_workspaces WHERE connection_id = ?")
        .bind(connection_id)
        .execute(pool)
        .await?;
    for wid in workspace_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO ssh_connection_workspaces (connection_id, workspace_id) VALUES (?, ?)",
        )
        .bind(connection_id)
        .bind(wid)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Replace all profile links for a connection.
async fn sync_profile_links(
    pool: &sqlx::SqlitePool,
    connection_id: &str,
    profile_ids: &[String],
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM ssh_connection_profiles WHERE connection_id = ?")
        .bind(connection_id)
        .execute(pool)
        .await?;
    for pid in profile_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO ssh_connection_profiles (connection_id, profile_id) VALUES (?, ?)",
        )
        .bind(connection_id)
        .bind(pid)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_connection_list(
    state: State<'_, AppState>,
    workspace_id: Option<String>,
    profile_id: Option<String>,
    search: Option<String>,
) -> CmdResult<Vec<SshConnection>> {
    // Build query depending on context filter
    let sql = if profile_id.is_some() {
        // Profile context: only connections linked to this profile
        "SELECT DISTINCT c.* FROM ssh_connections c
         JOIN ssh_connection_profiles p ON p.connection_id = c.id
         WHERE p.profile_id = ?1
           AND (?2 IS NULL OR lower(c.name) LIKE lower(?2) OR lower(c.host) LIKE lower(?2) OR lower(c.username) LIKE lower(?2))
         ORDER BY c.name"
    } else if workspace_id.is_some() {
        // Workspace context: global (no links) OR linked to this workspace
        "SELECT DISTINCT c.* FROM ssh_connections c
         WHERE (
           NOT EXISTS (SELECT 1 FROM ssh_connection_workspaces w WHERE w.connection_id = c.id)
           OR EXISTS (SELECT 1 FROM ssh_connection_workspaces w WHERE w.connection_id = c.id AND w.workspace_id = ?1)
         )
         AND (?2 IS NULL OR lower(c.name) LIKE lower(?2) OR lower(c.host) LIKE lower(?2) OR lower(c.username) LIKE lower(?2))
         ORDER BY c.name"
    } else {
        // Global: all connections
        "SELECT c.* FROM ssh_connections c
         WHERE ?1 IS NULL
           AND (?2 IS NULL OR lower(c.name) LIKE lower(?2) OR lower(c.host) LIKE lower(?2) OR lower(c.username) LIKE lower(?2))
         ORDER BY c.name"
    };

    let filter_param = profile_id.as_deref().or(workspace_id.as_deref());
    let search_param = search.as_deref().map(|s| format!("%{}%", s));

    let rows = sqlx::query_as::<_, SshConnectionRow>(sql)
        .bind(filter_param)
        .bind(search_param.as_deref())
        .fetch_all(&state.db)
        .await
        .map_err(AppError::db)?;

    let mut conns: Vec<SshConnection> = rows.into_iter().map(SshConnection::from).collect();
    load_links(&state.db, &mut conns).await.map_err(AppError::other)?;
    Ok(conns)
}

#[tauri::command]
pub async fn ssh_connection_get(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<SshConnection> {
    let row = sqlx::query_as::<_, SshConnectionRow>(
        "SELECT * FROM ssh_connections WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::db)?
    .ok_or_else(|| AppError::not_found(format!("SSH connection {id}")))?;

    let mut conn = SshConnection::from(row);
    let mut vec = vec![conn];
    load_links(&state.db, &mut vec).await.map_err(AppError::other)?;
    conn = vec.remove(0);
    Ok(conn)
}

#[tauri::command]
pub async fn ssh_connection_create(
    state: State<'_, AppState>,
    input: SshConnectionCreateInput,
) -> CmdResult<SshConnection> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO ssh_connections (
            id, name, host, port, username, auth_type,
            password, private_key, key_passphrase,
            requires_2fa, totp_entry_id, proxy_id,
            connect_timeout_sec, keepalive_sec, terminal_theme,
            default_cols, default_rows, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?,
            ?, ?, ?,
            ?, ?, ?,
            ?, ?, ?,
            ?, ?, ?, ?
        )",
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.host)
    .bind(input.port.unwrap_or(22))
    .bind(&input.username)
    .bind(&input.auth_type)
    .bind(&input.password)
    .bind(&input.private_key)
    .bind(&input.key_passphrase)
    .bind(input.requires_2fa.unwrap_or(false) as i64)
    .bind(&input.totp_entry_id)
    .bind(&input.proxy_id)
    .bind(input.connect_timeout_sec.unwrap_or(15))
    .bind(input.keepalive_sec.unwrap_or(30))
    .bind(&input.terminal_theme)
    .bind(input.default_cols.unwrap_or(120))
    .bind(input.default_rows.unwrap_or(32))
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    if let Some(ref wids) = input.workspace_ids {
        sync_workspace_links(&state.db, &id, wids).await.map_err(AppError::other)?;
    }
    if let Some(ref pids) = input.profile_ids {
        sync_profile_links(&state.db, &id, pids).await.map_err(AppError::other)?;
    }

    ssh_connection_get(state, id).await
}

#[tauri::command]
pub async fn ssh_connection_update(
    state: State<'_, AppState>,
    id: String,
    input: SshConnectionUpdateInput,
) -> CmdResult<SshConnection> {
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "UPDATE ssh_connections SET
            name                = COALESCE(?, name),
            host                = COALESCE(?, host),
            port                = COALESCE(?, port),
            username            = COALESCE(?, username),
            auth_type           = COALESCE(?, auth_type),
            password            = ?,
            private_key         = ?,
            key_passphrase      = ?,
            requires_2fa        = COALESCE(?, requires_2fa),
            totp_entry_id       = ?,
            proxy_id            = ?,
            connect_timeout_sec = COALESCE(?, connect_timeout_sec),
            keepalive_sec       = COALESCE(?, keepalive_sec),
            terminal_theme      = CASE WHEN ? IS NOT NULL THEN ? ELSE terminal_theme END,
            default_cols        = COALESCE(?, default_cols),
            default_rows        = COALESCE(?, default_rows),
            updated_at          = ?
         WHERE id = ?",
    )
    .bind(&input.name)
    .bind(&input.host)
    .bind(input.port)
    .bind(&input.username)
    .bind(&input.auth_type)
    .bind(&input.password)
    .bind(&input.private_key)
    .bind(&input.key_passphrase)
    .bind(input.requires_2fa.map(|v| v as i64))
    .bind(&input.totp_entry_id)
    .bind(&input.proxy_id)
    .bind(input.connect_timeout_sec)
    .bind(input.keepalive_sec)
    .bind(&input.terminal_theme).bind(&input.terminal_theme)
    .bind(input.default_cols)
    .bind(input.default_rows)
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(AppError::db)?;

    if let Some(ref wids) = input.workspace_ids {
        sync_workspace_links(&state.db, &id, wids).await.map_err(AppError::other)?;
    }
    if let Some(ref pids) = input.profile_ids {
        sync_profile_links(&state.db, &id, pids).await.map_err(AppError::other)?;
    }

    ssh_connection_get(state, id).await
}

#[tauri::command]
pub async fn ssh_connection_delete(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    sqlx::query("DELETE FROM ssh_connections WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

// ── Session Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn ssh_session_list(
    state: State<'_, AppState>,
) -> CmdResult<Vec<SshSessionInfo>> {
    let sessions = state.ssh_sessions.read().map_err(|e| AppError::other(e.to_string()))?;
    Ok(sessions.values().map(|s| s.info.clone()).collect())
}

#[tauri::command]
pub async fn ssh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    connection_id: String,
) -> CmdResult<String> {
    let conn = ssh_connection_get(state.clone(), connection_id.clone()).await?;

    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel::<SshInputCommand>(64);

    let info = SshSessionInfo {
        session_id: session_id.clone(),
        connection_id: connection_id.clone(),
        connection_name: conn.name.clone(),
        host: conn.host.clone(),
        port: conn.port,
        status: SshStatus::Connecting,
        error: None,
        connected_at: None,
    };

    {
        let mut sessions = state.ssh_sessions.write().map_err(|e| AppError::other(e.to_string()))?;
        sessions.insert(
            session_id.clone(),
            SshSessionState { info: info.clone(), writer: tx },
        );
    }

    emit_status(&app, &session_id, &connection_id, SshStatus::Connecting, None);

    // Load proxy if set
    let proxy = if let Some(ref proxy_id) = conn.proxy_id {
        sqlx::query_as::<_, crate::models::Proxy>(
            "SELECT * FROM proxies WHERE id = ?",
        )
        .bind(proxy_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
    } else {
        None
    };

    let sessions_arc = state.ssh_sessions.clone();
    let db = state.db.clone();
    let db_update = db.clone();
    let session_id_ret = session_id.clone();

    tokio::spawn(async move {
        let result = run_session(
            &app,
            &session_id,
            &conn,
            proxy,
            db,
            rx,
        )
        .await;

        let (status, error) = match result {
            Ok(()) => (SshStatus::Disconnected, None),
            Err(e) => (SshStatus::Error, Some(e.to_string())),
        };

        // Update session status
        if let Ok(mut sessions) = sessions_arc.write() {
            if let Some(s) = sessions.get_mut(&session_id) {
                s.info.status = status.clone();
                s.info.error = error.clone();
            }
        }

        emit_status(&app, &session_id, &conn.id, status, error);

        // Update last_connected_at
        let _ = sqlx::query(
            "UPDATE ssh_connections SET last_connected_at = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(&conn.id)
        .execute(&db_update)
        .await;
    });

    Ok(session_id_ret)
}

/// Establishes an HTTP CONNECT tunnel through an HTTP/HTTPS proxy.
async fn http_connect_tunnel(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> anyhow::Result<tokio::net::TcpStream> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(format!("{}:{}", proxy_host, proxy_port)).await?;

    let mut request = format!(
        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
        target_host, target_port, target_host, target_port
    );

    if let (Some(user), Some(pass)) = (username, password) {
        if !user.is_empty() {
            let credentials = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", user, pass));
            request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", credentials));
        }
    }
    request.push_str("\r\n");

    stream.write_all(request.as_bytes()).await?;

    // Read response until \r\n\r\n
    let mut buf = Vec::with_capacity(512);
    loop {
        let mut tmp = [0u8; 256];
        let n = stream.read(&mut tmp).await?;
        anyhow::ensure!(n > 0, "HTTP proxy closed connection during CONNECT");
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        anyhow::ensure!(buf.len() < 8192, "HTTP proxy response too large");
    }

    let response = String::from_utf8_lossy(&buf);
    let status_line = response.lines().next().unwrap_or("");
    anyhow::ensure!(
        status_line.contains("200"),
        "HTTP proxy CONNECT failed: {}",
        status_line
    );

    Ok(stream)
}

async fn run_session(
    app: &AppHandle,
    session_id: &str,
    conn: &SshConnection,
    proxy: Option<crate::models::Proxy>,
    db: sqlx::SqlitePool,
    mut rx: mpsc::Receiver<SshInputCommand>,
) -> anyhow::Result<()> {
    let config = Arc::new(client::Config::default());
    let host = conn.host.as_str();
    let port = conn.port as u16;

    // _jump_session keeps the Arc alive for the duration of the session
    let (_jump_session, mut handle): (Option<crate::proxy::ssh::SharedSession>, client::Handle<TerminalHandler>) = match proxy {
        Some(ref p) if p.proxy_type == "socks5" => {
            let stream = crate::proxy::local::socks5_connect(
                &p.host,
                p.port as u16,
                host,
                port,
                p.username.as_deref(),
                p.password.as_deref(),
            )
            .await?;
            (None, client::connect_stream(config, stream, TerminalHandler).await?)
        }
        Some(ref p) if p.proxy_type == "ssh" => {
            let jump_auth = if let Some(ref key) = p.private_key {
                crate::proxy::ssh::SshAuth::PrivateKey(key.clone())
            } else {
                crate::proxy::ssh::SshAuth::Password(
                    p.password.clone().unwrap_or_default(),
                )
            };
            let jump_result = crate::proxy::ssh::SshSession::connect(
                &p.host,
                p.port as u16,
                p.username.as_deref().unwrap_or("root"),
                jump_auth,
                p.server_fingerprint.clone(),
            )
            .await?;
            let channel = jump_result.session.open_channel(host, port).await?;
            let stream = channel.into_stream();
            // Keep the jump session Arc alive — dropping it would close the tunnel
            let session_arc = jump_result.session.clone();
            (Some(session_arc), client::connect_stream(config, stream, TerminalHandler).await?)
        }
        Some(ref p) if p.proxy_type == "http" || p.proxy_type == "https" => {
            // HTTP CONNECT tunnel
            let stream = http_connect_tunnel(
                &p.host,
                p.port as u16,
                host,
                port,
                p.username.as_deref(),
                p.password.as_deref(),
            )
            .await?;
            (None, client::connect_stream(config, stream, TerminalHandler).await?)
        }
        _ => {
            (None, client::connect(config, (host, port), TerminalHandler).await?)
        }
    };

    // Authenticate
    do_authenticate(
        &mut handle,
        app,
        session_id,
        &conn.username,
        &conn.auth_type,
        conn.password.as_deref(),
        conn.private_key.as_deref(),
        conn.key_passphrase.as_deref(),
        conn.requires_2fa,
        conn.totp_entry_id.as_deref(),
        &db,
        &mut rx,
    )
    .await?;

    // Open shell channel
    let mut channel = handle.channel_open_session().await?;

    channel
        .request_pty(
            false,
            "xterm-256color",
            conn.default_cols as u32,
            conn.default_rows as u32,
            0,
            0,
            &[],
        )
        .await?;

    channel.request_shell(false).await?;

    emit_status(app, session_id, &conn.id, SshStatus::Connected, None);

    // Main loop: forward channel output to frontend, forward frontend input to channel
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { ref data }) => {
                        emit_data(app, session_id, data);
                    }
                    Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                        emit_data(app, session_id, data);
                    }
                    Some(ChannelMsg::ExitStatus { .. }) | None => break,
                    _ => {}
                }
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SshInputCommand::Data(data)) => {
                        channel.data(data.as_slice()).await?;
                    }
                    Some(SshInputCommand::Resize { cols, rows }) => {
                        channel.window_change(cols, rows, 0, 0).await?;
                    }
                    Some(SshInputCommand::Disconnect) | None => break,
                    Some(SshInputCommand::PromptResponse(_)) => {} // handled during auth phase
                }
            }
        }
    }

    channel.close().await?;
    Ok(())
}

#[tauri::command]
pub async fn ssh_disconnect(
    state: State<'_, AppState>,
    session_id: String,
) -> CmdResult<()> {
    let sender = {
        let sessions = state.ssh_sessions.read().map_err(|e| AppError::other(e.to_string()))?;
        sessions.get(&session_id).map(|s| s.writer.clone())
    };
    if let Some(tx) = sender {
        let _ = tx.send(SshInputCommand::Disconnect).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_send_data(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> CmdResult<()> {
    let sender = {
        let sessions = state.ssh_sessions.read().map_err(|e| AppError::other(e.to_string()))?;
        sessions.get(&session_id).map(|s| s.writer.clone())
    };
    if let Some(tx) = sender {
        let _ = tx.send(SshInputCommand::Data(data)).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> CmdResult<()> {
    let sender = {
        let sessions = state.ssh_sessions.read().map_err(|e| AppError::other(e.to_string()))?;
        sessions.get(&session_id).map(|s| s.writer.clone())
    };
    if let Some(tx) = sender {
        let _ = tx.send(SshInputCommand::Resize { cols, rows }).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_session_remove(
    state: State<'_, AppState>,
    session_id: String,
) -> CmdResult<()> {
    let mut sessions = state.ssh_sessions.write().map_err(|e| AppError::other(e.to_string()))?;
    sessions.remove(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn ssh_respond_prompt(
    state: State<'_, AppState>,
    session_id: String,
    response: String,
) -> CmdResult<()> {
    let sender = {
        let sessions = state.ssh_sessions.read().map_err(|e| AppError::other(e.to_string()))?;
        sessions.get(&session_id).map(|s| s.writer.clone())
    };
    if let Some(tx) = sender {
        let _ = tx.send(SshInputCommand::PromptResponse(response)).await;
    }
    Ok(())
}
