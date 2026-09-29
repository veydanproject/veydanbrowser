// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Push notifications: what stands between the messenger and the phone.
//!
//! Two sides meet here. The phone's side (the push plugin) knows the
//! permission and the phone's address at the push service. The messenger's
//! side (the runtime) knows what to tell the push server and when. This
//! file passes the one to the other and keeps no logic of its own.
//!
//! The web page never talks to the push plugin. It calls the commands here,
//! and they call the plugin from Rust. A capability naming the plugin would
//! make every Android build without the messenger fail, the release one
//! included; this way the capabilities stay as they are.
//!
//! On a desktop the commands exist and say that pushes are not supported.

use super::map_err;
use crate::error::{AppError, CmdResult};
use crate::AppState;
use messenger_runtime::push::PushStatus;
use serde::Serialize;

#[cfg(target_os = "android")]
use messenger_runtime::push::PushChannel;
#[cfg(target_os = "android")]
use tauri_plugin_veydan_push::{Event, Permission, VeydanPush};

/// Emitted with nothing when a notification was tapped while the app was
/// running. The page answers by asking `messenger_push_take_tap`.
pub const EVENT_TAP: &str = "messenger://push-tap";

/// What the phone says about pushes.
#[derive(Debug, Clone, Serialize)]
pub struct PushDevice {
    /// False on a desktop and in a build without the push bridge.
    pub supported: bool,
    /// False when this phone cannot receive pushes; `reason` says why.
    pub available: bool,
    /// `no_firebase_config`, `no_play_services`, `token_failed`.
    pub reason: Option<String>,
    pub detail: Option<String>,
    /// `granted`, `denied`, `prompt`, `prompt-with-rationale`.
    pub permission: Option<String>,
}

impl PushDevice {
    #[cfg(not(target_os = "android"))]
    fn unsupported() -> Self {
        Self {
            supported: false,
            available: false,
            reason: None,
            detail: None,
            permission: None,
        }
    }
}

/// Everything the settings screen shows: the phone's side and the server's.
#[derive(Debug, Clone, Serialize)]
pub struct PushView {
    pub device: PushDevice,
    pub status: PushStatus,
}

/// A notification the user tapped.
#[derive(Debug, Clone, Serialize)]
pub struct PushTap {
    #[serde(rename = "type")]
    pub kind: String,
    /// `group:<id>` when the push was about a group; a direct message names
    /// no chat, because the server does not know who wrote it.
    pub chat: Option<String>,
}

#[cfg(target_os = "android")]
fn bridge(app: &tauri::AppHandle) -> CmdResult<tauri::State<'_, VeydanPush<tauri::Wry>>> {
    use tauri::Manager;
    app.try_state::<VeydanPush<tauri::Wry>>()
        .ok_or_else(|| AppError::Other("push bridge is not loaded".into()))
}

#[cfg(target_os = "android")]
fn failed(e: tauri_plugin_veydan_push::Error) -> AppError {
    AppError::Other(e.to_string())
}

#[cfg(target_os = "android")]
fn permission_name(p: Permission) -> Option<String> {
    serde_json::to_value(p).ok()?.as_str().map(str::to_string)
}

#[cfg(target_os = "android")]
async fn device(app: &tauri::AppHandle) -> CmdResult<PushDevice> {
    let s = bridge(app)?.state().await.map_err(failed)?;
    Ok(PushDevice {
        supported: true,
        available: s.available,
        reason: s.reason,
        detail: None,
        permission: permission_name(s.permission),
    })
}

#[cfg(not(target_os = "android"))]
async fn device(_app: &tauri::AppHandle) -> CmdResult<PushDevice> {
    Ok(PushDevice::unsupported())
}

