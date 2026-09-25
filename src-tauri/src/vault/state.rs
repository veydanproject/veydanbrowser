// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! In-memory password vault. The vault key never touches disk unwrapped.

use super::crypto::SecretKey;
use std::sync::Mutex;

enum Phase {
    Locked,
    /// Lock is open and no vault row exists yet.
    Pending { kek: SecretKey, salt: [u8; 16] },
    Open {
        kek: SecretKey,
        key: SecretKey,
        vault_id: String,
    },
    /// Lock password does not unwrap the synced vault.
    Mismatch,
}

pub struct VaultState {
    phase: Mutex<Phase>,
}

impl Default for VaultState {
    fn default() -> Self {
        Self {
            phase: Mutex::new(Phase::Locked),
        }
    }
}

/// What to store after a committed password change.
pub enum MemoryUpdate {
    Pending { kek: SecretKey, salt: [u8; 16] },
    Open {
        kek: SecretKey,
        key: SecretKey,
        vault_id: String,
    },
    Mismatch,
}

impl VaultState {
    pub fn lock(&self) {
        *self.phase.lock().expect("vault") = Phase::Locked;
    }

    pub fn apply(&self, update: MemoryUpdate) {
        *self.phase.lock().expect("vault") = match update {
            MemoryUpdate::Pending { kek, salt } => Phase::Pending { kek, salt },
            MemoryUpdate::Open { kek, key, vault_id } => Phase::Open { kek, key, vault_id },
            MemoryUpdate::Mismatch => Phase::Mismatch,
        };
    }

    pub fn set_pending(&self, kek: SecretKey, salt: [u8; 16]) {
        self.apply(MemoryUpdate::Pending { kek, salt });
    }

    pub fn set_open(&self, kek: SecretKey, key: SecretKey, vault_id: String) {
        self.apply(MemoryUpdate::Open { kek, key, vault_id });
    }

    pub fn set_mismatch(&self) {
        self.apply(MemoryUpdate::Mismatch);
    }

    /// `none` | `ok` | `mismatch`. `locked` is reported by the caller from the row.
    pub fn label(&self) -> &'static str {
        match *self.phase.lock().expect("vault") {
            Phase::Open { .. } => "ok",
            Phase::Mismatch => "mismatch",
            Phase::Pending { .. } => "none",
            Phase::Locked => "locked",
        }
    }

    pub fn is_mismatch(&self) -> bool {
        matches!(*self.phase.lock().expect("vault"), Phase::Mismatch)
    }

    pub fn open_key(&self) -> Option<(SecretKey, String)> {
        match &*self.phase.lock().expect("vault") {
            Phase::Open { key, vault_id, .. } => Some((key.clone_key(), vault_id.clone())),
            _ => None,
        }
    }

    pub fn pending(&self) -> Option<(SecretKey, [u8; 16])> {
        match &*self.phase.lock().expect("vault") {
            Phase::Pending { kek, salt } => Some((kek.clone_key(), *salt)),
            _ => None,
        }
    }

    /// KEK while the lock is open (pending or unlocked vault).
    pub fn kek(&self) -> Option<SecretKey> {
        match &*self.phase.lock().expect("vault") {
            Phase::Pending { kek, .. } | Phase::Open { kek, .. } => Some(kek.clone_key()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_locked_and_lock_drops_key() {
        let state = VaultState::default();
        assert!(state.open_key().is_none());
        assert_eq!(state.label(), "locked");
        state.set_open(SecretKey::random(), SecretKey::random(), "v".into());
        assert!(state.open_key().is_some());
        state.lock();
        assert!(state.open_key().is_none());
        assert_eq!(state.label(), "locked");
    }
}
