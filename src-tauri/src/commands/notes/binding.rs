// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Registry of note binding kinds. A binding is stored as `kind:value`
//! (JSON array in `notes.bindings` and frontmatter). Entity kinds point at a
//! Veydan object by id; value kinds carry the value itself (url, domain).
//! Also the platform-neutral lookup of entity names for the UI (mobile has no
//! proxy/ssh/profile commands, but the synced tables are there).

use crate::error::{AppError, CmdResult};
use crate::AppState;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BindingKind {
    Workspace,
    Profile,
    Proxy,
    Ssh,
    Totp,
    Password,
    Url,
    Domain,
}

impl BindingKind {
    /// Most specific first; used to rank related notes.
    pub const ALL: [BindingKind; 8] = [
        BindingKind::Url,
        BindingKind::Domain,
        BindingKind::Ssh,
        BindingKind::Proxy,
        BindingKind::Totp,
        BindingKind::Password,
        BindingKind::Profile,
        BindingKind::Workspace,
    ];

    pub fn prefix(self) -> &'static str {
        match self {
            BindingKind::Workspace => "workspace:",
            BindingKind::Profile => "profile:",
            BindingKind::Proxy => "proxy:",
            BindingKind::Ssh => "ssh:",
            BindingKind::Totp => "totp:",
            BindingKind::Password => "password:",
            BindingKind::Url => "url:",
            BindingKind::Domain => "domain:",
        }
    }

    /// Table that owns the entity; None for value kinds.
    pub fn table(self) -> Option<&'static str> {
        match self {
            BindingKind::Workspace => Some("workspaces"),
            BindingKind::Profile => Some("profiles"),
            BindingKind::Proxy => Some("proxies"),
            BindingKind::Ssh => Some("ssh_connections"),
            BindingKind::Totp => Some("totp_entries"),
            BindingKind::Password => Some("passwords"),
            BindingKind::Url | BindingKind::Domain => None,
        }
    }

    pub fn is_entity(self) -> bool {
        self.table().is_some()
    }

    pub fn name(self) -> &'static str {
        self.prefix().trim_end_matches(':')
    }

    pub fn from_name(name: &str) -> Option<BindingKind> {
        BindingKind::ALL.iter().copied().find(|k| k.name() == name)
    }

    /// `SELECT id, name, subtitle FROM ...` with public fields only; None for value kinds.
    fn summary_sql(self) -> Option<&'static str> {
        Some(match self {
            BindingKind::Workspace => "SELECT id, name, '' AS subtitle FROM workspaces",
            BindingKind::Profile => "SELECT id, name, browser_type AS subtitle FROM profiles",
            BindingKind::Proxy => {
                "SELECT id, name, upper(proxy_type) || CASE WHEN coalesce(country, '') = '' THEN '' ELSE ' · ' || country END AS subtitle FROM proxies"
            }
            BindingKind::Ssh => "SELECT id, name, username || '@' || host AS subtitle FROM ssh_connections",
            BindingKind::Totp => "SELECT id, name, coalesce(issuer, '') AS subtitle FROM totp_entries",
            BindingKind::Password => {
                "SELECT id, title AS name, coalesce(username, '') AS subtitle FROM passwords"
            }
            BindingKind::Url | BindingKind::Domain => return None,
        })
    }

    /// Workspace/profile scope a note "belongs to"; notes without one are global.
    pub fn is_scope(self) -> bool {
        matches!(self, BindingKind::Workspace | BindingKind::Profile)
    }

    /// Split `kind:value` into its kind and value.
    pub fn parse(binding: &str) -> Option<(BindingKind, &str)> {
        BindingKind::ALL
            .iter()
            .find_map(|k| binding.strip_prefix(k.prefix()).map(|v| (*k, v)))
    }

    pub fn with(self, value: &str) -> String {
        format!("{}{}", self.prefix(), value)
    }
}

/// True when the string is an entity binding, e.g. `ssh:{id}`.
pub(crate) fn is_entity_binding(s: &str) -> bool {
    BindingKind::parse(s).is_some_and(|(k, v)| k.is_entity() && !v.is_empty())
}

/// True when the binding scopes the note to a workspace or profile.
pub(crate) fn is_scope_binding(s: &str) -> bool {
    BindingKind::parse(s).is_some_and(|(k, _)| k.is_scope())
}

/// Public description of an entity behind a binding.
#[derive(Debug, Serialize, Clone)]
pub struct BindingSummary {
    pub binding: String,
    pub kind: String,
    pub name: String,
    pub subtitle: String,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: String,
    name: String,
    subtitle: String,
}

fn to_summary(kind: BindingKind, r: SummaryRow) -> BindingSummary {
    BindingSummary {
        binding: kind.with(&r.id),
        kind: kind.name().to_string(),
        name: r.name,
        subtitle: r.subtitle,
    }
}

/// Names and subtitles of the entities behind `bindings`; unknown or deleted ones are skipped.
#[tauri::command]
pub async fn note_binding_summaries(
    bindings: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<BindingSummary>> {
    let mut out = Vec::new();
    for b in bindings {
        let Some((kind, id)) = BindingKind::parse(&b) else { continue };
        let Some(sql) = kind.summary_sql() else { continue };
        let row = sqlx::query_as::<_, SummaryRow>(sqlx::AssertSqlSafe(format!("{sql} WHERE id = ?")))
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::db)?;
        if let Some(r) = row {
            out.push(to_summary(kind, r));
        }
    }
    Ok(out)
}

/// Entities of `kind` whose name contains `query`, for binding pickers.
#[tauri::command]
pub async fn note_entity_search(
    kind: String,
    query: String,
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<BindingSummary>> {
    let Some(kind) = BindingKind::from_name(&kind) else {
        return Ok(vec![]);
    };
    let Some(sql) = kind.summary_sql() else {
        return Ok(vec![]);
    };
    // Match name or subtitle (issuer for TOTP, user@host for SSH); SQLite allows the alias in WHERE
    let pattern = format!("%{}%", query.trim().to_lowercase());
    let rows = sqlx::query_as::<_, SummaryRow>(sqlx::AssertSqlSafe(format!(
        "SELECT * FROM ({sql}) AS entity WHERE lower(name) LIKE ? OR lower(subtitle) LIKE ? ORDER BY name LIMIT 30"
    )))
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(&state.db)
    .await
    .map_err(AppError::db)?;
    Ok(rows.into_iter().map(|r| to_summary(kind, r)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_prefixes() {
        assert_eq!(BindingKind::parse("ssh:abc"), Some((BindingKind::Ssh, "abc")));
        assert_eq!(BindingKind::parse("domain:x.com"), Some((BindingKind::Domain, "x.com")));
        assert_eq!(BindingKind::parse("other:1"), None);
    }

    #[test]
    fn entity_vs_scope() {
        assert!(is_entity_binding("proxy:1"));
        assert!(!is_entity_binding("url:https://a"));
        assert!(!is_entity_binding("ssh:"));
        assert!(is_scope_binding("workspace:1"));
        assert!(!is_scope_binding("proxy:1"));
    }
}
