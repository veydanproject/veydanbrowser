// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! What the phone's notification handler gets from the app: the keys it
//! opens events with, as a bundle the host keeps in the phone's own key
//! store, and the settings of what a notification may say.

use crate::MessengerRuntime;
use messenger_core::Result;
use messenger_notify::{Content, GroupKeyEntry, KeyBundle, Settings};

impl MessengerRuntime {
    /// The keys a push handler needs today, or None without a session.
    pub async fn notify_bundle(&self) -> Result<Option<KeyBundle>> {
        let Ok(keys) = self.session_keys().await else {
            return Ok(None);
        };
        let groups = self.groups().export_keys().await?.iter().map(|(g, k)| GroupKeyEntry::of(g, k)).collect();
        Ok(Some(KeyBundle::new(&keys, groups)))
    }

    /// Changes when `notify_bundle` would; costs no secret.
    pub async fn notify_fingerprint(&self) -> Result<Option<String>> {
        let Ok(keys) = self.session_keys().await else {
            return Ok(None);
        };
        Ok(Some(format!("{}|{}", keys.public_key().to_hex(), self.groups().keys_fingerprint().await?)))
    }

    pub async fn notify_settings(&self) -> Result<Settings> {
        Settings::load(self.store()).await
    }

    pub async fn notify_set_settings(&self, settings: Settings) -> Result<Settings> {
        settings.save(self.store()).await?;
        Ok(settings)
    }

    /// Whether the handler is to have the keys at all.
    pub async fn notify_wants_keys(&self) -> Result<bool> {
        Ok(self.notify_settings().await?.content != Content::None)
    }
}
