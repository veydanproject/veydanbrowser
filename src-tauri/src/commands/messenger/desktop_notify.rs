// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Notifications of a computer. What `Notifier.kt` is to the phone.
//!
//! No push is involved: the app runs, the session takes a message in and
//! raises a `notify` UiEvent (own, old, deleted messages and muted chats
//! are already left out). Here that becomes one of two things:
//!
//! - the window is on the screen and in focus: the card in the app says it
//!   (the event goes on to the page as it is);
//! - otherwise: a notification of the system, and the event is marked
//!   `os: true`, so the page shows no card when the window comes back.
//!
//! One notification per chat. A chat that writes again replaces it with the
//! last lines and a "+N"; reading the chat takes it away; a click brings
//! the window up and opens the chat.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use desktop_notify::{AppInfo, Notifier, Toast};
use messenger_notify::{Body, ChatKind, DesktopSettings, Outcome};
use messenger_runtime::MessengerRuntime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

use crate::error::{AppError, CmdResult};
use crate::AppState;

/// Emitted with `{chat}` when a notification is clicked.
pub const EVENT_NOTICE_TAP: &str = "messenger://notice-tap";

/// Lines of a chat's notification; older ones become "+N".
const MAX_LINES: usize = 5;
/// The key of notices that name no chat.
const NO_CHAT: &str = "dm";

const AVATAR_TIMEOUT: Duration = Duration::from_millis(2500);
const AVATAR_MAX_BYTES: usize = 2 * 1024 * 1024;

/// The words of the user's language. The page sends them (`i18n.ts` holds
/// every string of the app); these are the words until it does.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Words {
    pub app: String,
    pub new_message: String,
    /// `{n}` is the number.
    pub new_messages: String,
    /// `{n}` is the number.
    pub more: String,
    pub request: String,
    pub group_invite: String,
    pub group_request: String,
    pub group_welcome: String,
}

impl Default for Words {
    fn default() -> Self {
        Self {
            app: "Veydan Space".into(),
            new_message: "New message".into(),
            new_messages: "{n} new messages".into(),
            more: "+{n} more".into(),
            request: "Message request".into(),
            group_invite: "Invites you to a group".into(),
            group_request: "Asks to join the group".into(),
            group_welcome: "You are in the group".into(),
        }
    }
}

/// What the window looks like to the user right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowSeen {
    pub focused: bool,
    pub visible: bool,
    pub minimized: bool,
}

impl WindowSeen {
    fn of(app: &tauri::AppHandle) -> Self {
        let Some(w) = app.get_webview_window("main") else { return Self::default() };
        Self {
            focused: w.is_focused().unwrap_or(false),
            visible: w.is_visible().unwrap_or(false),
            minimized: w.is_minimized().unwrap_or(false),
        }
    }

    fn on_screen(self) -> bool {
        self.focused && self.visible && !self.minimized
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The card in the app.
    Card,
    /// A notification of the system.
    System,
}

/// The one decision: who tells the user. When the system cannot show
/// anything, or the user turned it off, the card is left to do it.
pub fn route(window: WindowSeen, enabled: bool, available: bool) -> Route {
    if window.on_screen() || !enabled || !available {
        Route::Card
    } else {
        Route::System
    }
}

/// What one chat's notification says so far.
#[derive(Default)]
struct Stack {
    lines: VecDeque<String>,
    count: u32,
}

impl Stack {
    fn push(&mut self, line: Option<String>) {
        self.count += 1;
        if let Some(line) = line {
            self.lines.push_back(line);
            while self.lines.len() > MAX_LINES {
                self.lines.pop_front();
            }
        }
    }

