//! From an event on a relay to a push at the push service: the watcher and
//! the pipeline against real relays (in this process) and a push service
//! played by a mock.

#[path = "support/key.rs"]
mod key;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nostr::prelude::*;
use nostr_sdk::local_relay::LocalRelay;
use serde_json::{json, Value};
use tokio::sync::watch;
use vpush_proto::{GroupWatch, Prefs};
use vpush_server::config::Config;
use vpush_server::delivery::fcm::{FcmClient, ServiceAccount};
use vpush_server::delivery::retry::RetryPolicy;
use vpush_server::delivery::Providers;
use vpush_server::pipeline::classify::mark_of;
use vpush_server::pipeline::Pipeline;
use vpush_server::relays::{normalize, RelayPolicy};
use vpush_server::store::{AllStore, DeviceInput, SqliteStore, Store, WatchStore, WatchedRelay};
use vpush_server::relay::Watch;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const APP: &str = "net.veydan.mobile";
const SEND: &str = "/v1/projects/veydan-test/messages:send";
const GROUP: &str = "1111111111111111111111111111111111111111111111111111111111111111";

struct Relay {
    relay: LocalRelay,
    url: String,
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
        let relay = LocalRelay::builder().port(port).build();
        match relay.run().await {
            Ok(()) => {
                let url = normalize(relay.url().await.as_str()).unwrap();
                return Relay { relay, url };
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
}

/// The server, without its HTTP side: a store, a watcher and a pipeline.
struct Server {
    store: Arc<SqliteStore>,
    policy: Arc<RelayPolicy>,
    providers: Arc<Providers>,
    google: MockServer,
    window: Duration,
    running: Option<Running>,
}

struct Running {
    watch: Arc<Watch>,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

async fn server(relays: &[&Relay], window: Duration) -> Server {
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
        window,
        running: None,
    };
    server.start();
    server
}

impl Server {
    fn start(&mut self) {
        let store: Arc<dyn AllStore> = self.store.clone();
        let pipeline = Pipeline::with_retry(
            store.clone(),
            self.providers.clone(),
            self.window,
            RetryPolicy {
                max_attempts: 2,
                base: Duration::from_millis(20),
                max_wait: Duration::from_secs(1),
            },
        );
        let watch = Watch::new();
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(vpush_server::relay::run(
            watch.clone(),
            store,
            self.policy.clone(),
            pipeline,
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
    async fn register(&self, owner: &Keys, device: &str, relays: &[&Relay], group_name: Option<&str>) -> DeviceInput {
        let input = DeviceInput {
            pubkey: owner.public_key().to_hex(),
            device_id: device.to_string(),
            app_id: APP.to_string(),
            provider: "fcm".to_string(),
            token: format!("token-of-{device}"),
            channel_json: None,
            locale: "ru".to_string(),
            app_version: None,
            prefs: Prefs::default(),
            author_key: Some(author_key(owner)),
            relays: relays
                .iter()
                .map(|r| WatchedRelay { url: r.url.clone(), dm: true, groups: true })
                .collect(),
            groups: vec![GroupWatch { id: GROUP.to_string(), name: group_name.map(str::to_string) }],
            now: Timestamp::now().as_secs(),
            expires_at: Timestamp::now().as_secs() + 86_400,
        };
        self.store.put_device(input.clone(), 10).await.unwrap();
        self.watch().plan_changed();
        input
    }

    /// Waits until stock was taken of everything the device watches.
    async fn watching(&self, owner: &Keys, relays: &[&Relay]) {
        let owner = owner.public_key().to_hex();
        for relay in relays {
            eventually("the relay is watched", || async {
                let known = self.store.baselined(&relay.url).await.unwrap();
                known.iter().any(|(_, target)| target == &owner)
                    && known.iter().any(|(_, target)| target == GROUP)
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

/// A sealed direct message, as a relay sees it: signed by a key made for
/// it, dated somewhere in the last two days.
fn wrap(to: &Keys, tags: &[&[&str]]) -> Event {
    let mut all = vec![Tag::public_key(to.public_key())];
    all.extend(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()));
    EventBuilder::new(Kind::GiftWrap, "sealed")
        .tags(all)
        .custom_created_at(Timestamp::from_secs(Timestamp::now().as_secs() - 86_400))
        .finalize(&Keys::generate())
        .unwrap()
}

/// An event of the group, written by `author`.
fn group_event(author: &Keys, tags: &[&[&str]]) -> Event {
    let signer = Keys::generate();
    let mark = mark_of(&author_key(author), &signer.public_key().to_hex()).unwrap();
    let mut all = vec![
        Tag::parse(["h", GROUP]).unwrap(),
        Tag::parse(["k", "key-1"]).unwrap(),
        Tag::parse(["vp", mark.as_str()]).unwrap(),
    ];
    all.extend(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()));
    EventBuilder::new(Kind::from(9u16), "sealed").tags(all).finalize(&signer).unwrap()
}

const SHORT: Duration = Duration::from_millis(100);

#[tokio::test]
async fn a_direct_message_becomes_a_push_in_the_language_of_the_device() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r], None).await;
    server.watching(&alice, &[&r]).await;

    let message = wrap(&alice, &[]);
    r.publish(&message).await;

    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["token"], "token-of-phone");
    let data = &pushes[0]["data"];
    assert_eq!(data["type"], "dm");
    assert_eq!(data["event_id"], message.id.to_hex());
    assert_eq!(data["relay"], r.url);
    assert_eq!(data["title"], "Личное сообщение");
    assert_eq!(data["body"], "Новое сообщение");
    assert!(data.get("count").is_none());
    assert!(data["trace"].as_str().unwrap().len() >= 8);
    assert!(pushes[0].get("notification").is_none());
    assert_eq!(pushes[0]["android"]["priority"], "HIGH");

    let device = server.store.device(&alice.public_key().to_hex(), "phone").await.unwrap().unwrap();
    assert_eq!(device.last_outcome.as_deref(), Some("delivered"));
    assert_eq!(server.watch().status(&r.url), vpush_proto::RelayStatus::Ok);
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

    server.register(&alice, "phone", &[&r], None).await;
    server.watching(&alice, &[&r]).await;
    server.pushes_are(0).await;

    let fresh = wrap(&alice, &[]);
    r.publish(&fresh).await;
    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["data"]["event_id"], fresh.id.to_hex());
}

#[tokio::test]
async fn what_is_marked_as_not_worth_a_push_is_not_pushed() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r], None).await;
    server.watching(&alice, &[&r]).await;

