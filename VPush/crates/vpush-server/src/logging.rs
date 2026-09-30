//! Logging that is steered while the server runs.
//!
//! Levels are set per module. The config gives the base; `vpush ctl log set`
//! lays overrides on top, each with a lifetime after which it goes away by
//! itself. Nothing here needs a restart.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, Layer, Registry};

/// Crate the server's modules live in; short module names expand to it.
const OWN_CRATE: &str = "vpush_server";

/// Modules that may be named without the crate: `relay=debug`.
const MODULES: &[&str] = &[
    "admin",
    "adminbot",
    "api",
    "auth",
    "broadcast",
    "config",
    "delivery",
    "logging",
    "netguard",
    "pipeline",
    "relay",
    "relays",
    "serve",
    "store",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(format!("`{other}` is not one of: text, json")),
        }
    }
}

fn parse_level(s: &str) -> Result<LevelFilter, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(LevelFilter::OFF),
        "error" => Ok(LevelFilter::ERROR),
        "warn" => Ok(LevelFilter::WARN),
        "info" => Ok(LevelFilter::INFO),
        "debug" => Ok(LevelFilter::DEBUG),
        "trace" => Ok(LevelFilter::TRACE),
        other => Err(format!(
            "`{other}` is not a level (off, error, warn, info, debug, trace)"
        )),
    }
}

fn level_name(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::OFF => "off",
        LevelFilter::ERROR => "error",
        LevelFilter::WARN => "warn",
        LevelFilter::INFO => "info",
        LevelFilter::DEBUG => "debug",
        LevelFilter::TRACE => "trace",
    }
}

/// `relay` and `vpush::relay` both mean `vpush_server::relay`; anything else
/// is taken as written, so other crates can be named too.
fn full_target(name: &str) -> Result<String, String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':');
    if !valid {
        return Err(format!("`{name}` is not a module name"));
    }
    if let Some(rest) = name.strip_prefix("vpush::") {
        return Ok(format!("{OWN_CRATE}::{rest}"));
    }
    if MODULES.contains(&name) {
        return Ok(format!("{OWN_CRATE}::{name}"));
    }
    Ok(name.to_string())
}

/// The way a target is shown back: own modules by their short name.
fn short_target(full: &str) -> &str {
    match full.strip_prefix(OWN_CRATE).and_then(|r| r.strip_prefix("::")) {
        Some(rest) if MODULES.contains(&rest) => rest,
        _ => full,
    }
}

/// A set of levels: one for everything, and one per named target.
///
/// Written as `info,relay=debug,nostr_sdk=warn`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSpec {
    pub default: Option<LevelFilter>,
    /// Keyed by the full target.
    pub targets: BTreeMap<String, LevelFilter>,
}

impl LogSpec {
    pub fn from_parts(level: &str, targets: &BTreeMap<String, String>) -> Result<Self, String> {
        let mut spec = LogSpec {
            default: Some(parse_level(level)?),
            targets: BTreeMap::new(),
        };
        for (name, level) in targets {
            spec.targets
                .insert(full_target(name)?, parse_level(level)?);
        }
        Ok(spec)
    }
}

impl FromStr for LogSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let mut spec = LogSpec {
            default: None,
            targets: BTreeMap::new(),
        };
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((name, level)) => {
                    spec.targets
                        .insert(full_target(name)?, parse_level(level)?);
                }
                None => spec.default = Some(parse_level(part)?),
            }
        }
        if spec.default.is_none() && spec.targets.is_empty() {
            return Err("nothing to set; write `debug` or `relay=debug`".to_string());
        }
        Ok(spec)
    }
}

impl fmt::Display for LogSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if let Some(level) = self.default {
            parts.push(level_name(level).to_string());
        }
        for (target, level) in &self.targets {
            parts.push(format!("{}={}", short_target(target), level_name(*level)));
        }
        f.write_str(&parts.join(","))
    }
}

/// `90s`, `15m`, `2h`, `1d`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let bad = || format!("`{s}` is not a duration like 90s, 15m, 2h, 1d");
    let split = s.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
    let (number, unit) = s.split_at(split);
    let number: u64 = number.parse().map_err(|_| bad())?;
    let unit = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err(bad()),
    };
    match number.checked_mul(unit) {
        Some(secs) if secs > 0 => Ok(Duration::from_secs(secs)),
        _ => Err(bad()),
    }
}

/// An override lives no longer than this. One that was forgotten at `trace`
/// fills the disk, so there is none without an end.
pub const MAX_TTL: Duration = Duration::from_secs(24 * 3600);

