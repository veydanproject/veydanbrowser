// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! System-tray integration.
//!
//! Two backends, one public API (`apply_tray_async` / `refresh_tray_async`):
//!
//! - **Linux** uses [`ksni`] — a native StatusNotifierItem implementation.
//!   Unlike Tauri's bundled libappindicator backend (menu-only), ksni exposes
//!   separate `activate` (primary/left click) and context-menu (right click)
//!   handlers, which KDE Plasma honors. So on Linux left-click opens the app
//!   and right-click opens the context menu.
//! - **Windows / macOS** use Tauri's `tray-icon`, which already delivers
//!   left-click events (window toggle) and shows the menu on right-click.
//!
//! Profile actions (start/stop) and navigation are dispatched back to the
//! webview via events so the heavy launch/teardown logic stays in the frontend
//! `api` layer instead of being duplicated here.

use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "veydan-main-tray";

/// Per profile under `--workdir`: every running instance owns its tray item.
fn tray_id() -> String {
    match crate::workdir::current() {
        Some(w) => format!("{TRAY_ID}-{}", w.tag),
        None => TRAY_ID.into(),
    }
}

/// Messages waiting in the messenger (chats that are not muted): a dot on
/// the icon and a line in the tooltip.
static UNREAD: AtomicUsize = AtomicUsize::new(0);

fn unread() -> usize {
    UNREAD.load(Ordering::Relaxed)
}

/// The messenger's count of waiting messages; the tray follows it.
#[cfg_attr(not(feature = "messenger"), allow(dead_code))]
pub fn set_unread(app: &AppHandle, n: usize) {
    if UNREAD.swap(n, Ordering::Relaxed) != n {
        imp::refresh(app);
    }
}

/// Tooltip with the live running count (and the profile name under `--workdir`),
/// and what waits in the messenger.
fn tooltip_text(labels: &TrayLabels, running: usize) -> String {
    let mut text = labels.tooltip.replace("{n}", &running.to_string());
    let waiting = unread();
    if waiting > 0 {
        text = format!("{text}\n{}", labels.unread.replace("{n}", &waiting.to_string()));
    }
    crate::workdir::caption(text)
}

/// An RGBA icon with a dot in its lower right corner: something waits.
fn with_dot(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let r = (width.min(height) as f32) * 0.22;
    let (cx, cy) = (width as f32 - r - 0.5, height as f32 - r - 0.5);
    for y in 0..height {
        for x in 0..width {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let i = ((y * width + x) * 4) as usize;
            if d <= r {
                out[i..i + 4].copy_from_slice(&[0xE5, 0x48, 0x4D, 0xFF]);
            } else if d <= r + 1.0 * (width as f32 / 32.0).max(1.0) {
                // A ring in the icon's background lets the dot stand apart.
                out[i + 3] = 0;
            }
        }
    }
    out
}

/// Localized labels for the static tray entries. Supplied by the frontend via
/// `tray_set_labels` so we never duplicate the i18n catalog in Rust. English
/// defaults are used until the frontend hands over the active locale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrayLabels {
    pub show: String,
    pub hide: String,
    pub quit: String,
    /// "Running profiles ({n})" — `{n}` is replaced with the live count.
    pub running: String,
    pub stop_all: String,
    pub no_running: String,
    pub launch_profile: String,
    pub no_profiles: String,
    pub section_workspaces: String,
    pub section_proxies: String,
    pub section_terminal: String,
    pub section_files: String,
    pub section_notes: String,
    pub password_generator: String,
    #[serde(default = "default_quick_capture_label")]
    pub quick_capture: String,
    /// "Veydan Space — {n} running" — `{n}` is replaced with the live count.
    pub tooltip: String,
    /// "{n} unread" — messages waiting in the messenger's chats that are not muted.
    #[serde(default = "default_unread_label")]
    pub unread: String,
}

fn default_quick_capture_label() -> String {
    "Quick note".into()
}

