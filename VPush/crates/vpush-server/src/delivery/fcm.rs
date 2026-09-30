//! Firebase Cloud Messaging, HTTP v1.
//!
//! Pushes are data-only, always. A `notification` part would make Android
//! show the push by itself, next to the one the app shows: two notifications
//! for one message.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;
use vpush_proto::Payload;

use super::{http, Attempt, Message, Outcome, ProviderKind, PushProvider, SendFuture, Target};
use crate::config::FcmConfig;

pub const DEFAULT_ENDPOINT: &str = "https://fcm.googleapis.com";
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
/// A token is replaced this long before the service would refuse it.
const TOKEN_MARGIN: Duration = Duration::from_secs(300);

/// The service account file of a Firebase project, as downloaded.
#[derive(Deserialize)]
pub struct ServiceAccount {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub token_uri: String,
    #[serde(default)]
    pub private_key_id: Option<String>,
}

impl ServiceAccount {
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let account: Self = serde_json::from_str(text)
            .map_err(|e| format!("not a service account file: {e}"))?;
        for (name, value) in [
            ("project_id", &account.project_id),
            ("client_email", &account.client_email),
            ("token_uri", &account.token_uri),
        ] {
            if value.trim().is_empty() {
                return Err(format!("`{name}` is empty"));
            }
        }
        account.key()?;
        Ok(account)
    }

    fn key(&self) -> Result<RsaKeyPair, String> {
        let pkcs8 = self.private_key.contains("BEGIN PRIVATE KEY");
        let pkcs1 = self.private_key.contains("BEGIN RSA PRIVATE KEY");
        if !pkcs8 && !pkcs1 {
            return Err("`private_key` is not a PEM private key".to_string());
        }
        let body: String = self
            .private_key
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .map(str::trim)
            .collect();
        let der = STANDARD
            .decode(body)
            .map_err(|e| format!("`private_key`: {e}"))?;
        let key = if pkcs8 {
            RsaKeyPair::from_pkcs8(&der)
        } else {
            RsaKeyPair::from_der(&der)
        };
        key.map_err(|e| format!("`private_key`: {e}"))
    }
}

struct AccessToken {
    value: String,
    good_until: Instant,
}

pub struct FcmClient {
    http: reqwest::Client,
    endpoint: String,
    project_id: String,
    client_email: String,
    token_uri: String,
    key_id: Option<String>,
    key: RsaKeyPair,
    /// Held while a token is fetched, so many pushes fetch one token.
    token: Mutex<Option<AccessToken>>,
}

/// Why there is no access token.
enum TokenError {
    /// Google refuses the account: the key is revoked or the file is wrong.
    Refused { status: u16, detail: String },
    /// Google did not answer, or answered with its own failure.
    Unavailable { status: Option<u16>, detail: String },
}

impl FcmClient {
    pub fn from_config(config: &FcmConfig) -> Result<Self, String> {
        let account = ServiceAccount::read(&config.service_account)?;
        let endpoint = config.endpoint.as_deref().unwrap_or(DEFAULT_ENDPOINT);
        Self::new(account, endpoint)
    }

