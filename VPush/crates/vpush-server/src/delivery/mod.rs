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
use vpush_proto::Payload;

use crate::config::Config;

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

/// A service that carries pushes.
pub trait PushProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    /// One attempt. Never fails: whatever happens is an [`Attempt`].
    fn send<'a>(&'a self, target: &'a Target, message: &'a Message) -> SendFuture<'a>;
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

    #[test]
    fn long_detail_is_cut_on_a_character() {
        let a = Attempt::new(Outcome::Rejected).detail("ж".repeat(400));
        let d = a.detail.unwrap();
        assert!(d.len() <= 304, "{}", d.len());
        assert!(d.ends_with('…'));
    }
}
