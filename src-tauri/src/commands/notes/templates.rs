// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Note templates: any note inside the `Templates` folder. Placeholders:
//! `{{date}}`, `{{time}}`, `{{datetime}}`, `{{title}}`, `{{profile}}`,
//! `{{workspace}}`, `{{url}}`, `{{domain}}`.

use super::files::{read_note_file, resolve_note_abs_path};
use crate::error::AppError;
use crate::AppState;
use chrono::Local;

#[derive(Debug, Default, Clone)]
pub(crate) struct TemplateVars {
    pub title: String,
    pub profile: String,
    pub workspace: String,
    pub url: String,
    pub domain: String,
}

impl TemplateVars {
    /// Fill profile/workspace names from bindings.
    pub(crate) async fn from_bindings(title: &str, bindings: &[String], state: &AppState) -> Self {
        let mut vars = Self { title: title.to_string(), ..Default::default() };
        for b in bindings {
            if let Some(id) = b.strip_prefix("profile:") {
                vars.profile = lookup_name("profiles", id, state).await;
            } else if let Some(id) = b.strip_prefix("workspace:") {
                vars.workspace = lookup_name("workspaces", id, state).await;
            } else if let Some(u) = b.strip_prefix("url:") {
                vars.url = u.to_string();
            } else if let Some(d) = b.strip_prefix("domain:") {
                vars.domain = d.to_string();
            }
        }
        vars
    }
}

async fn lookup_name(table: &str, id: &str, state: &AppState) -> String {
    let sql = format!("SELECT name FROM {table} WHERE id = ?");
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Replace `{{name}}` placeholders; unknown ones are left as-is.
pub(crate) fn render(body: &str, vars: &TemplateVars) -> String {
    let now = Local::now();
    let pairs = [
        ("date", now.format("%Y-%m-%d").to_string()),
        ("time", now.format("%H:%M").to_string()),
        ("datetime", now.format("%Y-%m-%d %H:%M").to_string()),
        ("title", vars.title.clone()),
        ("profile", vars.profile.clone()),
        ("workspace", vars.workspace.clone()),
        ("url", vars.url.clone()),
        ("domain", vars.domain.clone()),
    ];
    let mut out = body.to_string();
    for (key, value) in pairs {
        out = out.replace(&format!("{{{{{key}}}}}"), &value).replace(&format!("{{{{ {key} }}}}"), &value);
    }
    out
}

/// Body of a template note.
pub(crate) async fn template_body(template_id: &str, state: &AppState) -> Result<String, AppError> {
    let file_path: String = sqlx::query_scalar("SELECT file_path FROM notes WHERE id = ? AND deleted = 0")
        .bind(template_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found(format!("Template {template_id}")))?;
    let (_, _, body) = read_note_file(&resolve_note_abs_path(&state.app_data_dir, &file_path))?;
    Ok(body)
}

/// Render a template into content for a new note.
pub(crate) async fn render_template(template_id: &str, vars: &TemplateVars, state: &AppState) -> Result<String, AppError> {
    let body = template_body(template_id, state).await?;
    Ok(render(&body, vars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_known_placeholders_and_keeps_unknown() {
        let vars = TemplateVars { title: "T".into(), profile: "P".into(), ..Default::default() };
        let out = render("# {{title}} / {{ profile }} / {{unknown}}", &vars);
        assert_eq!(out, "# T / P / {{unknown}}");
    }
}
