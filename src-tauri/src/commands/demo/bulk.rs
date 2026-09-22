// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Extra demo volume (~5×) generated on top of the handcrafted pack.

use crate::commands::notes::{insert_note, NewNote};
use crate::error::{AppError, CmdResult};
use crate::AppState;
use uuid::Uuid;

const WS: &[&str] = &[
    "default",
    "demo-ws-smm",
    "demo-ws-dev",
    "demo-ws-devops",
    "demo-ws-biz",
    "demo-ws-freelance",
    "demo-ws-research",
];

const KANBAN: &[&[&str]] = &[
    &["inbox", "active", "done"],
    &["ideas", "in_progress", "published"],
    &["backlog", "coding", "review", "done_dev"],
    &["plan", "deploy", "monitor"],
    &["lead", "negotiation", "closed"],
    &["inbox", "active", "done"],
    &["inbox", "active", "done"],
];

const PRESETS: &[&str] = &["win10", "win11", "macos", "linux"];
const PROXY_TYPES: &[&str] = &["http", "https", "socks5"];
const COUNTRIES: &[(&str, &str)] = &[
    ("US", "New York"),
    ("DE", "Berlin"),
    ("NL", "Amsterdam"),
    ("GB", "London"),
    ("SG", "Singapore"),
    ("JP", "Tokyo"),
    ("FR", "Paris"),
    ("CA", "Toronto"),
    ("AU", "Sydney"),
    ("PL", "Warsaw"),
    ("ES", "Madrid"),
    ("IT", "Milan"),
    ("SE", "Stockholm"),
    ("BR", "Sao Paulo"),
    ("IN", "Mumbai"),
    ("KR", "Seoul"),
];

const ISSUERS: &[&str] = &[
    "GitHub", "GitLab", "Bitbucket", "AWS", "GCP", "Azure", "Cloudflare", "DigitalOcean",
    "Hetzner", "Vultr", "Linode", "Stripe", "PayPal", "Shopify", "Notion", "Slack",
    "Discord", "Telegram", "Twitter", "Meta", "Google", "Microsoft", "Apple", "Dropbox",
    "1Password", "Bitwarden", "Okta", "Auth0", "Twilio", "SendGrid", "Mailchimp", "HubSpot",
    "Salesforce", "Zendesk", "Jira", "Confluence", "Figma", "Notion", "Linear", "Vercel",
    "Netlify", "Heroku", "Railway", "Render", "Supabase", "PlanetScale", "MongoDB", "Redis",
];

/// Seed extra workspaces + bulk proxies/profiles/totp/ssh/notes.
pub async fn seed_bulk(state: &AppState, locale: &str, now: &str) -> CmdResult<()> {
    let ru = locale.eq_ignore_ascii_case("ru");
    seed_extra_workspaces(state, ru, now).await?;
    seed_bulk_proxies(state, now).await?;
    let profile_ids = seed_bulk_profiles(state, ru, now).await?;
    seed_bulk_totp(state, &profile_ids, now).await?;

    #[cfg(desktop)]
    seed_bulk_ssh(state, now).await?;

    seed_bulk_notes(state, ru, &profile_ids, now).await?;
    Ok(())
}

