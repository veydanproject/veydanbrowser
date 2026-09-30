// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! The traits that decouple the crates from each other and from the host.

use crate::error::Result;
use crate::outbound::Outbound;
use crate::types::{PubKey, RawEvent, RelayUrl, Timestamp};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use zeroize::Zeroizing;

/// What a relay answered to a publish.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ack {
    pub accepted_by: Vec<RelayUrl>,
    pub rejected_by: Vec<(RelayUrl, String)>,
}

impl Ack {
    pub fn is_delivered(&self) -> bool {
        !self.accepted_by.is_empty()
    }
}

/// Snapshot of relay connectivity for the UI.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RelayStatusSnapshot {
    pub relays: Vec<RelayStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayStatus {
    pub url: RelayUrl,
    pub state: RelayState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayState {
    Disconnected,
    Connecting,
    Connected,
    /// Circuit breaker open until the embedded time.
    Paused,
}

/// Network side. Implemented by `messenger-transport`; faked by `testkit`.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, out: Outbound) -> Result<Ack>;
    /// Stream of raw events. One receiver per runtime; cloning is the
    /// runtime's business.
    fn events(&self) -> tokio::sync::mpsc::Receiver<RawEvent>;
    async fn status(&self) -> RelayStatusSnapshot;
}

/// Host-provided secret storage. The host decides how secrets are protected
/// (Veydan Space wraps them with the vault; a standalone build may use the OS
/// keychain or a file). Values are zeroized on drop.
#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    async fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    /// `false` means `get`/`put` will fail with `MessengerError::SecretsLocked`.
    async fn is_unlocked(&self) -> bool;
}

/// Time source; injectable so handler tests are deterministic.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// Wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Timestamp(secs)
    }
}

/// Per-session facts every handler needs.
#[derive(Clone)]
pub struct Context {
    pub my_pubkey: PubKey,
    /// Events with `created_at` before this are "historical": stored, never
    /// notified, never answered.
    pub session_started_at: Timestamp,
    pub clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("my_pubkey", &self.my_pubkey)
            .field("session_started_at", &self.session_started_at)
            .finish_non_exhaustive()
    }
}

/// UI-facing event emitted by handlers. The host maps it to its own event
/// bus (`messenger://…` in Tauri).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

/// Side effects a handler asks the runtime to perform after it returns.
#[derive(Clone, Debug)]
pub enum Effect {
    Send(Outbound),
    Emit(UiEvent),
    /// A user-visible notification request; the host decides how to show it.
    Notify(Notice),
}

/// Something to tell the user about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// The chat as the list names it (a person, a group).
    pub title: String,
    /// One line of the message, or a code the host words (`group_invite`, …).
    pub body: Option<String>,
    pub chat_id: Option<String>,
    /// Who wrote, hex; in a group the author, not the group.
    pub sender: Option<String>,
    /// A stranger's first message, waiting for an answer.
    pub request: bool,
}

/// One handler per `Inbound` family. Handlers are pure with respect to the
/// network: they read/write their store and return effects.
#[async_trait]
pub trait Handler<I>: Send + Sync {
    async fn handle(&self, msg: I, ctx: &Context) -> Result<Vec<Effect>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_after_2025() {
        assert!(SystemClock.now().secs() > 1_735_689_600);
    }

    #[test]
    fn ack_delivered_when_any_relay_accepted() {
        let ok = Ack {
            accepted_by: vec![RelayUrl::parse("wss://a.example").unwrap()],
            rejected_by: vec![],
        };
        assert!(ok.is_delivered());
        let none = Ack { accepted_by: vec![], rejected_by: vec![] };
        assert!(!none.is_delivered());
    }
}
