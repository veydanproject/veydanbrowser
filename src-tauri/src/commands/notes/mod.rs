// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Notes subsystem. Split by concern:
//! - `models`   — shared DTO/row structs
//! - `files`    — on-disk note files (frontmatter, atomic write, hashing, preview)
//! - `tags`     — tag helpers + tag CRUD commands
//! - `index`    — FTS index, manifest, filesystem sync + watcher
//! - `crud`     — note CRUD / search / draft commands
//! - `folders`  — folder CRUD + note↔folder / note↔binding commands
//! - `nav`      — navigation tree with counts (mobile sidebar)
//! - `settings` — notes directory settings commands
//! - `history`  — version history (snapshot, diff, restore, merge) + commands
//! - `merge`    — EOL-normalized 3-way merge with structured conflict blocks
//! - `window`   — standalone notes window (desktop)
//! - `attachments` — files stored next to notes, referenced by relative links
//! - `transfer` — export/import of Markdown + attachments (desktop)
//! - `capture`  — requests from the browser extension (desktop)
//! - `filter`   — backend evaluation of `NoteFilter`
//! - `smart_views` — saved filters
//! - `quick_capture` — small always-on-top window + global shortcut (desktop)
//! - `links`    — wiki links, backlinks, related notes, entity mentions
//! - `binding`  — registry of `kind:value` binding kinds
//! - `templates` — notes from the Templates folder with placeholders
//! - `lock`     — password lock for the notes UI with auto-lock

mod attachments;
mod binding;
#[cfg(desktop)]
mod capture;
mod crud;
mod files;
mod filter;
mod folders;
mod history;
mod index;
mod links;
pub(crate) mod lock;
mod merge;
mod models;
mod nav;
#[cfg(desktop)]
mod quick_capture;
mod settings;
mod smart_views;
mod tags;
mod templates;
#[cfg(desktop)]
mod transfer;
#[cfg(desktop)]
mod window;

#[cfg(desktop)]
pub use attachments::clipboard_file_paths;
pub use attachments::{
    allow_asset_dir, note_attachment_add, note_attachment_add_from_path, note_attachment_delete,
    note_attachment_fetch, note_attachment_list, note_attachment_open, note_attachment_read,
    note_attachment_save, note_attachments_gc,
};
pub use binding::{note_binding_summaries, note_entity_search};
pub use templates::note_placeholder_values;
#[cfg(desktop)]
pub use capture::handle_capture;
pub use crud::{
    note_archive, note_create, note_delete, note_delete_many, note_draft_discard, note_draft_get,
    note_draft_save, note_get, note_list, note_open_external, note_open_folder, note_reindex,
    note_restore, note_search, note_set_tags, note_sync, note_trash_empty, note_update,
};
pub use folders::*;
pub use history::*;
pub use index::start_notes_watcher;
pub use links::{
    note_backlinks, note_entity_notes, note_links, note_related, note_resolve_link,
};
pub use lock::{
    notes_lock_lock, notes_lock_set, notes_lock_status, notes_lock_timeout_set, notes_lock_touch,
    notes_lock_unlock, start_auto_lock, NotesLock,
};
pub(crate) use lock::require_lock_password;
pub use nav::note_nav;
#[cfg(desktop)]
pub use quick_capture::{
    open_quick_capture, quick_capture_shortcut_get, quick_capture_shortcut_set,
    reapply_quick_capture_shortcut, register_quick_capture_shortcut, show_quick_capture,
};
pub use settings::*;
pub use smart_views::{
    note_smart_view_create, note_smart_view_delete, note_smart_view_list, note_smart_view_update,
};
pub use tags::{note_tag_create, note_tag_delete, note_tag_list, note_tag_update, TAG_PLACEHOLDER_COLOR};
#[cfg(desktop)]
pub use transfer::{note_export, note_import};
#[cfg(desktop)]
pub use window::note_open_window;

// Internals the sync module builds on (file format, index, tag links).
pub(crate) use attachments::{attachments_dir_for, is_staging_name, safe_file_name};
pub(crate) use crud::{insert_note, update_note, NewNote};
pub(crate) use files::{
    effective_docs_dir, parse_note_file, resolve_note_abs_path, write_note_file,
};
pub(crate) use history::{history_content_by_id, history_snapshot_by};
pub(crate) use settings::load_attachment_policy;
// Legacy mobile DB upgrade converts plain-text history into the compressed form.
#[cfg(mobile)]
pub(crate) use files::compute_hash;
#[cfg(mobile)]
pub(crate) use history::compress_content;
pub(crate) use index::{rebuild_manifest, sync_notes_index};
pub(crate) use links::reindex_links;
pub(crate) use merge::{merge3, MergeResult};
pub(crate) use models::{NoteFilter, NoteRow, NoteUpdateInput};
pub(crate) use tags::set_note_tag_links;
