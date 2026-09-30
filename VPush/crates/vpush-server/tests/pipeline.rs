//! From an event on a relay to a push at the push service: the watcher and
//! the pipeline against real relays (in this process) and a push service
//! played by a mock.

#[path = "support/key.rs"]
mod key;

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nostr::prelude::*;
use nostr_sdk::local_relay::{LocalRelay, QueryPolicy, QueryPolicyResult};
use serde_json::{json, Value};
use tokio::sync::watch;
use vpush_proto::Prefs;
use vpush_server::config::Config;
use vpush_server::counters::Counters;
use vpush_server::delivery::fcm::{FcmClient, ServiceAccount};
use vpush_server::delivery::retry::RetryPolicy;
use vpush_server::delivery::Providers;
use vpush_server::pipeline::classify::mark_of;
use vpush_server::pipeline::{Dealt, Pipeline, Waking};
use vpush_server::relays::{normalize, RelayPolicy};
use vpush_server::auth::sha256_hex;
use vpush_server::store::{
    AllStore, DeviceInput, DeviceLimits, Seen, SqliteStore, Store, WatchStore, WatchedGroup,
    WatchedRelay,
};
use vpush_server::relay::{Tuning, Watch};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const APP: &str = "net.veydan.mobile";
const SEND: &str = "/v1/projects/veydan-test/messages:send";
const GROUP: &str = "1111111111111111111111111111111111111111111111111111111111111111";

struct Relay {
    relay: LocalRelay,
    url: String,
    asked: Asked,
}

/// How many times a relay was asked for direct messages. The server asks
/// for them in one request while the keys are few, so this is how many
/// times it made its requests anew.
#[derive(Debug, Clone, Default)]
struct Asked(Arc<AtomicUsize>);

impl QueryPolicy for Asked {
    fn admit_query<'a>(
        &'a self,
        query: &'a mut Filter,
        _addr: &'a SocketAddr,
    ) -> Pin<Box<dyn Future<Output = QueryPolicyResult> + Send + 'a>> {
        Box::pin(async move {
            if query.kinds.as_ref().is_some_and(|kinds| kinds.contains(&Kind::GiftWrap)) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            QueryPolicyResult::Accept
        })
    }
}

/// Left to itself, a local relay picks a random port by trying it, and a
/// test running next to this one may take that port a moment later. A port
/// the system hands out collides far less, and a relay that lost its port
/// anyway is replaced: it keeps the first port it was given.
async fn relay() -> Relay {
    let mut last = None;
    for _ in 0..5 {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .unwrap()
            .port();
        let asked = Asked::default();
        let relay = LocalRelay::builder().port(port).query_policy(asked.clone()).build();
        match relay.run().await {
            Ok(()) => {
                let url = normalize(relay.url().await.as_str()).unwrap();
                return Relay { relay, url, asked };
            }
            Err(e) => last = Some(e),
        }
    }
    panic!("no port for the local relay: {last:?}");
}

impl Relay {
    async fn publish(&self, event: &Event) {
        self.relay.add_event(event.clone()).await.unwrap();
    }

    fn asked_for_direct_messages(&self) -> usize {
        self.asked.0.load(Ordering::Relaxed)
    }

    /// The relay goes off the line, as when it is restarted.
    fn goes_away(&self) {
        self.relay.shutdown();
    }

    /// A relay at the same address again, with nothing stored.
    async fn comes_back(&self) -> Relay {
        let port: u16 = self.url.rsplit(':').next().unwrap().parse().unwrap();
        let mut last = None;
        for _ in 0..50 {
            let relay = LocalRelay::builder().port(port).build();
            match relay.run().await {
                Ok(()) => return Relay { relay, url: self.url.clone(), asked: Asked::default() },
                Err(e) => last = Some(e),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the relay did not come back on its port: {last:?}");
    }
}

/// The server, without its HTTP side: a store, a watcher and a pipeline.
struct Server {
    store: Arc<SqliteStore>,
    policy: Arc<RelayPolicy>,
    providers: Arc<Providers>,
    google: MockServer,
    waking: Waking,
    tuning: Tuning,
    counters: Arc<Counters>,
    running: Option<Running>,
}

struct Running {
    watch: Arc<Watch>,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

/// A chat is held back for `window`, and a device is not: the tests that
/// are not about the cap of a device push as often as they like.
fn waking(window: Duration) -> Waking {
    Waking {
        chat_every: window,
        device_cap: 10_000,
        device_window: Duration::from_secs(60),
    }
}

/// A change of what is watched is asked for at once, as the tests that are
/// not about the pace of asking expect.
fn at_once() -> Tuning {
    Tuning {
        ask_every: Duration::ZERO,
        ..Tuning::default()
    }
}

async fn server(relays: &[&Relay], window: Duration) -> Server {
    server_with(relays, waking(window), at_once()).await
}

async fn server_with(relays: &[&Relay], waking: Waking, tuning: Tuning) -> Server {
    let google = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-1", "expires_in": 3600
        })))
        .mount(&google)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "m/1" })))
        .mount(&google)
        .await;

    let allow: String = relays
        .iter()
        .map(|r| format!("[[relays.allow]]\nurl = \"{}\"\n", r.url))
        .collect();
    let config = Config::parse(&format!("public_url = \"http://localhost:1\"\n{allow}")).unwrap();

    let account = ServiceAccount::parse(
        &json!({
            "project_id": "veydan-test",
            "private_key": key::TEST_RSA_KEY,
            "client_email": "push@veydan-test.iam.gserviceaccount.com",
            "token_uri": format!("{}/token", google.uri()),
        })
        .to_string(),
    )
    .unwrap();
    let mut providers = Providers::default();
    providers.insert(APP, Arc::new(FcmClient::new(account, &google.uri()).unwrap()));

    let mut server = Server {
        store: Arc::new(SqliteStore::in_memory().await.unwrap()),
        policy: Arc::new(RelayPolicy::from_config(&config).unwrap()),
        providers: Arc::new(providers),
        google,
        waking,
        tuning,
        counters: Arc::new(Counters::default()),
        running: None,
    };
    server.start();
    server
}