    pub fn new(account: ServiceAccount, endpoint: &str) -> Result<Self, String> {
        Ok(Self {
            http: http::client(Duration::from_secs(5), Duration::from_secs(15))?,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            key: account.key()?,
            project_id: account.project_id,
            client_email: account.client_email,
            token_uri: account.token_uri,
            key_id: account.private_key_id,
            token: Mutex::new(None),
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// The signed request for an access token.
    fn assertion(&self, now: u64) -> Result<String, String> {
        let mut header = json!({ "alg": "RS256", "typ": "JWT" });
        if let Some(kid) = &self.key_id {
            header["kid"] = json!(kid);
        }
        let claims = json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_uri,
            "iat": now,
            "exp": now + 3600,
        });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let mut signature = vec![0u8; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signed.as_bytes(),
                &mut signature,
            )
            .map_err(|e| format!("cannot sign: {e}"))?;
        Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    async fn fetch_token(&self) -> Result<AccessToken, TokenError> {
        #[derive(Deserialize)]
        struct Answer {
            access_token: String,
            expires_in: u64,
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let assertion = self.assertion(now).map_err(|detail| TokenError::Refused {
            status: 0,
            detail,
        })?;

        let response = self
            .http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|e| TokenError::Unavailable {
                status: None,
                detail: e.to_string(),
            })?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_client_error() && status.as_u16() != 429 {
            return Err(TokenError::Refused {
                status: status.as_u16(),
                detail: body,
            });
        }
        if !status.is_success() {
            return Err(TokenError::Unavailable {
                status: Some(status.as_u16()),
                detail: body,
            });
        }
        let answer: Answer = serde_json::from_str(&body).map_err(|e| TokenError::Unavailable {
            status: Some(status.as_u16()),
            detail: format!("token answer is not readable: {e}"),
        })?;
        tracing::debug!(
            project = %self.project_id,
            expires_in = answer.expires_in,
            "fcm access token received"
        );
        Ok(AccessToken {
            value: answer.access_token,
            good_until: Instant::now()
                + Duration::from_secs(answer.expires_in).saturating_sub(TOKEN_MARGIN),
        })
    }

    async fn access_token(&self) -> Result<String, TokenError> {
        let mut slot = self.token.lock().await;
        if let Some(token) = slot.as_ref() {
            if Instant::now() < token.good_until {
                return Ok(token.value.clone());
            }
        }
        let token = self.fetch_token().await?;
        let value = token.value.clone();
        *slot = Some(token);
        Ok(value)
    }

    /// Forgets the token, if it is still the one that was refused.
    async fn drop_token(&self, refused: &str) {
        let mut slot = self.token.lock().await;
        if slot.as_ref().is_some_and(|t| t.value == refused) {
            *slot = None;
        }
    }

    fn body(target: &Target, message: &Message, payload: &Payload) -> serde_json::Value {
        let mut android = json!({
            "priority": if message.urgent { "HIGH" } else { "NORMAL" },
            "ttl": format!("{}s", message.ttl.as_secs()),
        });
        if let Some(key) = &message.collapse_key {
            android["collapse_key"] = json!(key);
        }
        json!({
            "message": {
                "token": target.token,
                "data": payload.to_data(),
                "android": android,
            }
        })
    }

    async fn attempt(&self, target: &Target, message: &Message, payload: &Payload) -> Attempt {
        let token = match self.access_token().await {
            Ok(token) => token,
            Err(TokenError::Refused { status, detail }) => {
                let attempt = Attempt::new(Outcome::Rejected)
                    .code("PROVIDER_AUTH")
                    .detail(detail);
                return if status == 0 { attempt } else { attempt.status(status) };
            }
            Err(TokenError::Unavailable { status, detail }) => {
                let attempt = Attempt::new(Outcome::Retry)
                    .code("PROVIDER_AUTH_UNAVAILABLE")
                    .detail(detail);
                return match status {
                    Some(s) => attempt.status(s),
                    None => attempt,
                };
            }
        };

        let url = format!(
            "{}/v1/projects/{}/messages:send",
            self.endpoint, self.project_id
        );
        let response = match self
            .http
            .post(url)
            .bearer_auth(&token)
            .json(&Self::body(target, message, payload))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Attempt::new(Outcome::Retry)
                    .code("UNREACHABLE")
                    .detail(e.to_string())
            }
        };

        let status = response.status().as_u16();
        let asked = http::retry_after(response.headers());
        let body = response.text().await.unwrap_or_default();

        if (200..300).contains(&status) {
            return Attempt::new(Outcome::Delivered).status(status);
        }
        if status == 401 {
            // The token ran out early or was revoked; the next attempt takes a new one.
            self.drop_token(&token).await;
        }
        classify(status, &body).retry_after(asked)
    }
}

/// What an error answer of FCM means for us.
pub fn classify(status: u16, body: &str) -> Attempt {
    #[derive(Deserialize, Default)]
    struct Envelope {
        #[serde(default)]
        error: ErrorBody,
    }
    #[derive(Deserialize, Default)]
    struct ErrorBody {
        #[serde(default)]
        status: String,
        #[serde(default)]
        message: String,
        #[serde(default)]
        details: Vec<Detail>,
    }
    #[derive(Deserialize)]
    struct Detail {
        #[serde(rename = "errorCode", default)]
        error_code: Option<String>,
    }

    let error = serde_json::from_str::<Envelope>(body)
        .unwrap_or_default()
        .error;
    // FCM's own code says more than the general status next to it.
    let code = error
        .details
        .iter()
        .find_map(|d| d.error_code.clone())
        .filter(|c| !c.is_empty())
        .or_else(|| (!error.status.is_empty()).then(|| error.status.clone()));
    let detail = if error.message.is_empty() {
        body.to_string()
    } else {
        error.message
    };

    let outcome = match (code.as_deref(), status) {
        // The only answer that means the token is gone for good. A bare 404
        // is not enough: a wrong project id answers 404 too, and would cost
        // every user their registration.
        (Some("UNREGISTERED"), _) => Outcome::DeadToken,
        // A push FCM cannot read, or a token of another project: ours to fix.
        (Some("INVALID_ARGUMENT" | "SENDER_ID_MISMATCH" | "THIRD_PARTY_AUTH_ERROR"), _) => {
            Outcome::Rejected
        }
        (Some("QUOTA_EXCEEDED" | "UNAVAILABLE" | "INTERNAL"), _) => Outcome::Retry,
        (_, 401 | 408 | 429) => Outcome::Retry,
        (_, 500..=599) => Outcome::Retry,
        _ => Outcome::Rejected,
    };

    let attempt = Attempt::new(outcome).status(status).detail(detail);
    match code {
        Some(code) => attempt.code(code),
        None => attempt,
    }
}

