// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! `--workdir <dir>`: an independent profile of the whole app.
//!
//! Everything the app persists (databases, browser profiles, notes, messenger,
//! webview storage) lives inside the directory, and the per-user resources
//! (single-instance lock, capture socket, tray id) are keyed by it, so several
//! profiles run side by side. Without the flag nothing changes.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{Manager, Runtime, WebviewWindowBuilder};

const FLAG: &str = "workdir";
/// Same as the flag; the flag wins when both are given.
pub const ENV_VAR: &str = "VEYDAN_WORKDIR";

static CURRENT: OnceLock<Option<Workdir>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct Workdir {
    /// Absolute data directory of this profile.
    pub path: PathBuf,
    /// Short stable id derived from `path`; suffixes per-user resource names.
    pub tag: String,
    /// Directory name, shown in the window title and the tray.
    pub name: String,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    store_id: [u8; 16],
}

impl Workdir {
    fn new(path: PathBuf) -> Self {
        // Windows paths are case-insensitive: one directory, one profile.
        #[cfg(windows)]
        let key = path.to_string_lossy().to_lowercase();
        #[cfg(not(windows))]
        let key = path.to_string_lossy().into_owned();

        let digest = Sha256::digest(key.as_bytes());
        let tag = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
        let mut store_id = [0u8; 16];
        store_id.copy_from_slice(&digest[..16]);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self {
            path,
            tag,
            name,
            store_id,
        }
    }

    /// Webview storage (localStorage etc.) of this profile.
    pub fn webview_dir(&self) -> PathBuf {
        self.path.join("webview")
    }

    /// Key the single-instance lock by the profile and take the config windows
    /// over: their data directory can only be set from code.
    pub fn apply_to_context<R: Runtime>(&self, context: &mut tauri::Context<R>) {
        let config = context.config_mut();
        // `w`: a D-Bus name element must not start with a digit.
        config.identifier = format!("{}.w{}", config.identifier, self.tag);
        for window in &mut config.app.windows {
            window.create = false;
            window.title = format!("Veydan Space — {}", self.name);
        }
    }

    /// Create the windows `apply_to_context` held back.
    pub fn create_config_windows<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> tauri::Result<()> {
        for config in app.config().app.windows.clone() {
            self.isolate(WebviewWindowBuilder::from_config(app, &config)?)
                .build()?;
        }
        Ok(())
    }

    fn isolate<'a, R: Runtime, M: Manager<R>>(
        &self,
        builder: WebviewWindowBuilder<'a, R, M>,
    ) -> WebviewWindowBuilder<'a, R, M> {
        // WKWebView has no data directory; the data store needs macOS 14+,
        // older systems fall back to the shared default store.
        #[cfg(target_os = "macos")]
        {
            builder.data_store_identifier(self.store_id)
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder.data_directory(self.webview_dir())
        }
    }
}

/// Profile of this process; `None` for the default one.
pub fn current() -> Option<&'static Workdir> {
    CURRENT.get().and_then(|w| w.as_ref())
}

/// Resolve the profile from the command line / environment. Call once at startup.
pub fn init() -> Result<Option<&'static Workdir>, String> {
    let requested = match parse_args(std::env::args().skip(1))? {
        Some(dir) => Some(dir),
        None => std::env::var(ENV_VAR).ok().filter(|v| !v.trim().is_empty()),
    };
    let workdir = match requested {
        Some(dir) => Some(Workdir::new(prepare(Path::new(&dir))?)),
        None => None,
    };
    Ok(CURRENT.get_or_init(|| workdir).as_ref())
}

/// Give a window built in code the profile's webview storage.
pub fn apply_to_window<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    match current() {
        Some(workdir) => workdir.isolate(builder),
        None => builder,
    }
}

/// Append the profile name to a tray caption.
pub fn caption(text: String) -> String {
    match current() {
        Some(workdir) => format!("{text} [{}]", workdir.name),
        None => text,
    }
}

/// Accepts `--workdir DIR`, `--workdir=DIR` and the single-dash spelling.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<String>, String> {
    let mut args = args;
    let mut found = None;
    while let Some(arg) = args.next() {
        let Some(rest) = arg
            .strip_prefix("--")
            .or_else(|| arg.strip_prefix('-'))
            .and_then(|a| a.strip_prefix(FLAG))
        else {
            continue;
        };
        let value = match rest.strip_prefix('=') {
            Some(v) => v.to_string(),
            None if rest.is_empty() => args.next().unwrap_or_default(),
            // `--workdirs`, `--workdir-x`: not ours.
            None => continue,
        };
        if value.trim().is_empty() {
            return Err(format!("--{FLAG} requires a directory"));
        }
        found = Some(value);
    }
    Ok(found)
}

/// Absolute path of an existing directory. Relative paths resolve against the
/// current directory. Not `canonicalize`: its `\\?\` paths on Windows break
/// the browser launch.
fn prepare(dir: &Path) -> Result<PathBuf, String> {
    let path = std::path::absolute(dir)
        .map_err(|e| format!("--{FLAG}: bad path {}: {e}", dir.display()))?;
    // Drops the trailing separator of `vasa/`, so both spellings give one tag.
    let path: PathBuf = path.components().collect();
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("--{FLAG}: cannot create {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<String>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_flag() {
        assert_eq!(parse(&[]), Ok(None));
        assert_eq!(parse(&["--other", "x", "--workdirs=y"]), Ok(None));
    }

    #[test]
    fn all_spellings() {
        for args in [
            &["--workdir", "vasa"][..],
            &["--workdir=vasa"],
            &["-workdir", "vasa"],
            &["-workdir=vasa"],
            &["--first", "--workdir", "vasa", "--last"],
        ] {
            assert_eq!(parse(args), Ok(Some("vasa".into())), "{args:?}");
        }
    }

    #[test]
    fn missing_value() {
        assert!(parse(&["--workdir"]).is_err());
        assert!(parse(&["--workdir="]).is_err());
    }

    #[test]
    fn tag_is_stable_and_dbus_safe() {
        let a = Workdir::new(PathBuf::from("/tmp/vasa"));
        let b = Workdir::new(PathBuf::from("/tmp/vasa"));
        let c = Workdir::new(PathBuf::from("/tmp/petya"));
        assert_eq!(a.tag, b.tag);
        assert_ne!(a.tag, c.tag);
        assert_eq!(a.tag.len(), 8);
        assert!(a.tag.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(a.name, "vasa");
    }

    #[test]
    fn trailing_separator_is_the_same_profile() {
        let base = std::env::temp_dir().join(format!("veydan-workdir-{}", std::process::id()));
        let plain = prepare(&base).unwrap();
        let slashed = prepare(Path::new(&format!("{}/", base.display()))).unwrap();
        assert_eq!(plain, slashed);
        assert!(plain.is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }
}