    // A copy for the author's other devices, a signal, an operation of a group.
    r.publish(&wrap(&alice, &[&["silent", "1"]])).await;
    r.publish(&group_event(&Keys::generate(), &[&["silent", "1"]])).await;
    // And something for somebody who is not registered.
    r.publish(&wrap(&Keys::generate(), &[])).await;
    let message = wrap(&alice, &[]);
    r.publish(&message).await;

    let pushes = server.pushes_are(1).await;
    assert_eq!(pushes[0]["data"]["event_id"], message.id.to_hex());
}

#[tokio::test]
async fn one_event_on_two_relays_is_one_push() {
    let (a, b) = (relay().await, relay().await);
    let server = server(&[&a, &b], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&a, &b], None).await;
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
    server.register(&alice, "phone", &[&r], None).await;
    server.register(&alice, "tablet", &[&r], None).await;
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

#[tokio::test]
async fn a_group_message_is_told_to_everybody_but_its_author_by_the_name_each_gave_the_group() {
    let r = relay().await;
    let server = server(&[&r], SHORT).await;
    let (alice, bob, carol) = (Keys::generate(), Keys::generate(), Keys::generate());
    server.register(&alice, "alice-phone", &[&r], Some("Работа")).await;
    server.register(&alice, "alice-tablet", &[&r], Some("Работа")).await;
    server.register(&bob, "bob-phone", &[&r], Some("Коллеги")).await;
    server.register(&carol, "carol-phone", &[&r], None).await;
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

    let bob = &told["token-of-bob-phone"];
    assert_eq!(bob["type"], "group");
    assert_eq!(bob["group_id"], GROUP);
    assert_eq!(bob["group_name"], "Коллеги");
    assert_eq!(bob["title"], "Группа: Коллеги");
    assert_eq!(bob["body"], "Новое сообщение");
    assert_eq!(bob["event_id"], message.id.to_hex());

    let carol = &told["token-of-carol-phone"];
    assert_eq!(carol["title"], "Группа");
    assert!(carol.get("group_name").is_none());
}

#[tokio::test]
async fn a_burst_wakes_the_phone_twice_and_the_second_push_counts_the_rest() {
    let r = relay().await;
    let server = server(&[&r], Duration::from_secs(2)).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r], Some("Работа")).await;
    server.watching(&alice, &[&r]).await;

    let started = Instant::now();
    for _ in 0..6 {
        r.publish(&wrap(&alice, &[])).await;
    }
    // Another chat is not held back by this one.
    r.publish(&group_event(&Keys::generate(), &[])).await;

    eventually("the pushes that go at once", || async { server.pushes().await.len() >= 2 }).await;
    assert!(started.elapsed() < Duration::from_secs(2), "the first push does not wait");
    let first = server.pushes().await;
    assert_eq!(first.len(), 2, "{first:#?}");

    let pushes = server.pushes_are(3).await;
    assert!(started.elapsed() >= Duration::from_secs(2));
    let last = &pushes[2]["data"];
    assert_eq!(last["type"], "dm");
    assert_eq!(last["count"], "5");
    assert_eq!(last["body"], "Новых сообщений: 5");
}

#[tokio::test]
async fn what_came_while_the_server_was_away_is_pushed_once_when_it_is_back() {
    let r = relay().await;
    let mut server = server(&[&r], SHORT).await;
    let alice = Keys::generate();
    server.register(&alice, "phone", &[&r], None).await;
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
    let mut ids: Vec<_> = pushes.iter().map(|p| p["data"]["event_id"].as_str().unwrap().to_string()).collect();
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
    server.register(&alice, "phone", &[&r], None).await;
    server.register(&alice, "tablet", &[&r], None).await;
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
async fn a_relay_that_is_not_on_the_list_is_not_connected_to() {
    let (listed, other) = (relay().await, relay().await);
    let server = server(&[&listed], SHORT).await;
    let alice = Keys::generate();
    // The registration API would have left `other` out; the store is
    // written to directly here, as if it had not.
    server.register(&alice, "phone", &[&listed, &other], None).await;
    server.watching(&alice, &[&listed]).await;

    let urls: Vec<_> = server.watch().health().into_iter().map(|h| h.url).collect();
    assert_eq!(urls, std::slice::from_ref(&listed.url));

    other.publish(&wrap(&alice, &[])).await;
    server.pushes_are(0).await;
}