fn default_unread_label() -> String {
    "{n} unread".into()
}

impl Default for TrayLabels {
    fn default() -> Self {
        Self {
            show: "Show Veydan Space".into(),
            hide: "Hide window".into(),
            quit: "Quit".into(),
            running: "Running profiles ({n})".into(),
            stop_all: "Stop all".into(),
            no_running: "No running profiles".into(),
            launch_profile: "Launch profile".into(),
            no_profiles: "No profiles".into(),
            section_workspaces: "Workspaces".into(),
            section_proxies: "Proxies".into(),
            section_terminal: "Terminal".into(),
            section_files: "Files".into(),
            section_notes: "Notes".into(),
            password_generator: "Password generator".into(),
            quick_capture: default_quick_capture_label(),
            tooltip: "Veydan Space — {n} running".into(),
            unread: default_unread_label(),
        }
    }
}

/// (id, name) lists for the dynamic submenus.
#[derive(Default, Clone)]
struct MenuData {
    /// Currently running profiles.
    running: Vec<(String, String)>,
    /// Recently-updated, non-running profiles for quick launch.
    recent: Vec<(String, String)>,
}

/// Pull running profiles (with names) and a handful of recent non-running
/// profiles from the browser state + DB.
async fn load_menu_data(app: &AppHandle) -> MenuData {
    let (db, browser) = {
        let state = app.state::<AppState>();
        (state.db.clone(), state.browser.clone())
    };
    let running_ids = browser.running_ids().await;
    let running_set: HashSet<String> = running_ids.iter().cloned().collect();

    let mut running = Vec::with_capacity(running_ids.len());
    for id in &running_ids {
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM profiles WHERE id = ?")
            .bind(id)
            .fetch_optional(&db)
            .await
            .ok()
            .flatten();
        running.push((id.clone(), name.unwrap_or_else(|| id.clone())));
    }

    let recent = sqlx::query_as::<_, (String, String)>(
        "SELECT id, name FROM profiles ORDER BY updated_at DESC LIMIT 20",
    )
    .fetch_all(&db)
    .await
    .unwrap_or_default()
    .into_iter()
    .filter(|(id, _)| !running_set.contains(id))
    .take(8)
    .collect();

    MenuData { running, recent }
}

fn labels_of(app: &AppHandle) -> TrayLabels {
    app.state::<AppState>().tray_labels.lock().unwrap().clone()
}

// ── Window helpers (marshalled to the main thread; GTK-safe) ────────────────
//
// Stashing to the tray uses hide()+skip_taskbar so the window leaves the
// taskbar only while hidden. A visible window stays in the taskbar even when
// minimize-to-tray is enabled (otherwise Alt-Tab makes it look "gone").
// Client-side decorations (decorations:false + custom titlebar) avoid the
// old KWin hide()/show() decoration remap bug on Wayland.

/// Restore taskbar entry, show and focus the main window.
fn restore_window(w: &tauri::WebviewWindow) {
    let _ = w.set_skip_taskbar(false);
    if w.is_minimized().unwrap_or(false) {
        let _ = w.unminimize();
    }
    let _ = w.show();
    let _ = w.set_focus();
}

/// Hide to tray and drop the taskbar entry.
fn stash_window(w: &tauri::WebviewWindow) {
    let _ = w.set_skip_taskbar(true);
    let _ = w.hide();
}

/// Bring the window back to the foreground.
fn win_show(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = a.get_webview_window("main") {
            restore_window(&w);
        }
    });
}

/// Stash the window to the tray — fully leaves the taskbar.
fn win_hide(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = a.get_webview_window("main") {
            stash_window(&w);
        }
    });
}

/// Primary click on the tray icon (both platforms). Toggle:
/// - hidden → show + focus;
/// - visible but not focused → raise + focus (it's behind other windows);
/// - visible and focused → hide to the tray.
fn win_primary(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = a.get_webview_window("main") {
            let visible = w.is_visible().unwrap_or(false);
            if !visible {
                restore_window(&w);
            } else if w.is_focused().unwrap_or(false) {
                stash_window(&w);
            } else {
                let _ = w.set_focus();
            }
        }
    });
}

