// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Storage trait and the local-directory adapter.

use crate::{Result, SyncError};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// Minimal storage contract. Keys are `/`-separated, relative to the vault root.
#[async_trait]
pub trait Storage: Send + Sync {
    /// All keys under `prefix`, recursively.
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    /// Atomic on the storage side: readers never observe a partial object.
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}

/// Suffix of in-progress writes; skipped by every adapter's `list`.
pub const TMP_SUFFIX: &str = ".tmp";

/// Keys the vault never produces; ignored so a cloud client's metadata files
/// don't make a folder look "foreign".
pub fn is_noise_key(key: &str) -> bool {
    let name = key.rsplit('/').next().unwrap_or(key);
    name.starts_with('.')
        || name.ends_with(TMP_SUFFIX)
        || name.eq_ignore_ascii_case("desktop.ini")
        || name.eq_ignore_ascii_case("thumbs.db")
}

/// A plain directory. Works with any folder a cloud client mirrors.
pub struct LocalDir {
    root: PathBuf,
}

impl LocalDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> Result<PathBuf> {
        if key.split('/').any(|seg| seg == ".." || seg.is_empty()) {
            return Err(SyncError::Storage(format!("bad key {key}")));
        }
        Ok(self.root.join(key))
    }

    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let key = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if is_noise_key(&key) {
                continue;
            }
            let ty = entry.file_type()?;
            if ty.is_dir() {
                Self::walk(&entry.path(), &key, out)?;
            } else {
                out.push(key);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Storage for LocalDir {
    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        if !self.root.exists() {
            return Ok(vec![]);
        }
        let mut all = Vec::new();
        Self::walk(&self.root, "", &mut all)?;
        all.retain(|k| k.starts_with(prefix));
        Ok(all)
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.path_for(key)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        use std::io::Write;
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = PathBuf::from(format!("{}{}", path.display(), TMP_SUFFIX));
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.path_for(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
