// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Notes subsystem. Split by concern:
//! - `models`   — shared DTO/row structs
//! - `files`    — on-disk note files (frontmatter, atomic write, hashing, preview)
//! - `tags`     — tag helpers + tag CRUD commands
//! - `index`    — FTS index, manifest, filesystem sync + watcher
//! - `crud`     — note CRUD / search / draft commands
//! - `folders`  — folder CRUD + note↔folder / note↔binding commands
//! - `settings` — notes directory settings commands
//! - `history`  — version history (snapshot, diff, restore, merge) + commands
//! - `window`   — standalone notes window

mod crud;
mod files;
mod folders;
mod history;
mod index;
mod models;
mod settings;
mod tags;
mod window;

pub use crud::*;
pub use folders::*;
pub use history::*;
pub use index::start_notes_watcher;
pub use settings::*;
pub use tags::{note_tag_create, note_tag_delete, note_tag_list, note_tag_update};
pub use window::note_open_window;