    /// The last lines, and "+N" for the messages they leave out.
    fn body(&self, words: &Words) -> String {
        if self.lines.is_empty() {
            return if self.count > 1 {
                words.new_messages.replace("{n}", &self.count.to_string())
            } else {
                words.new_message.clone()
            };
        }
        let mut out: Vec<String> = self.lines.iter().cloned().collect();
        let left_out = self.count.saturating_sub(self.lines.len() as u32);
        if left_out > 0 {
            out.push(words.more.replace("{n}", &left_out.to_string()));
        }
        out.join("\n")
    }
}

/// A notice as a notification: its key, title, the line it adds (None: it
/// only counts), and the sender's picture.
#[derive(Debug, PartialEq, Eq)]
pub struct Worded {
    pub key: String,
    pub title: String,
    pub line: Option<String>,
    pub picture: Option<String>,
}

pub fn word(outcome: Outcome, words: &Words) -> Option<Worded> {
    match outcome {
        Outcome::Show(n) => {
            let text = match n.body {
                None => words.new_message.clone(),
                Some(Body::Text { text }) => text,
                Some(Body::Media { name, caption, .. }) => format!("📎 {}", caption.unwrap_or(name)),
                Some(Body::Invite { group_name }) => format!("{}: {group_name}", words.group_invite),
                Some(Body::JoinRequest { group_name }) => format!("{}: {group_name}", words.group_request),
                Some(Body::Welcome { group_name }) => format!("{}: {group_name}", words.group_welcome),
            };
            let line = match n.kind {
                ChatKind::Group => format!("{}: {text}", n.sender),
                ChatKind::Request => format!("{} · {text}", words.request),
                ChatKind::Dm => text,
            };
            Some(Worded {
                key: n.chat.unwrap_or_else(|| NO_CHAT.into()),
                title: n.title,
                line: Some(line),
                picture: n.picture,
            })
        }
        Outcome::Plain(p) => Some(Worded {
            key: p.chat.unwrap_or_else(|| NO_CHAT.into()),
            title: p.title.unwrap_or_else(|| words.app.clone()),
            line: None,
            picture: None,
        }),
        Outcome::Quiet { .. } => None,
    }
}

pub struct DesktopNotify {
    app: tauri::AppHandle,
    rt: Arc<MessengerRuntime>,
    notifier: Notifier,
    words: RwLock<Words>,
    stacks: Mutex<HashMap<String, Stack>>,
    avatars: Option<PathBuf>,
}

impl DesktopNotify {
    /// Needs the tokio runtime of Tauri; call from `setup`.
    pub fn start(app: tauri::AppHandle, rt: Arc<MessengerRuntime>) -> Arc<Self> {
        let info = AppInfo {
            name: app.package_info().name.clone(),
            desktop_entry: "veydanspace".into(),
            icon: "veydanspace".into(),
        };
        let tapped = app.clone();
        let on_click = Arc::new(move |key: String| clicked(&tapped, key));
        let notifier = tauri::async_runtime::block_on(async move { Notifier::start(info, on_click) });
        let avatars = app.path().app_cache_dir().ok().map(|d| d.join("notify-avatars"));
        Arc::new(Self {
            app,
            rt,
            notifier,
            words: RwLock::new(Words::default()),
            stacks: Mutex::new(HashMap::new()),
            avatars,
        })
    }

    pub fn available(&self) -> bool {
        self.notifier.available()
    }

    pub fn set_words(&self, words: Words) {
        *self.words.write().unwrap() = words;
    }

    /// Takes a `notify` payload. True when the system shows it: the card
    /// is then not to.
    pub async fn take(self: &Arc<Self>, payload: &serde_json::Value) -> bool {
        let Ok(notice) = serde_json::from_value::<messenger_core::Notice>(payload.clone()) else {
            return false;
        };
        // The module is hidden: the user does not want to hear from it.
        match self.app.try_state::<AppState>() {
            Some(state) if super::read_enabled(&state.db).await => {}
            _ => return false,
        }
        let enabled = self.rt.desktop_notify_settings().await.map(|s| s.enabled).unwrap_or(true);
        if route(WindowSeen::of(&self.app), enabled, self.available()) == Route::Card {
            return false;
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.show(notice).await });
        true
    }

