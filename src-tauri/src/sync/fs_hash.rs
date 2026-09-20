// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! File hashes with a per-process cache: a file is re-read only when its
//! mtime or size changed. Shared by attachments and profile files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use veydan_sync::sha256_hex;

#[derive(Clone)]
struct FileStamp {
    mtime: Option<SystemTime>,
    size: u64,
    hash: String,
}

#[derive(Default)]
pub struct HashCache(Mutex<HashMap<PathBuf, FileStamp>>);

impl HashCache {
    /// SHA-256 of the file, or `None` when it cannot be read.
    pub fn file_hash(&self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let (mtime, size) = (meta.modified().ok(), meta.len());
        let mut cache = self.0.lock().ok()?;
        if let Some(s) = cache.get(path) {
            if s.mtime == mtime && s.size == size {
                return Some(s.hash.clone());
            }
        }
        let hash = sha256_hex(&std::fs::read(path).ok()?);
        cache.insert(path.to_path_buf(), FileStamp { mtime, size, hash: hash.clone() });
        Some(hash)
    }
}
