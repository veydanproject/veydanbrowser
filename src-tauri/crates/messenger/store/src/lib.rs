// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Messenger persistence.
//!
//! The messenger owns a separate SQLite file (`<data_dir>/messenger.db`) so
//! the module can be removed or extracted without touching host data.
//! Migrations live in `migrations/` and are embedded at compile time.
//!
//! Repositories are plain functions over the pool, grouped per module
//! (`settings`, later `identity`, `relays`, `messages`, …).

pub mod identity;
pub mod relays;
pub mod settings;

/// Unix seconds now; the store stamps rows itself.
pub(crate) fn now() -> i64 {
    use messenger_core::Clock;
    messenger_core::traits::SystemClock.now().secs()
}

use messenger_core::{MessengerConfig, MessengerError, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;

/// Open (creating if needed) and migrate the messenger database.
#[derive(Clone)]
pub struct Store {
    pool: Pool<Sqlite>,
}

impl Store {
    pub async fn open(config: &MessengerConfig) -> Result<Self> {
        tokio::fs::create_dir_all(config.data_dir()).await?;
        let path = config.db_path();
        let url = format!("sqlite://{}", path.to_string_lossy());
        let options = SqliteConnectOptions::from_str(&url)
            .map_err(storage)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(storage)?;
        sqlx::migrate!("./migrations").run(&pool).await.map_err(storage)?;
        Ok(Self { pool })
    }

    /// In-memory database for tests. Same migrations, no file.
    pub async fn open_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(storage)?
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(storage)?;
        sqlx::migrate!("./migrations").run(&pool).await.map_err(storage)?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// Latest applied migration version, for `status()`.
    pub async fn schema_version(&self) -> Result<i64> {
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(&self.pool)
            .await
            .map_err(storage)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

pub(crate) fn storage(e: impl std::fmt::Display) -> MessengerError {
    MessengerError::Storage(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_and_migrates_a_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = MessengerConfig::new(dir.path().join("msg"));
        let store = Store::open(&cfg).await.unwrap();
        assert!(cfg.db_path().exists());
        assert!(store.schema_version().await.unwrap() >= 1);
        store.close().await;

        // Reopening is idempotent.
        let again = Store::open(&cfg).await.unwrap();
        assert!(again.schema_version().await.unwrap() >= 1);
    }
}
