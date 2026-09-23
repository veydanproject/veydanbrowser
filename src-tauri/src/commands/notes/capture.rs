// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Handles requests coming from the browser extension.
//! Capture kinds: `selection_new`, `selection_append`, `bookmark`, `article`, `screenshot`.
//! Query kinds: `list_notes`, `search_notes`, `open_note`.

use super::attachments::store_attachment;
use super::crud::{current_docs_dir, fts_match_query, insert_note, update_note, NewNote};
use super::files::{read_note_file, resolve_note_abs_path};
use super::folders::persist_bindings;
use super::models::{NoteRow, NoteUpdateInput};
use super::settings::{load_capture_rules, CaptureRule};
use super::templates::{render_template, TemplateVars};
use crate::capture::protocol::{CaptureAsset, CaptureRequest, CaptureResponse, NoteRef};
use crate::AppState;
use base64::Engine;
use chrono::Utc;
use tauri::Emitter;
use uuid::Uuid;

const MAX_TITLE: usize = 120;
const MAX_ASSET_BYTES: usize = 10 * 1024 * 1024;
const MAX_ASSETS: usize = 30;

/// URL without fragment, used for `url:` bindings.
pub(crate) fn canonical_url(raw: &str) -> String {
    let no_fragment = raw.split('#').next().unwrap_or("").trim();
    no_fragment.trim_end_matches('/').to_string()
}

