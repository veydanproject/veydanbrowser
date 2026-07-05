// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::AppState;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

const MOZLZ4_MAGIC: &[u8] = b"mozLz40\x00";

// ── Download state ─────────────────────────────────────────────────────────

#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadState {
    Idle,
    Downloading {
        downloaded: u64,
        total: u64,
        percent: u8,
    },
    Done {
        version: String,
    },
    Failed {
        error: String,
    },
}

pub struct DownloadManager {
    pub state: Arc<Mutex<DownloadState>>,
    cancel_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl Default for DownloadManager {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(DownloadState::Idle)),
            cancel_tx: Arc::new(Mutex::new(None)),
        }
    }
}

// ── GitHub types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

// ── Binary resolution ───────────────────────────────────────────────────────

pub fn local_binary_path(app_data_dir: &PathBuf) -> PathBuf {
    let dir = app_data_dir.join("camoufox");
    if cfg!(target_os = "windows") {
        dir.join("camoufox.exe")
    } else {
        dir.join("camoufox")
    }
}

pub fn resolve_binary(app_data_dir: &PathBuf) -> Option<PathBuf> {
    let local = local_binary_path(app_data_dir);
    if local.exists() {
        return Some(local);
    }
    which::which("camoufox").ok()
}

// ── omni.ja patching ────────────────────────────────────────────────────────

/// Search engine config injected into SearchEngineSelector.sys.mjs.
/// Replaces the broken hardcoded `if (true) { return [...] }` block in Camoufox.
/// Engines: DuckDuckGo (default), Google, Brave Search, Startpage.
const SEARCH_ENGINE_BLOCK: &str = r#"if (true) {
      return [
        {"recordType":"engine","identifier":"ddg","id":"ddg-engine","last_modified":1702906502241,"base":{"aliases":["duckduckgo","ddg"],"classification":"general","name":"DuckDuckGo","urls":{"search":{"base":"https://duckduckgo.com/","searchTermParamName":"q"}}},"variants":[{"environment":{"allRegionsAndLocales":true}}]},
        {"recordType":"engine","identifier":"google","id":"google-engine","last_modified":1702906502241,"base":{"aliases":["google"],"classification":"general","name":"Google","urls":{"search":{"base":"https://www.google.com/search","searchTermParamName":"q"}}},"variants":[{"environment":{"allRegionsAndLocales":true}}]},
        {"recordType":"engine","identifier":"brave","id":"brave-engine","last_modified":1702906502241,"base":{"aliases":["brave"],"classification":"general","name":"Brave Search","urls":{"search":{"base":"https://search.brave.com/search","searchTermParamName":"q"}}},"variants":[{"environment":{"allRegionsAndLocales":true}}]},
        {"recordType":"engine","identifier":"startpage","id":"startpage-engine","last_modified":1702906502241,"base":{"aliases":["startpage"],"classification":"general","name":"Startpage","urls":{"search":{"base":"https://www.startpage.com/search","searchTermParamName":"q"}}},"variants":[{"environment":{"allRegionsAndLocales":true}}]},
        {"recordType":"defaultEngines","id":"default-engines","last_modified":1702906502241,"globalDefault":"ddg","specificDefaults":[]},
        {"recordType":"availableLocales","id":"available-locales","last_modified":1702906502241,"locales":[]}
      ];
    }"#;

/// Marker file that stores the mtime (seconds) of omni.ja after the last successful patch.
fn patch_marker_path(install_dir: &Path) -> PathBuf {
    install_dir.join(".rb_patch_mtime")
}