    async fn show(&self, notice: messenger_core::Notice) {
        let locked = match self.app.try_state::<AppState>() {
            Some(state) => crate::commands::notes::lock::lock_enabled(&state).await,
            None => true,
        };
        let outcome = match self.rt.live_notice(&notice, locked).await {
            Ok(o) => o,
            Err(e) => {
                eprintln!("messenger notify: {e}");
                return;
            }
        };
        let words = self.words.read().unwrap().clone();
        let Some(w) = word(outcome, &words) else { return };
        let sound = self.rt.desktop_notify_settings().await.map(|s| s.sound).unwrap_or(true);
        let image = match &w.picture {
            Some(url) => self.avatar(url).await,
            None => None,
        };
        let body = {
            let mut stacks = self.stacks.lock().unwrap();
            let stack = stacks.entry(w.key.clone()).or_default();
            stack.push(w.line);
            stack.body(&words)
        };
        self.notifier.show(Toast { key: w.key, title: w.title, body, image, silent: !sound });
    }

    /// A notification to see that notifications work, from the settings.
    pub async fn show_test(&self, title: String, body: String) {
        let sound = self.rt.desktop_notify_settings().await.map(|s| s.sound).unwrap_or(true);
        self.notifier.show(Toast { key: "test".into(), title, body, image: None, silent: !sound });
    }

    /// Takes the chat's notification away (it was read), or all of them.
    pub fn clear(&self, key: Option<&str>) {
        let mut stacks = self.stacks.lock().unwrap();
        match key {
            Some(key) => {
                stacks.remove(key);
                self.notifier.clear(key);
                // What came about a person before a chat existed.
                if key.starts_with("dm:") && stacks.remove(NO_CHAT).is_some() {
                    self.notifier.clear(NO_CHAT);
                }
            }
            None => {
                stacks.clear();
                self.notifier.clear_all();
            }
        }
    }