impl Server {
    /// A pipeline over the server's store, pushing through its mock.
    fn pipeline(&self) -> Arc<Pipeline> {
        Pipeline::with_retry(
            self.store.clone(),
            self.providers.clone(),
            self.waking,
            self.counters.clone(),
            RetryPolicy {
                max_attempts: 2,
                base: Duration::from_millis(20),
                max_wait: Duration::from_secs(1),
            },
        )
    }

    fn start(&mut self) {
        let store: Arc<dyn AllStore> = self.store.clone();
        let watch = Watch::new();
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(vpush_server::relay::run(
            watch.clone(),
            store,
            self.policy.clone(),
            self.pipeline(),
            self.counters.clone(),
            self.tuning,
            stopped,
        ));
        self.running = Some(Running { watch, stop, task });
    }

    /// As when the server is taken down for a new release.
    async fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            let _ = running.stop.send(true);
            let _ = running.task.await;
        }
    }

    fn watch(&self) -> &Arc<Watch> {
        &self.running.as_ref().unwrap().watch
    }

    /// A phone that watches direct messages and the group on every relay given.
    async fn register(&self, owner: &Keys, device: &str, relays: &[&Relay]) -> DeviceInput {
        let input = phone(owner, device, relays);
        self.put(input.clone()).await;
        input
    }

    /// Writes a registration down, as the API does when it has taken one.
    async fn put(&self, input: DeviceInput) {
        let roomy = DeviceLimits { per_owner: 10, total: 1000 };
        self.store.put_device(input, roomy).await.unwrap();
        self.watch().plan_changed();
    }

    /// How many events the pushes stand for, all together.
    async fn told(&self) -> u64 {
        let count = |push: &Value| match push["data"]["count"].as_str() {
            Some(count) => count.parse().unwrap(),
            None => 1,
        };
        self.pushes().await.iter().map(count).sum()
    }

    /// Was stock taken of a key or a group on the relay.
    async fn knows(&self, relay: &Relay, target: &str) -> bool {
        let known = self.store.baselined(&relay.url).await.unwrap();
        known.iter().any(|(_, known)| known == target)
    }

    /// Waits until stock was taken of everything the device watches.
    async fn watching(&self, owner: &Keys, relays: &[&Relay]) {
        let owner = owner.public_key().to_hex();
        for relay in relays {
            eventually("the relay is watched", || async {
                self.knows(relay, &owner).await && self.knows(relay, GROUP).await
            })
            .await;
        }
    }

    async fn pushes(&self) -> Vec<Value> {
        self.google
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.url.path() == SEND)
            .map(|r| serde_json::from_slice::<Value>(&r.body).unwrap()["message"].clone())
            .collect()
    }

    async fn pushes_are(&self, count: usize) -> Vec<Value> {
        eventually(&format!("{count} pushes"), || async { self.pushes().await.len() >= count }).await;
        // A moment more, for a push that should not have come.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let pushes = self.pushes().await;
        assert_eq!(pushes.len(), count, "{pushes:#?}");
        pushes
    }
}

/// The registration of a phone that watches direct messages and the group
/// on every relay given.
fn phone(owner: &Keys, device: &str, relays: &[&Relay]) -> DeviceInput {
    DeviceInput {
        pubkey: owner.public_key().to_hex(),
        device_id: device.to_string(),
        app_id: APP.to_string(),
        provider: "fcm".to_string(),
        token: format!("token-of-{device}"),
        channel_json: None,
        app_version: None,
        prefs: Prefs::default(),
        author_key: Some(author_key(owner)),
        relays: relays
            .iter()
            .map(|r| WatchedRelay { url: r.url.clone(), dm: true, groups: true })
            .collect(),
        groups: vec![member_of(GROUP)],
        now: Timestamp::now().as_secs(),
        expires_at: Timestamp::now().as_secs() + 86_400,
        token_checked_at: None,
    }
}

