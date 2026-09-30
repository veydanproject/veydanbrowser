//! What the server remembers: devices and what is watched for them.
//!
//! Everything the rest of the server knows about the database is the
//! [`Store`] trait. SQL does not leave this module, so another database can
//! take the place of SQLite by writing one more implementation.

mod sqlite;

pub use sqlite::SqliteStore;

use async_trait::async_trait;
use vpush_proto::Prefs;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Database(String),
    /// The owner already has as many devices as the server allows.
    #[error("too many devices")]
    TooManyDevices,
    /// The server holds as many devices as it takes.
    #[error("the server is full")]
    Full,
}

/// How many devices there may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceLimits {
    /// Of one owner.
    pub per_owner: u32,
    /// Of all owners together.
    pub total: u32,
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// A relay a device asked to be watched. Only relays the server agreed to
/// watch get here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedRelay {
    /// Normalized.
    pub url: String,
    pub dm: bool,
    pub groups: bool,
}

/// A group watched for a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedGroup {
    pub id: String,
    /// The push keys the device registered for the group; sorted when read
    /// back. Empty for a group registered before there were push keys: the
    /// device is told nothing of that group until it registers again.
    pub keys: Vec<String>,
}

/// Everything about a device, as it is written on registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInput {
    pub pubkey: String,
    pub device_id: String,
    pub app_id: String,
    pub provider: String,
    pub token: String,
    pub channel_json: Option<String>,
    pub app_version: Option<String>,
    pub prefs: Prefs,
    pub author_key: Option<String>,
    pub relays: Vec<WatchedRelay>,
    /// The groups watched for the device.
    pub groups: Vec<WatchedGroup>,
    /// Unix seconds.
    pub now: u64,
    pub expires_at: u64,
    /// When the push service said, for this registration, that the token
    /// is one it can push to. `None`: it was not asked, since it had said
    /// so of this very token before, or it could not say.
    pub token_checked_at: Option<u64>,
}

/// A device as it is remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub pubkey: String,
    pub device_id: String,
    pub app_id: String,
    pub provider: String,
    pub token: String,
    pub app_version: Option<String>,
    pub prefs: Prefs,
    pub author_key: Option<String>,
    /// `active` or `dead_token`.
    pub state: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub last_push_at: Option<u64>,
    pub last_outcome: Option<String>,
    /// When the push service said the token is one it can push to, or took
    /// a push for it. `None`: nothing is known of the token yet.
    pub token_checked_at: Option<u64>,
    pub relays: Vec<WatchedRelay>,
    pub groups: Vec<WatchedGroup>,
}

/// How many of what there is, for `vpush ctl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Counts {
    pub devices: u64,
    pub owners: u64,
    pub dead_tokens: u64,
}

#[async_trait]
pub trait Store: Send + Sync + 'static {
    /// Writes the device as given, replacing what was there: relays, groups
    /// and push keys of groups that are not named are no longer kept.
    ///
    /// A device of another owner with the same address at the push service
    /// is removed: the phone has changed hands, or identities.
    ///
    /// A token the push service has just vouched for makes the device
    /// `active`, whatever it was. Without that a token that stays is what it
    /// was, dead when it was dead, and a new one is `active` and not yet
    /// asked about.
    async fn put_device(&self, device: DeviceInput, limits: DeviceLimits) -> Result<()>;

    async fn device(&self, pubkey: &str, device_id: &str) -> Result<Option<Device>>;

    /// Every device of an owner, the newest first.
    async fn devices_of(&self, pubkey: &str) -> Result<Vec<Device>>;

    /// True when there was one.
    async fn delete_device(&self, pubkey: &str, device_id: &str) -> Result<bool>;

    /// Notes what became of the last push to the device. `dead_token` also
    /// marks the device so that nothing more is sent to it, and `delivered`
    /// says of a token nobody had vouched for that it is one.
    ///
    /// `token` is the one the push went to. A device that has registered a
    /// new token since is left as it is: what the push service said of the
    /// former token says nothing of the new one.
    async fn record_outcome(
        &self,
        pubkey: &str,
        device_id: &str,
        token: &str,
        outcome: &str,
        now: u64,
    ) -> Result<()>;

    /// Forgets the registrations nobody renewed. Returns how many.
    async fn purge_expired(&self, now: u64) -> Result<u64>;

    async fn counts(&self) -> Result<Counts>;
}