/// One override laid over the base. `target: None` is the level of everything.
#[derive(Debug, Clone)]
struct Override {
    target: Option<String>,
    level: LevelFilter,
    expires: Instant,
}

#[derive(Debug)]
struct State {
    base: LogSpec,
    overrides: Vec<Override>,
}

impl State {
    fn effective(&self) -> LogSpec {
        let mut spec = self.base.clone();
        for o in &self.overrides {
            match &o.target {
                None => spec.default = Some(o.level),
                Some(t) => {
                    spec.targets.insert(t.clone(), o.level);
                }
            }
        }
        spec
    }

    fn put(&mut self, spec: &LogSpec, expires: Instant) {
        let mut put_one = |target: Option<String>, level| {
            self.overrides.retain(|o| o.target != target);
            self.overrides.push(Override {
                target,
                level,
                expires,
            });
        };
        if let Some(level) = spec.default {
            put_one(None, level);
        }
        for (target, level) in &spec.targets {
            put_one(Some(target.clone()), *level);
        }
    }

    /// Drops what has run out; returns what was dropped.
    fn expire(&mut self, now: Instant) -> Vec<Override> {
        let (gone, kept) = std::mem::take(&mut self.overrides)
            .into_iter()
            .partition(|o| o.expires <= now);
        self.overrides = kept;
        gone
    }
}

fn to_targets(spec: &LogSpec) -> Targets {
    let mut targets = Targets::new().with_default(spec.default.unwrap_or(LevelFilter::INFO));
    for (target, level) in &spec.targets {
        targets = targets.with_target(target.clone(), *level);
    }
    targets
}

/// What `vpush ctl log show` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogView {
    /// From the config.
    pub base: String,
    /// What is in force now.
    pub effective: String,
    pub overrides: Vec<OverrideView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideView {
    /// `*` is the level of everything.
    pub target: String,
    pub level: String,
    pub expires_in_secs: u64,
}

/// Handle to the running log filter.
pub struct LogControl {
    handle: reload::Handle<Targets, Registry>,
    state: Mutex<State>,
}

impl LogControl {
    fn apply(&self, state: &State) {
        // Fails only when the subscriber is gone, which means the process is ending.
        let _ = self.handle.reload(to_targets(&state.effective()));
    }

    /// Lays `spec` over the base for `ttl`, or until reset.
    ///
    /// `ttl` is a number somebody typed. One the clock cannot count to is
    /// refused like any other that is too long: added to now without a
    /// look, it takes the process down.
    pub fn set(&self, spec: &LogSpec, ttl: Duration) -> Result<LogView, String> {
        let expires = Some(ttl)
            .filter(|ttl| *ttl <= MAX_TTL)
            .and_then(|ttl| Instant::now().checked_add(ttl))
            .ok_or("an override of log levels lasts 24h at most")?;
        let mut state = self.state.lock().unwrap();
        state.put(spec, expires);
        self.apply(&state);
        Ok(view(&state))
    }

    /// Removes every override.
    pub fn reset(&self) -> LogView {
        let mut state = self.state.lock().unwrap();
        state.overrides.clear();
        self.apply(&state);
        view(&state)
    }

    /// Replaces the base, as after a config reload. Overrides stay.
    pub fn set_base(&self, base: LogSpec) -> LogView {
        let mut state = self.state.lock().unwrap();
        state.base = base;
        self.apply(&state);
        view(&state)
    }

    pub fn show(&self) -> LogView {
        view(&self.state.lock().unwrap())
    }

    fn expire(&self) {
        let gone = {
            let mut state = self.state.lock().unwrap();
            let gone = state.expire(Instant::now());
            if !gone.is_empty() {
                self.apply(&state);
            }
            gone
        };
        for o in gone {
            tracing::info!(
                target = o.target.as_deref().map(short_target).unwrap_or("*"),
                level = level_name(o.level),
                "log override ran out"
            );
        }
    }

    /// Watches the lifetimes of overrides. Runs until the process ends.
    pub fn spawn_expiry(self: &Arc<Self>) {
        let control = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                control.expire();
            }
        });
    }
}

fn view(state: &State) -> LogView {
    let now = Instant::now();
    LogView {
        base: state.base.to_string(),
        effective: state.effective().to_string(),
        overrides: state
            .overrides
            .iter()
            .map(|o| OverrideView {
                target: o
                    .target
                    .as_deref()
                    .map(short_target)
                    .unwrap_or("*")
                    .to_string(),
                level: level_name(o.level).to_string(),
                expires_in_secs: o.expires.saturating_duration_since(now).as_secs(),
            })
            .collect(),
    }
}

