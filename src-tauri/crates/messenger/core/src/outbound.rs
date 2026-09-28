// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Everything a handler wants the transport to do. The transport never
//! inspects event content; it only routes by the variant and the scope.

use crate::types::{EventId, PubKey, RelayUrl, SubId};
use serde::{Deserialize, Serialize};

/// Routing scope for scoped publish/subscribe (groups, channels, servers).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum Scope {
    /// The user's own enabled relays.
    Own,
    Group { id: String },
    Channel { id: String },
    Server { id: String },
}

/// A signed event ready for the wire. Stage 0 keeps it as JSON; stage 2 swaps
/// the payload for the transport crate's typed event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireEvent {
    pub id: EventId,
    pub json: serde_json::Value,
}

/// Relay-side filter. Mirrors NIP-01 REQ semantics; kept as JSON so the
/// contract does not depend on a protocol crate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Filter(pub serde_json::Value);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Outbound {
    /// Deliver to a peer's inbox relays (resolved by transport), with hints
    /// the handler already knows about.
    PublishToInbox { recipient: PubKey, event: WireEvent, hint_relays: Vec<RelayUrl> },
    /// Publish to the user's own relays (self-copies, profile, lists).
    PublishOwn { event: WireEvent },
    PublishScoped { scope: Scope, event: WireEvent },
    Subscribe { id: SubId, filter: Filter, scope: Scope },
    Unsubscribe { id: SubId },
    /// Negentropy reconciliation against `local_ids`.
    Sync { scope: Scope, filter: Filter, local_ids: Vec<EventId> },
}
