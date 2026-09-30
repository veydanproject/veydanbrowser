//! Which relays the server agrees to watch.
//!
//! Today that is a list in the config. A relay that is not on it is refused,
//! and the client is told so by name.

use std::collections::BTreeMap;
use std::path::Path;

use vpush_proto::RelayStatus;

use crate::config::Config;

/// One form for every spelling of a relay's address, so that
/// `WSS://Relay.Example.org/` and `wss://relay.example.org` are one relay.
///
/// `wss` only; `ws` is accepted for this machine, for tests.
pub fn normalize(url: &str) -> Result<String, String> {
    let url = url.trim();
    // The address itself is never put into the reason: it may carry a key.
    let parsed = url::Url::parse(url).map_err(|e| format!("not an address: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "no host".to_string())?
        .to_ascii_lowercase();
    let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    match parsed.scheme() {
        "wss" => {}
        "ws" if local => {}
        "ws" => return Err("ws is for this machine only, use wss".to_string()),
        other => return Err(format!("`{other}` is not wss")),
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("a name or a password in the address".to_string());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("a query or a fragment in the address".to_string());
    }
    let mut out = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        out.push_str(&format!(":{port}"));
    }
    let path = parsed.path().trim_end_matches('/');
    out.push_str(path);
    Ok(out)
}

/// An address that was refused, as it may be shown and logged: without the
/// query, where a key may sit, and not longer than an address needs to be.
pub fn shown(url: &str) -> String {
    let url = url.trim();
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    url[..cut].chars().take(200).collect()
}

/// A key of a relay's gate is at least this long.
const SHORTEST_KEY: usize = 16;

/// The key of a relay's gate, from its file.
///
/// The key is put into the address of the relay as it is, and looked for
/// in every line of the log as it is. So it is made of what an address
/// takes without rewriting, and is long enough not to be found in words
/// that are no secret. What is wrong is said of the file: the key itself
/// is never put into a reason.
fn read_key(path: &Path) -> Result<String, String> {
    let key = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let key = key.trim();
    let plain = key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'));
    if key.len() < SHORTEST_KEY || !plain {
        return Err(format!(
            "{}: a key is {SHORTEST_KEY} or more letters, digits, `.`, `_`, `~` or `-`",
            path.display()
        ));
    }
    Ok(key.to_string())
}

/// A relay the server watches.
#[derive(Debug, Clone)]
pub struct AllowedRelay {
    pub url: String,
    /// What the relay's gate asks for. Never logged, never sent to a client.
    pub api_key: Option<String>,
}

/// The relays of the config, by their normalized address.
#[derive(Debug, Clone, Default)]
pub struct RelayPolicy {
    allowed: BTreeMap<String, AllowedRelay>,
}

impl RelayPolicy {
    pub fn from_config(config: &Config) -> Result<Self, String> {
        let mut allowed = BTreeMap::new();
        for relay in &config.relays.allow {
            let url = normalize(&relay.url)?;
            let api_key = match &relay.api_key_file {
                None => None,
                Some(path) => Some(read_key(path)?),
            };
            allowed.insert(url.clone(), AllowedRelay { url, api_key });
        }
        Ok(Self { allowed })
    }

    /// The keys of the gated relays: what must never be seen in a log.
    pub fn secrets(&self) -> Vec<String> {
        self.allowed.values().filter_map(|r| r.api_key.clone()).collect()
    }

    pub fn urls(&self) -> Vec<String> {
        self.allowed.keys().cloned().collect()
    }

    pub fn get(&self, normalized: &str) -> Option<&AllowedRelay> {
        self.allowed.get(normalized)
    }

    pub fn all(&self) -> impl Iterator<Item = &AllowedRelay> {
        self.allowed.values()
    }

    /// What to answer a client that named this relay: the address as the
    /// server writes it, and whether it will be watched.
    pub fn judge(&self, url: &str) -> (String, RelayStatus, Option<String>) {
        match normalize(url) {
            // Given back cut short, without what follows a `?`.
            Err(e) => (shown(url), RelayStatus::Invalid, Some(e)),
            Ok(normalized) => {
                if self.allowed.contains_key(&normalized) {
                    (normalized, RelayStatus::Pending, None)
                } else {
                    (
                        normalized,
                        RelayStatus::NotAllowed,
                        Some("this server does not watch this relay".to_string()),
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_one_relay_are_one_address() {
        for spelling in [
            "wss://relay.example.org",
            "wss://relay.example.org/",
            "WSS://Relay.Example.ORG",
            "  wss://relay.example.org/  ",
            "wss://relay.example.org:443",
        ] {
            assert_eq!(normalize(spelling).unwrap(), "wss://relay.example.org", "{spelling}");
        }
        assert_eq!(normalize("wss://r.example.org:8443/nostr/").unwrap(), "wss://r.example.org:8443/nostr");
    }

    #[test]
    fn what_is_not_a_relay_is_refused() {
        for bad in [
            "",
            "relay.example.org",
            "https://relay.example.org",
            "ws://relay.example.org",
            "wss://user:pass@relay.example.org",
            "wss://relay.example.org/?key=secret",
            "wss://relay.example.org/#x",
            "wss://",
        ] {
            assert!(normalize(bad).is_err(), "`{bad}` was accepted");
        }
    }

    #[test]
    fn ws_is_fine_on_this_machine() {
        assert_eq!(normalize("ws://127.0.0.1:7777").unwrap(), "ws://127.0.0.1:7777");
    }

    #[test]
    fn a_key_an_address_or_the_log_would_not_take_is_refused_by_its_file() {
        let dir = std::env::temp_dir().join(format!("vpush-relay-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.key");
        let read = |key: &str| {
            std::fs::write(&path, key).unwrap();
            read_key(&path)
        };

        assert_eq!(read("d0f9c18a09ab2547f5c9aaddc5dfec3b\n").unwrap(), "d0f9c18a09ab2547f5c9aaddc5dfec3b");
        assert_eq!(read("  Gate-key_0.1~2-abc  ").unwrap(), "Gate-key_0.1~2-abc");
        for bad in [
            "",
            "short-key-15-ch",
            "sixteen&more=but?splits#the/address",
            "sixteen and more with spaces",
            "sixteen-and-more\nin-two-lines-of-it",
            "шестнадцать-и-больше",
        ] {
            let reason = read(bad).unwrap_err();
            assert!(reason.contains("relay.key"), "{reason}");
            assert!(bad.is_empty() || !reason.contains(bad), "the key is in the reason: {reason}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_in_the_address_reaches_neither_the_reason_nor_the_answer() {
        let url = "wss://node-1.veydan.net/?key=topsecret";
        let reason = normalize(url).unwrap_err();
        assert!(reason.contains("query"), "{reason}");
        assert!(!reason.contains("topsecret"), "{reason}");

        let (given_back, status, detail) = RelayPolicy::default().judge(url);
        assert_eq!(status, RelayStatus::Invalid);
        assert_eq!(given_back, "wss://node-1.veydan.net/");
        assert!(!detail.unwrap().contains("topsecret"));
    }
}