/// What is written in place of a secret.
const REDACTED: &str = "***";
/// A secret shorter than this is not looked for: it would be found in
/// words that are no secret at all.
const SHORTEST_SECRET: usize = 8;

/// Takes the known secrets out of whatever is logged, by whomever: this
/// server, or a library that logs an address with a key in it.
///
/// The secrets are the keys of gated relays. They have to be put into an
/// address to connect, and an address is what libraries like to log.
#[derive(Clone, Default)]
pub struct Redactor {
    secrets: Arc<Vec<String>>,
}

impl Redactor {
    pub fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .filter(|s| s.len() >= SHORTEST_SECRET)
            .collect();
        // The longer first, in case one is a part of another.
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        Self { secrets: Arc::new(secrets) }
    }

    /// `None`: there is nothing to take out.
    pub fn clean(&self, text: &str) -> Option<String> {
        if !self.secrets.iter().any(|s| text.contains(s.as_str())) {
            return None;
        }
        let mut out = text.to_string();
        for secret in self.secrets.iter() {
            out = out.replace(secret.as_str(), REDACTED);
        }
        Some(out)
    }
}

pub struct RedactingWriter {
    redactor: Redactor,
    out: std::io::Stdout,
}

impl std::io::Write for RedactingWriter {
    // A line of the log comes in one piece, so a secret is never cut in two.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.redactor.clean(&String::from_utf8_lossy(buf)) {
            Some(cleaned) => self.out.write_all(cleaned.as_bytes())?,
            None => self.out.write_all(buf)?,
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Redactor {
    type Writer = RedactingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            redactor: self.clone(),
            out: std::io::stdout(),
        }
    }
}

/// Installs the process-wide logger. Call once, before anything logs.
pub fn init(format: LogFormat, base: LogSpec, redactor: Redactor) -> Arc<LogControl> {
    let state = State {
        base,
        overrides: Vec::new(),
    };
    let (filter, handle) = reload::Layer::new(to_targets(&state.effective()));

    let out: Box<dyn Layer<Registry> + Send + Sync> = match format {
        LogFormat::Text => tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_writer(redactor)
            .boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_writer(redactor)
            .boxed(),
    };

    tracing_subscriber::registry()
        .with(out.with_filter(filter))
        .init();

    Arc::new(LogControl {
        handle,
        state: Mutex::new(state),
    })
}

#[cfg(test)]
mod redactor_tests {
    use super::*;

    const KEY: &str = "d0f9c18a09ab2547f5c9aaddc5dfec3b";

    #[test]
    fn a_key_in_an_address_is_taken_out() {
        let r = Redactor::new([KEY.to_string()]);
        let line = format!("connecting to wss://node-1.veydan.net/?key={KEY} (attempt 2)\n");
        assert_eq!(
            r.clean(&line).unwrap(),
            "connecting to wss://node-1.veydan.net/?key=*** (attempt 2)\n"
        );
    }

    #[test]
    fn every_place_and_every_secret() {
        let r = Redactor::new(["first-secret".to_string(), "second-secret".to_string()]);
        let cleaned = r.clean("a first-secret b second-secret c first-secret").unwrap();
        assert_eq!(cleaned, "a *** b *** c ***");
    }

    #[test]
    fn a_line_without_secrets_is_left_as_it_is() {
        let r = Redactor::new([KEY.to_string()]);
        assert_eq!(r.clean("relay state=connected"), None);
        assert_eq!(Redactor::default().clean(KEY), None);
    }

    #[test]
    fn what_is_too_short_to_be_a_secret_is_not_looked_for() {
        let r = Redactor::new(["key".to_string(), String::new()]);
        assert_eq!(r.clean("the key of the relay"), None);
    }

