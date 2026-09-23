// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Shared on every platform
pub mod demo;
pub mod media;
pub mod notes;
pub mod password;
pub mod totp;

// Desktop only: browser profiles, proxies, SSH/SFTP, backups, tray settings
#[cfg(desktop)]
pub mod backup;
#[cfg(desktop)]
pub mod camoufox;
#[cfg(desktop)]
pub mod fs;
#[cfg(desktop)]
pub mod profiles;
#[cfg(desktop)]
pub mod proxies;
#[cfg(desktop)]
pub mod settings;
#[cfg(desktop)]
pub mod sftp;
#[cfg(desktop)]
pub mod ssh;
#[cfg(desktop)]
pub mod ssh_keys;
#[cfg(desktop)]
pub mod transfer;
#[cfg(desktop)]
pub mod workspaces;
