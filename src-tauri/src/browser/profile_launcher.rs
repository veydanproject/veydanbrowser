// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::browser::{launch as browser_launch, userjs};
use crate::models::{Profile, Proxy};
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;

pub struct LaunchResult {
    pub pid: u32,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn read_window_size_from_xulstore(firefox_profile_dir: &std::path::Path) -> Option<(i64, i64)> {
    let content = std::fs::read_to_string(firefox_profile_dir.join("xulstore.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let win = json
        .get("chrome://browser/content/browser.xhtml")?
        .get("main-window")?;
    let w: i64 = win.get("width")?.as_str()?.parse().ok()?;
    let h: i64 = win.get("height")?.as_str()?.parse().ok()?;
    if w > 0 && h > 0 { Some((w, h)) } else { None }
}

/// Core launch orchestrator: proxy setup → user.js → binary resolution → spawn.
/// Does not touch the DB — callers handle DB reads and status updates.
pub async fn launch_profile(
    profile: &Profile,
    proxy: Option<&Proxy>,
    state: &AppState,
    app_handle: tauri::AppHandle,
) -> Result<LaunchResult, String> {
    let profile_path = PathBuf::from(&profile.profile_path);
    let firefox_profile_dir = profile_path.join("firefox-profile");
    std::fs::create_dir_all(&firefox_profile_dir).map_err(err)?;

    let (effective_proxy, local_proxy_stop) = setup_proxy(proxy).await?;

    let user_js_content = userjs::generate(profile, effective_proxy.as_ref());
    std::fs::write(firefox_profile_dir.join("user.js"), user_js_content).map_err(err)?;

    if profile.browser_type == "camoufox" {
        crate::commands::camoufox::write_search_engine_to_profile(
            &firefox_profile_dir,
            &profile.default_search_engine,
        )
        .unwrap_or_else(|e| {
            eprintln!("write_search_engine_to_profile failed: {e}");
        });
    }

    let (binary_path, camoufox_config) = resolve_binary_and_config(
        profile,
        &firefox_profile_dir,
        effective_proxy.as_ref(),
        state,
    )
    .await?;

    let pid = browser_launch::launch(
        profile.id.clone(),
        profile_path,
        binary_path,
        profile.timezone.clone(),
        camoufox_config,
        local_proxy_stop,
        Arc::clone(&state.browser),
        state.db.clone(),
        app_handle,
    )
    .await
    .map_err(err)?;

    Ok(LaunchResult { pid })
}

/// Wraps any proxy type in a local HTTP proxy on 127.0.0.1.
/// Returns the effective proxy (pointing to 127.0.0.1) and a stop channel.
async fn setup_proxy(
    proxy: Option<&Proxy>,
) -> Result<(Option<Proxy>, Option<tokio::sync::oneshot::Sender<()>>), String> {
    match proxy {
        Some(p) if matches!(p.proxy_type.as_str(), "http" | "https") => {
            let upstream = crate::proxy::local::Upstream::Http {
                host: p.host.clone(),
                port: p.port as u16,
                username: p.username.clone().unwrap_or_default(),
                password: p.password.clone().unwrap_or_default(),
            };
            match crate::proxy::local::spawn(upstream).await {
                Ok((local_port, stop_tx)) => {
                    let mut local_p = p.clone();
                    local_p.host = "127.0.0.1".to_string();
                    local_p.port = local_port as i64;
                    local_p.username = None;
                    local_p.password = None;
                    Ok((Some(local_p), Some(stop_tx)))
                }
                Err(e) => Err(format!("Failed to start local proxy: {e}")),
            }
        }
        Some(p) if p.proxy_type == "socks5" => {
            let upstream = crate::proxy::local::Upstream::Socks5 {
                host: p.host.clone(),
                port: p.port as u16,
                username: p.username.clone().filter(|u| !u.is_empty()),
                password: p.password.clone().filter(|pw| !pw.is_empty()),
            };
            match crate::proxy::local::spawn(upstream).await {
                Ok((local_port, stop_tx)) => {
                    let mut local_p = p.clone();
                    local_p.proxy_type = "http".to_string();
                    local_p.host = "127.0.0.1".to_string();
                    local_p.port = local_port as i64;
                    local_p.username = None;
                    local_p.password = None;
                    Ok((Some(local_p), Some(stop_tx)))
                }
                Err(e) => Err(format!("Failed to start local proxy: {e}")),
            }
        }
        Some(p) if p.proxy_type == "ssh" => {
            let auth = if let Some(key) = &p.private_key {
                if !key.is_empty() {
                    crate::proxy::ssh::SshAuth::PrivateKey(key.clone())
                } else {
                    crate::proxy::ssh::SshAuth::Password(p.password.clone().unwrap_or_default())
                }
            } else {
                crate::proxy::ssh::SshAuth::Password(p.password.clone().unwrap_or_default())
            };
            let username = p.username.clone().unwrap_or_default();
            match crate::proxy::ssh::SshSession::connect(
                &p.host,
                p.port as u16,
                &username,
                auth,
                p.server_fingerprint.clone(),
            )
            .await
            {
                Ok(r) => {
                    let upstream = crate::proxy::local::Upstream::Ssh { session: r.session };
                    match crate::proxy::local::spawn(upstream).await {
                        Ok((local_port, stop_tx)) => {
                            let mut local_p = p.clone();
                            local_p.proxy_type = "http".to_string();
                            local_p.host = "127.0.0.1".to_string();
                            local_p.port = local_port as i64;
                            local_p.username = None;
                            local_p.password = None;
                            Ok((Some(local_p), Some(stop_tx)))
                        }
                        Err(e) => Err(format!("Failed to start SSH local proxy: {e}")),
                    }
                }
                Err(e) => Err(format!("SSH connection failed: {e}")),
            }
        }
        _ => Ok((proxy.cloned(), None)),
    }
}

async fn resolve_binary_and_config(
    profile: &Profile,
    firefox_profile_dir: &std::path::Path,
    effective_proxy: Option<&Proxy>,
    state: &AppState,
) -> Result<(PathBuf, Option<serde_json::Value>), String> {
    match profile.browser_type.as_str() {
        "camoufox" => {
            let bin = crate::commands::camoufox::resolve_binary(&state.app_data_dir)
                .ok_or("Camoufox not found. Please download it in Settings.")?;

            if let Some(install_dir) = bin.parent() {
                crate::commands::camoufox::ensure_omni_patched(install_dir, &state.app_data_dir);
            }

            let (wcolor, wname) = if let Some(wid) = &profile.workspace_id {
                let row = sqlx::query_as::<_, (String, String)>(
                    "SELECT color, name FROM workspaces WHERE id = ?",
                )
                .bind(wid)
                .fetch_optional(&state.db)
                .await
                .map_err(err)?;
                row.unwrap_or_else(|| ("#6366f1".to_string(), String::new()))
            } else {
                ("#6366f1".to_string(), String::new())
            };

            let tags: Vec<String> = serde_json::from_str(&profile.tags).unwrap_or_default();
            let tcolor = if let (Some(first_tag), Some(wid)) = (tags.first(), &profile.workspace_id)
            {
                sqlx::query_scalar::<_, String>(
                    "SELECT color FROM workspace_columns WHERE workspace_id = ? AND tag_name = ?",
                )
                .bind(wid)
                .bind(first_tag)
                .fetch_optional(&state.db)
                .await
                .map_err(err)?
                .unwrap_or_else(|| wcolor.clone())
            } else {
                wcolor.clone()
            };

            let label = if wname.is_empty() {
                profile.name.clone()
            } else {
                format!("{} · {}", wname, profile.name)
            };

            let chrome_dir = firefox_profile_dir.join("chrome");
            std::fs::create_dir_all(&chrome_dir).map_err(err)?;
            std::fs::write(
                chrome_dir.join("userChrome.css"),
                userjs::camoufox_user_chrome(&wcolor, &tcolor, &label),
            )
            .map_err(err)?;

            if let Some(install_dir) = bin.parent() {
                crate::commands::camoufox::patch_chrome_css(install_dir, &wcolor, &tcolor, &label);
            }

            let _ = effective_proxy; // proxy already encoded in user.js

            let win_size = read_window_size_from_xulstore(firefox_profile_dir);
            let cfg = crate::commands::profiles::build_camoufox_config(profile, win_size);
            Ok((bin, Some(cfg)))
        }
        _ => {
            let bin = which::which("firefox")
                .map_err(|_| "Firefox not found in PATH. Please install it first.".to_string())?;
            Ok((bin, None))
        }
    }
}
