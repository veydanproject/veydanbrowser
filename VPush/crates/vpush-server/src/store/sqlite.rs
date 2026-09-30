//! The store in one SQLite file.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
};
use sqlx::{Pool, Row, Sqlite};
use vpush_proto::Prefs;

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
        migrate(&pool).await?;
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
        migrate(&pool).await?;
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
        let groups = sqlx::query_scalar::<_, String>(
            "SELECT group_id FROM device_groups WHERE device = ? ORDER BY group_id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;

        let secs = |name: &str| -> Result<u64> {
            Ok(row.try_get::<i64, _>(name).map_err(db)?.max(0) as u64)
        };
        Ok(Device {
            pubkey: row.try_get("pubkey").map_err(db)?,
            device_id: row.try_get("device_id").map_err(db)?,
            app_id: row.try_get("app_id").map_err(db)?,
            provider: row.try_get("provider").map_err(db)?,
            token: row.try_get("token").map_err(db)?,
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
                        token = ?, channel_json = ?, app_version = ?,
                        pref_dm = ?, pref_groups = ?, author_key = ?,
                        updated_at = ?, expires_at = ?
                     WHERE id = ?",
                )
                .bind(&d.app_id)
                .bind(&d.provider)
                .bind(&d.token)
                .bind(&d.token)
                .bind(&d.channel_json)
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
                        (pubkey, device_id, app_id, provider, token, channel_json,
                         app_version, pref_dm, pref_groups, author_key,
                         created_at, updated_at, expires_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&d.pubkey)
                .bind(&d.device_id)
                .bind(&d.app_id)
                .bind(&d.provider)
                .bind(&d.token)
                .bind(&d.channel_json)
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
        for group_id in &d.groups {
            sqlx::query("INSERT OR REPLACE INTO device_groups (device, group_id) VALUES (?, ?)")
                .bind(id)
                .bind(group_id)
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
mod tests_support {
    pub use super::super::RelayPlan;
    pub use super::*;

    /// A phone that watches one relay and one group; registered at 1000,
    /// runs out at 2000.
    pub fn input(pubkey: &str, device_id: &str, token: &str) -> DeviceInput {
        DeviceInput {
            pubkey: pubkey.into(),
            device_id: device_id.into(),
            app_id: "net.veydan.mobile".into(),
            provider: "fcm".into(),
            token: token.into(),
            channel_json: None,
            app_version: Some("4.0.1".into()),
            prefs: Prefs::default(),
            author_key: None,
            relays: vec![WatchedRelay {
                url: "wss://node-1.veydan.net".into(),
                dm: true,
                groups: true,
            }],
            groups: vec!["11".repeat(32)],
            now: 1000,
            expires_at: 2000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::input;
    use super::*;

    const ALICE: &str = "aa";
    const BOB: &str = "bb";

    async fn store() -> SqliteStore {
        SqliteStore::in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn a_device_reads_back_as_written() {
        let s = store().await;
        s.put_device(input(ALICE, "phone-1", "t1"), 10).await.unwrap();

        let d = s.device(ALICE, "phone-1").await.unwrap().unwrap();
        assert_eq!(d.token, "t1");
        assert_eq!(d.state, "active");
        assert_eq!((d.created_at, d.updated_at, d.expires_at), (1000, 1000, 2000));
        assert_eq!(d.relays.len(), 1);
        assert_eq!(d.groups, ["11".repeat(32)]);
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
        assert_eq!(s.schema_version().await.unwrap(), 3);
        s.close().await;

        let s = SqliteStore::open(&path).await.unwrap();
        assert!(s.device(ALICE, "phone-1").await.unwrap().is_some());
        s.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}


fn recipient(row: &SqliteRow) -> Result<super::Recipient> {
    Ok(super::Recipient {
        pubkey: row.try_get("pubkey").map_err(db)?,
        device_id: row.try_get("device_id").map_err(db)?,
        app_id: row.try_get("app_id").map_err(db)?,
        provider: row.try_get("provider").map_err(db)?,
        token: row.try_get("token").map_err(db)?,
        author_key: row.try_get("author_key").map_err(db)?,
    })
}

#[async_trait]
impl super::WatchStore for SqliteStore {
    async fn watch_plan(&self, now: u64) -> Result<Vec<super::RelayPlan>> {
        let mut plans: std::collections::BTreeMap<String, super::RelayPlan> = Default::default();
        let dm = sqlx::query(
            "SELECT DISTINCT r.url AS url, d.pubkey AS target
             FROM device_relays r JOIN devices d ON d.id = r.device
             WHERE r.dm = 1 AND d.pref_dm = 1 AND d.state = 'active' AND d.expires_at > ?
             ORDER BY 1, 2"
        )
        .bind(now as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        for row in dm {
            let url: String = row.try_get("url").map_err(db)?;
            let plan = plans.entry(url.clone()).or_insert_with(|| super::RelayPlan { url, ..Default::default() });
            plan.dm.push(row.try_get("target").map_err(db)?);
        }
        let groups = sqlx::query(
            "SELECT DISTINCT r.url AS url, g.group_id AS target
             FROM device_relays r
             JOIN devices d ON d.id = r.device
             JOIN device_groups g ON g.device = d.id
             WHERE r.groups = 1 AND d.pref_groups = 1 AND d.state = 'active' AND d.expires_at > ?
             ORDER BY 1, 2"
        )
        .bind(now as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        for row in groups {
            let url: String = row.try_get("url").map_err(db)?;
            let plan = plans.entry(url.clone()).or_insert_with(|| super::RelayPlan { url, ..Default::default() });
            plan.groups.push(row.try_get("target").map_err(db)?);
        }
        Ok(plans.into_values().collect())
    }

    async fn baselined(&self, url: &str) -> Result<Vec<(super::WatchKind, String)>> {
        sqlx::query("SELECT kind, target FROM baselines WHERE url = ?")
            .bind(url)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?
            .into_iter()
            .map(|row| {
                let kind: String = row.try_get("kind").map_err(db)?;
                let kind = if kind == "dm" { super::WatchKind::Dm } else { super::WatchKind::Group };
                Ok((kind, row.try_get("target").map_err(db)?))
            })
            .collect()
    }

    async fn set_baselined(
        &self,
        url: &str,
        targets: &[(super::WatchKind, String)],
        now: u64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        for (kind, target) in targets {
            sqlx::query("INSERT OR IGNORE INTO baselines (url, kind, target, at) VALUES (?, ?, ?, ?)")
                .bind(url)
                .bind(kind.as_str())
                .bind(target)
                .bind(now as i64)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        tx.commit().await.map_err(db)
    }

    async fn first_seen(&self, event_id: &str, what: super::Seen, now: u64) -> Result<bool> {
        let done = sqlx::query("INSERT OR IGNORE INTO seen_events (event_id, flag, seen_at) VALUES (?, ?, ?)")
            .bind(event_id)
            .bind(what as i64)
            .bind(now as i64)
            .execute(&self.pool)
            .await
            .map_err(db)?;
        Ok(done.rows_affected() > 0)
    }

    async fn purge_seen(&self, before: u64) -> Result<u64> {
        let done = sqlx::query("DELETE FROM seen_events WHERE seen_at < ?")
            .bind(before as i64)
            .execute(&self.pool)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }

    async fn dm_recipients(&self, pubkey: &str, now: u64) -> Result<Vec<super::Recipient>> {
        sqlx::query(
            "SELECT d.pubkey, d.device_id, d.app_id, d.provider, d.token, d.author_key
             FROM devices d
             WHERE d.pubkey = ? AND d.pref_dm = 1 AND d.state = 'active' AND d.expires_at > ?
               AND EXISTS (SELECT 1 FROM device_relays r WHERE r.device = d.id AND r.dm = 1)
             ORDER BY d.id"
        )
        .bind(pubkey)
        .bind(now as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?
        .iter()
        .map(recipient)
        .collect()
    }

    async fn group_recipients(&self, group_id: &str, now: u64) -> Result<Vec<super::Recipient>> {
        sqlx::query(
            "SELECT d.pubkey, d.device_id, d.app_id, d.provider, d.token, d.author_key
             FROM device_groups g JOIN devices d ON d.id = g.device
             WHERE g.group_id = ? AND d.pref_groups = 1 AND d.state = 'active' AND d.expires_at > ?
               AND EXISTS (SELECT 1 FROM device_relays r WHERE r.device = d.id AND r.groups = 1)
             ORDER BY d.id"
        )
        .bind(group_id)
        .bind(now as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?
        .iter()
        .map(recipient)
        .collect()
    }

    async fn relay_alive(&self, url: &str, now: u64) -> Result<()> {
        sqlx::query(
            "INSERT INTO relay_state (url, last_alive_at) VALUES (?, ?)
             ON CONFLICT (url) DO UPDATE SET last_alive_at = excluded.last_alive_at",
        )
        .bind(url)
        .bind(now as i64)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        Ok(())
    }

    async fn relay_last_alive(&self, url: &str) -> Result<Option<u64>> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT last_alive_at FROM relay_state WHERE url = ?")
                .bind(url)
                .fetch_optional(&self.pool)
                .await
                .map_err(db)?
                .map(|v| v.max(0) as u64),
        )
    }
}

#[cfg(test)]
mod watch_tests {
    use super::super::{Seen, WatchKind, WatchStore};
    use super::tests_support::*;


    #[tokio::test]
    async fn the_plan_is_what_living_devices_want_watched() {
        let s = SqliteStore::in_memory().await.unwrap();
        s.put_device(input("aa", "phone", "t1"), 10).await.unwrap();
        // A second device of the same owner: the key is watched once.
        s.put_device(input("aa", "tablet", "t2"), 10).await.unwrap();
        let mut bob = input("bb", "phone", "t3");
        bob.relays[0].groups = false;
        bob.relays.push(WatchedRelay { url: "wss://nos.lol".into(), dm: false, groups: true });
        bob.groups = vec!["22".repeat(32)];
        s.put_device(bob, 10).await.unwrap();

        let plan = s.watch_plan(1500).await.unwrap();
        assert_eq!(
            plan,
            vec![
                RelayPlan {
                    url: "wss://node-1.veydan.net".into(),
                    dm: vec!["aa".into(), "bb".into()],
                    groups: vec!["11".repeat(32)],
                },
                RelayPlan { url: "wss://nos.lol".into(), dm: vec![], groups: vec!["22".repeat(32)] },
            ]
        );
    }

    #[tokio::test]
    async fn what_is_dead_expired_or_turned_off_is_not_watched() {
        let s = SqliteStore::in_memory().await.unwrap();
        s.put_device(input("aa", "dead", "t1"), 10).await.unwrap();
        s.record_outcome("aa", "dead", "dead_token", 1100).await.unwrap();
        let mut off = input("bb", "off", "t2");
        off.prefs.dm = false;
        off.prefs.groups = false;
        s.put_device(off, 10).await.unwrap();
        s.put_device(input("cc", "expired", "t3"), 10).await.unwrap();

        assert!(s.watch_plan(2500).await.unwrap().is_empty(), "cc ran out at 2000");
        let plan = s.watch_plan(1500).await.unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].dm, ["cc"]);

        assert!(s.dm_recipients("aa", 1500).await.unwrap().is_empty());
        assert!(s.dm_recipients("bb", 1500).await.unwrap().is_empty());
        assert_eq!(s.dm_recipients("cc", 1500).await.unwrap().len(), 1);
        assert!(s.dm_recipients("cc", 2500).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn every_device_of_the_owner_is_told_and_a_group_is_told_to_every_owner() {
        let s = SqliteStore::in_memory().await.unwrap();
        for d in [input("aa", "phone", "t1"), input("aa", "tablet", "t2"), input("bb", "phone", "t3")] {
            s.put_device(d, 10).await.unwrap();
        }

        let dm = s.dm_recipients("aa", 1500).await.unwrap();
        assert_eq!(dm.iter().map(|r| r.device_id.as_str()).collect::<Vec<_>>(), ["phone", "tablet"]);

        let group = s.group_recipients(&"11".repeat(32), 1500).await.unwrap();
        let tokens: Vec<_> = group.iter().map(|r| r.token.as_str()).collect();
        assert_eq!(tokens, ["t1", "t2", "t3"]);
        assert!(s.group_recipients(&"99".repeat(32), 1500).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_event_is_new_once() {
        let s = SqliteStore::in_memory().await.unwrap();
        assert!(s.first_seen("e1", Seen::Pushed, 100).await.unwrap());
        assert!(!s.first_seen("e1", Seen::Pushed, 101).await.unwrap());
        assert!(!s.first_seen("e1", Seen::Baseline, 102).await.unwrap());
        assert!(s.first_seen("e2", Seen::Quiet, 200).await.unwrap());

        assert_eq!(s.purge_seen(150).await.unwrap(), 1);
        assert!(s.first_seen("e1", Seen::Pushed, 300).await.unwrap(), "forgotten, so new again");
        assert!(!s.first_seen("e2", Seen::Pushed, 300).await.unwrap());
    }

    #[tokio::test]
    async fn stock_is_taken_per_relay() {
        let s = SqliteStore::in_memory().await.unwrap();
        let targets = vec![(WatchKind::Dm, "aa".to_string()), (WatchKind::Group, "11".repeat(32))];
        s.set_baselined("wss://a", &targets, 100).await.unwrap();
        s.set_baselined("wss://a", &targets, 200).await.unwrap();

        let mut got = s.baselined("wss://a").await.unwrap();
        got.sort();
        assert_eq!(got, targets);
        assert!(s.baselined("wss://b").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_last_time_a_relay_was_on_the_line() {
        let s = SqliteStore::in_memory().await.unwrap();
        assert_eq!(s.relay_last_alive("wss://a").await.unwrap(), None);
        s.relay_alive("wss://a", 100).await.unwrap();
        s.relay_alive("wss://a", 200).await.unwrap();
        assert_eq!(s.relay_last_alive("wss://a").await.unwrap(), Some(200));
    }
}

/// Brings the schema up to date.
///
/// A database that a newer release has been at holds migrations this
/// release does not know. They only added to the schema, so this release
/// runs on it as it is: that is what makes going back a release possible.
async fn migrate(pool: &Pool<Sqlite>) -> Result<()> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.set_ignore_missing(true);
    migrator.run(pool).await.map_err(db)
}

#[cfg(test)]
mod migrate_tests {
    use super::tests_support::input;
    use super::*;

    #[tokio::test]
    async fn a_database_a_newer_release_has_been_at_is_opened_by_an_older_one() {
        let dir = std::env::temp_dir().join(format!("vpush-rollback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vpush.db");

        let s = SqliteStore::open(&path).await.unwrap();
        s.put_device(input("aa", "phone", "t1"), 10).await.unwrap();
        // What a release of the future would leave behind.
        sqlx::query("CREATE TABLE of_the_future (id INTEGER PRIMARY KEY)")
            .execute(&s.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES (999, 'of the future', 1, x'00', 0)",
        )
        .execute(&s.pool)
        .await
        .unwrap();
        s.close().await;

        let s = SqliteStore::open(&path).await.expect("the older release opens it");
        assert!(s.device("aa", "phone").await.unwrap().is_some());
        s.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