fn file_mtime_secs(path: &Path) -> u64 {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Reads all zip entries into memory and writes a new zip replacing specified files.
fn patch_zip_file(zip_path: &Path, replacements: &[(&str, &[u8])]) -> Result<(), String> {
    use std::io::Read;
    use zip::write::SimpleFileOptions;
    use zip::ZipArchive;
    use zip::ZipWriter;

    // Read all entries into memory
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    {
        let src = std::fs::File::open(zip_path)
            .map_err(|e| format!("open {}: {e}", zip_path.display()))?;
        let mut archive = ZipArchive::new(src)
            .map_err(|e| format!("read zip {}: {e}", zip_path.display()))?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| format!("entry {i}: {e}"))?;
            let name = entry.name().to_string();
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("read {name}: {e}"))?;
            entries.push((name, buf));
        }
    }

    let replacement_map: std::collections::HashMap<&str, &[u8]> =
        replacements.iter().copied().collect();

    let tmp_path = zip_path.with_extension("tmp");
    {
        let dst = std::fs::File::create(&tmp_path)
            .map_err(|e| format!("create tmp: {e}"))?;
        let mut writer = ZipWriter::new(dst);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (name, content) in &entries {
            writer
                .start_file(name, options)
                .map_err(|e| format!("start_file {name}: {e}"))?;
            if let Some(&new_content) = replacement_map.get(name.as_str()) {
                writer
                    .write_all(new_content)
                    .map_err(|e| format!("write replacement {name}: {e}"))?;
            } else {
                writer
                    .write_all(content)
                    .map_err(|e| format!("write {name}: {e}"))?;
            }
        }
        writer.finish().map_err(|e| format!("zip finish: {e}"))?;
    }

    std::fs::rename(&tmp_path, zip_path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Patches SearchEngineSelector.sys.mjs and Utils.sys.mjs inside toolkit/omni.ja.
///
/// SearchEngineSelector: replaces the broken `if (true) { return [...] }` block with
/// a valid engine config (DDG, Google, Brave, Startpage).
///
/// Utils.sys.mjs: patches LOAD_DUMPS getter to always return `true`, so local
/// search config dumps from omni.ja are used WITHOUT requiring a live Remote Settings
/// server URL. This prevents live Mozilla RS syncs from overriding our engine config.
fn patch_toolkit_omni(install_dir: &Path) -> Result<(), String> {
    let omni_path = install_dir.join("omni.ja");
    if !omni_path.exists() {
        return Ok(());
    }

    use std::io::Read;
    use zip::ZipArchive;

    let selector_target = "moz-src/toolkit/components/search/SearchEngineSelector.sys.mjs";
    let utils_target = "modules/services-settings/Utils.sys.mjs";

    let src = std::fs::File::open(&omni_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(src).map_err(|e| e.to_string())?;

    // Patch SearchEngineSelector.sys.mjs
    let patched_selector = {
        let mut entry = match archive.by_name(selector_target) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&buf);

        let marker = "if (true) {";
        let Some(start) = text.find(marker) else {
            return Ok(()); // already patched or structure changed
        };

        let mut depth = 0usize;
        let mut end = start;
        for (i, ch) in text[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        format!("{}{}{}", &text[..start], SEARCH_ENGINE_BLOCK, &text[end..])
    };

    // Patch Utils.sys.mjs: force LOAD_DUMPS to always return true.
    // This enables local omni.ja dumps without needing a live RS server URL,
    // preventing Mozilla's live search config from overriding our engine config.
    let patched_utils = {
        let mut entry = match archive.by_name(utils_target) {
            Ok(e) => e,
            Err(_) => None.ok_or("Utils.sys.mjs not found")?,
        };
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&buf).into_owned();

        // Replace the LOAD_DUMPS return expression: `return [...].includes(server)` → `return true`
        // The marker is unique enough to identify the right location
        if let Some(pos) = text.find("].includes(server)") {
            // Walk backwards from pos to find `return [`
            if let Some(ret_pos) = text[..pos].rfind("return [") {
                let before = &text[..ret_pos];
                let after = &text[pos + "].includes(server)".len()..];
                format!("{}return true{}", before, after)
            } else {
                text
            }
        } else {
            text // already patched or different structure
        }
    };

    drop(archive);

    patch_zip_file(
        &omni_path,
        &[
            (selector_target, patched_selector.as_bytes()),
            (utils_target, patched_utils.as_bytes()),
        ],
    )
}

/// Clears search-config-overrides-v2.json in browser/omni.ja to prevent parse errors.
fn patch_browser_omni(install_dir: &Path) -> Result<(), String> {
    let omni_path = install_dir.join("browser").join("omni.ja");
    if !omni_path.exists() {
        return Ok(());
    }

    let empty_overrides = br#"{"data":[],"timestamp":0}"#;
    patch_zip_file(
        &omni_path,
        &[(
            "defaults/settings/main/search-config-overrides-v2.json",
            empty_overrides,
        )],
    )
}

/// Patches both omni.ja files. Safe to call even if already patched (mtime guard in ensure_omni_patched).
pub fn patch_omni_ja(install_dir: &Path) -> Result<(), String> {
    patch_toolkit_omni(install_dir)?;
    patch_browser_omni(install_dir)?;
    Ok(())
}

/// Deletes startupCache and Remote Settings IndexedDB for all profiles.
/// Called after patching omni.ja so Firefox rebuilds JS caches and settings from scratch.
pub fn clear_all_startup_caches(app_data_dir: &Path) {
    let profiles_dir = app_data_dir.join("profiles");
    let Ok(entries) = std::fs::read_dir(&profiles_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let ff_profile = entry.path().join("firefox-profile");

        let startup_cache = ff_profile.join("startupCache");
        if startup_cache.is_dir() {
            std::fs::remove_dir_all(&startup_cache).ok();
        }

        // Clear Remote Settings IndexedDB so stale overrides don't block new engines
        let idb_dir = ff_profile
            .join("storage")
            .join("permanent")
            .join("chrome")
            .join("idb");
        if idb_dir.is_dir() {
            std::fs::remove_dir_all(&idb_dir).ok();
        }
    }
}

/// Checks whether omni.ja has changed since the last patch (by mtime).
/// If so, re-patches and clears caches. Idempotent.
pub fn ensure_omni_patched(install_dir: &Path, app_data_dir: &Path) {
    let omni_path = install_dir.join("omni.ja");
    if !omni_path.exists() {
        return;
    }

    let current_mtime = file_mtime_secs(&omni_path);
    let marker = patch_marker_path(install_dir);
    let stored_mtime: u64 = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    if current_mtime == stored_mtime {
        return; // omni.ja unchanged since last patch — skip
    }

    if let Err(e) = patch_omni_ja(install_dir) {
        eprintln!("[Veydan Browser] omni.ja patch failed: {e}");
        return;
    }

    // Store mtime of the patched omni.ja
    let new_mtime = file_mtime_secs(&omni_path);
    std::fs::write(&marker, new_mtime.to_string()).ok();

    clear_all_startup_caches(app_data_dir);
}

fn build_stripe_bg(color_left: &str, color_right: &str, label: &str) -> String {
    let gradient = format!("linear-gradient(to right, {color_left}, {color_right})");
    if label.is_empty() {
        return gradient;
    }
    let safe = label
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 10'>\
         <text x='50' y='8' text-anchor='middle' fill='white' fill-opacity='0.8' \
         font-size='7' font-family='system-ui,sans-serif'>{safe}</text></svg>"
    );
    let encoded = svg
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('#', "%23");
    format!("url(\"data:image/svg+xml,{encoded}\"), {gradient}")
}

