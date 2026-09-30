//! Delivery: handing a push to the service that carries it to the device.
//!
//! A provider reports what happened in terms the rest of the server acts on,
//! not in HTTP codes. The most important distinction is between a token that
//! is dead (the device is forgotten) and everything else (it is not): a
//! mistake of ours must never cost a user their registration.

pub mod fcm;
pub mod http;
pub mod retry;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use vpush_proto::{Payload, PushType};

use crate::config::Config;
use retry::{Delivery, RetryPolicy};

/// Which service carries the push.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Fcm,
    Apns,
    Unifiedpush,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fcm => "fcm",
            Self::Apns => "apns",
            Self::Unifiedpush => "unifiedpush",
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "fcm" => Ok(Self::Fcm),
            "apns" => Ok(Self::Apns),
            "unifiedpush" => Ok(Self::Unifiedpush),
            other => Err(format!(
                "`{other}` is not a provider (fcm, apns, unifiedpush)"
            )),
        }
    }
}

/// Where a push goes: what the device gave at registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub token: String,
}

impl Target {
    /// The token as it may appear in a log: enough to tell two apart, not
    /// enough to push with.
    pub fn masked(&self) -> String {
        mask(&self.token)
    }
}

pub fn mask(secret: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, secret.as_bytes());
    let hex: String = digest.as_ref()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("#{hex}")
}

/// How long a test push waits for the phone.
///
/// A phone whose connection to the push service went stale behind a NAT
/// learns about it only at its next heartbeat, many minutes later. A test
/// push that lived one minute was dropped in that time, and nothing told
/// anyone: the service had accepted it.
pub const TEST_TTL: Duration = Duration::from_secs(600);

/// How long a test push may take. Somebody is waiting for its outcome:
/// behind the time limit of a request, which is 15 seconds unless the
/// config says otherwise, or at a terminal.
pub const TEST_DEADLINE: Duration = Duration::from_secs(10);

/// One push, ready to be sent.
#[derive(Debug, Clone)]
pub struct Message {
    pub payload: Payload,
    /// The same push in its other form, `event_id` and `relay` in place of
    /// the event, for a service that refuses the payload as too big. Only a
    /// push that carries an event has one.
    pub fallback: Option<Payload>,
    /// A later push with the same key replaces this one while it waits.
    pub collapse_key: Option<String>,
    /// How long the service keeps trying when the device is offline.
    pub ttl: Duration,
    /// Wake the device now, or deliver when convenient.
    pub urgent: bool,
}

/// What happened to one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The service took it.
    Delivered,
    /// The service says this token will never work again.
    DeadToken,
    /// The service refused, and trying again will not help. The cause is on
    /// our side or in the configuration; the device stays registered.
    Rejected,
    /// The service is busy or down. Worth another attempt.
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    /// The service's own word for what happened: `UNREGISTERED`, `QUOTA_EXCEEDED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// With `Retry`: not sooner than this, when the service said so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    pub latency_ms: u64,
}

impl Attempt {
    pub fn new(outcome: Outcome) -> Self {
        Self {
            outcome,
            http_status: None,
            code: None,
            detail: None,
            retry_after_ms: None,
            latency_ms: 0,
        }
    }

    pub fn status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        // Answers of a service may be long; a log line and a journal row are not.
        let mut detail: String = detail.into();
        if detail.len() > 300 {
            let cut = (0..=300).rev().find(|i| detail.is_char_boundary(*i)).unwrap_or(0);
            detail.truncate(cut);
            detail.push('…');
        }
        self.detail = Some(detail);
        self
    }

    pub fn retry_after(mut self, after: Option<Duration>) -> Self {
        self.retry_after_ms = after.map(|d| d.as_millis() as u64);
        self
    }
}

pub type SendFuture<'a> = Pin<Box<dyn Future<Output = Attempt> + Send + 'a>>;

/// What a push service says of a token before anything is sent to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenCheck {
    /// The service knows the token, and would push to it.
    Valid,
    /// The token is none the service can push to: made up, gone, or of
    /// another project.
    Invalid,
    /// The service could not say: it is busy, out of reach, or refuses us.
    /// Nothing is known of the token.
    Unknown,
}

impl TokenCheck {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Unknown => "unknown",
        }
    }
}

pub type CheckFuture<'a> = Pin<Box<dyn Future<Output = TokenCheck> + Send + 'a>>;

/// How long a registration waits for the push service to say whether the
/// token is one. A service that is silent for longer is taken for one that
/// cannot say: whoever registers is waiting, and the first push will tell.
pub const CHECK_DEADLINE: Duration = Duration::from_secs(5);

/// A service that carries pushes.
pub trait PushProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    /// One attempt. Never fails: whatever happens is an [`Attempt`].
    fn send<'a>(&'a self, target: &'a Target, message: &'a Message) -> SendFuture<'a>;

    /// Asks the service whether it can push to the target. Nothing reaches
    /// the device.
    fn check<'a>(&'a self, target: &'a Target) -> CheckFuture<'a>;
}

/// [`PushProvider::check`], given up on at the deadline.
pub async fn check_token(provider: &dyn PushProvider, target: &Target) -> TokenCheck {
    tokio::time::timeout(CHECK_DEADLINE, provider.check(target))
        .await
        .unwrap_or(TokenCheck::Unknown)
}

