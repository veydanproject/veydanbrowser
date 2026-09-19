// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Browser -> notes capture bridge.
//!
//! Camoufox page --(WebExtension)--> native messaging host (this executable,
//! started by the browser with the extension id in argv) --(local socket)-->
//! IPC server inside the running Tauri app --> notes commands.
//!
//! - `extension` — builds the per-profile .xpi and registers the native host manifest
//! - `host`      — stdio native-messaging host mode (no Tauri runtime)
//! - `server`    — local socket server inside the app
//! - `protocol`  — request/response types shared by all three

pub mod extension;
pub mod host;
pub mod protocol;
pub mod server;

/// Gecko extension id; also the argv marker the browser passes to the native host.
pub const EXTENSION_ID: &str = "notes@veydan.net";
/// Native messaging host name (manifest file name and `sendNativeMessage` target).
pub const HOST_NAME: &str = "veydan_notes";

/// True when this process was started by the browser as the native messaging host.
pub fn is_host_invocation() -> bool {
    std::env::args().skip(1).any(|a| a == EXTENSION_ID)
}

/// Per-user local IPC endpoint shared by the host and the app.
pub fn ipc_endpoint() -> String {
    #[cfg(windows)]
    {
        r"\\.\pipe\veydan-capture".to_string()
    }
    #[cfg(not(windows))]
    {
        // XDG_RUNTIME_DIR is already per-user; the /tmp fallback is suffixed with the user name.
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            return format!("{}/veydan-capture.sock", dir.trim_end_matches('/'));
        }
        let dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        let user = std::env::var("USER").unwrap_or_else(|_| "default".to_string());
        format!("{}/veydan-capture-{user}.sock", dir.trim_end_matches('/'))
    }
}