/// Patches shared chrome.css in the Camoufox install dir.
/// Applies structural fixes + full UI block + profile-specific color stripe.
/// Idempotent — safe to call on every launch.
pub fn patch_chrome_css(
    install_dir: &std::path::Path,
    color_left: &str,
    color_right: &str,
    label: &str,
) {
    let css_path = install_dir.join("chrome.css");
    let Ok(content) = std::fs::read_to_string(&css_path) else {
        return;
    };

    // Remove any previous Veydan Browser block (idempotent cleanup)
    let mut patched = content.clone();
    if let Some(start) = patched.find("\n/* Veydan Browser:") {
        patched.truncate(start);
    }

    let mut patched = patched
        // Show tab close button (hidden by camoufox minimalistic theme)
        .replace(
            ".tab-close-button,\n#TabsToolbar .toolbarbutton-1 {",
            "#TabsToolbar .toolbarbutton-1 {",
        )
        // Exclude new-tab buttons from the toolbarbutton-1 hide rule
        .replace(
            "#TabsToolbar .toolbarbutton-1 {\n  display: none !important;\n}",
            "#TabsToolbar .toolbarbutton-1:not(#new-tab-button):not(#tabs-newtab-button) {\n  display: none !important;\n}",
        )
        // Re-enable pointer events so tabs are clickable and draggable
        .replace(
            "  pointer-events: none !important;",
            "  pointer-events: auto !important;",
        )
        // Prevent tabs from acting as window-drag areas
        .replace(
            "  -moz-window-dragging: inherit !important;",
            "  -moz-window-dragging: no-drag !important;",
        )
        // Fix tabs being stretched to full window width
        .replace(
            "  max-width: 100% !important;",
            "  max-width: 240px !important;",
        );

    let stripe_bg = build_stripe_bg(color_left, color_right, label);

    patched.push_str(&format!(
        r#"
/* Veydan Browser: UI overrides */

.browser-titlebar,
#main-menubar {{
  background-color: #282829 !important;
}}

:root {{
  --tab-min-height: 22px !important;
  --tab-max-height: 22px !important;
  --toolbar-field-height: 28px !important;
}}
.tabbrowser-tab,
.tab-background {{
  max-height: unset !important;
}}

.tab-background {{
  border-radius: 4px 4px 0 0 !important;
  margin-inline: 1px !important;
  border: none !important;
  box-shadow: none !important;
}}

.tabbrowser-tab[selected="true"] .tab-background {{
  background-color: #494949 !important;
  outline: none !important;
  box-shadow: none !important;
}}

.tabbrowser-tab:not([selected]) .tab-background {{
  background-color: transparent !important;
  border: none !important;
}}
.tabbrowser-tab:not([selected]):hover .tab-background {{
  background-color: rgba(255,255,255,0.08) !important;
}}

/* Profile color stripe + padding */
#TabsToolbar {{
  background-image: {stripe_bg} !important;
  background-size: 100% 10px !important;
  background-position: top !important;
  background-repeat: no-repeat !important;
  padding-block-start: 14px !important;
  padding-block-end: 0 !important;
  border-block-end: none !important;
}}

