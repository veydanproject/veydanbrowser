// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Messenger network side.
//!
//! - `manifest`: the signed infrastructure manifest (relays, media servers,
//!   further sources, regions) with anti-rollback and region resolution.
//! - `pool`: `RelayPool`, the `core::Transport` implementation over
//!   `nostr-sdk`'s relay pool.
//!
//! This crate knows nothing about message types or storage. It receives
//! `Outbound`, emits `RawEvent`, and reports relay status.

pub mod manifest;
pub mod pool;

pub use manifest::{Manifest, ManifestRelay, RelayChanges, EMBEDDED_MANIFEST_JSON};
pub use pool::{RelayConfig, RelayPool};