/// One push of the type `test` to one address, and what became of it.
///
/// The device shows a test push in its own words; the push says only what
/// it is and how to find it in the log. It is tried once: whoever asked is
/// told that the service is busy, and tries again when they like. A service
/// that does not answer by the deadline is not waited for.
pub async fn test_push(provider: &dyn PushProvider, target: &Target, trace: &str) -> Delivery {
    let mut payload = Payload::new(PushType::Test);
    payload.trace = Some(trace.to_string());
    let message = Message {
        payload,
        fallback: None,
        collapse_key: None,
        ttl: TEST_TTL,
        urgent: true,
    };
    let once = RetryPolicy {
        max_attempts: 1,
        ..RetryPolicy::default()
    };
    let delivery = retry::deliver(provider, target, &message, once);
    match tokio::time::timeout(TEST_DEADLINE, delivery).await {
        Ok(delivery) => delivery,
        Err(_) => {
            let mut attempt = Attempt::new(Outcome::Retry)
                .code("DEADLINE")
                .detail("the push service did not answer in time");
            attempt.latency_ms = TEST_DEADLINE.as_millis() as u64;
            Delivery {
                outcome: Outcome::Retry,
                attempts: vec![attempt],
            }
        }
    }
}

/// The providers of every app in the config.
#[derive(Default)]
pub struct Providers {
    by_app: BTreeMap<String, BTreeMap<ProviderKind, Arc<dyn PushProvider>>>,
}

impl Providers {
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        let mut out = Self::default();
        for (app_id, app) in &config.apps {
            if let Some(fcm) = &app.fcm {
                let client = fcm::FcmClient::from_config(fcm)
                    .map_err(|e| anyhow::anyhow!("apps.{app_id}.fcm: {e}"))?;
                out.insert(app_id, Arc::new(client));
            }
        }
        Ok(out)
    }

    pub fn insert(&mut self, app_id: &str, provider: Arc<dyn PushProvider>) {
        self.by_app
            .entry(app_id.to_string())
            .or_default()
            .insert(provider.kind(), provider);
    }

    pub fn get(&self, app_id: &str, kind: ProviderKind) -> Result<Arc<dyn PushProvider>, String> {
        let app = self
            .by_app
            .get(app_id)
            .ok_or_else(|| match self.by_app.keys().next() {
                None => format!("app `{app_id}` is unknown: no apps are configured"),
                Some(_) => format!(
                    "app `{app_id}` is unknown; configured: {}",
                    self.by_app.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            })?;
        app.get(&kind).cloned().ok_or_else(|| {
            format!(
                "app `{app_id}` has no {} configured",
                kind.as_str()
            )
        })
    }

    /// `app: fcm, unifiedpush` per app, for the log at start.
    pub fn summary(&self) -> Vec<(String, String)> {
        self.by_app
            .iter()
            .map(|(app, kinds)| {
                let kinds: Vec<_> = kinds.keys().map(|k| k.as_str()).collect();
                (app.clone(), kinds.join(", "))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_token_tells_tokens_apart_and_hides_them() {
        let a = mask("token-of-device-a");
        let b = mask("token-of-device-b");
        assert_ne!(a, b);
        assert_eq!(a, mask("token-of-device-a"));
        assert_eq!(a.len(), 9);
        assert!(!a.contains("token"));
    }

    /// A push service that takes a push and never says what became of it.
    struct Silent;

    impl PushProvider for Silent {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Fcm
        }

        fn send<'a>(&'a self, _: &'a Target, _: &'a Message) -> SendFuture<'a> {
            Box::pin(std::future::pending())
        }

        fn check<'a>(&'a self, _: &'a Target) -> CheckFuture<'a> {
            Box::pin(std::future::pending())
        }
    }

    /// A push service that is busy, and counts how often it was asked.
    #[derive(Default)]
    struct Busy {
        asked: std::sync::atomic::AtomicU32,
    }

    impl PushProvider for Busy {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Fcm
        }

        fn send<'a>(&'a self, _: &'a Target, _: &'a Message) -> SendFuture<'a> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(std::future::ready(Attempt::new(Outcome::Retry).status(503)))
        }

        fn check<'a>(&'a self, _: &'a Target) -> CheckFuture<'a> {
            Box::pin(std::future::ready(TokenCheck::Unknown))
        }
    }

    fn phone() -> Target {
        Target {
            token: "token-of-the-phone".into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_test_push_nobody_answers_is_given_up_on_and_says_so() {
        let started = tokio::time::Instant::now();
        let delivery = test_push(&Silent, &phone(), "abcd1234").await;
        assert_eq!(started.elapsed(), TEST_DEADLINE);
        assert_eq!(delivery.outcome, Outcome::Retry);
        assert_eq!(delivery.attempts.len(), 1);
        assert_eq!(delivery.attempts[0].code.as_deref(), Some("DEADLINE"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_service_that_does_not_answer_a_check_is_one_that_cannot_say() {
        let started = tokio::time::Instant::now();
        assert_eq!(check_token(&Silent, &phone()).await, TokenCheck::Unknown);
        assert_eq!(started.elapsed(), CHECK_DEADLINE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_test_push_is_tried_once() {
        let busy = Busy::default();
        let started = tokio::time::Instant::now();
        let delivery = test_push(&busy, &phone(), "abcd1234").await;
        assert_eq!(delivery.outcome, Outcome::Retry);
        assert_eq!(delivery.attempts.len(), 1);
        assert_eq!(busy.asked.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(started.elapsed(), Duration::ZERO, "and nothing is waited for");
    }

    #[test]
    fn long_detail_is_cut_on_a_character() {
        let a = Attempt::new(Outcome::Rejected).detail("ж".repeat(400));
        let d = a.detail.unwrap();
        assert!(d.len() <= 304, "{}", d.len());
        assert!(d.ends_with('…'));
    }
}
