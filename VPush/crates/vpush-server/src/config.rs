//! Configuration: one TOML file, checked as a whole.
//!
//! A bad file is reported with every problem at once, not the first one, so
//! fixing it takes one round.

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::logging::{LogFormat, LogSpec};

/// Where the config is looked for when neither `--config` nor `VPUSH_CONFIG` is given.
pub const DEFAULT_PATH: &str = "/opt/vpush/etc/vpush.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The address clients use, as they see it: `https://push.example.org`.
    pub public_url: String,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub log: LogConfig,
    /// Apps whose devices are served, by app id. Tokens of FCM and APNs
    /// belong to an app, so each app brings its own keys.
    #[serde(default)]
    pub apps: BTreeMap<String, AppConfig>,
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub relays: RelaysConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub pipeline: PipelineConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StoreConfig {
    /// The database file. Created when missing.
    pub path: PathBuf,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("/opt/vpush/data/vpush.db"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RelaysConfig {
    /// `allow_list`: only the relays named in `allow` are watched.
    pub policy: String,
    pub allow: Vec<AllowedRelayConfig>,
}

impl Default for RelaysConfig {
    fn default() -> Self {
        Self {
            policy: "allow_list".to_string(),
            allow: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedRelayConfig {
    /// `wss://relay.example.org`, without a key in it.
    pub url: String,
    /// File with the key the relay's gate asks for, when it has a gate.
    pub api_key_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsConfig {
    pub devices_per_pubkey: u32,
    pub relays_per_device: u32,
    pub groups_per_device: u32,
    /// A registration nobody renewed for this long is forgotten.
    pub registration_days: u32,
    /// Test pushes a device may ask for in an hour.
    pub test_per_hour: u32,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            devices_per_pubkey: 10,
            relays_per_device: 16,
            groups_per_device: 500,
            registration_days: 30,
            test_per_hour: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PipelineConfig {
    /// A device is woken about one chat once in this many seconds. What
    /// comes in between is counted and told in one push when the time is up.
    pub throttle_secs: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { throttle_secs: 20 }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub fcm: Option<FcmConfig>,
    /// Reserved: the section is read and checked, delivery comes later.
    pub apns: Option<ApnsConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FcmConfig {
    /// The service account file of the Firebase project.
    pub service_account: PathBuf,
    /// Where FCM is. Set in tests only.
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApnsConfig {
    pub key_file: PathBuf,
    pub key_id: String,
    pub team_id: String,
    /// Bundle id of the app.
    pub topic: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ServerConfig {
    /// TLS is the reverse proxy's job, so this is normally a loopback address.
    pub listen: String,
    pub request_timeout_secs: u64,
    pub max_body_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8090".to_string(),
            request_timeout_secs: 15,
            max_body_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AdminConfig {
    /// Unix socket of `vpush ctl`. Reachable by the file's owner and group only.
    pub socket: PathBuf,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            socket: PathBuf::from("/run/vpush/admin.sock"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogConfig {
    pub format: String,
    /// Level of everything not named in `targets`.
    pub level: String,
    /// Level per module or crate: `relay = "debug"`, `nostr_sdk = "warn"`.
    pub targets: BTreeMap<String, String>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: "text".to_string(),
            level: "info".to_string(),
            targets: BTreeMap::new(),
        }
    }
}

/// Everything wrong with a config file.
#[derive(Debug)]
pub struct ConfigError {
    pub path: PathBuf,
    pub problems: Vec<String>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "config {} is not usable:", self.path.display())?;
        for p in &self.problems {
            writeln!(f, "  - {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let fail = |problem: String| ConfigError {
            path: path.to_path_buf(),
            problems: vec![problem],
        };
        let text = std::fs::read_to_string(path).map_err(|e| fail(format!("cannot read: {e}")))?;
        let config = Self::parse(&text).map_err(|problems| ConfigError {
            path: path.to_path_buf(),
            problems,
        })?;
        let problems = config.file_problems();
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(ConfigError {
                path: path.to_path_buf(),
                problems,
            })
        }
    }

    pub fn parse(text: &str) -> Result<Self, Vec<String>> {
        let config: Config = toml::from_str(text).map_err(|e| vec![e.to_string()])?;
        let problems = config.problems();
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(problems)
        }
    }

    fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();

        match url::Url::parse(&self.public_url) {
            Err(e) => out.push(format!("public_url: {e}")),
            Ok(u) => {
                let local = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
                match u.scheme() {
                    "https" => {}
                    "http" if local => {}
                    "http" => out.push(
                        "public_url: http is accepted for localhost only, use https".to_string(),
                    ),
                    other => out.push(format!("public_url: scheme `{other}` is not http(s)")),
                }
                if u.query().is_some() || u.fragment().is_some() {
                    out.push("public_url: must not carry a query or a fragment".to_string());
                }
                if self.public_url.ends_with('/') {
                    out.push("public_url: drop the trailing slash".to_string());
                }
            }
        }

        if self.server.listen.parse::<SocketAddr>().is_err() {
            out.push(format!(
                "server.listen: `{}` is not an address like 127.0.0.1:8090",
                self.server.listen
            ));
        }
        if self.server.request_timeout_secs == 0 {
            out.push("server.request_timeout_secs: must be above zero".to_string());
        }
        if self.server.max_body_bytes == 0 {
            out.push("server.max_body_bytes: must be above zero".to_string());
        }

        if !self.admin.socket.is_absolute() {
            out.push(format!(
                "admin.socket: `{}` must be an absolute path",
                self.admin.socket.display()
            ));
        }

        if let Err(e) = self.log_format() {
            out.push(format!("log.format: {e}"));
        }
        if let Err(e) = self.log_spec() {
            out.push(format!("log: {e}"));
        }

        for (id, app) in &self.apps {
            let valid = !id.is_empty()
                && id.len() <= 100
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
            if !valid {
                out.push(format!(
                    "apps: `{id}` is not an app id (letters, digits, `.`, `_`, `-`)"
                ));
            }
            if let Some(fcm) = &app.fcm {
                if !fcm.service_account.is_absolute() {
                    out.push(format!(
                        "apps.{id}.fcm.service_account: `{}` must be an absolute path",
                        fcm.service_account.display()
                    ));
                }
                if let Some(endpoint) = &fcm.endpoint {
                    match url::Url::parse(endpoint) {
                        Ok(u) if matches!(u.scheme(), "http" | "https") => {}
                        _ => out.push(format!(
                            "apps.{id}.fcm.endpoint: `{endpoint}` is not an http(s) address"
                        )),
                    }
                }
            }
            if let Some(apns) = &app.apns {
                if !apns.key_file.is_absolute() {
                    out.push(format!(
                        "apps.{id}.apns.key_file: `{}` must be an absolute path",
                        apns.key_file.display()
                    ));
                }
                for (name, value) in [
                    ("key_id", &apns.key_id),
                    ("team_id", &apns.team_id),
                    ("topic", &apns.topic),
                ] {
                    if value.trim().is_empty() {
                        out.push(format!("apps.{id}.apns.{name}: is empty"));
                    }
                }
            }
        }

        if !self.store.path.is_absolute() {
            out.push(format!(
                "store.path: `{}` must be an absolute path",
                self.store.path.display()
            ));
        }

        if self.relays.policy != "allow_list" {
            out.push(format!(
                "relays.policy: `{}` is not known; the only one today is allow_list",
                self.relays.policy
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (i, relay) in self.relays.allow.iter().enumerate() {
            match crate::relays::normalize(&relay.url) {
                Err(e) => out.push(format!(
                    "relays.allow[{i}].url: `{}`: {e}",
                    crate::relays::shown(&relay.url)
                )),
                Ok(url) => {
                    if !seen.insert(url.clone()) {
                        out.push(format!("relays.allow[{i}].url: `{url}` is named twice"));
                    }
                }
            }
            if let Some(path) = &relay.api_key_file {
                if !path.is_absolute() {
                    out.push(format!(
                        "relays.allow[{i}].api_key_file: `{}` must be an absolute path",
                        path.display()
                    ));
                }
            }
        }

        for (name, value) in [
            ("devices_per_pubkey", self.limits.devices_per_pubkey),
            ("relays_per_device", self.limits.relays_per_device),
            ("groups_per_device", self.limits.groups_per_device),
            ("registration_days", self.limits.registration_days),
        ] {
            if value == 0 {
                out.push(format!("limits.{name}: must be above zero"));
            }
        }

        // Zero is not "as fast as it goes": with no window nothing is held
        // back, and every event of a burst wakes the phone.
        if self.pipeline.throttle_secs == 0 {
            out.push("pipeline.throttle_secs: must be above zero".to_string());
        }

        out
    }

    /// Problems with the files the config names. Looked for when the config
    /// is loaded from disk, on the machine it will run on.
    fn file_problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Err(e) = crate::relays::RelayPolicy::from_config(self) {
            out.push(format!("relays.allow: {e}"));
        }
        for (id, app) in &self.apps {
            if let Some(fcm) = &app.fcm {
                if let Err(e) = crate::delivery::fcm::ServiceAccount::read(&fcm.service_account) {
                    out.push(format!("apps.{id}.fcm.service_account: {e}"));
                }
            }
            if let Some(apns) = &app.apns {
                if let Err(e) = std::fs::metadata(&apns.key_file) {
                    out.push(format!(
                        "apps.{id}.apns.key_file: cannot read {}: {e}",
                        apns.key_file.display()
                    ));
                }
            }
        }
        out
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.server
            .listen
            .parse()
            .expect("checked when the config was loaded")
    }

    pub fn log_format(&self) -> Result<LogFormat, String> {
        self.log.format.parse()
    }

    pub fn log_spec(&self) -> Result<LogSpec, String> {
        LogSpec::from_parts(&self.log.level, &self.log.targets)
    }

    /// What the server runs with, for the log at start. Secrets never get here.
    pub fn summary(&self) -> Vec<(String, String)> {
        let mut out = self.summary_base();
        for (id, app) in &self.apps {
            if let Some(fcm) = &app.fcm {
                // The path, never the contents.
                out.push((
                    format!("apps.{id}.fcm.service_account"),
                    fcm.service_account.display().to_string(),
                ));
                if let Some(endpoint) = &fcm.endpoint {
                    out.push((format!("apps.{id}.fcm.endpoint"), endpoint.clone()));
                }
            }
            if let Some(apns) = &app.apns {
                out.push((
                    format!("apps.{id}.apns"),
                    format!("topic {} (delivery is not built yet)", apns.topic),
                ));
            }
        }
        out.push(("store.path".to_string(), self.store.path.display().to_string()));
        out.push(("relays.policy".to_string(), self.relays.policy.clone()));
        for relay in &self.relays.allow {
            // Whether there is a key, never the key.
            let gate = if relay.api_key_file.is_some() { " (with a key)" } else { "" };
            out.push((
                "relays.allow".to_string(),
                format!("{}{gate}", crate::relays::shown(&relay.url)),
            ));
        }
        let l = &self.limits;
        out.push((
            "limits".to_string(),
            format!(
                "devices_per_pubkey={} relays_per_device={} groups_per_device={} registration_days={} test_per_hour={}",
                l.devices_per_pubkey, l.relays_per_device, l.groups_per_device, l.registration_days, l.test_per_hour
            ),
        ));
        out.push((
            "pipeline.throttle_secs".to_string(),
            self.pipeline.throttle_secs.to_string(),
        ));
        out
    }

    fn summary_base(&self) -> Vec<(String, String)> {
        vec![
            ("public_url".to_string(), self.public_url.clone()),
            ("server.listen".to_string(), self.server.listen.clone()),
            (
                "server.request_timeout_secs".to_string(),
                self.server.request_timeout_secs.to_string(),
            ),
            (
                "server.max_body_bytes".to_string(),
                self.server.max_body_bytes.to_string(),
            ),
            (
                "admin.socket".to_string(),
                self.admin.socket.display().to_string(),
            ),
            ("log.format".to_string(), self.log.format.clone()),
            (
                "log".to_string(),
                self.log_spec().map(|s| s.to_string()).unwrap_or_default(),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_gets_defaults() {
        let c = Config::parse(r#"public_url = "https://push.example.org""#).unwrap();
        assert_eq!(c.server.listen, "127.0.0.1:8090");
        assert_eq!(c.admin.socket, PathBuf::from("/run/vpush/admin.sock"));
        assert_eq!(c.log.level, "info");
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let problems = Config::parse(
            r#"
            public_url = "http://push.example.org/"
            [server]
            listen = "nowhere"
            [admin]
            socket = "relative.sock"
            [log]
            format = "xml"
            level = "loud"
            "#,
        )
        .unwrap_err();
        let all = problems.join("\n");
        for needle in [
            "public_url: http is accepted",
            "trailing slash",
            "server.listen",
            "admin.socket",
            "log.format",
            "`loud`",
        ] {
            assert!(all.contains(needle), "missing `{needle}` in:\n{all}");
        }
    }

    #[test]
    fn unknown_key_is_an_error() {
        let problems = Config::parse(
            r#"
            public_url = "https://push.example.org"
            [server]
            lisen = "127.0.0.1:1"
            "#,
        )
        .unwrap_err();
        assert!(problems[0].contains("lisen"), "{problems:?}");
    }

    #[test]
    fn wrong_type_is_an_error_not_a_panic() {
        let problems = Config::parse(
            r#"
            public_url = "https://push.example.org"
            [server]
            request_timeout_secs = "15"
            "#,
        )
        .unwrap_err();
        assert!(problems[0].contains("request_timeout_secs"), "{problems:?}");
    }

    #[test]
    fn a_throttle_of_no_seconds_is_refused() {
        let with = |secs: u64| {
            Config::parse(&format!(
                "public_url = \"https://push.example.org\"\n[pipeline]\nthrottle_secs = {secs}\n"
            ))
        };
        let problems = with(0).unwrap_err();
        assert_eq!(problems, ["pipeline.throttle_secs: must be above zero"]);
        assert_eq!(with(1).unwrap().pipeline.throttle_secs, 1);
    }

    #[test]
    fn http_is_fine_for_localhost() {
        Config::parse(r#"public_url = "http://localhost:8090""#).unwrap();
    }

    #[test]
    fn log_targets_are_read() {
        let c = Config::parse(
            r#"
            public_url = "https://push.example.org"
            [log]
            level = "warn"
            [log.targets]
            relay = "debug"
            "#,
        )
        .unwrap();
        assert_eq!(c.log_spec().unwrap().to_string(), "warn,relay=debug");
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;

    #[test]
    fn apps_are_read() {
        let c = Config::parse(
            r#"
            public_url = "https://push.example.org"
            [apps."net.veydan.mobile".fcm]
            service_account = "/opt/vpush/etc/secrets/fcm.json"
            [apps."net.veydan.mobile".apns]
            key_file = "/opt/vpush/etc/secrets/apns.p8"
            key_id = "ABC"
            team_id = "DEF"
            topic = "net.veydan.mobile"
            "#,
        )
        .unwrap();
        let app = &c.apps["net.veydan.mobile"];
        assert!(app.fcm.is_some());
        assert_eq!(app.apns.as_ref().unwrap().topic, "net.veydan.mobile");
    }

    #[test]
    fn app_problems_are_reported() {
        let problems = Config::parse(
            r#"
            public_url = "https://push.example.org"
            [apps."bad id".fcm]
            service_account = "fcm.json"
            endpoint = "ftp://x"
            "#,
        )
        .unwrap_err()
        .join("\n");
        for needle in ["not an app id", "must be an absolute path", "not an http(s) address"] {
            assert!(problems.contains(needle), "missing `{needle}` in:\n{problems}");
        }
    }

    #[test]
    fn a_relay_key_that_will_not_do_is_found_when_loading_and_is_not_shown() {
        let dir = std::env::temp_dir().join(format!("vpush-cfg-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (path, key) = (dir.join("vpush.toml"), dir.join("relay.key"));
        std::fs::write(&key, "top?secret&of=the-relay\n").unwrap();
        std::fs::write(
            &path,
            format!(
                "public_url = \"https://push.example.org\"\n\
                 [[relays.allow]]\nurl = \"wss://node-1.veydan.net\"\napi_key_file = \"{}\"\n",
                key.display()
            ),
        )
        .unwrap();
        let e = Config::load(&path).unwrap_err().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(e.contains(&format!("relays.allow: {}: a key is", key.display())), "{e}");
        assert!(!e.contains("secret"), "{e}");
    }

    #[test]
    fn a_missing_key_file_is_found_when_loading() {
        let dir = std::env::temp_dir().join(format!("vpush-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vpush.toml");
        std::fs::write(
            &path,
            "public_url = \"https://push.example.org\"\n\
             [apps.app.fcm]\nservice_account = \"/nonexistent/fcm.json\"\n",
        )
        .unwrap();
        let e = Config::load(&path).unwrap_err().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(e.contains("apps.app.fcm.service_account: cannot read"), "{e}");
    }
}