    #[test]
    fn a_secret_that_holds_another_is_taken_out_whole() {
        let r = Redactor::new(["abcdefgh".to_string(), "abcdefgh-and-more".to_string()]);
        assert_eq!(r.clean("x abcdefgh-and-more y").unwrap(), "x *** y");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(s: &str) -> LogSpec {
        s.parse().unwrap()
    }

    fn state(base: &str) -> State {
        State {
            base: spec(base),
            overrides: Vec::new(),
        }
    }

    /// When an override put now runs out, in a test that does not wait for it.
    fn in_an_hour() -> Instant {
        Instant::now() + Duration::from_secs(3600)
    }

    /// A control whose filter no subscriber listens to.
    fn control(base: &str) -> LogControl {
        let state = state(base);
        let (_filter, handle) = reload::Layer::new(to_targets(&state.effective()));
        LogControl {
            handle,
            state: Mutex::new(state),
        }
    }

    #[test]
    fn spec_round_trips() {
        assert_eq!(
            spec(" info , relay=debug, nostr_sdk=warn ").to_string(),
            "info,nostr_sdk=warn,relay=debug"
        );
    }

    #[test]
    fn short_and_prefixed_names_are_one_target() {
        let a = spec("relay=debug");
        let b = spec("vpush::relay=debug");
        let c = spec("vpush_server::relay=debug");
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert!(a.targets.contains_key("vpush_server::relay"));
    }

    #[test]
    fn nested_module_keeps_its_path() {
        let s = spec("vpush::relay::probe=trace");
        assert!(s.targets.contains_key("vpush_server::relay::probe"));
        assert_eq!(s.to_string(), "vpush_server::relay::probe=trace");
    }

    #[test]
    fn bad_specs_are_refused() {
        for bad in ["", "loud", "relay=loud", "re lay=debug", "=debug", ","] {
            assert!(bad.parse::<LogSpec>().is_err(), "`{bad}` was accepted");
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("15m").unwrap(), Duration::from_secs(900));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
        for bad in ["", "15", "m", "0s", "1w", "-5m", "1.5h"] {
            assert!(parse_duration(bad).is_err(), "`{bad}` was accepted");
        }
    }

    #[test]
    fn override_wins_over_base_and_base_returns() {
        let mut s = state("info,relay=warn");
        s.put(&spec("relay=trace,api=debug"), in_an_hour());
        assert_eq!(s.effective().to_string(), "info,api=debug,relay=trace");
        s.overrides.clear();
        assert_eq!(s.effective().to_string(), "info,relay=warn");
    }

    #[test]
    fn second_override_of_a_target_replaces_the_first() {
        let mut s = state("info");
        s.put(&spec("relay=trace"), in_an_hour());
        s.put(&spec("relay=debug"), in_an_hour());
        assert_eq!(s.overrides.len(), 1);
        assert_eq!(s.effective().to_string(), "info,relay=debug");
    }

    #[test]
    fn only_what_ran_out_is_dropped() {
        let now = Instant::now();
        let mut s = state("info");
        s.put(&spec("relay=trace"), now + Duration::from_secs(10));
        s.put(&spec("api=debug"), now + Duration::from_secs(100));
        s.put(&spec("debug"), now + Duration::from_secs(1000));

        assert!(s.expire(now).is_empty());

        let gone = s.expire(now + Duration::from_secs(11));
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].target.as_deref(), Some("vpush_server::relay"));
        assert_eq!(s.effective().to_string(), "debug,api=debug");

        s.expire(now + Duration::from_secs(999));
        assert_eq!(s.effective().to_string(), "debug");

        s.expire(now + Duration::from_secs(1000));
        assert_eq!(s.effective().to_string(), "info", "nothing is laid over the config for ever");
    }

    #[test]
    fn an_override_lasts_a_day_at_most() {
        let control = control("info");
        let view = control.set(&spec("relay=debug"), MAX_TTL).unwrap();
        assert_eq!(view.effective, "info,relay=debug");
        assert!(view.overrides[0].expires_in_secs >= MAX_TTL.as_secs() - 1);

        // The last two are beyond what the clock can count to: adding them
        // to now without looking takes the process down.
        for too_long in [
            MAX_TTL + Duration::from_secs(1),
            Duration::from_secs(9_300_000_000_000_000_000),
            Duration::MAX,
        ] {
            let refused = control.set(&spec("relay=trace"), too_long).unwrap_err();
            assert!(refused.contains("24h"), "{refused}");
        }
        assert_eq!(control.show().effective, "info,relay=debug", "a refused change changes nothing");
    }

    #[test]
    fn view_names_targets_the_short_way() {
        let mut s = state("info");
        s.put(&spec("trace,relay=debug"), in_an_hour());
        let v = view(&s);
        let targets: Vec<_> = v.overrides.iter().map(|o| o.target.as_str()).collect();
        assert_eq!(targets, ["*", "relay"]);
        assert_eq!(v.base, "info");
        assert_eq!(v.effective, "trace,relay=debug");
    }
}
