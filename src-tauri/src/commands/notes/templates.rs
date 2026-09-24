// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Note templates: any note inside the `Templates` folder. Placeholders:
//! `{{date}}`, `{{time}}`, `{{datetime}}`, `{{title}}`, `{{profile}}`,
//! `{{workspace}}`, `{{url}}`, `{{domain}}`, `{{proxy}}`, `{{proxy_type}}`,
//! `{{proxy_host}}`, `{{proxy_port}}`, `{{proxy_region}}`, `{{ssh}}`,
//! `{{ssh_host}}`, `{{ssh_port}}`, `{{ssh_user}}`, `{{totp}}`.
//! Only non-secret fields are ever exposed.

use super::binding::BindingKind;
use super::files::{read_note_file, resolve_note_abs_path};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use chrono::Local;
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub(crate) struct TemplateVars {
    pub title: String,
    pub profile: String,
    pub workspace: String,
    pub url: String,
    pub domain: String,
    pub proxy: String,
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_region: String,
    pub ssh: String,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub totp: String,
}

#[derive(sqlx::FromRow)]
struct ProxyPublic {
    name: String,
    proxy_type: String,
    host: String,
    port: i64,
    country: Option<String>,
    city: Option<String>,
}

#[derive(sqlx::FromRow)]
struct SshPublic {
    name: String,
    host: String,
    port: i64,
    username: String,
}

impl TemplateVars {
    /// Fill entity names and public fields from bindings.
    pub(crate) async fn from_bindings(title: &str, bindings: &[String], state: &AppState) -> Self {
        let mut vars = Self {
            title: title.to_string(),
            ..Default::default()
        };
        for b in bindings {
            let Some((kind, value)) = BindingKind::parse(b) else { continue };
            match kind {
                BindingKind::Profile => vars.profile = lookup_name(kind, value, state).await,
                BindingKind::Workspace => vars.workspace = lookup_name(kind, value, state).await,
                BindingKind::Totp => vars.totp = lookup_name(kind, value, state).await,
                BindingKind::Url => vars.url = value.to_string(),
                BindingKind::Domain => vars.domain = value.to_string(),
                BindingKind::Proxy => {
                    if let Some(p) = sqlx::query_as::<_, ProxyPublic>(
                        "SELECT name, proxy_type, host, port, country, city FROM proxies WHERE id = ?",
                    )
                    .bind(value)
                    .fetch_optional(&state.db)
                    .await
                    .ok()
                    .flatten()
                    {
                        vars.proxy = p.name;
                        vars.proxy_type = p.proxy_type;
                        vars.proxy_host = p.host;
                        vars.proxy_port = p.port.to_string();
                        vars.proxy_region = [p.country, p.city]
                            .into_iter()
                            .flatten()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(", ");
                    }
                }
                BindingKind::Ssh => {
                    if let Some(s) = sqlx::query_as::<_, SshPublic>(
                        "SELECT name, host, port, username FROM ssh_connections WHERE id = ?",
                    )
                    .bind(value)
                    .fetch_optional(&state.db)
                    .await
                    .ok()
                    .flatten()
                    {
                        vars.ssh = s.name;
                        vars.ssh_host = s.host;
                        vars.ssh_port = s.port.to_string();
                        vars.ssh_user = s.username;
                    }
                }
            }
        }
        vars
    }
}

/// `name` column of the entity table behind an entity binding kind.
pub(crate) async fn lookup_name(kind: BindingKind, id: &str, state: &AppState) -> String {
    let Some(table) = kind.table() else { return String::new() };
    let sql = format!("SELECT name FROM {table} WHERE id = ?");
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Current value of every placeholder.
fn pairs(vars: &TemplateVars) -> [(&'static str, String); 18] {
    let now = Local::now();
    [
        ("date", now.format("%Y-%m-%d").to_string()),
        ("time", now.format("%H:%M").to_string()),
        ("datetime", now.format("%Y-%m-%d %H:%M").to_string()),
        ("title", vars.title.clone()),
        ("profile", vars.profile.clone()),
        ("workspace", vars.workspace.clone()),
        ("url", vars.url.clone()),
        ("domain", vars.domain.clone()),
        ("proxy", vars.proxy.clone()),
        ("proxy_type", vars.proxy_type.clone()),
        ("proxy_host", vars.proxy_host.clone()),
        ("proxy_port", vars.proxy_port.clone()),
        ("proxy_region", vars.proxy_region.clone()),
        ("ssh", vars.ssh.clone()),
        ("ssh_host", vars.ssh_host.clone()),
        ("ssh_port", vars.ssh_port.clone()),
        ("ssh_user", vars.ssh_user.clone()),
        ("totp", vars.totp.clone()),
    ]
}

/// Replace `{{name}}` placeholders; unknown ones are left as-is.
pub(crate) fn render(body: &str, vars: &TemplateVars) -> String {
    let mut out = body.to_string();
    for (key, value) in pairs(vars) {
        out = out
            .replace(&format!("{{{{{key}}}}}"), &value)
            .replace(&format!("{{{{ {key} }}}}"), &value);
    }
    out
}

/// Placeholder name -> value for a note with `title` and `bindings`, e.g. to expand `{{date}}` in place.
#[tauri::command]
pub async fn note_placeholder_values(
    title: String,
    bindings: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<HashMap<String, String>> {
    let vars = TemplateVars::from_bindings(&title, &bindings, &state).await;
    Ok(pairs(&vars).into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Body of a template note.
pub(crate) async fn template_body(template_id: &str, state: &AppState) -> Result<String, AppError> {
    let file_path: String =
        sqlx::query_scalar("SELECT file_path FROM notes WHERE id = ? AND deleted = 0")
            .bind(template_id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found(format!("Template {template_id}")))?;
    let (_, _, body) = read_note_file(&resolve_note_abs_path(&state.app_data_dir, &file_path))?;
    Ok(body)
}

/// Render a template into content for a new note.
pub(crate) async fn render_template(
    template_id: &str,
    vars: &TemplateVars,
    state: &AppState,
) -> Result<String, AppError> {
    let body = template_body(template_id, state).await?;
    Ok(render(&body, vars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_known_placeholders_and_keeps_unknown() {
        let vars = TemplateVars {
            title: "T".into(),
            profile: "P".into(),
            ..Default::default()
        };
        let out = render("# {{title}} / {{ profile }} / {{unknown}}", &vars);
        assert_eq!(out, "# T / P / {{unknown}}");
    }
}
