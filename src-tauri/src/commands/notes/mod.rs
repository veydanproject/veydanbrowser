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
//! - `merge`    — EOL-normalized 3-way merge with structured conflict blocks
//! - `window`   — standalone notes window
//! - `attachments` — files stored next to notes, referenced by relative links
//! - `transfer` — export/import of Markdown + attachments
//! - `capture`  — requests from the browser extension (selection / bookmark)
//! - `filter`   — backend evaluation of `NoteFilter`
//! - `smart_views` — saved filters
//! - `quick_capture` — small always-on-top window + global shortcut
//! - `links`    — wiki links, backlinks, related notes
//! - `templates` — notes from the Templates folder with placeholders
//! - `lock`     — password lock for the notes UI with auto-lock

mod attachments;
mod capture;
mod crud;
mod files;
mod filter;
mod folders;
mod links;
mod lock;
mod quick_capture;
mod smart_views;
mod templates;
mod history;
mod index;
mod merge;
mod models;
mod settings;
mod tags;
mod transfer;
mod window;

pub use attachments::{
    allow_asset_dir, clipboard_file_paths, note_attachment_add, note_attachment_add_from_path, note_attachment_delete,
    note_attachment_list, note_attachment_open, note_attachment_save, note_attachments_gc,
};
pub use capture::handle_capture;
pub use crud::{
    note_archive, note_create, note_delete, note_draft_discard, note_draft_get, note_draft_save,
    note_get, note_list, note_open_external, note_open_folder, note_reindex, note_restore,
    note_search, note_set_tags, note_sync, note_update,
};
pub use folders::*;
pub use history::*;
pub use index::start_notes_watcher;
pub use links::{note_backlinks, note_related};
pub use lock::{
    notes_lock_lock, notes_lock_set, notes_lock_status, notes_lock_timeout_set, notes_lock_touch,
    notes_lock_unlock, start_auto_lock, NotesLock,
};
pub use quick_capture::{
    open_quick_capture, quick_capture_shortcut_get, quick_capture_shortcut_set, register_quick_capture_shortcut,
    show_quick_capture,
};
pub use settings::*;
pub use smart_views::{
    note_smart_view_create, note_smart_view_delete, note_smart_view_list, note_smart_view_update,
};
pub use tags::{note_tag_create, note_tag_delete, note_tag_list, note_tag_update};
pub use transfer::{note_export, note_import};
pub use window::note_open_window;

// Internals the sync module builds on (file format, index, tag links).
pub(crate) use attachments::{attachments_dir_for, safe_file_name};
pub(crate) use crud::update_note;
pub(crate) use files::{effective_docs_dir, parse_note_file, resolve_note_abs_path, write_note_file};
pub(crate) use history::{history_content_by_id, history_snapshot_by};
pub(crate) use index::sync_notes_index;
pub(crate) use merge::{merge3, MergeResult};
pub(crate) use models::{NoteRow, NoteUpdateInput};
pub(crate) use tags::set_note_tag_links;