/// Lowercased host without credentials/port, used for `domain:` bindings.
pub(crate) fn domain_of(raw: &str) -> Option<String> {
    let rest = raw.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    let host = host.trim().to_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Rule matches the exact domain or any of its subdomains.
fn rule_matches(rule: &CaptureRule, domain: &str) -> bool {
    let pattern = rule.domain.trim().trim_start_matches("*.").to_lowercase();
    !pattern.is_empty() && (domain == pattern || domain.ends_with(&format!(".{pattern}")))
}

fn page_bindings(req: &CaptureRequest, workspace_id: Option<&str>) -> Vec<String> {
    let mut b = vec![format!("profile:{}", req.profile_id)];
    if let Some(w) = workspace_id {
        b.push(format!("workspace:{w}"));
    }
    let url = canonical_url(&req.url);
    if !url.is_empty() {
        b.push(format!("url:{url}"));
    }
    if let Some(d) = domain_of(&req.url) {
        b.push(format!("domain:{d}"));
    }
    b
}

fn truncate_title(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= MAX_TITLE {
        return s;
    }
    let cut: String = s.chars().take(MAX_TITLE - 1).collect();
    format!("{}…", cut.trim_end())
}

fn note_title(req: &CaptureRequest) -> String {
    let t = req.title.trim();
    if !t.is_empty() {
        return truncate_title(t);
    }
    domain_of(&req.url).unwrap_or_else(|| "Captured page".to_string())
}

/// Markdown header with source link and capture time, plus quoted selection.
fn capture_block(req: &CaptureRequest, captured_at: &str) -> String {
    let title = if req.title.trim().is_empty() {
        req.url.clone()
    } else {
        req.title.trim().to_string()
    };
    let title = title.replace(['[', ']'], " ");
    let mut out = format!(
        "Source: [{}]({})  \nCaptured: {}\n",
        title, req.url, captured_at
    );
    let text = req.selection_text.trim();
    if !text.is_empty() {
        out.push('\n');
        for line in text.lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

async fn profile_workspace(profile_id: &str, state: &AppState) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>("SELECT workspace_id FROM profiles WHERE id = ?")
        .bind(profile_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .flatten()
}

/// Live notes whose bindings JSON contains the given binding, newest first.
async fn notes_with_binding(binding: &str, limit: i64, state: &AppState) -> Vec<NoteRow> {
    let pattern = format!("%{}%", serde_json::to_string(binding).unwrap_or_default());
    sqlx::query_as::<_, NoteRow>(
        "SELECT * FROM notes WHERE deleted = 0 AND archived = 0 AND bindings LIKE ? ORDER BY updated_at DESC LIMIT ?",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
}

/// Decode and store extension assets; returns `(original src, relative link)` pairs.
fn store_assets(note_id: &str, assets: &[CaptureAsset], state: &AppState) -> Vec<(String, String)> {
    let note_file = current_docs_dir(state).join(format!("{note_id}.md"));
    let mut total = 0usize;
    let mut links = Vec::new();
    for asset in assets.iter().take(MAX_ASSETS) {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&asset.data_b64) else {
            continue;
        };
        total += bytes.len();
        if bytes.is_empty() || total > MAX_ASSET_BYTES {
            break;
        }
        if let Ok(att) = store_attachment(&note_file, note_id, &asset.name, &bytes) {
            links.push((asset.src.clone(), att.rel_path));
        }
    }
    links
}

/// Apply the first matching domain rule: tags and template into the note; returns the folder to link.
async fn apply_rules(domain: Option<&str>, new: &mut NewNote, state: &AppState) -> Option<String> {
    let domain = domain?;
    let rules = load_capture_rules(state).await;
    let rule = rules.iter().find(|r| rule_matches(r, domain))?;
    for tag in &rule.tags {
        let tag = tag.trim();
        if !tag.is_empty() && !new.tags.iter().any(|t| t == tag) {
            new.tags.push(tag.to_string());
        }
    }
    if let Some(template_id) = rule.template_id.as_deref().filter(|t| !t.is_empty()) {
        let vars = TemplateVars::from_bindings(&new.title, &new.bindings, state).await;
        if let Ok(rendered) = render_template(template_id, &vars, state).await {
            new.content = format!("{rendered}\n{}", new.content);
        }
    }
    rule.folder_id.clone().filter(|f| !f.is_empty())
}

async fn create_note(
    req: &CaptureRequest,
    body: String,
    state: &AppState,
) -> Result<(String, String), String> {
    let now = Utc::now().to_rfc3339();
    let workspace = profile_workspace(&req.profile_id, state).await;
    let id = Uuid::new_v4().to_string();
    let title = note_title(req);

    let mut content = format!("{}\n{}", capture_block(req, &now), body);
    for (src, rel) in store_assets(&id, &req.assets, state) {
        if src.is_empty() {
            content.push_str(&format!("\n![]({rel})\n"));
        } else {
            content = content.replace(&format!("]({src})"), &format!("]({rel})"));
        }
    }

    let mut new = NewNote {
        id: id.clone(),
        title: title.clone(),
        format: "md".to_string(),
        bindings: page_bindings(req, workspace.as_deref()),
        tags: vec![],
        content,
        created_at: now.clone(),
        updated_at: now,
    };
    let domain = domain_of(&req.url);
    let folder_id = apply_rules(domain.as_deref(), &mut new, state).await;

    insert_note(new, state).await.map_err(|e| e.to_string())?;
    if let Some(folder_id) = folder_id {
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO note_folder_links (note_id, folder_id) VALUES (?, ?)",
        )
        .bind(&id)
        .bind(&folder_id)
        .execute(&state.db)
        .await;
    }
    Ok((id, title))
}

async fn note_by_id(id: &str, state: &AppState) -> Option<NoteRow> {
    sqlx::query_as::<_, NoteRow>("SELECT * FROM notes WHERE id = ? AND deleted = 0")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

/// Append the capture to `req.note_id`, or to the profile's most recent note;
/// creates a new note when neither exists.
async fn append_to_note(
    req: &CaptureRequest,
    state: &AppState,
) -> Result<(String, String), String> {
    let target = match req.note_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => Some(
            note_by_id(id, state)
                .await
                .ok_or_else(|| format!("Note {id} not found"))?,
        ),
        None => notes_with_binding(&format!("profile:{}", req.profile_id), 1, state)
            .await
            .pop(),
    };
    let Some(row) = target else {
        return create_note(req, String::new(), state).await;
    };
    let file_path = resolve_note_abs_path(&state.app_data_dir, &row.file_path);
    let (_, _, body) = read_note_file(&file_path).map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    let mut content = body.trim_end().to_string();
    if !content.is_empty() {
        content.push_str("\n\n---\n\n");
    }
    content.push_str(&capture_block(req, &now));

    let input = NoteUpdateInput {
        title: None,
        content: Some(content),
        pinned: None,
        base_hash: None,
    };
    update_note(&row.id, input, state)
        .await
        .map_err(|e| e.to_string())?;

    // Merge page bindings so the note shows up for this url/domain too
    let mut bindings: Vec<String> = serde_json::from_str(&row.bindings).unwrap_or_default();
    let mut changed = false;
    for b in page_bindings(req, None) {
        if !bindings.contains(&b) {
            bindings.push(b);
            changed = true;
        }
    }
    if changed {
        persist_bindings(&row.id, &bindings, state)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok((row.id, row.title))
}

/// Notes for the popup: exact url first, then domain, then the rest of the profile.
async fn list_notes(req: &CaptureRequest, state: &AppState) -> Vec<NoteRef> {
    let mut out: Vec<NoteRef> = Vec::new();
    let mut push = |rows: Vec<NoteRow>, scope: &str| {
        for r in rows {
            if !out.iter().any(|n| n.id == r.id) {
                out.push(NoteRef {
                    id: r.id,
                    title: r.title,
                    updated_at: r.updated_at,
                    scope: scope.to_string(),
                });
            }
        }
    };
    let url = canonical_url(&req.url);
    if !url.is_empty() {
        push(
            notes_with_binding(&format!("url:{url}"), 50, state).await,
            "url",
        );
    }
    if let Some(d) = domain_of(&req.url) {
        push(
            notes_with_binding(&format!("domain:{d}"), 50, state).await,
            "domain",
        );
    }
    push(
        notes_with_binding(&format!("profile:{}", req.profile_id), 20, state).await,
        "profile",
    );
    out
}

/// Notes for the popup picker: FTS over title/content/tags, live notes only.
async fn search_notes(req: &CaptureRequest, state: &AppState) -> Vec<NoteRef> {
    let Some(fts) = fts_match_query(&req.query) else {
        return Vec::new();
    };
    sqlx::query_as::<_, NoteRow>(
        "SELECT n.* FROM notes_fts f JOIN notes n ON n.id = f.note_id
         WHERE notes_fts MATCH ? AND n.deleted = 0 AND n.archived = 0
         ORDER BY rank LIMIT 20",
    )
    .bind(&fts)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|r| NoteRef {
        id: r.id,
        title: r.title,
        updated_at: r.updated_at,
        scope: "all".to_string(),
    })
    .collect()
}

/// Entry point for the IPC server.
pub async fn handle_capture(
    req: CaptureRequest,
    state: &AppState,
    app: &tauri::AppHandle,
) -> CaptureResponse {
    if req.profile_id.is_empty() {
        return CaptureResponse::err("profile_id is required");
    }
    match req.kind.as_str() {
        // Titles stay hidden while the notes UI is locked
        "list_notes" | "search_notes" if super::lock::is_locked(state).await => {
            return CaptureResponse::notes(Vec::new())
        }
        "list_notes" => return CaptureResponse::notes(list_notes(&req, state).await),
        "search_notes" => return CaptureResponse::notes(search_notes(&req, state).await),
        "open_note" => {
            let Some(id) = req.note_id.clone().filter(|s| !s.is_empty()) else {
                return CaptureResponse::err("note_id is required");
            };
            if let Err(e) =
                super::window::open_notes_window(app.clone(), "Notes".to_string(), Some(id)).await
            {
                return CaptureResponse::err(e.to_string());
            }
            return CaptureResponse::done();
        }
        _ => {}
    }
    if req.url.is_empty() {
        return CaptureResponse::err("url is required");
    }
    let result = match req.kind.as_str() {
        "selection_new" | "bookmark" | "screenshot" => {
            create_note(&req, String::new(), state).await
        }
        "article" => create_note(&req, req.markdown.trim().to_string(), state).await,
        "selection_append" => append_to_note(&req, state).await,
        other => Err(format!("unknown capture kind: {other}")),
    };
    match result {
        Ok((id, title)) => {
            let _ = app.emit("notes://external-change", &id);
            CaptureResponse::ok(id, title)
        }
        Err(e) => CaptureResponse::err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_url_strips_fragment_and_trailing_slash() {
        assert_eq!(canonical_url("https://a.b/c/#x"), "https://a.b/c");
        assert_eq!(canonical_url("https://a.b/c?q=1#x"), "https://a.b/c?q=1");
    }

    #[test]
    fn domain_of_extracts_host() {
        assert_eq!(
            domain_of("https://User@Www.Example.com:8443/p?q").as_deref(),
            Some("www.example.com")
        );
        assert_eq!(domain_of("about:blank"), None);
    }

    #[test]
    fn rule_matches_domain_and_subdomains() {
        let rule = CaptureRule {
            domain: "*.example.com".into(),
            folder_id: None,
            tags: vec![],
            template_id: None,
        };
        assert!(rule_matches(&rule, "example.com"));
        assert!(rule_matches(&rule, "docs.example.com"));
        assert!(!rule_matches(&rule, "notexample.com"));
    }
}
