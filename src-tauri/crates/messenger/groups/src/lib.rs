// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Groups.
//!
//! There is no server that owns a group. The group is a log of operations,
//! each made by a member who was allowed to make it, and every device
//! computes the same state from the same set of operations, whatever
//! order they arrived in.
//!
//! - `roles`: owner, admin, moderator, member, and what each may do.
//! - `op`: operations and their identity.
//! - `state`: the state of a group and how one operation changes it.
//! - `log`: the set of operations of a group, its linear order, the
//!   resulting state, and whether the current key can still be trusted.
//!
//! Everything in this crate is pure: no network, no storage, no clock.
//! Signatures are checked where operations enter (the wire layer); here an
//! operation's `author` is already known to be genuine.

pub mod log;
pub mod op;
pub mod roles;
pub mod state;

pub use log::{Insert, KeyStatus, OpLog};
pub use op::{GroupKind, KeyId, Op, OpBody, OpId};
pub use roles::{permits, Action, Role};
pub use state::{GroupState, Member, Rejection};
