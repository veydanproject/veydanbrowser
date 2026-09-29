//! Who is asking: NIP-98.
//!
//! A request carries an event of kind 27235 signed by the user's key. The
//! event names the address, the method and the body it was signed for, so
//! it opens one request and no other, and only for a minute.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use nostr::event::{Event, Kind};

/// How far the client's clock may be from the server's, either way.
pub const WINDOW: Duration = Duration::from_secs(60);
/// Longer than any header of this kind has a reason to be.
const MAX_HEADER: usize = 16 * 1024;
const SCHEME: &str = "Nostr ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Missing,
    Invalid(String),
    Expired,
    Replay,
    UrlMismatch,
}

/// Checks requests, and remembers the events it has seen for as long as they
/// could be used again.
pub struct Nip98 {
    /// The server's address as clients see it, without a trailing slash.
    public_url: String,
    seen: Mutex<HashMap<[u8; 32], Instant>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

impl Nip98 {
    pub fn new(public_url: &str) -> Self {
        Self {
            public_url: public_url.trim_end_matches('/').to_string(),
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// The address a request for `path_and_query` must have been signed for.
    pub fn url_of(&self, path_and_query: &str) -> String {
        format!("{}{}", self.public_url, path_and_query)
    }

    /// The key that signed the request, as 64 hex characters.
    ///
    /// `now` is unix seconds by the server's clock.
    pub fn check(
        &self,
        header: Option<&str>,
        method: &str,
        path_and_query: &str,
        body: &[u8],
        now: u64,
    ) -> Result<String, AuthError> {
        let header = header.ok_or(AuthError::Missing)?;
        if header.len() > MAX_HEADER {
            return Err(AuthError::Invalid("the header is too long".into()));
        }
        let encoded = header
            .strip_prefix(SCHEME)
            .ok_or_else(|| AuthError::Invalid("the scheme is not Nostr".into()))?;
        let json = STANDARD
            .decode(encoded.trim())
            .map_err(|_| AuthError::Invalid("not base64".into()))?;
        let event: Event = serde_json::from_slice(&json)
            .map_err(|_| AuthError::Invalid("not an event".into()))?;

        if event.kind != Kind::HttpAuth {
            return Err(AuthError::Invalid("the event is not of kind 27235".into()));
        }
        event
            .verify()
            .map_err(|_| AuthError::Invalid("the signature does not match".into()))?;

        let at = event.created_at.as_secs();
        if at.abs_diff(now) > WINDOW.as_secs() {
            return Err(AuthError::Expired);
        }

        let tag = |name: &str| -> Option<&str> {
            event.tags.iter().find_map(|t| match t.as_slice() {
                [kind, value, ..] if kind == name => Some(value.as_str()),
                _ => None,
            })
        };

        match tag("u") {
            Some(u) if u == self.url_of(path_and_query) => {}
            Some(_) => return Err(AuthError::UrlMismatch),
            None => return Err(AuthError::Invalid("no `u` tag".into())),
        }
        match tag("method") {
            Some(m) if m.eq_ignore_ascii_case(method) => {}
            Some(_) => return Err(AuthError::Invalid("signed for another method".into())),
            None => return Err(AuthError::Invalid("no `method` tag".into())),
        }
        if !body.is_empty() {
            match tag("payload") {
                Some(p) if p.eq_ignore_ascii_case(&sha256_hex(body)) => {}
                Some(_) => return Err(AuthError::Invalid("signed for another body".into())),
                None => return Err(AuthError::Invalid("no `payload` tag".into())),
            }
        }

        // Last, so that a request refused for another reason does not use
        // the event up.
        self.use_once(event.id.to_bytes())?;
        Ok(event.pubkey.to_hex())
    }

    fn use_once(&self, id: [u8; 32]) -> Result<(), AuthError> {
        // An event is refused by its age after WINDOW either side of now, so
        // twice the window is as long as it needs to be remembered.
        let keep = WINDOW * 2;
        let now = Instant::now();
        let mut seen = self.seen.lock().unwrap();
        if seen.len() >= 1024 {
            seen.retain(|_, at| now.duration_since(*at) < keep);
        }
        match seen.get(&id) {
            Some(at) if now.duration_since(*at) < keep => Err(AuthError::Replay),
            _ => {
                seen.insert(id, now);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::prelude::*;

    const BASE: &str = "https://push.example.org";
    const PATH: &str = "/v1/devices/phone-0001";
    const NOW: u64 = 1_790_000_000;

    struct Signed<'a> {
        url: String,
        method: &'a str,
        body: &'a [u8],
        at: u64,
        kind: Kind,
        payload: Option<String>,
    }

    impl<'a> Signed<'a> {
        fn new(method: &'a str, body: &'a [u8]) -> Self {
            Self {
                url: format!("{BASE}{PATH}"),
                method,
                body,
                at: NOW,
                kind: Kind::HttpAuth,
                payload: None,
            }
        }

        fn header(&self, keys: &Keys) -> String {
            let mut tags = vec![
                Tag::parse(["u", self.url.as_str()]).unwrap(),
                Tag::parse(["method", self.method]).unwrap(),
            ];
            let payload = self
                .payload
                .clone()
                .or_else(|| (!self.body.is_empty()).then(|| sha256_hex(self.body)));
            if let Some(p) = payload {
                tags.push(Tag::parse(["payload", p.as_str()]).unwrap());
            }
            let event = EventBuilder::new(self.kind, "")
                .tags(tags)
                .custom_created_at(Timestamp::from_secs(self.at))
                .finalize(keys)
                .unwrap();
            format!("Nostr {}", STANDARD.encode(serde_json::to_vec(&event).unwrap()))
        }
    }

    fn check(auth: &Nip98, header: &str, method: &str, body: &[u8]) -> Result<String, AuthError> {
        auth.check(Some(header), method, PATH, body, NOW)
    }

    #[test]
    fn a_signed_request_names_its_signer() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        let body = br#"{"app_id":"x"}"#;
        let header = Signed::new("PUT", body).header(&keys);
        assert_eq!(check(&auth, &header, "PUT", body).unwrap(), keys.public_key().to_hex());
    }

    #[test]
    fn delete_and_get_carry_no_body() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        for method in ["DELETE", "GET"] {
            let header = Signed::new(method, b"").header(&keys);
            check(&auth, &header, method, b"").unwrap();
        }
    }

    #[test]
    fn no_header_and_a_wrong_header() {
        let auth = Nip98::new(BASE);
        assert_eq!(auth.check(None, "GET", PATH, b"", NOW), Err(AuthError::Missing));
        for bad in ["Bearer abc", "Nostr !!!", "Nostr e30=", "Nostr"] {
            assert!(
                matches!(check(&auth, bad, "GET", b""), Err(AuthError::Invalid(_))),
                "`{bad}` was accepted"
            );
        }
    }

    #[test]
    fn signed_for_another_address() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        for url in [
            "https://push.example.org/v1/devices/phone-0002",
            "https://other.example.org/v1/devices/phone-0001",
            "http://push.example.org/v1/devices/phone-0001",
            "https://push.example.org/v1/devices/phone-0001/",
            "https://push.example.org/v1/devices/phone-0001?x=1",
        ] {
            let mut signed = Signed::new("GET", b"");
            signed.url = url.to_string();
            assert_eq!(
                check(&auth, &signed.header(&keys), "GET", b""),
                Err(AuthError::UrlMismatch),
                "{url}"
            );
        }
    }

    #[test]
    fn the_query_is_a_part_of_the_address() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        let mut signed = Signed::new("GET", b"");
        signed.url = format!("{BASE}/v1/status?full=1");
        let header = signed.header(&keys);
        auth.check(Some(&header), "GET", "/v1/status?full=1", b"", NOW).unwrap();
    }

