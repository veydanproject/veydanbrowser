// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! HTTPS client factory for the messenger.
//!
//! The TLS setup is explicit instead of inherited from the process:
//!
//! - **Roots** are the bundled Mozilla set. The platform verifier needs a
//!   JNI context on Android that a library cannot count on, and a bundled
//!   set behaves the same on every system.
//! - **Crypto provider** is `ring`, named explicitly, so a host that
//!   installed another default (or none) changes nothing here.

use messenger_core::{MessengerError, Result};
use std::sync::Arc;
use std::time::Duration;

fn tls_config() -> Result<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| MessengerError::Crypto(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(config)
}

/// A builder with TLS and timeouts set; callers add what is specific to
/// them (redirect policy, user agent).
pub fn builder(connect: Duration, total: Duration) -> Result<reqwest::ClientBuilder> {
    Ok(reqwest::Client::builder()
        .tls_backend_preconfigured(tls_config()?)
        .connect_timeout(connect)
        .timeout(total))
}

/// A client with the given connect and total timeouts.
pub fn client(connect: Duration, total: Duration) -> Result<reqwest::Client> {
    builder(connect, total)?.build().map_err(|e| MessengerError::Transport(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_builds_without_a_process_default_provider() {
        let c = client(Duration::from_secs(1), Duration::from_secs(2));
        assert!(c.is_ok(), "{:?}", c.err());

    }
}