async fn eventually<F, Fut>(what: &str, check: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(15);
    while !check().await {
        assert!(Instant::now() < deadline, "never happened: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The key of the marks, as the messenger makes it. Here any 32 bytes that
/// belong to the owner will do.
fn author_key(owner: &Keys) -> String {
    owner.public_key().to_hex().chars().rev().collect()
}

/// The push key of a group, as the messenger makes it from the key of the
/// group. Here any 32 bytes the members of the group share will do.
fn push_key(group: &str) -> String {
    sha256_hex(format!("the key of {group}").as_bytes())
}

/// The push key of the key the group had before its key was changed.
fn push_key_before(group: &str) -> String {
    sha256_hex(format!("the key of {group} before it was changed").as_bytes())
}

/// What a member of a group registers: the group, and the push keys held.
fn with_keys(group: &str, keys: &[String]) -> WatchedGroup {
    WatchedGroup { id: group.to_string(), keys: keys.to_vec() }
}

fn member_of(group: &str) -> WatchedGroup {
    with_keys(group, &[push_key(group)])
}

/// A sealed direct message, as a relay sees it: signed by a key made for
/// it, dated somewhere in the last two days.
fn wrap(to: &Keys, tags: &[&[&str]]) -> Event {
    wrap_with(to, "sealed", tags)
}

fn wrap_with(to: &Keys, content: &str, tags: &[&[&str]]) -> Event {
    wrap_dated(to, content, tags, Timestamp::now().as_secs() - 86_400)
}

/// A sealed direct message that says it was written at `at`.
fn wrap_dated(to: &Keys, content: &str, tags: &[&[&str]], at: u64) -> Event {
    let mut all = vec![Tag::public_key(to.public_key())];
    all.extend(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()));
    EventBuilder::new(Kind::GiftWrap, content)
        .tags(all)
        .custom_created_at(Timestamp::from_secs(at))
        .finalize(&Keys::generate())
        .unwrap()
}

/// The event a push carries, opened.
fn carried(data: &Value) -> Value {
    serde_json::from_str(data["event"].as_str().expect("the push carries the event")).unwrap()
}

/// An event of the group, written by `author`.
fn group_event(author: &Keys, tags: &[&[&str]]) -> Event {
    event_of(GROUP, author, tags)
}

/// An event of the group `group`, written by `author`, a member: it carries
/// the mark of the group's push key.
fn event_of(group: &str, author: &Keys, tags: &[&[&str]]) -> Event {
    marked(group, author, &[push_key(group)], tags)
}

/// An event of the group `group`, written by `author`, with the marks of
/// these push keys on it: what a holder of each of them would have put.
fn marked(group: &str, author: &Keys, push_keys: &[String], tags: &[&[&str]]) -> Event {
    let signer = Keys::generate();
    let signed_by = signer.public_key().to_hex();
    let mark = mark_of(&author_key(author), &signed_by).unwrap();
    let mut all = vec![
        Tag::parse(["h", group]).unwrap(),
        Tag::parse(["k", "key-1"]).unwrap(),
        Tag::parse(["vp", mark.as_str()]).unwrap(),
    ];
    for key in push_keys {
        let mark = mark_of(key, &signed_by).unwrap();
        all.push(Tag::parse(["gp", mark.as_str()]).unwrap());
    }
    all.extend(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()));
    EventBuilder::new(Kind::from(9u16), "sealed").tags(all).finalize(&signer).unwrap()
}

const SHORT: Duration = Duration::from_millis(100);

#[tokio::test]
async fn a_direct_message_becomes_a_push_that_carries_it() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    let message = wrap(&alice, &[]);
    r.publish(&message).await;

    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["token"], "token-of-phone");
    let data = &pushes[0]["data"];
    assert_eq!(data["v"], "2");
    assert_eq!(data["type"], "dm");
    assert_eq!(carried(data), serde_json::to_value(&message).unwrap());
    // The event is there, so where to find it is not; and no text: the
    // device writes its own.
    for absent in ["event_id", "relay", "count", "title", "body"] {
        assert!(data.get(absent).is_none(), "{absent}: {data}");
    }
    assert!(data["trace"].as_str().unwrap().len() >= 8);
    assert!(pushes[0].get("notification").is_none());
    assert_eq!(pushes[0]["android"]["priority"], "HIGH");

    let device = server.store.device(&alice.public_key().to_hex(), "phone").await.unwrap().unwrap();
    assert_eq!(device.last_outcome.as_deref(), Some("delivered"));
    assert_eq!(server.watch().status(&r.url), vpush_proto::RelayStatus::Ok);
}

#[tokio::test]
async fn a_message_too_big_for_a_push_is_named_and_left_on_the_relay() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    let message = wrap_with(&alice, &"x".repeat(5_000), &[]);
    r.publish(&message).await;

    let pushes = server.pushes_are(1).await;
    let data = &pushes[0]["data"];
    assert_eq!(data["type"], "dm");
    assert_eq!(data["event_id"], message.id.to_hex());
    assert_eq!(data["relay"], r.url);
    assert!(data.get("event").is_none(), "{data}");
}

#[tokio::test]
async fn what_was_on_the_relay_before_the_watch_began_is_not_pushed() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    for _ in 0..3 {
        r.publish(&wrap(&alice, &[])).await;
    }
    r.publish(&group_event(&Keys::generate(), &[])).await;

    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;
    server.pushes_are(0).await;

    let fresh = wrap(&alice, &[]);
    r.publish(&fresh).await;
    let pushes = server.pushes_are(1).await;
    assert_eq!(carried(&pushes[0]["data"])["id"], fresh.id.to_hex());
}

#[tokio::test]
async fn what_is_marked_as_not_worth_a_push_is_not_pushed() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    // A copy for the author's other devices, a signal, an operation of a group.
    r.publish(&wrap(&alice, &[&["silent", "1"]])).await;
    r.publish(&group_event(&Keys::generate(), &[&["silent", "1"]])).await;
    // And something for somebody who is not registered.
    r.publish(&wrap(&Keys::generate(), &[])).await;
    let message = wrap(&alice, &[]);
    r.publish(&message).await;

    let pushes = server.pushes_are(1).await;
    assert_eq!(carried(&pushes[0]["data"])["id"], message.id.to_hex());
}