#navigator-toolbox #nav-bar {{
  min-height: unset !important;
  max-height: unset !important;
  padding-block: 4px !important;
  margin-block-start: 0 !important;
  border-block-start: 1px solid rgba(255,255,255,0.08) !important;
}}

.titlebar-spacer[type="pre-tabs"]  {{ width: 0 !important; }}
.titlebar-spacer[type="post-tabs"] {{ width: 0 !important; }}

.titlebar-buttonbox-container {{
  width: auto !important;
  border-inline-start: 1px solid rgba(255,255,255,0.12) !important;
  padding-inline-start: 8px !important;
  margin-inline-start: 4px !important;
}}

/* New tab "+" button — force visible, override tabs.css attribute-based hide rules */
#new-tab-button,
#tabs-newtab-button {{
  display: -moz-box !important;
  visibility: visible !important;
  color: rgba(255,255,255,0.6) !important;
  width: 26px !important;
  min-width: 26px !important;
  padding: 0 !important;
  margin-inline-start: 2px !important;
  border-radius: 4px !important;
  list-style-image: url(chrome://global/skin/icons/plus.svg) !important;
}}
#new-tab-button:hover,
#tabs-newtab-button:hover {{
  background: rgba(255,255,255,0.1) !important;
  color: rgba(255,255,255,0.9) !important;
}}

/* Bookmarks toolbar */
#PersonalToolbar {{
  display: flex !important;
  visibility: visible !important;
}}

/* Add-to-bookmarks star button in URL bar */
#star-button-box {{
  display: flex !important;
  visibility: visible !important;
}}

/* Override Camoufox dimension locks (set via inline style when window.outerWidth/outerHeight
   is passed in CAMOU_CONFIG_1). Camoufox sets height on both :root and #browser.
   :root gets height:100% (fills GTK window) instead of auto (which would collapse).
   #browser gets flex:1 so it fills remaining space after toolbars. */
html, :root {{ width: auto !important; height: 100% !important; }}
#browser {{ width: auto !important; height: auto !important; flex: 1 1 auto !important; min-height: 0 !important; }}
"#
    ));

    std::fs::write(&css_path, patched).ok();
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CamoufoxStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub camoufox_tag: Option<String>,
    pub path: Option<String>,
}

#[tauri::command]
pub async fn camoufox_status(state: tauri::State<'_, AppState>) -> Result<CamoufoxStatus, String> {
    match resolve_binary(&state.app_data_dir) {
        Some(path) => {
            let version = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let camoufox_tag =
                std::fs::read_to_string(state.app_data_dir.join("camoufox").join("version.txt"))
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());

            Ok(CamoufoxStatus {
                installed: true,
                version,
                camoufox_tag,
                path: Some(path.to_string_lossy().to_string()),
            })
        }
        None => Ok(CamoufoxStatus {
            installed: false,
            version: None,
            camoufox_tag: None,
            path: None,
        }),
    }
}

#[tauri::command]
pub async fn camoufox_latest_version() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("VeydanBrowser/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let release: GhRelease = client
        .get("https://api.github.com/repos/daijro/camoufox/releases/latest")
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;

    Ok(release.tag_name)
}