/// What is watched on one relay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayPlan {
    /// Normalized.
    pub url: String,
    /// Keys whose direct messages are watched, sorted.
    pub dm: Vec<String>,
    /// Groups whose events are watched, sorted.
    pub groups: Vec<String>,
}

/// What a watch is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WatchKind {
    Dm,
    Group,
}

impl WatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dm => "dm",
            Self::Group => "group",
        }
    }
}

/// What became of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    Pushed = 0,
    /// It was on the relay before the watch began.
    Baseline = 1,
    /// Marked by its author as not worth a push.
    Quiet = 2,
    /// For nobody who is registered.
    Nobody = 3,
    /// Dated too far from now to be pushed.
    Misdated = 4,
    /// Of a group, and without a mark of a key registered for it.
    Unmarked = 5,
}

/// A device a push goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub pubkey: String,
    pub device_id: String,
    pub app_id: String,
    pub provider: String,
    pub token: String,
    pub author_key: Option<String>,
}

/// What the server does on the relays.
#[async_trait]
pub trait WatchStore: Send + Sync + 'static {
    /// What to watch, by relay: for the devices that are registered, alive,
    /// and want to be told.
    async fn watch_plan(&self, now: u64) -> Result<Vec<RelayPlan>>;

    /// What was taken stock of on a relay.
    async fn baselined(&self, url: &str) -> Result<Vec<(WatchKind, String)>>;

    async fn set_baselined(&self, url: &str, targets: &[(WatchKind, String)], now: u64) -> Result<()>;

    /// Forgets that stock was taken of these keys and groups on a relay.
    /// They are no longer watched there; when they are again, what the
    /// relay holds for them by then is old, and stock is taken anew.
    async fn unset_baselined(&self, url: &str, targets: &[(WatchKind, String)]) -> Result<()>;

    /// Forgets the stock taken on every relay but these. Returns how many
    /// keys and groups were forgotten.
    async fn keep_baselined(&self, urls: &[String]) -> Result<u64>;

    /// True the first time an event is seen, false ever after.
    async fn first_seen(&self, event_id: &str, what: Seen, now: u64) -> Result<bool>;

    /// Forgets the events seen before `before`. Returns how many.
    async fn purge_seen(&self, before: u64) -> Result<u64>;

    /// The devices to tell about a direct message to `pubkey`.
    async fn dm_recipients(&self, pubkey: &str, now: u64) -> Result<Vec<Recipient>>;

    /// The push keys registered for a group by the devices that could be
    /// told of its events: each key once, `limit` of them at most.
    ///
    /// The keys more devices hold come first, and keys held by as many are
    /// in the order of the keys themselves. Anybody may register a key of
    /// their own making for a group; when there are more keys than are
    /// asked for, it is the ones held by the fewest that are left out, and
    /// not the key of the members.
    async fn group_keys(&self, group_id: &str, now: u64, limit: usize) -> Result<Vec<String>>;

    /// The devices to tell about an event of a group: those that registered
    /// one of `keys` for it.
    async fn group_recipients(
        &self,
        group_id: &str,
        keys: &[String],
        now: u64,
    ) -> Result<Vec<Recipient>>;

    async fn relay_alive(&self, url: &str, now: u64) -> Result<()>;

    async fn relay_last_alive(&self, url: &str) -> Result<Option<u64>>;
}

/// Everything the server keeps.
pub trait AllStore: Store + WatchStore {}
impl<T: Store + WatchStore> AllStore for T {}