#[tokio::test]
async fn one_event_on_two_relays_is_one_push() {
    let (a, b) = (relay().await, relay().await);
    let server = server(&[&a, &b], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&a, &b]).await;
    server.watching(&alice, &[&a, &b]).await;

    let message = wrap(&alice, &[]);
    a.publish(&message).await;
    b.publish(&message).await;

    server.pushes_are(1).await;
}

#[tokio::test]
async fn every_device_of_the_owner_is_told() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.register(&alice, "tablet", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    r.publish(&wrap(&alice, &[])).await;

    let mut tokens: Vec<_> = server
        .pushes_are(2)
        .await
        .iter()
        .map(|p| p["token"].as_str().unwrap().to_string())
        .collect();
    tokens.sort();
    assert_eq!(tokens, ["token-of-phone", "token-of-tablet"]);
}

/// The event carries both marks: the one of the group (`gp`), by which bob
/// and carol are told, and the one of the author (`vp`), by which alice,
/// who holds the key of the group as they do, is not.
#[tokio::test]
async fn a_group_message_is_told_to_everybody_but_its_author() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let (alice, bob, carol) = (Keys::generate(), Keys::generate(), Keys::generate());
    server.register(&alice, "alice-phone", &[&r]).await;
    server.register(&alice, "alice-tablet", &[&r]).await;
    server.register(&bob, "bob-phone", &[&r]).await;
    server.register(&carol, "carol-phone", &[&r]).await;
    for owner in [&alice, &bob, &carol] {
        server.watching(owner, &[&r]).await;
    }

    let message = group_event(&alice, &[]);
    r.publish(&message).await;

    let pushes = server.pushes_are(2).await;
    let told: std::collections::BTreeMap<_, _> = pushes
        .iter()
        .map(|p| (p["token"].as_str().unwrap(), p["data"].clone()))
        .collect();
    assert!(!told.contains_key("token-of-alice-phone"), "the author is not told of her own message");
    assert!(!told.contains_key("token-of-alice-tablet"), "on none of her devices");

    for token in ["token-of-bob-phone", "token-of-carol-phone"] {
        let data = &told[token];
        assert_eq!(data["type"], "group");
        assert_eq!(data["group_id"], GROUP);
        assert_eq!(carried(data), serde_json::to_value(&message).unwrap());
        assert!(data.get("title").is_none() && data.get("group_name").is_none(), "{data}");
    }
}

/// The tokens the pushes went to, sorted.
fn tokens(pushes: &[Value]) -> Vec<&str> {
    let mut tokens: Vec<_> = pushes.iter().map(|p| p["token"].as_str().unwrap()).collect();
    tokens.sort();
    tokens
}

/// The id of a group is on every one of its events, in the open: anybody
/// can sign an event that names it. Without the mark of the group such an
/// event wakes nobody, and is carried to nobody.
#[tokio::test]
async fn a_group_event_without_the_mark_of_the_group_is_told_to_nobody_and_counted() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let (bob, stranger) = (Keys::generate(), Keys::generate());
    server.register(&bob, "bob-phone", &[&r]).await;
    server.watching(&bob, &[&r]).await;

    // No mark at all, as an app older than the mark writes and as whoever
    // knows nothing of it does; and the mark of a key the stranger made up.
    let bare = marked(GROUP, &stranger, &[], &[]);
    let forged = marked(GROUP, &stranger, &[push_key("a guess")], &[]);
    r.publish(&bare).await;
    r.publish(&forged).await;
    eventually("both are counted", || async { server.counters.group_events_unmarked.get() >= 2 }).await;
    server.pushes_are(0).await;

    // They are noted, and not counted a second time when a relay gives
    // them again.
    let pipeline = server.pipeline();
    let now = Timestamp::now().as_secs();
    for event in [&bare, &forged] {
        assert!(!server.store.first_seen(&event.id.to_hex(), Seen::Pushed, now).await.unwrap());
        assert_eq!(pipeline.event(&r.url, event, false).await, Dealt::Unmarked);
    }
    assert_eq!(server.counters.group_events_unmarked.get(), 2);

    // A member writes, and bob is told.
    let message = event_of(GROUP, &Keys::generate(), &[]);
    r.publish(&message).await;
    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["token"], "token-of-bob-phone");
    assert_eq!(carried(&pushes[0]["data"]), serde_json::to_value(&message).unwrap());
    assert_eq!(server.counters.group_events_unmarked.get(), 2);
}