#[tauri::command]
pub async fn camoufox_download_state(
    state: tauri::State<'_, AppState>,
) -> Result<DownloadState, String> {
    Ok(state.download.state.lock().await.clone())
}

#[tauri::command]
pub async fn camoufox_download_cancel(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut tx = state.download.cancel_tx.lock().await;
    if let Some(sender) = tx.take() {
        sender.send(()).ok();
    }
    *state.download.state.lock().await = DownloadState::Idle;
    Ok(())
}

#[tauri::command]
pub async fn camoufox_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // Don't start if already downloading
    {
        let current = state.download.state.lock().await;
        if matches!(*current, DownloadState::Downloading { .. }) {
            return Err("Download already in progress".into());
        }
    }

    let app_data_dir = state.app_data_dir.clone();
    let dl_state = Arc::clone(&state.download.state);
    let cancel_arc = Arc::clone(&state.download.cancel_tx);

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    *cancel_arc.lock().await = Some(cancel_tx);

    // Spawn background task — command returns immediately
    tokio::spawn(async move {
        let result = run_download(app.clone(), app_data_dir, dl_state.clone(), cancel_rx).await;

        match result {
            Ok(version) => {
                *dl_state.lock().await = DownloadState::Done {
                    version: version.clone(),
                };
                app.emit("camoufox://done", version).ok();
            }
            Err(e) => {
                let is_cancelled = e.contains("cancelled");
                if is_cancelled {
                    *dl_state.lock().await = DownloadState::Idle;
                } else {
                    *dl_state.lock().await = DownloadState::Failed { error: e.clone() };
                }
                app.emit("camoufox://error", e).ok();
            }
        }
    });

    Ok(())
}

// ── Core download logic ─────────────────────────────────────────────────────

async fn run_download(
    app: tauri::AppHandle,
    app_data_dir: PathBuf,
    dl_state: Arc<Mutex<DownloadState>>,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("VeydanBrowser/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let os = if cfg!(target_os = "windows") {
        "win"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "lin"
    };
    let target_suffix = format!("{os}.{arch}.zip");

    // Fetch up to 20 releases and find the latest one that has an asset for this platform.
    // Some releases may skip certain platforms (e.g. Windows builds missing in recent releases).
    let releases: Vec<GhRelease> = client
        .get("https://api.github.com/repos/daijro/camoufox/releases?per_page=20")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release info: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {e}"))?;

    let (release, asset) = releases
        .into_iter()
        .find_map(|r| {
            let a = r.assets.iter().find(|a| a.name.ends_with(&target_suffix))?;
            Some((r.tag_name.clone(), (a.browser_download_url.clone(), a.size)))
        })
        .ok_or_else(|| format!("No release found with asset for '{target_suffix}'"))?;

    let version = release;
    let (download_url, total_size) = asset;

    let tmp_path = app_data_dir.join("camoufox_download.tmp.zip");

    // Resume support: check existing partial file
    let existing_size = tmp_path.metadata().map(|m| m.len()).unwrap_or(0);
    let start_from = if existing_size > 0 && existing_size < total_size {
        existing_size
    } else {
        0
    };

    let mut req = client.get(&download_url);
    if start_from > 0 {
        req = req.header("Range", format!("bytes={start_from}-"));
    }

    let response = req
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    // Open file: append if resuming, overwrite otherwise
    let mut file = if start_from > 0 {
        std::fs::OpenOptions::new().append(true).open(&tmp_path)
    } else {
        std::fs::File::create(&tmp_path).map(|f| f)
    }
    .map_err(|e| format!("Cannot open temp file: {e}"))?;

    let mut downloaded = start_from;
    let mut stream = response.bytes_stream();

    loop {
        tokio::select! {
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(data)) => {
                        file.write_all(&data).map_err(|e| format!("Write error: {e}"))?;
                        downloaded += data.len() as u64;

                        let percent = if total_size > 0 {
                            ((downloaded * 100) / total_size).min(100) as u8
                        } else { 0 };

                        let state_val = DownloadState::Downloading { downloaded, total: total_size, percent };
                        *dl_state.lock().await = state_val.clone();
                        app.emit("camoufox://progress", state_val).ok();
                    }
                    Some(Err(e)) => return Err(format!("Stream error: {e}")),
                    None => break,
                }
            }
            _ = &mut cancel_rx => {
                return Err("Download cancelled".into());
            }
        }
    }

    drop(file);

    // Extract zip
    app.emit("camoufox://extracting", ()).ok();

    let dest_dir = app_data_dir.join("camoufox");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;

    let zip_file = std::fs::File::open(&tmp_path).map_err(|e| format!("Cannot open zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| format!("Bad zip: {e}"))?;

    // Security: validate all paths before extracting (prevent Zip Slip)
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("Bad zip entry: {e}"))?;
        let entry_path = entry.enclosed_name().ok_or("Invalid path in ZIP")?;
        if entry_path.is_absolute() {
            return Err("Invalid path in ZIP: absolute path".to_string());
        }
        for component in entry_path.components() {
            if let std::path::Component::ParentDir = component {
                return Err("Invalid path in ZIP: path traversal attempt".to_string());
            }
        }
    }

    // Extract all entries
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Bad zip entry: {e}"))?;
        let entry_path = entry
            .enclosed_name()
            .ok_or("Invalid path in ZIP")?
            .to_owned();
        let dest = dest_dir.join(&entry_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }

    std::fs::remove_file(&tmp_path).ok();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let binary = local_binary_path(&app_data_dir);
        if binary.exists() {
            let mut perms = std::fs::metadata(&binary)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&binary, perms).map_err(|e| e.to_string())?;
        }
    }

    let binary = local_binary_path(&app_data_dir);
    if !binary.exists() {
        return Err(format!(
            "Binary not found after extraction at {}",
            binary.display()
        ));
    }

    patch_chrome_css(&dest_dir, "#6366f1", "#6366f1", "");
    patch_omni_ja(&dest_dir).ok();

    // Store mtime after initial patch so ensure_omni_patched skips on first launch
    let omni_path = dest_dir.join("omni.ja");
    let mtime = file_mtime_secs(&omni_path);
    std::fs::write(patch_marker_path(&dest_dir), mtime.to_string()).ok();

    std::fs::write(dest_dir.join("version.txt"), &version).ok();

    Ok(version)
}

