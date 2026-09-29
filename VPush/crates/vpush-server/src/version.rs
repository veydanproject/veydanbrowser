//! What this binary is: version, commit, build time.

use std::sync::OnceLock;
use std::time::Instant;

use vpush_proto::{Health, HealthStatus};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_SHA: &str = env!("VPUSH_GIT_SHA");

pub fn built_at() -> u64 {
    env!("VPUSH_BUILT_AT").parse().unwrap_or(0)
}

/// `0.1.0-ab12cd34e`, the name of a release directory on the server.
pub fn release_name() -> String {
    format!("{VERSION}-{GIT_SHA}")
}

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Starts the uptime clock. Later calls change nothing.
pub fn mark_started() {
    STARTED.get_or_init(Instant::now);
}

pub fn health() -> Health {
    Health {
        status: HealthStatus::Ok,
        version: VERSION.to_string(),
        git_sha: GIT_SHA.to_string(),
        built_at: built_at(),
        uptime_secs: STARTED.get().map(|s| s.elapsed().as_secs()).unwrap_or(0),
    }
}