/// What stands in a `gp` tag and is no mark is passed over, and no more
/// than the first four tags are read.
#[tokio::test]
async fn what_is_no_mark_is_passed_over_and_the_fifth_tag_is_not_read() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let bob = Keys::generate();
    server.register(&bob, "bob-phone", &[&r]).await;
    let pipeline = server.pipeline();

    // An event of the group with the `gp` tags `tags` makes of the mark of
    // the group.
    let with = |tags: fn(String) -> Vec<String>| {
        let signer = Keys::generate();
        let mark = mark_of(&push_key(GROUP), &signer.public_key().to_hex()).unwrap();
        let mut all = vec![Tag::parse(["h", GROUP]).unwrap()];
        all.extend(tags(mark).iter().map(|value| Tag::parse(["gp", value.as_str()]).unwrap()));
        EventBuilder::new(Kind::from(9u16), "sealed").tags(all).finalize(&signer).unwrap()
    };
    fn no() -> String {
        "not-a-mark".to_string()
    }
    let told = Dealt::Pushed { now: 1, later: 0, own: 0, over: 0 };

    let shouted = with(|mark| vec![mark.to_uppercase()]);
    assert_eq!(pipeline.event(&r.url, &shouted, false).await, Dealt::Unmarked);
    let cut = with(|mark| vec![mark[..15].to_string()]);
    assert_eq!(pipeline.event(&r.url, &cut, false).await, Dealt::Unmarked);
    let fifth = with(|mark| vec![no(), no(), no(), no(), mark]);
    assert_eq!(pipeline.event(&r.url, &fifth, false).await, Dealt::Unmarked);
    assert_eq!(server.counters.group_events_unmarked.get(), 3);
    server.pushes_are(0).await;

    let fourth = with(|mark| vec![no(), no(), no(), mark]);
    assert_eq!(pipeline.event(&r.url, &fourth, false).await, told);
    server.pushes_are(1).await;
}

/// The key of a group was changed. For a week its events carry two marks:
/// of the key of now, and of the one before, for the phones that slept
/// through the change.
#[tokio::test]
async fn after_a_change_of_the_key_an_event_is_told_by_the_marks_it_carries() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let (awake, asleep) = (Keys::generate(), Keys::generate());
    let (now, before) = (push_key(GROUP), push_key_before(GROUP));

    // One phone registered after the change, with both keys. The other
    // has not been opened since, and the server has the key before only.
    let mut input = phone(&awake, "awake-phone", &[&r]);
    input.groups = vec![with_keys(GROUP, &[now.clone(), before.clone()])];
    server.put(input).await;
    let mut input = phone(&asleep, "asleep-phone", &[&r]);
    input.groups = vec![with_keys(GROUP, std::slice::from_ref(&before))];
    server.put(input).await;
    for owner in [&awake, &asleep] {
        server.watching(owner, &[&r]).await;
    }

    // Within the week: both marks, and both phones are told, each once.
    let both = marked(GROUP, &Keys::generate(), &[now.clone(), before], &[]);
    r.publish(&both).await;
    let pushes = server.pushes_are(2).await;
    assert_eq!(tokens(&pushes), ["token-of-asleep-phone", "token-of-awake-phone"]);

    // After it: the mark of the key of now only. The phone that never
    // registered that key is not told.
    let new_only = marked(GROUP, &Keys::generate(), &[now], &[]);
    r.publish(&new_only).await;
    let pushes = server.pushes_are(3).await;
    assert_eq!(pushes[2]["token"], "token-of-awake-phone");
    assert_eq!(carried(&pushes[2]["data"])["id"], new_only.id.to_hex());
    assert_eq!(server.counters.group_events_unmarked.get(), 0, "both events came from members");
}

/// Whoever knows the id of a group can register for it. Without the key of
/// the group they register a key of their own making, and are told nothing.
#[tokio::test]
async fn a_device_that_registered_a_wrong_key_for_a_group_is_told_nothing_of_it() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let (bob, mallory) = (Keys::generate(), Keys::generate());
    server.register(&bob, "bob-phone", &[&r]).await;
    let made_up = push_key("a guess");
    let mut input = phone(&mallory, "mallory-phone", &[&r]);
    input.groups = vec![with_keys(GROUP, std::slice::from_ref(&made_up))];
    server.put(input).await;
    for owner in [&bob, &mallory] {
        server.watching(owner, &[&r]).await;
    }

    let message = event_of(GROUP, &Keys::generate(), &[]);
    r.publish(&message).await;
    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["token"], "token-of-bob-phone");

    // And what somebody marks with the made-up key reaches the one who
    // made it up, and no member.
    r.publish(&marked(GROUP, &Keys::generate(), std::slice::from_ref(&made_up), &[])).await;
    let pushes = server.pushes_are(2).await;
    assert_eq!(pushes[1]["token"], "token-of-mallory-phone");
}

/// A device registered by 0.2.2 has its groups and no key of any: it waits
/// for the app to register anew.
#[tokio::test]
async fn a_group_nobody_registered_a_key_for_is_for_nobody() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let bob = Keys::generate();
    let mut input = phone(&bob, "bob-phone", &[&r]);
    input.groups = vec![with_keys(GROUP, &[])];
    server.put(input).await;
    let pipeline = server.pipeline();

    let message = event_of(GROUP, &Keys::generate(), &[]);
    assert_eq!(pipeline.event(&r.url, &message, false).await, Dealt::Nobody);
    assert_eq!(
        server.counters.group_events_unmarked.get(),
        0,
        "there is no key to hold the mark against: the event is not called a stranger's"
    );
    // Direct messages are pushed to such a device as before.
    let dealt = pipeline.event(&r.url, &wrap(&bob, &[]), false).await;
    assert_eq!(dealt, Dealt::Pushed { now: 1, later: 0, own: 0, over: 0 });
    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["data"]["type"], "dm");

    // The app registers anew, with the key, and is told of the group again.
    server.register(&bob, "bob-phone", &[&r]).await;
    let next = event_of(GROUP, &Keys::generate(), &[]);
    assert_eq!(
        pipeline.event(&r.url, &next, false).await,
        Dealt::Pushed { now: 1, later: 0, own: 0, over: 0 }
    );
    server.pushes_are(2).await;
}