    /// The sender's picture as a file the system can read. Nothing when it
    /// cannot be had quickly: the notification goes without it.
    async fn avatar(&self, url: &str) -> Option<PathBuf> {
        let dir = self.avatars.as_ref()?;
        let name: String = Sha256::digest(url.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let path = dir.join(name);
        if path.exists() {
            return Some(path);
        }
        let client = reqwest::Client::builder().timeout(AVATAR_TIMEOUT).build().ok()?;
        let resp = client.get(url).send().await.ok()?.error_for_status().ok()?;
        if resp.content_length().is_some_and(|l| l as usize > AVATAR_MAX_BYTES) {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() > AVATAR_MAX_BYTES {
            return None;
        }
        tokio::fs::create_dir_all(dir).await.ok()?;
        tokio::fs::write(&path, &bytes).await.ok()?;
        Some(path)
    }
}

/// A click: the window comes up and the page opens the chat.
fn clicked(app: &tauri::AppHandle, key: String) {
    crate::tray::show_from_tray(app);
    let chat = (key.starts_with("dm:") || key.starts_with("group:")).then_some(key.clone());
    if let Some(d) = app.try_state::<AppState>().and_then(|s| s.messenger.desktop()) {
        d.stacks.lock().unwrap().remove(&key);
    }
    let _ = app.emit(EVENT_NOTICE_TAP, serde_json::json!({ "chat": chat }));
}

// ─── Commands ───────────────────────────────────────────────────────────────

/// The settings of this computer's notifications, and what stands in their way.
#[derive(Debug, Clone, Serialize)]
pub struct DesktopNotifyView {
    pub enabled: bool,
    pub sound: bool,
    /// The system can show notifications (a notification server, an app bundle).
    pub available: bool,
    /// Closing the window keeps the app running; otherwise nothing comes after it.
    pub close_to_tray: bool,
}

fn desktop(state: &AppState) -> CmdResult<Arc<DesktopNotify>> {
    state.messenger.desktop().ok_or_else(|| AppError::Other("notify_unavailable".into()))
}

#[tauri::command]
pub async fn messenger_desktop_notify_get(state: tauri::State<'_, AppState>) -> CmdResult<DesktopNotifyView> {
    let d = desktop(&state)?;
    let s = d.rt.desktop_notify_settings().await.map_err(super::map_err)?;
    Ok(DesktopNotifyView {
        enabled: s.enabled,
        sound: s.sound,
        available: d.available(),
        close_to_tray: state.tray_settings.close_to_tray.load(std::sync::atomic::Ordering::Relaxed),
    })
}

#[tauri::command]
pub async fn messenger_desktop_notify_set(
    enabled: bool,
    sound: bool,
    state: tauri::State<'_, AppState>,
) -> CmdResult<DesktopNotifyView> {
    let d = desktop(&state)?;
    d.rt.desktop_notify_set_settings(DesktopSettings { enabled, sound }).await.map_err(super::map_err)?;
    if !enabled {
        d.clear(None);
    }
    messenger_desktop_notify_get(state).await
}

#[tauri::command]
pub async fn messenger_desktop_notify_test(
    title: String,
    body: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<()> {
    desktop(&state)?.show_test(title, body).await;
    Ok(())
}

/// The page's words for notifications, in the user's language.
#[tauri::command]
pub fn messenger_notice_words(words: Words, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    desktop(&state)?.set_words(words);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use messenger_notify::{Notice, Plain, Reason};

    const ON_SCREEN: WindowSeen = WindowSeen { focused: true, visible: true, minimized: false };

    #[test]
    fn the_card_while_the_window_is_seen_the_system_otherwise() {
        assert_eq!(route(ON_SCREEN, true, true), Route::Card);
        for w in [
            WindowSeen { focused: false, ..ON_SCREEN },
            WindowSeen { visible: false, ..ON_SCREEN },
            WindowSeen { minimized: true, ..ON_SCREEN },
        ] {
            assert_eq!(route(w, true, true), Route::System, "{w:?}");
            assert_eq!(route(w, false, true), Route::Card, "turned off");
            assert_eq!(route(w, true, false), Route::Card, "nothing to show it with");
        }
    }

    fn words() -> Words {
        Words::default()
    }

    #[test]
    fn a_stack_keeps_the_last_lines_and_counts_the_rest() {
        let mut s = Stack::default();
        s.push(Some("one".into()));
        assert_eq!(s.body(&words()), "one");
        for i in 2..=7 {
            s.push(Some(format!("line {i}")));
        }
        assert_eq!(s.body(&words()), "line 3\nline 4\nline 5\nline 6\nline 7\n+2 more");
    }

    #[test]
    fn a_plain_stack_only_counts() {
        let mut s = Stack::default();
        s.push(None);
        assert_eq!(s.body(&words()), "New message");
        s.push(None);
        assert_eq!(s.body(&words()), "2 new messages");
    }

    fn notice(kind: ChatKind, body: Option<Body>) -> Outcome {
        Outcome::Show(Notice {
            kind,
            chat: Some("group:g".into()),
            title: "Team".into(),
            sender: "Alice".into(),
            sender_key: String::new(),
            picture: None,
            body,
            muted: false,
            hide_on_lockscreen: false,
            count: 1,
        })
    }

    #[test]
    fn lines_say_who_wrote_where_it_matters() {
        let text = Some(Body::Text { text: "hi".into() });
        let w = word(notice(ChatKind::Group, text.clone()), &words()).unwrap();
        assert_eq!((w.key.as_str(), w.title.as_str(), w.line.as_deref()), ("group:g", "Team", Some("Alice: hi")));
        assert_eq!(word(notice(ChatKind::Dm, text.clone()), &words()).unwrap().line.as_deref(), Some("hi"));
        assert_eq!(
            word(notice(ChatKind::Request, text), &words()).unwrap().line.as_deref(),
            Some("Message request · hi")
        );
        assert_eq!(word(notice(ChatKind::Dm, None), &words()).unwrap().line.as_deref(), Some("New message"));
        assert_eq!(
            word(notice(ChatKind::Dm, Some(Body::Invite { group_name: "X".into() })), &words()).unwrap().line.as_deref(),
            Some("Invites you to a group: X")
        );
    }

    #[test]
    fn plain_and_quiet() {
        let p = Outcome::Plain(Plain { kind: ChatKind::Dm, chat: None, title: None, muted: false, count: 1 });
        let w = word(p, &words()).unwrap();
        assert_eq!((w.key.as_str(), w.title.as_str(), w.line), (NO_CHAT, "Veydan Space", None));
        assert_eq!(word(Outcome::Quiet { reason: Reason::Own }, &words()), None);
    }
}