    #[test]
    fn a_trailing_slash_in_the_config_changes_nothing() {
        let keys = Keys::generate();
        let auth = Nip98::new("https://push.example.org/");
        let header = Signed::new("GET", b"").header(&keys);
        check(&auth, &header, "GET", b"").unwrap();
    }

    #[test]
    fn signed_for_another_method_or_body() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);

        let header = Signed::new("GET", b"").header(&keys);
        assert!(matches!(check(&auth, &header, "DELETE", b""), Err(AuthError::Invalid(_))));

        let header = Signed::new("PUT", b"one").header(&keys);
        assert!(matches!(check(&auth, &header, "PUT", b"two"), Err(AuthError::Invalid(_))));

        // A body with no word about it in the event.
        let header = Signed::new("PUT", b"").header(&keys);
        assert!(matches!(check(&auth, &header, "PUT", b"body"), Err(AuthError::Invalid(_))));
    }

    #[test]
    fn too_old_and_from_the_future() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        for (at, ok) in [
            (NOW - 60, true),
            (NOW + 60, true),
            (NOW - 61, false),
            (NOW + 61, false),
            (0, false),
        ] {
            let mut signed = Signed::new("GET", b"");
            signed.at = at;
            let got = check(&auth, &signed.header(&keys), "GET", b"");
            assert_eq!(got.is_ok(), ok, "at {at}: {got:?}");
            if !ok {
                assert_eq!(got, Err(AuthError::Expired));
            }
        }
    }

    #[test]
    fn the_same_event_opens_one_request() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        let header = Signed::new("GET", b"").header(&keys);
        check(&auth, &header, "GET", b"").unwrap();
        assert_eq!(check(&auth, &header, "GET", b""), Err(AuthError::Replay));
    }

    #[test]
    fn a_refused_request_does_not_use_the_event_up() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        let header = Signed::new("GET", b"").header(&keys);
        assert!(check(&auth, &header, "DELETE", b"").is_err());
        check(&auth, &header, "GET", b"").unwrap();
    }

    #[test]
    fn an_event_of_another_kind() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        let mut signed = Signed::new("GET", b"");
        signed.kind = Kind::TextNote;
        assert!(matches!(
            check(&auth, &signed.header(&keys), "GET", b""),
            Err(AuthError::Invalid(_))
        ));
    }

    #[test]
    fn a_changed_event_is_refused() {
        let keys = Keys::generate();
        let auth = Nip98::new(BASE);
        let header = Signed::new("GET", b"").header(&keys);
        let json = STANDARD.decode(header.strip_prefix("Nostr ").unwrap()).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        // Somebody else's key on a signed event.
        value["pubkey"] = serde_json::json!(Keys::generate().public_key().to_hex());
        let forged = format!("Nostr {}", STANDARD.encode(value.to_string()));
        assert!(matches!(check(&auth, &forged, "GET", b""), Err(AuthError::Invalid(_))));
    }
}
