// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Commands of the Kotlin side. The web page never calls them: the app's Rust
// code does. They are listed because the plugin build wants the list.
const COMMANDS: &[&str] = &[
    "get_state",
    "get_token",
    "delete_token",
    "permission_state",
    "request_permission",
    "set_context",
    "take_tap",
    "cancel",
    "store_keys",
    "clear_keys",
    "register_listener",
    "remove_listener",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