/// Hide main window to tray (skip taskbar). Safe from any thread.
pub fn hide_to_tray(app: &AppHandle) {
    win_hide(app);
}

/// Show main window and restore taskbar. Safe from any thread.
pub fn show_from_tray(app: &AppHandle) {
    win_show(app);
}

/// Dispatch a tray menu action id back to the frontend / window.
fn dispatch(app: &AppHandle, id: &str) {
    match id {
        "show" => win_show(app),
        "hide" => win_hide(app),
        "quit" => app.exit(0),
        "stop_all" => {
            let _ = app.emit("tray://stop-all", ());
        }
        "pwgen" => {
            win_show(app);
            let _ = app.emit("tray://open-pwgen", ());
        }
        "quick_capture" => crate::commands::notes::show_quick_capture(app),
        other => {
            if let Some(pid) = other.strip_prefix("stop:") {
                let _ = app.emit("tray://stop-profile", pid.to_string());
            } else if let Some(pid) = other.strip_prefix("launch:") {
                let _ = app.emit("tray://launch-profile", pid.to_string());
            } else if let Some(route) = other.strip_prefix("nav:") {
                win_show(app);
                let _ = app.emit("tray://navigate", route.to_string());
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Linux backend — ksni (StatusNotifierItem)
// ════════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use ksni::menu::{MenuItem, StandardItem, SubMenu};
    use ksni::{Category, Handle, Icon, Status, ToolTip, Tray, TrayMethods};
    use std::sync::Mutex;

    static TRAY: Mutex<Option<Handle<VeydanTray>>> = Mutex::new(None);

    pub struct VeydanTray {
        app: AppHandle,
        labels: TrayLabels,
        data: MenuData,
        icon: Vec<Icon>,
        /// The same with a dot: messages wait.
        icon_dot: Vec<Icon>,
        visible: bool,
    }

    fn item(
        label: impl Into<String>,
        activate: impl Fn(&mut VeydanTray) + Send + 'static,
    ) -> MenuItem<VeydanTray> {
        StandardItem {
            label: label.into(),
            activate: Box::new(activate),
            ..Default::default()
        }
        .into()
    }

    fn disabled(label: impl Into<String>) -> MenuItem<VeydanTray> {
        StandardItem {
            label: label.into(),
            enabled: false,
            ..Default::default()
        }
        .into()
    }

    impl Tray for VeydanTray {
        fn id(&self) -> String {
            tray_id()
        }

        fn title(&self) -> String {
            crate::workdir::caption("Veydan Space".into())
        }

        fn category(&self) -> Category {
            Category::ApplicationStatus
        }

        fn status(&self) -> Status {
            if !self.visible {
                Status::Passive
            } else if unread() > 0 {
                Status::NeedsAttention
            } else {
                Status::Active
            }
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            if unread() > 0 { self.icon_dot.clone() } else { self.icon.clone() }
        }

        fn attention_icon_pixmap(&self) -> Vec<Icon> {
            self.icon_dot.clone()
        }

        fn tool_tip(&self) -> ToolTip {
            let n = self.data.running.len();
            ToolTip {
                title: tooltip_text(&self.labels, n),
                description: String::new(),
                icon_name: String::new(),
                icon_pixmap: Vec::new(),
            }
        }

        /// Left / primary click → focus-aware toggle (show / raise / hide).
        fn activate(&mut self, _x: i32, _y: i32) {
            win_primary(&self.app);
        }

        /// Right click → this menu (rendered by the SNI host as the context menu).
        fn menu(&self) -> Vec<MenuItem<Self>> {
            let l = &self.labels;

            // Running-profiles submenu
            let running_sub = if self.data.running.is_empty() {
                vec![disabled(l.no_running.clone())]
            } else {
                let mut v: Vec<MenuItem<VeydanTray>> = self
                    .data
                    .running
                    .iter()
                    .map(|(id, name)| {
                        let id = id.clone();
                        item(format!("● {name}"), move |t| {
                            let _ = t.app.emit("tray://stop-profile", id.clone());
                        })
                    })
                    .collect();
                v.push(MenuItem::Separator);
                v.push(item(l.stop_all.clone(), |t| {
                    let _ = t.app.emit("tray://stop-all", ());
                }));
                v
            };

            // Quick-launch submenu
            let launch_sub = if self.data.recent.is_empty() {
                vec![disabled(l.no_profiles.clone())]
            } else {
                self.data
                    .recent
                    .iter()
                    .map(|(id, name)| {
                        let id = id.clone();
                        item(name.clone(), move |t| {
                            let _ = t.app.emit("tray://launch-profile", id.clone());
                        })
                    })
                    .collect()
            };

            let running_label = l
                .running
                .replace("{n}", &self.data.running.len().to_string());

            fn nav(label: &str, route: &'static str) -> MenuItem<VeydanTray> {
                item(label.to_string(), move |t| {
                    dispatch(&t.app, &format!("nav:{route}"))
                })
            }

            vec![
                item(l.show.clone(), |t| win_show(&t.app)),
                item(l.hide.clone(), |t| win_hide(&t.app)),
                MenuItem::Separator,
                SubMenu {
                    label: running_label,
                    submenu: running_sub,
                    ..Default::default()
                }
                .into(),
                SubMenu {
                    label: l.launch_profile.clone(),
                    submenu: launch_sub,
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                nav(&l.section_workspaces, "/"),
                nav(&l.section_proxies, "/proxies"),
                nav(&l.section_terminal, "/terminal"),
                nav(&l.section_files, "/files"),
                nav(&l.section_notes, "/notes"),
                MenuItem::Separator,
                item(l.password_generator.clone(), |t| dispatch(&t.app, "pwgen")),
                item(l.quick_capture.clone(), |t| {
                    dispatch(&t.app, "quick_capture")
                }),
                MenuItem::Separator,
                item(l.quit.clone(), |t| t.app.exit(0)),
            ]
        }
    }

    /// Convert the app's window icon (RGBA8) to ksni's ARGB32 network-byte-order.
    /// The app's icon as SNI wants it (ARGB), plain and with the dot.
    fn build_icons(app: &AppHandle) -> (Vec<Icon>, Vec<Icon>) {
        let Some(img) = app.default_window_icon() else {
            return (Vec::new(), Vec::new());
        };
        let argb = |rgba: &[u8]| {
            let mut data = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                data.push(px[3]); // A
                data.push(px[0]); // R
                data.push(px[1]); // G
                data.push(px[2]); // B
            }
            vec![Icon { width: img.width() as i32, height: img.height() as i32, data }]
        };
        let dot = with_dot(img.rgba(), img.width(), img.height());
        (argb(img.rgba()), argb(&dot))
    }

    fn current_handle() -> Option<Handle<VeydanTray>> {
        TRAY.lock().unwrap().clone()
    }

    pub fn apply(app: &AppHandle, want: bool) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if !want {
                if let Some(h) = current_handle() {
                    let _ = h.update(|t| t.visible = false).await;
                }
                return;
            }
            if let Some(h) = current_handle() {
                let data = load_menu_data(&app).await;
                let labels = labels_of(&app);
                let _ = h
                    .update(move |t| {
                        t.visible = true;
                        t.data = data;
                        t.labels = labels;
                    })
                    .await;
                return;
            }
            // First activation: build and register the SNI service.
            let icons = build_icons(&app);
            let tray = VeydanTray {
                app: app.clone(),
                labels: labels_of(&app),
                data: load_menu_data(&app).await,
                icon: icons.0,
                icon_dot: icons.1,
                visible: true,
            };
            match tray.spawn().await {
                Ok(handle) => *TRAY.lock().unwrap() = Some(handle),
                Err(e) => {
                    eprintln!("tray: ksni spawn failed: {e}");
                    // Hide-on-close would stash the window with no way to restore it
                    let state = app.state::<AppState>();
                    state
                        .tray_settings
                        .close_to_tray
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    state
                        .tray_settings
                        .minimize_to_tray
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });
    }

    pub fn refresh(app: &AppHandle) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(h) = current_handle() {
                let data = load_menu_data(&app).await;
                let labels = labels_of(&app);
                let _ = h
                    .update(move |t| {
                        t.data = data;
                        t.labels = labels;
                    })
                    .await;
            }
        });
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Windows / macOS backend — Tauri tray-icon
// ════════════════════════════════════════════════════════════════════════════
#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::Wry;

    fn build_menu(
        app: &AppHandle,
        labels: &TrayLabels,
        data: &MenuData,
    ) -> tauri::Result<Menu<Wry>> {
        let n = data.running.len();
        let running_label = labels.running.replace("{n}", &n.to_string());

        let mut running_sub = SubmenuBuilder::new(app, &running_label);
        if data.running.is_empty() {
            running_sub = running_sub.item(
                &MenuItemBuilder::with_id("noop:running", &labels.no_running)
                    .enabled(false)
                    .build(app)?,
            );
        } else {
            for (id, name) in &data.running {
                running_sub = running_sub.item(
                    &MenuItemBuilder::with_id(format!("stop:{id}"), format!("● {name}"))
                        .build(app)?,
                );
            }
            running_sub = running_sub
                .separator()
                .item(&MenuItemBuilder::with_id("stop_all", &labels.stop_all).build(app)?);
        }
        let running_sub = running_sub.build()?;

        let mut launch_sub = SubmenuBuilder::new(app, &labels.launch_profile);
        if data.recent.is_empty() {
            launch_sub = launch_sub.item(
                &MenuItemBuilder::with_id("noop:launch", &labels.no_profiles)
                    .enabled(false)
                    .build(app)?,
            );
        } else {
            for (id, name) in &data.recent {
                launch_sub = launch_sub
                    .item(&MenuItemBuilder::with_id(format!("launch:{id}"), name).build(app)?);
            }
        }
        let launch_sub = launch_sub.build()?;

        MenuBuilder::new(app)
            .item(&MenuItemBuilder::with_id("show", &labels.show).build(app)?)
            .item(&MenuItemBuilder::with_id("hide", &labels.hide).build(app)?)
            .separator()
            .item(&running_sub)
            .item(&launch_sub)
            .separator()
            .item(&MenuItemBuilder::with_id("nav:/", &labels.section_workspaces).build(app)?)
            .item(&MenuItemBuilder::with_id("nav:/proxies", &labels.section_proxies).build(app)?)
            .item(&MenuItemBuilder::with_id("nav:/terminal", &labels.section_terminal).build(app)?)
            .item(&MenuItemBuilder::with_id("nav:/files", &labels.section_files).build(app)?)
            .item(&MenuItemBuilder::with_id("nav:/notes", &labels.section_notes).build(app)?)
            .separator()
            .item(&MenuItemBuilder::with_id("pwgen", &labels.password_generator).build(app)?)
            .item(&MenuItemBuilder::with_id("quick_capture", &labels.quick_capture).build(app)?)
            .separator()
            .item(&MenuItemBuilder::with_id("quit", &labels.quit).build(app)?)
            .build()
    }

    fn show_tray(app: &AppHandle) -> tauri::Result<()> {
        let state = app.state::<AppState>();
        {
            let guard = state.tray.lock().unwrap();
            if let Some(tray) = guard.as_ref() {
                let _ = tray.set_visible(true);
                drop(guard);
                refresh_sync(app);
                return Ok(());
            }
        }

        let labels = labels_of(app);
        let data = tauri::async_runtime::block_on(load_menu_data(app));
        let menu = build_menu(app, &labels, &data)?;
        let tooltip = tooltip_text(&labels, data.running.len());

        let mut builder = TrayIconBuilder::with_id(tray_id())
            .tooltip(&tooltip)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| dispatch(app, event.id.as_ref()))
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    win_primary(tray.app_handle());
                }
            });
        if let Some(icon) = current_icon(app) {
            builder = builder.icon(icon);
        }
        let tray = builder.build(app)?;
        *state.tray.lock().unwrap() = Some(tray);
        Ok(())
    }

    /// The app's icon, with the dot while messages wait.
    fn current_icon(app: &AppHandle) -> Option<tauri::image::Image<'static>> {
        let img = app.default_window_icon()?;
        if unread() == 0 {
            return Some(img.clone().to_owned());
        }
        Some(tauri::image::Image::new_owned(with_dot(img.rgba(), img.width(), img.height()), img.width(), img.height()))
    }

    fn hide_tray(app: &AppHandle) {
        let state = app.state::<AppState>();
        let guard = state.tray.lock().unwrap();
        if let Some(tray) = guard.as_ref() {
            let _ = tray.set_visible(false);
        }
    }

    fn refresh_sync(app: &AppHandle) {
        let state = app.state::<AppState>();
        let guard = state.tray.lock().unwrap();
        let Some(tray) = guard.as_ref() else { return };
        let labels = labels_of(app);
        let data = tauri::async_runtime::block_on(load_menu_data(app));
        if let Ok(menu) = build_menu(app, &labels, &data) {
            let _ = tray.set_menu(Some(menu));
            let tooltip = tooltip_text(&labels, data.running.len());
            let _ = tray.set_tooltip(Some(&tooltip));
        }
        let _ = tray.set_icon(current_icon(app));
    }

    pub fn apply(app: &AppHandle, want: bool) {
        let a = app.clone();
        let _ = app.run_on_main_thread(move || {
            if want {
                if let Err(e) = show_tray(&a) {
                    eprintln!("tray: failed to create icon: {e}");
                }
            } else {
                hide_tray(&a);
            }
        });
    }

    pub fn refresh(app: &AppHandle) {
        let a = app.clone();
        let _ = app.run_on_main_thread(move || refresh_sync(&a));
    }
}

