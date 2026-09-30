//! Notifications of the desktop systems.
//!
//! One notification per `key` (a chat): showing a key again replaces what
//! is on the screen, clearing a key takes it away, a click hands the key
//! back. Knows nothing about the messenger: the caller words the text.
//!
//! Linux speaks `org.freedesktop.Notifications` over D-Bus. Where the
//! system cannot show anything (no notification daemon, no app bundle on
//! macOS), the notifier says so through [`Notifier::available`] and the
//! calls do nothing.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, watch};

#[cfg(target_os = "linux")]
mod linux;

/// Who is notifying.
#[derive(Clone, Debug)]
pub struct AppInfo {
    /// The name the system shows as the source.
    pub name: String,
    /// The desktop entry without `.desktop` (Linux: icon, grouping).
    pub desktop_entry: String,
    /// An icon name or an absolute path (Linux).
    pub icon: String,
}

/// One notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    /// What it is about; the same key replaces, a click returns it.
    pub key: String,
    pub title: String,
    /// Plain text, lines split by `\n`. Markup is escaped here.
    pub body: String,
    /// A picture of who wrote (a local file).
    pub image: Option<PathBuf>,
    /// No sound.
    pub silent: bool,
}

pub type OnClick = Arc<dyn Fn(String) + Send + Sync>;

pub(crate) enum Command {
    Show(Toast),
    Clear(String),
    ClearAll,
}

/// Handle to the notifications; cheap to clone, calls never block.
#[derive(Clone)]
pub struct Notifier {
    tx: mpsc::UnboundedSender<Command>,
    available: watch::Receiver<bool>,
}

impl Notifier {
    /// Starts the notifier on the current tokio runtime.
    pub fn start(app: AppInfo, on_click: OnClick) -> Self {
        Self::spawn(app, on_click, None)
    }

    /// Linux, tests: the notification server on a bus of its own.
    #[doc(hidden)]
    #[cfg(target_os = "linux")]
    pub fn start_on_bus(app: AppInfo, on_click: OnClick, address: String) -> Self {
        Self::spawn(app, on_click, Some(address))
    }

    fn spawn(app: AppInfo, on_click: OnClick, bus: Option<String>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let (up, available) = watch::channel(false);
        #[cfg(target_os = "linux")]
        tokio::spawn(linux::run(app, rx, up, on_click, bus));
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (app, on_click, up, bus);
            tokio::spawn(drain(rx));
        }
        Self { tx, available }
    }

    /// Whether the system shows notifications at all.
    pub fn available(&self) -> bool {
        *self.available.borrow()
    }

    pub fn show(&self, toast: Toast) {
        let _ = self.tx.send(Command::Show(toast));
    }

    pub fn clear(&self, key: &str) {
        let _ = self.tx.send(Command::Clear(key.to_string()));
    }

    pub fn clear_all(&self) {
        let _ = self.tx.send(Command::ClearAll);
    }
}

#[cfg(not(target_os = "linux"))]
async fn drain(mut rx: mpsc::UnboundedReceiver<Command>) {
    while rx.recv().await.is_some() {}
}

/// `&`, `<`, `>` as entities: some servers read the body as markup.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(escape("привет"), "привет");
    }
}