/// Reads a mozlz4 file and returns the decompressed bytes.
fn read_mozlz4(path: &Path) -> Result<Vec<u8>, String> {
    let raw = std::fs::read(path).map_err(|e| e.to_string())?;
    if !raw.starts_with(MOZLZ4_MAGIC) {
        return Err("invalid mozlz4 magic".into());
    }
    let size_bytes: [u8; 4] = raw[8..12].try_into().map_err(|_| "truncated header")?;
    let orig_size = u32::from_le_bytes(size_bytes) as usize;
    lz4_flex::decompress(&raw[12..], orig_size).map_err(|e| e.to_string())
}

/// Writes bytes as a mozlz4 file.
fn write_mozlz4(path: &Path, data: &[u8]) -> Result<(), String> {
    let compressed = lz4_flex::compress(data);
    let mut out = Vec::with_capacity(12 + compressed.len());
    out.extend_from_slice(MOZLZ4_MAGIC);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    std::fs::write(path, out).map_err(|e| e.to_string())
}

/// Sets `defaultEngineId` in `search.json.mozlz4` for the given Firefox profile directory.
/// For app-provided (config) engines no hash verification is required by Firefox.
pub fn write_search_engine_to_profile(
    firefox_profile_dir: &Path,
    engine_id: &str,
) -> Result<(), String> {
    let search_json = firefox_profile_dir.join("search.json.mozlz4");

    let mut doc: serde_json::Value = if search_json.exists() {
        let bytes = read_mozlz4(&search_json)?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({
            "version": 13,
            "metaData": {
                "locale": "en-US",
                "region": "unknown",
                "channel": "default",
                "experiment": "",
                "distroID": "",
                "appDefaultEngineId": "ddg",
                "useSavedOrder": false
            },
            "engines": []
        })
    };

    let meta = doc
        .get_mut("metaData")
        .and_then(|v| v.as_object_mut())
        .ok_or("missing metaData")?;

    meta.insert("defaultEngineId".into(), serde_json::Value::String(engine_id.into()));
    meta.insert("privateDefaultEngineId".into(), serde_json::Value::String(engine_id.into()));
    meta.remove("defaultEngineIdHash");
    meta.remove("privateDefaultEngineIdHash");

    let json_bytes = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    write_mozlz4(&search_json, &json_bytes)
}
