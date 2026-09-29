//! The store in one SQLite file.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
};
use sqlx::{Pool, Row, Sqlite};
use vpush_proto::{GroupWatch, Prefs};

use super::{Counts, Device, DeviceInput, Result, Store, StoreError, WatchedRelay};

fn db(e: impl std::fmt::Display) -> StoreError {
    StoreError::Database(e.to_string())
}

#[derive(Clone)]
pub struct SqliteStore {
    pool: Pool<Sqlite>,
}

impl SqliteStore {
    /// Opens the file, creating it when missing, and brings the schema up to date.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| db(format!("cannot create {}: {e}", dir.display())))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|e| db(format!("cannot open {}: {e}", path.display())))?;
        sqlx::migrate!("./migrations").run(&pool).await.map_err(db)?;
        Ok(Self { pool })
    }

    /// A database that lives as long as the value does. For tests.
    pub async fn in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(db)?
            .foreign_keys(true);
        // One connection: every connection to `:memory:` is its own database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(db)?;
        sqlx::migrate!("./migrations").run(&pool).await.map_err(db)?;
        Ok(Self { pool })
    }

    pub async fn schema_version(&self) -> Result<i64> {
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(&self.pool)
            .await
            .map_err(db)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn fill(&self, row: SqliteRow) -> Result<Device> {
        let id: i64 = row.try_get("id").map_err(db)?;
        let relays = sqlx::query(
            "SELECT url, dm, groups FROM device_relays WHERE device = ? ORDER BY url",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?
        .into_iter()
        .map(|r| {
            Ok(WatchedRelay {
                url: r.try_get("url").map_err(db)?,
                dm: r.try_get::<i64, _>("dm").map_err(db)? != 0,
                groups: r.try_get::<i64, _>("groups").map_err(db)? != 0,
            })
        })
        .collect::<Result<Vec<_>>>()?;
        let groups = sqlx::query(
            "SELECT group_id, name FROM device_groups WHERE device = ? ORDER BY group_id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?
        .into_iter()
        .map(|r| {
            Ok(GroupWatch {
                id: r.try_get("group_id").map_err(db)?,
                name: r.try_get("name").map_err(db)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

        let secs = |name: &str| -> Result<u64> {
            Ok(row.try_get::<i64, _>(name).map_err(db)?.max(0) as u64)
        };
        Ok(Device {
            pubkey: row.try_get("pubkey").map_err(db)?,
            device_id: row.try_get("device_id").map_err(db)?,
            app_id: row.try_get("app_id").map_err(db)?,
            provider: row.try_get("provider").map_err(db)?,
            token: row.try_get("token").map_err(db)?,
            locale: row.try_get("locale").map_err(db)?,
            app_version: row.try_get("app_version").map_err(db)?,
            prefs: Prefs {
                dm: row.try_get::<i64, _>("pref_dm").map_err(db)? != 0,
                groups: row.try_get::<i64, _>("pref_groups").map_err(db)? != 0,
            },
            author_key: row.try_get("author_key").map_err(db)?,
            state: row.try_get("state").map_err(db)?,
            created_at: secs("created_at")?,
            updated_at: secs("updated_at")?,
            expires_at: secs("expires_at")?,
            last_push_at: row
                .try_get::<Option<i64>, _>("last_push_at")
                .map_err(db)?
                .map(|v| v.max(0) as u64),
            last_outcome: row.try_get("last_outcome").map_err(db)?,
            relays,
            groups,
        })
    }
}

#[async_trait]
impl Store for SqliteStore {
    async fn put_device(&self, d: DeviceInput, max_per_owner: u32) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(db)?;

        // The address at the push service moved to this owner and device.
        sqlx::query(
            "DELETE FROM devices
             WHERE app_id = ? AND provider = ? AND token = ?
               AND NOT (pubkey = ? AND device_id = ?)",
        )
        .bind(&d.app_id)
        .bind(&d.provider)
        .bind(&d.token)
        .bind(&d.pubkey)
        .bind(&d.device_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;

        let known: Option<i64> =
            sqlx::query_scalar("SELECT id FROM devices WHERE pubkey = ? AND device_id = ?")
                .bind(&d.pubkey)
                .bind(&d.device_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;

        let id = match known {
            Some(id) => {
                // A new address means the push service has not refused it yet.
                sqlx::query(
                    "UPDATE devices SET
                        app_id = ?, provider = ?,
                        state = CASE WHEN token = ? THEN state ELSE 'active' END,
                        token = ?, channel_json = ?, locale = ?, app_version = ?,
                        pref_dm = ?, pref_groups = ?, author_key = ?,
                        updated_at = ?, expires_at = ?
                     WHERE id = ?",
                )
                .bind(&d.app_id)
                .bind(&d.provider)
                .bind(&d.token)
                .bind(&d.token)
                .bind(&d.channel_json)
                .bind(&d.locale)
                .bind(&d.app_version)
                .bind(d.prefs.dm)
                .bind(d.prefs.groups)
                .bind(&d.author_key)
                .bind(d.now as i64)
                .bind(d.expires_at as i64)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
                id
            }
            None => {
                let owned: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM devices WHERE pubkey = ?")
                        .bind(&d.pubkey)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(db)?;
                if owned >= i64::from(max_per_owner) {
                    return Err(StoreError::TooManyDevices);
                }
                sqlx::query(
                    "INSERT INTO devices
                        (pubkey, device_id, app_id, provider, token, channel_json, locale,
                         app_version, pref_dm, pref_groups, author_key,
                         created_at, updated_at, expires_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&d.pubkey)
                .bind(&d.device_id)
                .bind(&d.app_id)
                .bind(&d.provider)
                .bind(&d.token)
                .bind(&d.channel_json)
                .bind(&d.locale)
                .bind(&d.app_version)
                .bind(d.prefs.dm)
                .bind(d.prefs.groups)
                .bind(&d.author_key)
                .bind(d.now as i64)
                .bind(d.now as i64)
                .bind(d.expires_at as i64)
                .execute(&mut *tx)
                .await
                .map_err(db)?
                .last_insert_rowid()
            }
        };

        sqlx::query("DELETE FROM device_relays WHERE device = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        for relay in &d.relays {
            sqlx::query(
                "INSERT OR REPLACE INTO device_relays (device, url, dm, groups) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(&relay.url)
            .bind(relay.dm)
            .bind(relay.groups)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }

        sqlx::query("DELETE FROM device_groups WHERE device = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        for group in &d.groups {
            sqlx::query(
                "INSERT OR REPLACE INTO device_groups (device, group_id, name) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(&group.id)
            .bind(&group.name)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }

        tx.commit().await.map_err(db)
    }

    async fn device(&self, pubkey: &str, device_id: &str) -> Result<Option<Device>> {
        let row = sqlx::query("SELECT * FROM devices WHERE pubkey = ? AND device_id = ?")
            .bind(pubkey)
            .bind(device_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db)?;
        match row {
            Some(row) => Ok(Some(self.fill(row).await?)),
            None => Ok(None),
        }
    }

    async fn devices_of(&self, pubkey: &str) -> Result<Vec<Device>> {
        let rows = sqlx::query("SELECT * FROM devices WHERE pubkey = ? ORDER BY updated_at DESC, id DESC")
            .bind(pubkey)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.fill(row).await?);
        }
        Ok(out)
    }

    async fn delete_device(&self, pubkey: &str, device_id: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM devices WHERE pubkey = ? AND device_id = ?")
            .bind(pubkey)
            .bind(device_id)
            .execute(&self.pool)
            .await
            .map_err(db)?;
        Ok(done.rows_affected() > 0)
    }

    async fn record_outcome(
        &self,
        pubkey: &str,
        device_id: &str,
        outcome: &str,
        now: u64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE devices SET
                last_push_at = ?, last_outcome = ?,
                state = CASE WHEN ? = 'dead_token' THEN 'dead_token' ELSE state END
             WHERE pubkey = ? AND device_id = ?",
        )
        .bind(now as i64)
        .bind(outcome)
        .bind(outcome)
        .bind(pubkey)
        .bind(device_id)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        Ok(())
    }

    async fn purge_expired(&self, now: u64) -> Result<u64> {
        let done = sqlx::query("DELETE FROM devices WHERE expires_at <= ?")
            .bind(now as i64)
            .execute(&self.pool)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }

    async fn counts(&self) -> Result<Counts> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS devices,
                    COUNT(DISTINCT pubkey) AS owners,
                    COALESCE(SUM(state = 'dead_token'), 0) AS dead
             FROM devices",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db)?;
        Ok(Counts {
            devices: row.try_get::<i64, _>("devices").map_err(db)? as u64,
            owners: row.try_get::<i64, _>("owners").map_err(db)? as u64,
            dead_tokens: row.try_get::<i64, _>("dead").map_err(db)? as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "aa";
    const BOB: &str = "bb";

    fn input(pubkey: &str, device_id: &str, token: &str) -> DeviceInput {
        DeviceInput {
            pubkey: pubkey.into(),
            device_id: device_id.into(),
            app_id: "net.veydan.mobile".into(),
            provider: "fcm".into(),
            token: token.into(),
            channel_json: None,
            locale: "ru".into(),
            app_version: Some("4.0.1".into()),
            prefs: Prefs::default(),
            author_key: None,
            relays: vec![WatchedRelay {
                url: "wss://node-1.veydan.net".into(),
                dm: true,
                groups: true,
            }],
            groups: vec![GroupWatch {
                id: "11".repeat(32),
                name: Some("Команда".into()),
            }],
            now: 1000,
            expires_at: 2000,
        }
    }

    async fn store() -> SqliteStore {
        SqliteStore::in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn a_device_reads_back_as_written() {
        let s = store().await;
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();

        let d = s.device(ALICE, "phone-1").await.unwrap().unwrap();
        assert_eq!(d.token, "t1");
        assert_eq!(d.locale, "ru");
        assert_eq!(d.state, "active");
        assert_eq!((d.created_at, d.updated_at, d.expires_at), (1000, 1000, 2000));
        assert_eq!(d.relays.len(), 1);
        assert_eq!(d.groups[0].name.as_deref(), Some("Команда"));
        assert!(s.device(ALICE, "phone-2").await.unwrap().is_none());
        assert!(s.device(BOB, "phone-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_second_put_replaces_what_is_watched() {
        let s = store().await;
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();

        let mut next = input(ALICE, "phone-1", "t1");
        next.now = 1500;
        next.expires_at = 2500;
        next.relays = vec![WatchedRelay {
            url: "wss://nos.lol".into(),
            dm: true,
            groups: false,
        }];
        next.groups = vec![];
        next.prefs.groups = false;
        s.put_device(next, 10).await.unwrap();

        let d = s.device(ALICE, "phone-1").await.unwrap().unwrap();
        assert_eq!(d.created_at, 1000, "the device is the same one");
        assert_eq!((d.updated_at, d.expires_at), (1500, 2500));
        assert_eq!(d.relays.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), ["wss://nos.lol"]);
        assert!(d.groups.is_empty());
        assert!(!d.prefs.groups);
    }

    #[tokio::test]
    async fn deleting_one_device_leaves_the_owners_other_device_whole() {
        let s = store().await;
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();
        s.put_device(input(ALICE, "tablet", "t2"), 10).await.unwrap();

        assert!(s.delete_device(ALICE, "phone-1").await.unwrap());
        assert!(!s.delete_device(ALICE, "phone-1").await.unwrap(), "already gone");

        let left = s.device(ALICE, "tablet").await.unwrap().unwrap();
        assert_eq!(left.relays.len(), 1);
        assert_eq!(left.groups.len(), 1);
        assert_eq!(s.devices_of(ALICE).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_token_under_a_new_owner_takes_the_row_of_the_former_one() {
        let s = store().await;
        s.put_device(input(ALICE, "phone-1", "same-token"), 10).await.unwrap();
        s.put_device(input(BOB, "phone-1", "same-token"), 10).await.unwrap();

        assert!(s.device(ALICE, "phone-1").await.unwrap().is_none());
        assert!(s.device(BOB, "phone-1").await.unwrap().is_some());
        assert_eq!(s.counts().await.unwrap().devices, 1);
    }

    #[tokio::test]
    async fn names_of_a_group_are_kept_apart_by_device() {
        let s = store().await;
        let mut a = input(ALICE, "phone-1", "t1");
        a.groups[0].name = Some("Работа".into());
        let mut b = input(BOB, "phone-1", "t2");
        b.groups[0].name = Some("Спам".into());
        s.put_device(a, 10).await.unwrap();
        s.put_device(b, 10).await.unwrap();

        let a = s.device(ALICE, "phone-1").await.unwrap().unwrap();
        assert_eq!(a.groups[0].name.as_deref(), Some("Работа"));
    }

    #[tokio::test]
    async fn the_limit_of_devices_counts_new_ones_only() {
        let s = store().await;
        s.put_device(input(ALICE, "d1", "t1"), 2).await.unwrap();
        s.put_device(input(ALICE, "d2", "t2"), 2).await.unwrap();
        let e = s.put_device(input(ALICE, "d3", "t3"), 2).await.unwrap_err();
        assert!(matches!(e, StoreError::TooManyDevices));

        // Renewing one of the two is not a third.
        s.put_device(input(ALICE, "d1", "t1-new"), 2).await.unwrap();
        // Another owner has a count of their own.
        s.put_device(input(BOB, "d1", "t4"), 2).await.unwrap();
    }

    #[tokio::test]
    async fn a_dead_token_marks_the_device_and_a_new_token_revives_it() {
        let s = store().await;
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();

        s.record_outcome(ALICE, "phone-1", "delivered", 1100).await.unwrap();
        let d = s.device(ALICE, "phone-1").await.unwrap().unwrap();
        assert_eq!((d.state.as_str(), d.last_outcome.as_deref(), d.last_push_at), ("active", Some("delivered"), Some(1100)));

        s.record_outcome(ALICE, "phone-1", "dead_token", 1200).await.unwrap();
        assert_eq!(s.device(ALICE, "phone-1").await.unwrap().unwrap().state, "dead_token");
        assert_eq!(s.counts().await.unwrap().dead_tokens, 1);

        // The same token again: still dead.
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();
        assert_eq!(s.device(ALICE, "phone-1").await.unwrap().unwrap().state, "dead_token");

        s.put_device(input(ALICE, "phone-1", "t1-new"), 10).await.unwrap();
        assert_eq!(s.device(ALICE, "phone-1").await.unwrap().unwrap().state, "active");
    }

    #[tokio::test]
    async fn what_nobody_renewed_is_forgotten() {
        let s = store().await;
        s.put_device(input(ALICE, "old", "t1"), 10).await.unwrap();
        let mut fresh = input(BOB, "fresh", "t2");
        fresh.expires_at = 9000;
        s.put_device(fresh, 10).await.unwrap();

        assert_eq!(s.purge_expired(1999).await.unwrap(), 0);
        assert_eq!(s.purge_expired(2000).await.unwrap(), 1);
        assert!(s.device(ALICE, "old").await.unwrap().is_none());
        assert!(s.device(BOB, "fresh").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn the_file_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!("vpush-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("vpush.db");

        let s = SqliteStore::open(&path).await.unwrap();
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();
        assert_eq!(s.schema_version().await.unwrap(), 1);
        s.close().await;

        let s = SqliteStore::open(&path).await.unwrap();
        assert!(s.device(ALICE, "phone-1").await.unwrap().is_some());
        s.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