impl PushProvider for FcmClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Fcm
    }

    fn send<'a>(&'a self, target: &'a Target, message: &'a Message) -> SendFuture<'a> {
        Box::pin(async move {
            let started = Instant::now();
            let mut attempt = self.attempt(target, message, &message.payload).await;
            // FCM says `INVALID_ARGUMENT` for a data map it finds too big,
            // among other things. A push that carried the event is sent
            // once more without it; anything else is refused for good.
            if attempt.code.as_deref() == Some("INVALID_ARGUMENT") {
                if let Some(fallback) = &message.fallback {
                    tracing::info!(
                        trace = message.payload.trace.as_deref().unwrap_or(""),
                        token = %target.masked(),
                        bytes = message.payload.data_len(),
                        detail = attempt.detail.as_deref(),
                        "fcm refused the push with the event; sent again without it"
                    );
                    attempt = self.attempt(target, message, fallback).await;
                }
            }
            attempt.latency_ms = started.elapsed().as_millis() as u64;
            attempt
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fcm_error(status: &str, code: Option<&str>) -> String {
        let details = match code {
            Some(c) => format!(
                r#","details":[{{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"{c}"}}]"#
            ),
            None => String::new(),
        };
        format!(r#"{{"error":{{"code":0,"message":"why","status":"{status}"{details}}}}}"#)
    }

    #[test]
    fn only_unregistered_kills_a_token() {
        let a = classify(404, &fcm_error("NOT_FOUND", Some("UNREGISTERED")));
        assert_eq!(a.outcome, Outcome::DeadToken);
        assert_eq!(a.code.as_deref(), Some("UNREGISTERED"));
    }

    #[test]
    fn a_bare_404_does_not_kill_a_token() {
        assert_eq!(
            classify(404, &fcm_error("NOT_FOUND", None)).outcome,
            Outcome::Rejected
        );
        assert_eq!(classify(404, "<html>no such project</html>").outcome, Outcome::Rejected);
    }

    #[test]
    fn invalid_argument_does_not_kill_a_token() {
        for body in [
            fcm_error("INVALID_ARGUMENT", Some("INVALID_ARGUMENT")),
            fcm_error("INVALID_ARGUMENT", None),
        ] {
            assert_eq!(classify(400, &body).outcome, Outcome::Rejected, "{body}");
        }
    }

    #[test]
    fn a_token_of_another_project_is_kept() {
        let a = classify(403, &fcm_error("PERMISSION_DENIED", Some("SENDER_ID_MISMATCH")));
        assert_eq!(a.outcome, Outcome::Rejected);
    }

    #[test]
    fn a_busy_service_is_tried_again() {
        assert_eq!(
            classify(429, &fcm_error("RESOURCE_EXHAUSTED", Some("QUOTA_EXCEEDED"))).outcome,
            Outcome::Retry
        );
        assert_eq!(
            classify(503, &fcm_error("UNAVAILABLE", Some("UNAVAILABLE"))).outcome,
            Outcome::Retry
        );
        assert_eq!(classify(500, "").outcome, Outcome::Retry);
        assert_eq!(classify(502, "<html>bad gateway</html>").outcome, Outcome::Retry);
        assert_eq!(
            classify(401, &fcm_error("UNAUTHENTICATED", None)).outcome,
            Outcome::Retry
        );
    }

    #[test]
    fn the_push_is_data_only() {
        let mut payload = Payload::new(vpush_proto::PushType::Dm);
        payload.event = Some(r#"{"kind":1059}"#.into());
        payload.trace = Some("abcd1234".into());
        let message = Message {
            payload: payload.clone(),
            fallback: None,
            collapse_key: Some("k".into()),
            ttl: Duration::from_secs(60),
            urgent: true,
        };
        let body = FcmClient::body(&Target { token: "t".into() }, &message, &payload);
        let message = &body["message"];
        assert!(message.get("notification").is_none());
        assert!(message["android"].get("notification").is_none());
        assert_eq!(message["android"]["priority"], "HIGH");
        assert_eq!(message["android"]["ttl"], "60s");
        assert_eq!(message["android"]["collapse_key"], "k");
        assert_eq!(message["data"]["event"], r#"{"kind":1059}"#);
        assert_eq!(message["data"]["v"], "2");
        assert!(message["data"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v.is_string()));
    }

    #[test]
    fn a_file_that_is_not_a_service_account_is_refused_with_a_reason() {
        let e = ServiceAccount::parse(r#"{"type":"service_account"}"#).err().unwrap();
        assert!(e.contains("not a service account file"), "{e}");

        let e = ServiceAccount::parse(
            r#"{"project_id":"p","client_email":"e","token_uri":"u","private_key":"nope"}"#,
        )
        .err()
        .unwrap();
        assert!(e.contains("not a PEM private key"), "{e}");
    }
}