// ── Public, platform-agnostic API ───────────────────────────────────────────

/// Show or hide the tray icon, from any thread.
pub fn apply_tray_async(app: &AppHandle, want: bool) {
    imp::apply(app, want);
}

/// Rebuild the tray menu/tooltip against the current state, from any thread.
pub fn refresh_tray_async(app: &AppHandle) {
    imp::refresh(app);
}

/// Align skip_taskbar with current visibility (visible → in taskbar).
/// Call after tray-setting changes so a still-open window is not orphaned.
pub fn sync_taskbar_to_visibility(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = a.get_webview_window("main") {
            let skip = !w.is_visible().unwrap_or(true);
            let _ = w.set_skip_taskbar(skip);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_sits_in_the_lower_right_corner() {
        let (w, h) = (32u32, 32u32);
        let icon = vec![0x10u8; (w * h * 4) as usize];
        let out = with_dot(&icon, w, h);
        let px = |x: u32, y: u32| &out[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        assert_eq!(px(w - 4, h - 4), &[0xE5, 0x48, 0x4D, 0xFF], "the dot");
        assert_eq!(px(2, 2), &[0x10; 4], "the icon elsewhere is as it was");
    }

    #[test]
    fn the_tooltip_says_what_waits() {
        let labels = TrayLabels::default();
        UNREAD.store(0, Ordering::Relaxed);
        assert!(!tooltip_text(&labels, 1).contains("unread"));
        UNREAD.store(3, Ordering::Relaxed);
        assert!(tooltip_text(&labels, 1).ends_with("3 unread"));
        UNREAD.store(0, Ordering::Relaxed);
    }
}
