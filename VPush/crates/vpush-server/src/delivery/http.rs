//! The HTTPS client providers talk through.
//!
//! TLS is set up here and not taken from the process: bundled roots and a
//! named crypto provider, so the static binary behaves the same on every
//! server, whatever is installed there.

use std::sync::Arc;
use std::time::Duration;

fn tls_config() -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

pub fn client(connect: Duration, total: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls_config()?)
        .connect_timeout(connect)
        .timeout(total)
        .user_agent(concat!("vpush/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// `Retry-After` in its seconds form. The date form is rare with push
/// services and is read as absent.
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_builds_without_a_process_default_provider() {
        client(Duration::from_secs(1), Duration::from_secs(2)).unwrap();
    }
}