/// Anybody can register keys of their own making for a group they know
/// the id of. An event is held against sixty-four keys and no more, and
/// the key the members hold must not be the one that is left out.
#[tokio::test]
async fn keys_made_up_for_a_group_do_not_crowd_out_the_key_of_its_members() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    // Two members. Their key is the last of all by its letters.
    let members_key = "ff".repeat(32);
    for n in 0..2 {
        let mut input = phone(&Keys::generate(), &format!("member-{n}"), &[&r]);
        input.groups = vec![with_keys(GROUP, std::slice::from_ref(&members_key))];
        server.put(input).await;
    }
    // Seventy strangers, each with a key of its own making.
    let made_up = |n: usize| format!("{n:064x}");
    for n in 0..70 {
        let mut input = phone(&Keys::generate(), &format!("stranger-{n}"), &[&r]);
        input.groups = vec![with_keys(GROUP, &[made_up(n)])];
        server.put(input).await;
    }
    let pipeline = server.pipeline();

    let message = marked(GROUP, &Keys::generate(), std::slice::from_ref(&members_key), &[]);
    assert_eq!(
        pipeline.event(&r.url, &message, false).await,
        Dealt::Pushed { now: 2, later: 0, own: 0, over: 0 }
    );
    // Of the made-up keys, each held by one device, the first sixty-three
    // by their letters are tried, and the rest are not.
    let tried = marked(GROUP, &Keys::generate(), &[made_up(62)], &[]);
    assert_eq!(
        pipeline.event(&r.url, &tried, false).await,
        Dealt::Pushed { now: 1, later: 0, own: 0, over: 0 }
    );
    let left_out = marked(GROUP, &Keys::generate(), &[made_up(63)], &[]);
    assert_eq!(pipeline.event(&r.url, &left_out, false).await, Dealt::Unmarked);
    server.pushes_are(3).await;
}

#[tokio::test]
async fn a_burst_wakes_the_phone_twice_and_the_second_push_counts_the_rest() {
    let r = relay().await;
    let server = server(&[&r], Duration::from_secs(2)).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    let started = Instant::now();
    let mut ids = Vec::new();
    for _ in 0..6 {
        let message = wrap(&alice, &[]);
        ids.push(message.id.to_hex());
        r.publish(&message).await;
    }
    // Another chat is not held back by this one.
    r.publish(&group_event(&Keys::generate(), &[])).await;

    eventually("the pushes that go at once", || async { server.pushes().await.len() >= 2 }).await;
    assert!(started.elapsed() < Duration::from_secs(2), "the first push does not wait");
    let first = server.pushes().await;
    assert_eq!(first.len(), 2, "{first:#?}");

    let pushes = server.pushes_are(3).await;
    assert!(started.elapsed() >= Duration::from_secs(2));
    let at_once = pushes[..2].iter().map(|p| &p["data"]).find(|d| d["type"] == "dm").unwrap();
    let last = &pushes[2]["data"];
    assert_eq!(last["type"], "dm");
    assert_eq!(last["count"], "5");
    // Five messages cannot be shown from one event: the push names one of
    // the counted, and the phone takes them from the relay.
    assert!(last.get("event").is_none(), "{last}");
    assert_eq!(last["relay"], r.url);
    let named = last["event_id"].as_str().unwrap();
    assert!(ids.iter().any(|id| id == named), "{named} is one of the messages");
    assert_ne!(carried(at_once)["id"], named, "and not the one pushed at once");
}

#[tokio::test]
async fn what_came_while_the_server_was_away_is_pushed_once_when_it_is_back() {
    let r = relay().await;
    let mut server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    let before = wrap(&alice, &[]);
    r.publish(&before).await;
    server.pushes_are(1).await;

    server.stop().await;
    let missed = wrap(&alice, &[]);
    r.publish(&missed).await;
    let missed_in_group = group_event(&Keys::generate(), &[]);
    r.publish(&missed_in_group).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.start();

    let pushes = server.pushes_are(3).await;
    let mut ids: Vec<_> = pushes.iter().map(|p| carried(&p["data"])["id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    let mut expected = vec![before.id.to_hex(), missed.id.to_hex(), missed_in_group.id.to_hex()];
    expected.sort();
    assert_eq!(ids, expected, "what was pushed before is not pushed again");
}

#[tokio::test]
async fn a_device_that_leaves_takes_nothing_from_the_one_that_stays() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    let owner = alice.public_key().to_hex();
    server.register(&alice, "phone", &[&r]).await;
    server.register(&alice, "tablet", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    assert!(server.store.delete_device(&owner, "tablet").await.unwrap());
    server.watch().plan_changed();
    tokio::time::sleep(Duration::from_millis(1500)).await;

    r.publish(&wrap(&alice, &[])).await;
    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["token"], "token-of-phone");

    // The last device leaves: nothing is watched, and nothing is pushed.
    assert!(server.store.delete_device(&owner, "phone").await.unwrap());
    server.watch().plan_changed();
    eventually("the relay is let go", || async { server.watch().health().is_empty() }).await;
    r.publish(&wrap(&alice, &[])).await;
    server.pushes_are(1).await;
}

#[tokio::test]
async fn what_a_relay_took_while_nobody_watched_it_is_old_when_the_watch_begins_again() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    let owner = alice.public_key().to_hex();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    // Pushes are turned off on the only phone that named this relay.
    assert!(server.store.delete_device(&owner, "phone").await.unwrap());
    server.watch().plan_changed();
    eventually("the stock taken on the relay is forgotten", || async {
        server.store.baselined(&r.url).await.unwrap().is_empty()
    })
    .await;
    let meanwhile = wrap(&alice, &[]);
    r.publish(&meanwhile).await;

    // And on again.
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;
    let fresh = wrap(&alice, &[]);
    r.publish(&fresh).await;

    let pushes = server.pushes_are(1).await;
    assert_eq!(carried(&pushes[0]["data"])["id"], fresh.id.to_hex());
}