async fn seed_extra_workspaces(state: &AppState, ru: bool, now: &str) -> CmdResult<()> {
    let extras: &[(&str, &str, &str, &str, &str)] = if ru {
        &[
            ("demo-ws-freelance", "Фриланс", "Клиентские проекты", "#8b5cf6", "laptop"),
            ("demo-ws-research", "Исследования", "Разведка и конкуренты", "#14b8a6", "search"),
        ]
    } else {
        &[
            ("demo-ws-freelance", "Freelance", "Client projects", "#8b5cf6", "laptop"),
            ("demo-ws-research", "Research", "Competitive intel", "#14b8a6", "search"),
        ]
    };

    for (id, name, desc, color, icon) in extras {
        sqlx::query(
            "INSERT OR IGNORE INTO workspaces (id, name, description, color, icon, is_default, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(desc)
        .bind(color)
        .bind(icon)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

        for (i, (col_name, tag)) in [
            ("Inbox", "inbox"),
            (if ru { "В работе" } else { "Active" }, "active"),
            (if ru { "Готово" } else { "Done" }, "done"),
        ]
        .into_iter()
        .enumerate()
        {
            let col_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO workspace_columns (id, workspace_id, name, tag_name, color, position, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&col_id)
            .bind(id)
            .bind(col_name)
            .bind(tag)
            .bind(["#94a3b8", "#3b82f6", "#22c55e"][i])
            .bind(i as i64)
            .bind(now)
            .execute(&state.db)
            .await
            .map_err(AppError::db)?;
        }
    }
    Ok(())
}

async fn seed_bulk_proxies(state: &AppState, now: &str) -> CmdResult<()> {
    // 8 handcrafted + 32 bulk = 40
    for i in 1..=32 {
        let (cc, city) = COUNTRIES[(i as usize - 1) % COUNTRIES.len()];
        let ptype = PROXY_TYPES[(i as usize - 1) % PROXY_TYPES.len()];
        let ws = WS[(i as usize - 1) % WS.len()];
        let id = format!("demo-px-bulk-{i:02}");
        let name = format!("{cc} {ptype} #{i:02}");
        let host = format!("proxy-{}.example", cc.to_lowercase());
        let port = 10000 + i as i64;
        let tags = serde_json::to_string(&vec![format!("workspace:{ws}")]).map_err(AppError::other)?;
        let user = format!("demo-u{i}");
        let pass = format!("demo-pass-{i}");

        sqlx::query(
            "INSERT INTO proxies
             (id, name, proxy_type, host, port, username, password, country, city, status, tags, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'unknown', ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(ptype)
        .bind(&host)
        .bind(port)
        .bind(&user)
        .bind(&pass)
        .bind(cc)
        .bind(city)
        .bind(&tags)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

async fn seed_bulk_profiles(state: &AppState, ru: bool, now: &str) -> CmdResult<Vec<String>> {
    let profiles_root = state.app_data_dir.join("profiles");
    std::fs::create_dir_all(&profiles_root).map_err(AppError::io)?;

    let labels_en = [
        "Shop", "Ads", "Support", "QA", "Staging", "Docs", "Blog", "Forum", "Store", "Portal",
        "Admin", "Partner", "Affiliate", "Review", "Monitor", "Backup", "Legacy", "Sandbox",
        "Client", "Vendor",
    ];
    let labels_ru = [
        "Магазин", "Реклама", "Поддержка", "QA", "Staging", "Доки", "Блог", "Форум", "Витрина",
        "Портал", "Админ", "Партнёр", "Аффилиат", "Обзор", "Монитор", "Бэкап", "Legacy",
        "Песочница", "Клиент", "Вендор",
    ];
    let labels = if ru { &labels_ru[..] } else { &labels_en[..] };

    let mut ids = Vec::with_capacity(56);
    // 14 handcrafted + 56 bulk = 70
    for i in 1..=56 {
        let ws_i = (i as usize - 1) % WS.len();
        let ws = WS[ws_i];
        let cols = KANBAN[ws_i];
        let status = cols[(i as usize - 1) % cols.len()];
        let id = format!("demo-pr-bulk-{i:02}");
        let label = labels[(i as usize - 1) % labels.len()];
        let name = format!("{label} #{i:02}");
        let profile_path = profiles_root.join(&id);
        std::fs::create_dir_all(&profile_path).map_err(AppError::io)?;
        let tags = serde_json::to_string(&vec![status.to_string()]).map_err(AppError::other)?;
        let proxy_id = if i % 3 == 0 {
            Some(format!("demo-px-bulk-{:02}", ((i - 1) % 32) + 1))
        } else {
            None
        };
        let preset = PRESETS[(i as usize - 1) % PRESETS.len()];
        let (cc, _) = COUNTRIES[(i as usize - 1) % COUNTRIES.len()];

        sqlx::query(
            "INSERT INTO profiles
             (id, name, status, profile_path, browser_type, proxy_id, fingerprint_preset,
              user_agent, platform, timezone, locale, languages, screen_width, screen_height,
              webrtc_mode, geolocation_enabled, latitude, longitude, webgl_vendor, webgl_renderer,
              notes, workspace_id, kanban_status, kanban_order, tags, default_search_engine,
              history_enabled, created_at, updated_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(&name)
        .bind("stopped")
        .bind(profile_path.to_string_lossy().as_ref())
        .bind("camoufox")
        .bind(&proxy_id)
        .bind(preset)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(format!("en-{cc}"))
        .bind("en-US,en")
        .bind(1920_i64)
        .bind(1080_i64)
        .bind("disable")
        .bind(0_i64)
        .bind(None::<f64>)
        .bind(None::<f64>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(ws)
        .bind(status)
        .bind(((i as i64 - 1) % 8) + 10)
        .bind(&tags)
        .bind("ddg")
        .bind(1_i64)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

        ids.push(id);
    }
    Ok(ids)
}

fn demo_secret(n: u32) -> String {
    const ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut s = String::with_capacity(16);
    let mut x = n.wrapping_mul(2654435761);
    for _ in 0..16 {
        s.push(ALPH[(x % 32) as usize] as char);
        x = x.wrapping_mul(2246822519).wrapping_add(n.wrapping_mul(7) + 1);
    }
    s
}

async fn seed_bulk_totp(state: &AppState, profile_ids: &[String], now: &str) -> CmdResult<()> {
    // 12 handcrafted + 48 bulk = 60
    for i in 1..=48 {
        let issuer = ISSUERS[(i as usize - 1) % ISSUERS.len()];
        let id = format!("demo-totp-bulk-{i:02}");
        let name = format!("{}-{}", issuer.to_lowercase(), i);
        let secret = demo_secret(i);
        let ws = WS[(i as usize - 1) % WS.len()];
        let mut tags = vec![format!("workspace:{ws}")];
        if i % 2 == 0 && !profile_ids.is_empty() {
            let pid = &profile_ids[(i as usize - 1) % profile_ids.len()];
            tags.push(format!("profile:{pid}"));
        }
        let tags_json = serde_json::to_string(&tags).map_err(AppError::other)?;

        sqlx::query(
            "INSERT INTO totp_entries (id, name, issuer, secret, algorithm, digits, period, tags, created_at, updated_at)
             VALUES (?, ?, ?, ?, 'SHA1', 6, 30, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(issuer)
        .bind(&secret)
        .bind(&tags_json)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

#[cfg(desktop)]
async fn seed_bulk_ssh(state: &AppState, now: &str) -> CmdResult<()> {
    use crate::commands::ssh_keys::generate_key_material;

    // 2 handcrafted + 4 bulk keys = 6 (ed25519 only — fast)
    let mut key_ids = vec![
        "demo-key-deploy".to_string(),
        "demo-key-laptop".to_string(),
    ];
    for i in 1..=4 {
        let id = format!("demo-key-bulk-{i}");
        let name = format!("bulk-ed25519-{i}");
        let material = tokio::task::spawn_blocking({
            let comment = format!("demo@{name}");
            move || generate_key_material("ed25519", None, comment, None)
        })
        .await
        .map_err(|e| AppError::other(e.to_string()))??;

        sqlx::query(
            "INSERT INTO ssh_keys (
                id, name, algorithm, bits, comment,
                private_key, public_key, passphrase, fingerprint, source,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, 'generated', ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(&material.algorithm)
        .bind(material.bits)
        .bind(&material.comment)
        .bind(&material.private_pem)
        .bind(&material.public_openssh)
        .bind(&material.fingerprint)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
        key_ids.push(id);
    }

    // 5 handcrafted + 20 bulk = 25
    let roles = [
        "web", "api", "db", "cache", "worker", "queue", "metrics", "log", "bastion", "ci",
        "staging", "canary", "edge", "cdn", "vpn", "mail", "dns", "backup", "mirror", "lab",
    ];
    for i in 1..=20 {
        let id = format!("demo-ssh-bulk-{i:02}");
        let role = roles[(i as usize - 1) % roles.len()];
        let name = format!("{role}-{i:02}");
        let host = format!("{role}{i}.example.com");
        let auth = if i % 3 == 0 { "password" } else { "key" };
        let key_id = if auth == "key" {
            Some(key_ids[(i as usize - 1) % key_ids.len()].clone())
        } else {
            None
        };
        let password = if auth == "password" {
            Some(format!("demo-ssh-pass-{i}"))
        } else {
            None
        };
        let ws = WS[(i as usize - 1) % WS.len()];
        let totp = if i % 5 == 0 {
            Some(format!("demo-totp-bulk-{:02}", ((i - 1) % 48) + 1))
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO ssh_connections (
                id, name, host, port, username, auth_type,
                password, private_key, key_passphrase, ssh_key_id,
                requires_2fa, totp_entry_id, proxy_id,
                connect_timeout_sec, keepalive_sec, terminal_theme,
                default_cols, default_rows, created_at, updated_at
            ) VALUES (
                ?, ?, ?, 22, 'deploy', ?,
                ?, NULL, NULL, ?,
                ?, ?, NULL,
                15, 30, NULL,
                120, 32, ?, ?
            )",
        )
        .bind(&id)
        .bind(&name)
        .bind(&host)
        .bind(auth)
        .bind(&password)
        .bind(&key_id)
        .bind(if totp.is_some() { 1_i64 } else { 0 })
        .bind(&totp)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

        sqlx::query(
            "INSERT OR IGNORE INTO ssh_connection_workspaces (connection_id, workspace_id) VALUES (?, ?)",
        )
        .bind(&id)
        .bind(ws)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;
    }
    Ok(())
}

async fn seed_bulk_notes(
    state: &AppState,
    ru: bool,
    profile_ids: &[String],
    now: &str,
) -> CmdResult<()> {
    let folders = [
        "demo-folder-smm-content",
        "demo-folder-smm-clients",
        "demo-folder-dev",
        "demo-folder-dev-arch",
        "demo-folder-dev-snip",
        "demo-folder-devops-run",
        "demo-folder-devops-check",
        "demo-folder-biz-meet",
        "demo-folder-biz-fin",
        "demo-folder-personal",
        "demo-folder-inbox",
    ];
    let tag_sets: &[&[&str]] = &[
        &["work/smm", "status/todo"],
        &["work/smm", "status/wip", "client/brand-a"],
        &["work/dev", "status/todo"],
        &["work/dev", "status/wip"],
        &["work/devops", "infra/prod"],
        &["work/devops", "infra/staging", "status/todo"],
        &["work/biz", "status/wip"],
        &["work/biz", "status/todo"],
        &["personal"],
        &["personal", "status/todo"],
        &["status/wip"],
        &["status/done"],
    ];

    let titles_en: &[&str] = &[
        "Standup notes",
        "Retrospective",
        "Bug triage",
        "Copy draft",
        "Campaign idea",
        "Server checklist",
        "Client call",
        "Invoice reminder",
        "Research scrap",
        "Snippet dump",
        "Weekly goals",
        "Risk log",
        "Design feedback",
        "SEO notes",
        "A/B test plan",
        "Onboarding checklist",
        "Vendor comparison",
        "Security review",
        "Cost estimate",
        "Travel plan",
    ];
    let titles_ru: &[&str] = &[
        "Заметки стендапа",
        "Ретроспектива",
        "Триаж багов",
        "Черновик текста",
        "Идея кампании",
        "Чеклист сервера",
        "Звонок клиенту",
        "Напоминание по счёту",
        "Черновик ресёрча",
        "Свалка сниппетов",
        "Цели недели",
        "Журнал рисков",
        "Фидробек по дизайну",
        "Заметки SEO",
        "План A/B теста",
        "Чеклист онбординга",
        "Сравнение вендоров",
        "Security review",
        "Оценка стоимости",
        "План поездки",
    ];
    let titles = if ru { titles_ru } else { titles_en };

    // 36 handcrafted + 144 bulk ≈ 180
    for i in 1..=144 {
        let title_base = titles[(i as usize - 1) % titles.len()];
        let title = format!("{title_base} #{i:03}");
        let folder = folders[(i as usize - 1) % folders.len()];
        let tags = tag_sets[(i as usize - 1) % tag_sets.len()];
        let ws = WS[(i as usize - 1) % WS.len()];
        let mut bindings = vec![format!("workspace:{ws}")];
        if i % 4 == 0 && !profile_ids.is_empty() {
            bindings.push(format!(
                "profile:{}",
                profile_ids[(i as usize - 1) % profile_ids.len()]
            ));
        }
        if i % 11 == 0 {
            bindings.push("domain:github.com".into());
        }
        if i % 13 == 0 {
            bindings.push("domain:instagram.com".into());
        }

        let content = if ru {
            format!(
                "# {title}\n\nАвтосгенерированная демо-заметка #{i}.\n\n## Контекст\nWorkspace `{ws}`.\n\n## Задачи\n- [ ] Пункт A\n- [x] Пункт B\n- [ ] Пункт C\n\n## Заметки\nКраткий черновик для видео-демо. Можно править или удалить.\n\n```\necho demo-{i}\n```\n"
            )
        } else {
            format!(
                "# {title}\n\nAuto-generated demo note #{i}.\n\n## Context\nWorkspace `{ws}`.\n\n## Tasks\n- [ ] Item A\n- [x] Item B\n- [ ] Item C\n\n## Notes\nShort draft for the video demo. Safe to edit or delete.\n\n```\necho demo-{i}\n```\n"
            )
        };

        let note = insert_note(
            NewNote {
                id: Uuid::new_v4().to_string(),
                title,
                format: "md".into(),
                bindings,
                tags: tags.iter().map(|s| s.to_string()).collect(),
                content,
                created_at: now.to_string(),
                updated_at: now.to_string(),
            },
            state,
        )
        .await?;

        sqlx::query(
            "INSERT OR IGNORE INTO note_folder_links (note_id, folder_id) VALUES (?, ?)",
        )
        .bind(&note.id)
        .bind(folder)
        .execute(&state.db)
        .await
        .map_err(AppError::db)?;

        // Sprinkle pinned / archived
        if i % 37 == 0 {
            sqlx::query("UPDATE notes SET pinned = 1 WHERE id = ?")
                .bind(&note.id)
                .execute(&state.db)
                .await
                .map_err(AppError::db)?;
        }
        if i % 41 == 0 {
            sqlx::query("UPDATE notes SET archived = 1 WHERE id = ?")
                .bind(&note.id)
                .execute(&state.db)
                .await
                .map_err(AppError::db)?;
        }
        if i % 73 == 0 {
            let fts: Option<(Option<i64>,)> =
                sqlx::query_as("SELECT fts_rowid FROM notes WHERE id = ?")
                    .bind(&note.id)
                    .fetch_optional(&state.db)
                    .await
                    .map_err(AppError::db)?;
            sqlx::query("UPDATE notes SET deleted = 1, updated_at = ? WHERE id = ?")
                .bind(now)
                .bind(&note.id)
                .execute(&state.db)
                .await
                .map_err(AppError::db)?;
            if let Some((Some(rowid),)) = fts {
                let _ = sqlx::query("DELETE FROM notes_fts WHERE rowid = ?")
                    .bind(rowid)
                    .execute(&state.db)
                    .await;
            }
        }
    }
    Ok(())
}
