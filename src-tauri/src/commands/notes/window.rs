// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::error::{AppError, CmdResult};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

const NOTES_WINDOW_LABEL: &str = "notes";

/// Open the standalone notes window, or focus it if it already exists.
#[tauri::command]
pub async fn note_open_window(title: String, app: tauri::AppHandle) -> CmdResult<()> {
    let title = if title.trim().is_empty() {
        "Notes".to_string()
    } else {
        title
    };
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window(NOTES_WINDOW_LABEL) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }

        let mut builder = WebviewWindowBuilder::new(
            &app2,
            NOTES_WINDOW_LABEL,
            WebviewUrl::App("/notes".into()),
        )
        .title(title)
        .inner_size(960.0, 700.0)
        .min_inner_size(640.0, 480.0);

        #[cfg(target_os = "linux")]
        {
            builder = builder.decorations(false).transparent(true);
        }

        if let Err(e) = builder.build() {
            eprintln!("[notes] failed to open window: {e}");
        }
    })
    .map_err(AppError::other)?;
    Ok(())
}