#[tokio::test]
async fn what_came_for_a_key_while_it_was_not_watched_is_old_when_it_is_watched_again() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let (alice, bob) = (Keys::generate(), Keys::generate());
    let owner = alice.public_key().to_hex();
    server.register(&alice, "alice-phone", &[&r]).await;
    server.register(&bob, "bob-phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;
    server.watching(&bob, &[&r]).await;

    // Alice leaves; the relay stays on the line for bob.
    assert!(server.store.delete_device(&owner, "alice-phone").await.unwrap());
    server.watch().plan_changed();
    eventually("the stock taken of alice is forgotten", || async {
        !server.knows(&r, &owner).await
    })
    .await;
    assert!(server.knows(&r, &bob.public_key().to_hex()).await, "bob is known as before");
    let meanwhile = wrap(&alice, &[]);
    r.publish(&meanwhile).await;
    // Something for bob after it: when the push about that is out, the
    // server has dealt with whatever the relay sent before, and alice was
    // nobody's to tell at the time.
    let to_bob = group_event(&Keys::generate(), &[]);
    r.publish(&to_bob).await;
    server.pushes_are(1).await;

    server.register(&alice, "alice-phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;
    let fresh = wrap(&alice, &[]);
    r.publish(&fresh).await;

    let pushes = server.pushes_are(2).await;
    assert_eq!(pushes[0]["token"], "token-of-bob-phone");
    assert_eq!(carried(&pushes[0]["data"])["id"], to_bob.id.to_hex());
    assert_eq!(pushes[1]["token"], "token-of-alice-phone");
    assert_eq!(carried(&pushes[1]["data"])["id"], fresh.id.to_hex());
}

#[tokio::test]
async fn an_event_dated_far_from_now_is_noted_and_not_pushed() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    // Straight to the pipeline, as from a relay that does not look at the
    // date it was asked from: an honest one keeps most of these back.
    let pipeline = server.pipeline();
    let now = Timestamp::now().as_secs();
    let dated = |at: u64| wrap_dated(&alice, "sealed", &[], at);

    let (ahead, stale) = (dated(now + 16 * 60), dated(now - 3 * 86_400 - 60));
    for event in [&ahead, &stale] {
        assert_eq!(pipeline.event(&r.url, event, false).await, Dealt::Misdated);
        let id = event.id.to_hex();
        assert!(!server.store.first_seen(&id, Seen::Pushed, now).await.unwrap(), "it was noted");
    }

    // A clock a few minutes ahead, and a sender who dated a sealed message
    // as far back as senders do.
    let (soon, backdated) = (dated(now + 10 * 60), dated(now - 2 * 86_400));
    for event in [&soon, &backdated] {
        let dealt = pipeline.event(&r.url, event, false).await;
        assert!(matches!(dealt, Dealt::Pushed { .. }), "{dealt:?}");
    }

    let pushes = server.pushes_are(2).await;
    let mut ids: Vec<_> = pushes
        .iter()
        .map(|p| carried(&p["data"])["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    let mut expected = vec![soon.id.to_hex(), backdated.id.to_hex()];
    expected.sort();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn a_relay_that_is_not_on_the_list_is_not_connected_to() {
    let (listed, other) = (relay().await, relay().await);
    let server = server(&[&listed], SHORT).await;
    let alice = Keys::generate();
    // The registration API would have left `other` out; the store is
    // written to directly here, as if it had not.
    server.register(&alice, "phone", &[&listed, &other]).await;
    server.watching(&alice, &[&listed]).await;

    let urls: Vec<_> = server.watch().health().into_iter().map(|h| h.url).collect();
    assert_eq!(urls, std::slice::from_ref(&listed.url));

    other.publish(&wrap(&alice, &[])).await;
    server.pushes_are(0).await;
}

#[tokio::test]
async fn who_registers_while_a_relay_is_away_is_watched_when_it_is_back() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    // The relay is away when somebody new registers: the request for their
    // key waits for it.
    r.goes_away();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let bob = Keys::generate();
    server.register(&bob, "bob-phone", &[&r]).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Back on the line, it is asked, not within the hour but at once. The
    // wait is the library's own before it tries the relay again. (A relay
    // that failed for so long that the library refuses requests for it is
    // not played here: that takes minutes. `Line::unsent` is for it.)
    let r = r.comes_back().await;
    let bob_key = bob.public_key().to_hex();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !server.knows(&r, &bob_key).await {
        assert!(Instant::now() < deadline, "the relay was never asked about the new key");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    r.publish(&wrap(&bob, &[])).await;
    server.pushes_are(1).await;
}

/// Every change of what is watched makes the relay send two days of direct
/// messages again, and whoever registers a device can cause one.
#[tokio::test]
async fn a_burst_of_registrations_makes_a_relay_be_asked_twice_and_no_more() {
    let r = relay().await;
    let tuning = Tuning {
        ask_every: Duration::from_secs(4),
        ..Tuning::default()
    };
    let server = server_with(&[&r], waking(SHORT), tuning).await;
    let first = Keys::generate();
    server.register(&first, "phone-0", &[&r]).await;
    server.watching(&first, &[&r]).await;
    assert_eq!(r.asked_for_direct_messages(), 1);

    // Registrations for longer than a second. The watcher reads them
    // every second, and what is watched on the relay is another thing
    // every time it does.
    let mut owners = Vec::new();
    for n in 1..=8 {
        let owner = Keys::generate();
        server.register(&owner, &format!("phone-{n}"), &[&r]).await;
        owners.push(owner);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    for owner in &owners {
        server.watching(owner, &[&r]).await;
    }
    assert_eq!(
        r.asked_for_direct_messages(),
        2,
        "once for the first, at once, and once for all who came after"
    );

    // Whoever came last is watched like the rest.
    let message = wrap(owners.last().unwrap(), &[]);
    r.publish(&message).await;
    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["token"], "token-of-phone-8");
    assert_eq!(r.asked_for_direct_messages(), 2);
}

#[tokio::test]
async fn a_device_is_pushed_to_thirty_times_and_told_of_the_rest_in_one_push() {
    let r = relay().await;
    let cap = Waking {
        chat_every: SHORT,
        device_cap: 30,
        device_window: Duration::from_secs(2),
    };
    let server = server_with(&[&r], cap, at_once()).await;
    let alice = Keys::generate();
    // Forty chats of one phone.
    let groups: Vec<String> = (1..=40).map(|n| format!("{n:064x}")).collect();
    let mut input = phone(&alice, "phone", &[&r]);
    input.groups = groups.iter().map(|group| member_of(group)).collect();
    server.put(input).await;

    // Straight to the pipeline: a message in every one of them, at once.
    let pipeline = server.pipeline();
    let mut pushed = Vec::new();
    for group in &groups {
        let event = event_of(group, &Keys::generate(), &[]);
        pushed.push(pipeline.event(&r.url, &event, false).await);
    }
    let at_once = Dealt::Pushed { now: 1, later: 0, own: 0, over: 0 };
    let counted = Dealt::Pushed { now: 0, later: 0, own: 0, over: 1 };
    assert!(pushed[..30].iter().all(|dealt| *dealt == at_once), "{pushed:?}");
    assert!(pushed[30..].iter().all(|dealt| *dealt == counted), "{pushed:?}");

    let pushes = server.pushes_are(30).await;
    assert!(pushes.iter().all(|p| p["data"]["type"] == "group"), "{pushes:#?}");
    assert_eq!(server.counters.sync_pushes.get(), 0, "the rest is told when the time is up");

    // The ten that came over the cap: one push, which says how many and
    // nothing of any one of them.
    let pushes = server.pushes_are(31).await;
    let sync = &pushes[30];
    assert_eq!(sync["token"], "token-of-phone");
    let data = sync["data"].as_object().unwrap();
    assert_eq!(data["type"], "sync");
    assert_eq!(data["count"], "10");
    assert_eq!(data["v"], "2");
    let mut keys: Vec<_> = data.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["count", "trace", "type", "v"], "no event, no id of one, no group");
    assert_eq!(sync["android"]["priority"], "HIGH");
    assert_eq!(server.counters.sync_pushes.get(), 1);

    // Every one of the forty was told: thirty by itself, ten by the count.
    assert_eq!(server.told().await, 40);
}

/// Sixty messages for a line that has room for eight.
#[tokio::test]
async fn a_line_with_more_to_deal_with_than_it_has_room_for_starts_over_and_loses_nothing() {
    let r = relay().await;
    let tuning = Tuning {
        queue: 8,
        ask_every: Duration::from_millis(100),
        ..Tuning::default()
    };
    let mut server = server_with(&[&r], waking(SHORT), tuning).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    // While the server is away. When it is back the relay sends them all
    // at once.
    server.stop().await;
    for _ in 0..60 {
        r.publish(&wrap(&alice, &[])).await;
    }
    server.start();

    eventually("every message is told", || async { server.told().await >= 60 }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(server.told().await, 60, "every one of them, and none of them twice");
    assert!(
        server.counters.lines_started_over.get() >= 1,
        "sixty do not fit where there is room for eight"
    );

    // The line that started over watches as before.
    let after = wrap(&alice, &[]);
    r.publish(&after).await;
    eventually("what comes after is told", || async { server.told().await >= 61 }).await;
    assert_eq!(server.watch().status(&r.url), vpush_proto::RelayStatus::Ok);
}

/// nostr-sdk hands on what a relay says through a channel that drops the
/// oldest when it is full, and says nothing. Here it has eight places, and
/// the relay sends sixty messages at once.
#[tokio::test]
async fn a_burst_bigger_than_the_channel_of_nostr_sdk_loses_nothing() {
    let r = relay().await;
    let tuning = Tuning {
        notifications: 8,
        ..at_once()
    };
    let mut server = server_with(&[&r], waking(SHORT), tuning).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r]).await;
    server.watching(&alice, &[&r]).await;

    server.stop().await;
    for _ in 0..60 {
        r.publish(&wrap(&alice, &[])).await;
    }
    server.start();

    eventually("every message is told", || async { server.told().await >= 60 }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(server.told().await, 60);
    assert_eq!(
        server.counters.lines_started_over.get(),
        0,
        "the queue had room for all of them: nothing was lost, and nothing was asked for again"
    );
}