/// Starts what runs for as long as the app does.
///
/// - Taps are passed to the page.
/// - A new address at the push service is passed to the runtime.
/// - The plugin is told whether the messenger is receiving messages by
///   itself, in which case a push about a message is not shown while the
///   app is on the screen.
/// - The runtime's own loop, which keeps the push server told.
#[cfg(target_os = "android")]
pub fn spawn_bridge(app: tauri::AppHandle, rt: std::sync::Arc<messenger_runtime::MessengerRuntime>) {
    use std::time::Duration;
    use tauri::{Emitter, Manager};

    tauri::async_runtime::spawn(rt.clone().push_loop());

    tauri::async_runtime::spawn(async move {
        // The plugin is set up before the app's own setup ends, but this task
        // may start sooner than the window exists.
        let push = loop {
            if let Some(push) = app.try_state::<VeydanPush<tauri::Wry>>() {
                break push;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };

        let page = app.clone();
        if let Err(e) = push
            .listen(Event::Tap, move |_| {
                let _ = page.emit(EVENT_TAP, ());
            })
            .await
        {
            eprintln!("messenger push: cannot listen for taps: {e}");
        }

        let (runtime, package, version) = (
            rt.clone(),
            app.package_info().name.clone(),
            app.package_info().version.to_string(),
        );
        let app_id = app.config().identifier.clone();
        let _ = package;
        if let Err(e) = push
            .listen(Event::Token, move |payload| {
                let Some(token) = payload.get("token").and_then(|t| t.as_str()) else {
                    return;
                };
                let channel = PushChannel {
                    provider: "fcm".into(),
                    token: token.to_string(),
                    app_id: app_id.clone(),
                    app_version: Some(version.clone()),
                };
                let runtime = runtime.clone();
                tauri::async_runtime::spawn(async move {
                    // Kept whether pushes are on or not; told to the server
                    // only when they are.
                    if let Err(e) = runtime.push_set_channel(Some(channel)).await {
                        eprintln!("messenger push: the new token was not kept: {e}");
                        return;
                    }
                    if let Err(e) = runtime.push_reconcile(false).await {
                        eprintln!("messenger push: {e}");
                    }
                });
            })
            .await
        {
            eprintln!("messenger push: cannot listen for tokens: {e}");
        }

        let mut told: Option<bool> = None;
        loop {
            let live = match rt.status().await {
                Ok(s) => s.session_active && !s.silent_mode && s.relays_connected > 0,
                Err(_) => false,
            };
            if told != Some(live) && push.set_context(live).await.is_ok() {
                told = Some(live);
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

/// Asks nothing of the push service or the push server.
#[tauri::command]
pub async fn messenger_push_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PushView> {
    let rt = state.messenger.runtime()?;
    Ok(PushView {
        device: device(&app).await?,
        status: rt.push_status().await.map_err(map_err)?,
    })
}

/// Turning pushes on is the user's agreement: the permission is asked for,
/// then the phone's address at the push service, and then the push server
/// is told. Any of the three may fail; what failed is in the answer, and
/// pushes stay off.
///
/// Turning them off takes the registration back and gives the address up.
#[tauri::command]
pub async fn messenger_push_set_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PushView> {
    let rt = state.messenger.runtime()?;
    rt.push_mark_offered().await.map_err(map_err)?;

    #[cfg(target_os = "android")]
    {
        let push = bridge(&app)?;
        if !enabled {
            let status = rt.push_set_enabled(false).await.map_err(map_err)?;
            rt.push_set_channel(None).await.map_err(map_err)?;
            if let Err(e) = push.delete_token().await {
                eprintln!("messenger push: the token was not given up: {e}");
            }
            return Ok(PushView { device: device(&app).await?, status });
        }

        let mut device = device(&app).await?;
        let stays_off = |device: PushDevice, rt: std::sync::Arc<messenger_runtime::MessengerRuntime>| async move {
            Ok(PushView { device, status: rt.push_status().await.map_err(map_err)? })
        };
        if !device.available {
            return stays_off(device, rt.clone()).await;
        }
        let permission = push.request_permission().await.map_err(failed)?;
        device.permission = permission_name(permission);
        if permission != Permission::Granted {
            return stays_off(device, rt.clone()).await;
        }
        let answer = push.token().await.map_err(failed)?;
        let Some(token) = answer.token.filter(|_| answer.available) else {
            device.available = false;
            device.reason = answer.reason.or(Some("token_failed".into()));
            device.detail = answer.detail;
            return stays_off(device, rt.clone()).await;
        };
        rt.push_set_channel(Some(PushChannel {
            provider: "fcm".into(),
            token,
            app_id: answer.app_id.unwrap_or_else(|| app.config().identifier.clone()),
            app_version: Some(app.package_info().version.to_string()),
        }))
        .await
        .map_err(map_err)?;
        let status = rt.push_set_enabled(true).await.map_err(map_err)?;
        Ok(PushView { device, status })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = enabled;
        Ok(PushView {
            device: device(&app).await?,
            status: rt.push_status().await.map_err(map_err)?,
        })
    }
}

/// The user was asked about pushes and said "not now".
#[tauri::command]
pub async fn messenger_push_mark_offered(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    state.messenger.runtime()?.push_mark_offered().await.map_err(map_err)
}

/// `None` goes back to the server of the manifest.
#[tauri::command]
pub async fn messenger_push_set_server(
    url: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PushView> {
    let rt = state.messenger.runtime()?;
    let status = rt.push_set_server(url).await.map_err(map_err)?;
    Ok(PushView { device: device(&app).await?, status })
}

#[tauri::command]
pub async fn messenger_push_set_prefs(
    dm: bool,
    groups: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PushView> {
    let rt = state.messenger.runtime()?;
    let status = rt.push_set_prefs(dm, groups).await.map_err(map_err)?;
    Ok(PushView { device: device(&app).await?, status })
}

/// Language of the push texts: the language of the app.
#[tauri::command]
pub async fn messenger_push_set_locale(locale: String, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let rt = state.messenger.runtime()?;
    rt.push_set_locale(&locale).await.map_err(map_err)?;
    rt.push_reconcile(false).await.map_err(map_err)?;
    Ok(())
}

/// Tells the push server again, now.
#[tauri::command]
pub async fn messenger_push_refresh(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<PushView> {
    let rt = state.messenger.runtime()?;
    let status = rt.push_reconcile(true).await.map_err(map_err)?;
    Ok(PushView { device: device(&app).await?, status })
}

/// What the push service answered to a test push: `delivered`, `dead_token`,
/// `rejected`, `retry`; and the id of the push in the server's log.
#[derive(Debug, Clone, Serialize)]
pub struct PushTest {
    pub outcome: String,
    pub trace: String,
}

#[tauri::command]
pub async fn messenger_push_test(state: tauri::State<'_, AppState>) -> CmdResult<PushTest> {
    let answer = state.messenger.runtime()?.push_test().await.map_err(map_err)?;
    Ok(PushTest { outcome: answer.outcome, trace: answer.trace })
}

/// The tapped notification, once. Asked at start, to learn about the tap
/// that opened the app, and after every `EVENT_TAP`.
#[tauri::command]
pub async fn messenger_push_take_tap(app: tauri::AppHandle) -> CmdResult<Option<PushTap>> {
    #[cfg(target_os = "android")]
    {
        let tap = bridge(&app)?.take_tap().await.map_err(failed)?;
        Ok(tap.map(|t| PushTap { kind: t.kind, chat: t.chat }))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(None)
    }
}

/// Removes shown notifications: of one chat (`dm`, `group:<id>`), or all
/// that are about messages.
#[tauri::command]
pub async fn messenger_push_clear(key: Option<String>, app: tauri::AppHandle) -> CmdResult<()> {
    #[cfg(target_os = "android")]
    {
        bridge(&app)?.cancel(key.as_deref()).await.map_err(failed)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (key, app);
        Ok(())
    }
}

// Used on Android only; named here so that a desktop build does not warn.
#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
fn _unused(_: AppError) {}
