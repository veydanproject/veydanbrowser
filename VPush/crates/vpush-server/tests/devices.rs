//! The registration API, asked over HTTP by clients that sign as real ones do.

#[path = "support/key.rs"]
mod key;

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use nostr::prelude::*;
use serde_json::{json, Value};
use vpush_server::api::{self, Api};
use vpush_server::auth::sha256_hex;
use vpush_server::config::Config;
use vpush_server::counters::Counters;
use vpush_server::delivery::fcm::{FcmClient, ServiceAccount};
use vpush_server::delivery::Providers;
use vpush_server::relays::RelayPolicy;
use vpush_server::store::{SqliteStore, Store, WatchedGroup};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const APP: &str = "net.veydan.mobile";
const SEND: &str = "/v1/projects/veydan-test/messages:send";

struct Server {
    base: String,
    /// The address clients sign for.
    public: String,
    http: reqwest::Client,
    store: Arc<SqliteStore>,
    counters: Arc<Counters>,
    google: MockServer,
}

/// What marks the request that asks FCM about a token and sends nothing.
const VALIDATE_ONLY: &str = r#""validate_only":true"#;

async fn start() -> Server {
    start_with("").await
}

async fn start_with(more_config: &str) -> Server {
    let google = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-1", "expires_in": 3600
        })))
        .mount(&google)
        .await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let public = format!("http://localhost:{port}");

    let config = Config::parse(&format!(
        r#"
        public_url = "{public}"
        [apps."{APP}".fcm]
        service_account = "/nowhere/fcm.json"
        [apps."app.without.push"]
        [[relays.allow]]
        url = "wss://node-1.veydan.net"
        [[relays.allow]]
        url = "wss://nos.lol"
        {more_config}
        "#
    ))
    .unwrap();

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

    let store = Arc::new(SqliteStore::in_memory().await.unwrap());
    let relays = RelayPolicy::from_config(&config).unwrap();
    let counters = Arc::new(Counters::default());
    let api = Api::new(
        Arc::new(config),
        store.clone() as Arc<dyn Store>,
        Arc::new(providers),
        Arc::new(relays),
        vpush_server::relay::Watch::new(),
        counters.clone(),
    );
    tokio::spawn(async move {
        axum::serve(listener, api::router(api)).await.unwrap();
    });

    Server {
        base: format!("http://127.0.0.1:{port}"),
        public,
        http: reqwest::Client::new(),
        store,
        counters,
        google,
    }
}

fn now() -> u64 {
    Timestamp::now().as_secs()
}

/// The header a client sends: an event signed for this very request.
///
/// The `nonce` makes two requests of one second two events. Without it the
/// second of them is the first one used again, and is refused as such.
fn signed(keys: &Keys, method: &str, url: &str, body: &[u8], at: u64) -> String {
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tags = vec![
        Tag::parse(["u", url]).unwrap(),
        Tag::parse(["method", method]).unwrap(),
        Tag::parse(["nonce", nonce.to_string().as_str()]).unwrap(),
    ];
    if !body.is_empty() {
        tags.push(Tag::parse(["payload", sha256_hex(body).as_str()]).unwrap());
    }
    let event = EventBuilder::new(Kind::HttpAuth, "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(at))
        .finalize(keys)
        .unwrap();
    format!("Nostr {}", STANDARD.encode(serde_json::to_vec(&event).unwrap()))
}

struct Answer {
    status: u16,
    body: Value,
    retry_after: Option<String>,
    request_id: Option<String>,
}

impl Server {
    async fn send(&self, method: &str, path: &str, header: Option<String>, body: Vec<u8>) -> Answer {
        self.send_with(method, path, header, body, &[]).await
    }

    /// `more`: headers a proxy in front, or somebody who plays one, adds.
    async fn send_with(
        &self,
        method: &str,
        path: &str,
        header: Option<String>,
        body: Vec<u8>,
        more: &[(&str, &str)],
    ) -> Answer {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(h) = header {
            request = request.header("authorization", h);
        }
        for (name, value) in more {
            request = request.header(*name, *value);
        }
        if !body.is_empty() {
            request = request.header("content-type", "application/json").body(body);
        }
        let response = request.send().await.unwrap();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .map(|v| v.to_str().unwrap().to_string())
        };
        let (retry_after, request_id) = (header("retry-after"), header("x-request-id"));
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        Answer {
            status,
            body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
            retry_after,
            request_id,
        }
    }

    /// A request signed the way a client signs it.
    async fn ask(&self, keys: &Keys, method: &str, path: &str, body: Option<Value>) -> Answer {
        let body = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
        let header = signed(keys, method, &format!("{}{path}", self.public), &body, now());
        self.send(method, path, Some(header), body).await
    }

    /// A registration as a client behind the proxy makes it: the proxy
    /// says whom it came from.
    async fn register_from(&self, from: &str, keys: &Keys, path: &str, body: Value) -> Answer {
        let body = body.to_string().into_bytes();
        let header = signed(keys, "PUT", &format!("{}{path}", self.public), &body, now());
        self.send_with("PUT", path, Some(header), body, &[("x-real-ip", from)]).await
    }

    async fn fcm_answers(&self, answer: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(SEND))
            .respond_with(answer)
            .mount(&self.google)
            .await;
    }

    /// What FCM says when it is asked about a token. Mounted before
    /// `fcm_answers`, it leaves the pushes to that one.
    async fn fcm_says_of_a_token(&self, answer: ResponseTemplate) {
        self.fcm_says_of_a_token_times(u64::MAX, answer).await;
    }

    /// The same, for the first `times` questions; the answer mounted after
    /// this one is for the rest.
    async fn fcm_says_of_a_token_times(&self, times: u64, answer: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(SEND))
            .and(body_string_contains(VALIDATE_ONLY))
            .respond_with(answer)
            .up_to_n_times(times)
            .mount(&self.google)
            .await;
    }

    /// Everything sent to FCM's address for pushes, as JSON.
    async fn sent(&self) -> Vec<Value> {
        self.google
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.url.path() == SEND)
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    async fn pushes(&self) -> Vec<Value> {
        let mut sent = self.sent().await;
        sent.retain(|body| body.get("validate_only").is_none());
        sent
    }

    /// The questions about tokens: nothing of these reaches a phone.
    async fn checks(&self) -> Vec<Value> {
        let mut sent = self.sent().await;
        sent.retain(|body| body["validate_only"] == true);
        sent
    }

    /// When the push service vouched for the token of the device.
    async fn vouched_at(&self, owner: &Keys, device: &str) -> Option<u64> {
        let device = self.store.device(&owner.public_key().to_hex(), device).await.unwrap();
        device.expect("the device is registered").token_checked_at
    }
}

fn registration() -> Value {
    json!({
        "app_id": APP,
        "channel": { "provider": "fcm", "token": "token-of-the-phone" },
        "relays": [
            { "url": "wss://node-1.veydan.net", "dm": true, "groups": true },
            { "url": "WSS://NOS.LOL/", "dm": true, "groups": false },
            { "url": "wss://relay.example.org" },
            { "url": "https://not-a-relay.example.org" },
        ],
        "groups": [
            // The key of this group changed a while ago: the push key of
            // now, and the one before.
            { "id": "11".repeat(32), "keys": [PUSH_KEY, "c0".repeat(32)] },
            { "id": "22".repeat(32), "keys": [PUSH_KEY] },
        ],
    })
}

/// The push key of a group: 64 hex characters, made by the app from the
/// key of the group.
const PUSH_KEY: &str = "c960ddf616adb9a57ac7d0eb04da5e25cf3d139c95d5828addb8f1f0ed50da28";

fn code(answer: &Answer) -> &str {
    answer.body["error"]["code"].as_str().unwrap_or("")
}

const DEVICE: &str = "/v1/devices/phone-0001";

#[tokio::test]
async fn a_device_is_registered_read_and_removed() {
    let server = start().await;
    let alice = Keys::generate();

    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(put.status, 200, "{}", put.body);
    assert_eq!(put.body["device_id"], "phone-0001");
    let days = (put.body["expires_at"].as_u64().unwrap() - now()) / 86_400;
    assert!((29..=30).contains(&days), "{days}");
    assert_eq!(
        put.body["relays"],
        json!([
            { "url": "wss://node-1.veydan.net", "status": "pending" },
            { "url": "wss://nos.lol", "status": "pending" },
            { "url": "wss://relay.example.org", "status": "not_allowed",
              "detail": "this server does not watch this relay" },
            { "url": "https://not-a-relay.example.org", "status": "invalid",
              "detail": "`https` is not wss" },
        ])
    );

    let get = server.ask(&alice, "GET", DEVICE, None).await;
    assert_eq!(get.status, 200, "{}", get.body);
    assert_eq!(get.body["state"], "active");
    assert!(get.body.get("locale").is_none(), "the server keeps no language");
    assert_eq!(get.body["provider"], "fcm");
    // Only what the server agreed to watch is kept.
    assert_eq!(
        get.body["relays"],
        json!([
            { "url": "wss://node-1.veydan.net", "dm": true, "groups": true, "status": "pending" },
            { "url": "wss://nos.lol", "dm": true, "groups": false, "status": "pending" },
        ])
    );
    assert_eq!(get.body["groups"], json!(["11".repeat(32), "22".repeat(32)]));
    // The token is the device's own business; it is not given back. Nor
    // are the push keys of the groups, in either answer.
    for answer in [&put.body, &get.body] {
        let text = answer.to_string();
        assert!(!text.contains("token-of-the-phone"), "{text}");
        assert!(!text.contains(PUSH_KEY) && !text.contains("c0c0"), "{text}");
    }
    // They are kept, for the events of the groups to be held against.
    let kept = server.store.device(&alice.public_key().to_hex(), "phone-0001").await.unwrap().unwrap();
    assert_eq!(
        kept.groups,
        [
            WatchedGroup { id: "11".repeat(32), keys: vec!["c0".repeat(32), PUSH_KEY.to_string()] },
            WatchedGroup { id: "22".repeat(32), keys: vec![PUSH_KEY.to_string()] },
        ]
    );

    let delete = server.ask(&alice, "DELETE", DEVICE, None).await;
    assert_eq!(delete.status, 204);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.status, 404);
    // Removing what is gone is not an error.
    assert_eq!(server.ask(&alice, "DELETE", DEVICE, None).await.status, 204);
}

/// The examples the app's tests read are what the server takes: the whole
/// registration as it is written in `golden`, and the least of one.
#[tokio::test]
async fn the_examples_of_a_registration_are_taken_as_they_are() {
    let server = start().await;
    let alice = Keys::generate();
    let golden = |name: &str| -> Value {
        let path = format!("{}/../../golden/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    };

    let put = server.ask(&alice, "PUT", DEVICE, Some(golden("device_put.json"))).await;
    assert_eq!(put.status, 200, "{}", put.body);
    let kept = server.store.device(&alice.public_key().to_hex(), "phone-0001").await.unwrap().unwrap();
    assert_eq!(
        kept.groups,
        [
            WatchedGroup { id: "11".repeat(32), keys: vec![PUSH_KEY.to_string()] },
            WatchedGroup { id: "22".repeat(32), keys: vec!["aa".repeat(32), "bb".repeat(32)] },
        ]
    );

    // No groups: nothing of a group is watched, and nothing is wrong.
    let put = server.ask(&alice, "PUT", DEVICE, Some(golden("device_put_minimal.json"))).await;
    assert_eq!(put.status, 200, "{}", put.body);
    let get = server.ask(&alice, "GET", DEVICE, None).await;
    assert_eq!(get.body["groups"], json!([]));
}

#[tokio::test]
async fn nobody_reads_or_removes_a_device_of_another() {
    let server = start().await;
    let (alice, mallory) = (Keys::generate(), Keys::generate());
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;

    let get = server.ask(&mallory, "GET", DEVICE, None).await;
    assert_eq!((get.status, code(&get)), (404, "not_found"));

    assert_eq!(server.ask(&mallory, "DELETE", DEVICE, None).await.status, 204);
    assert_eq!(
        server.ask(&alice, "GET", DEVICE, None).await.status,
        200,
        "the device of alice is where it was"
    );

    let test = server.ask(&mallory, "POST", &format!("{DEVICE}/test"), None).await;
    assert_eq!(test.status, 404);
    assert!(server.pushes().await.is_empty());
}

#[tokio::test]
async fn a_request_that_is_not_signed_as_it_should_be() {
    let server = start().await;
    let alice = Keys::generate();
    let body = registration().to_string().into_bytes();
    let url = format!("{}{DEVICE}", server.public);

    let none = server.send("PUT", DEVICE, None, body.clone()).await;
    assert_eq!((none.status, code(&none)), (401, "auth_missing"));
    assert!(none.request_id.is_some());
    assert_eq!(none.body["request_id"], json!(none.request_id));

    let other = signed(&alice, "PUT", &format!("{}/v1/devices/phone-0002", server.public), &body, now());
    let answer = server.send("PUT", DEVICE, Some(other), body.clone()).await;
    assert_eq!((answer.status, code(&answer)), (401, "auth_url_mismatch"));

    // Signed for the address the server listens on, not the one it is known by.
    let inner = signed(&alice, "PUT", &format!("{}{DEVICE}", server.base), &body, now());
    let answer = server.send("PUT", DEVICE, Some(inner), body.clone()).await;
    assert_eq!((answer.status, code(&answer)), (401, "auth_url_mismatch"));

    let old = signed(&alice, "PUT", &url, &body, now() - 600);
    let answer = server.send("PUT", DEVICE, Some(old), body.clone()).await;
    assert_eq!((answer.status, code(&answer)), (401, "auth_expired"));
    let server_time = answer.body["error"]["server_time"].as_u64().unwrap();
    assert!(server_time.abs_diff(now()) <= 2, "{server_time}");

    let other_body = signed(&alice, "PUT", &url, b"{}", now());
    let answer = server.send("PUT", DEVICE, Some(other_body), body.clone()).await;
    assert_eq!((answer.status, code(&answer)), (401, "auth_invalid"));

    let get = signed(&alice, "GET", &url, &body, now());
    let answer = server.send("PUT", DEVICE, Some(get), body.clone()).await;
    assert_eq!((answer.status, code(&answer)), (401, "auth_invalid"));

    let once = signed(&alice, "PUT", &url, &body, now());
    assert_eq!(server.send("PUT", DEVICE, Some(once.clone()), body.clone()).await.status, 200);
    let again = server.send("PUT", DEVICE, Some(once), body).await;
    assert_eq!((again.status, code(&again)), (401, "auth_replay"));
}

#[tokio::test]
async fn what_the_server_does_not_serve() {
    let server = start().await;
    let alice = Keys::generate();

    let mut r = registration();
    r["app_id"] = json!("com.somebody.else");
    let answer = server.ask(&alice, "PUT", DEVICE, Some(r)).await;
    assert_eq!((answer.status, code(&answer)), (422, "unknown_app"));

    let mut r = registration();
    r["app_id"] = json!("app.without.push");
    let answer = server.ask(&alice, "PUT", DEVICE, Some(r)).await;
    assert_eq!((answer.status, code(&answer)), (422, "provider_disabled"));

    let mut r = registration();
    r["channel"] = json!({ "provider": "apns", "token": "abc", "environment": "production" });
    let answer = server.ask(&alice, "PUT", DEVICE, Some(r)).await;
    assert_eq!((answer.status, code(&answer)), (422, "provider_disabled"));

    for (what, bad) in [
        ("no channel", json!({ "app_id": APP })),
        ("unknown provider", json!({ "app_id": APP, "channel": { "provider": "pigeon" } })),
        ("empty token", json!({ "app_id": APP, "channel": { "provider": "fcm", "token": " " } })),
        ("author key", json!({ "app_id": APP, "channel": { "provider": "fcm", "token": "t" },
                               "author_key": "short" })),
    ] {
        let answer = server.ask(&alice, "PUT", DEVICE, Some(bad)).await;
        assert_eq!((answer.status, code(&answer)), (400, "bad_request"), "{what}: {}", answer.body);
    }

    let (id, other) = ("11".repeat(32), "c0".repeat(32));
    for (what, groups) in [
        // What an app older than 0.3.0 sends.
        ("a group by its id alone", json!([id])),
        ("no keys", json!([{ "id": id }])),
        ("an empty list of keys", json!([{ "id": id, "keys": [] }])),
        ("three keys", json!([{ "id": id, "keys": [PUSH_KEY, other, "c2".repeat(32)] }])),
        ("a key that is not a list", json!([{ "id": id, "keys": PUSH_KEY }])),
        ("a short key", json!([{ "id": id, "keys": [&PUSH_KEY[..62]] }])),
        ("a key in capitals", json!([{ "id": id, "keys": [PUSH_KEY.to_uppercase()] }])),
        ("a key that is not hex", json!([{ "id": id, "keys": ["zz".repeat(32)] }])),
        ("one key twice", json!([{ "id": id, "keys": [PUSH_KEY, PUSH_KEY] }])),
        ("a short id", json!([{ "id": "xyz", "keys": [PUSH_KEY] }])),
        ("an id in capitals", json!([{ "id": "AB".repeat(32), "keys": [PUSH_KEY] }])),
        ("no id", json!([{ "keys": [PUSH_KEY] }])),
        ("one group twice", json!([{ "id": id, "keys": [PUSH_KEY] }, { "id": id, "keys": [other] }])),
    ] {
        let mut r = registration();
        r["groups"] = groups;
        // An owner of its own every time: one owner registers ten times a
        // minute and no more.
        let answer = server.ask(&Keys::generate(), "PUT", DEVICE, Some(r)).await;
        assert_eq!((answer.status, code(&answer)), (400, "bad_request"), "{what}: {}", answer.body);
        // Whatever was wrong with a key, the answer does not say it back.
        assert!(!answer.body.to_string().to_lowercase().contains(&PUSH_KEY[..62]), "{what}: {}", answer.body);
    }

    for path in ["/v1/devices/short", "/v1/devices/with%20space-1234"] {
        let answer = server.ask(&alice, "PUT", path, Some(registration())).await;
        assert_eq!((answer.status, code(&answer)), (400, "bad_request"), "{path}");
    }
    assert_eq!(server.store.counts().await.unwrap().devices, 0);
}

#[tokio::test]
async fn limits() {
    let server = start_with(
        "[limits]\ndevices_per_pubkey = 2\nrelays_per_device = 3\ngroups_per_device = 2\n",
    )
    .await;
    let alice = Keys::generate();

    let answer = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!((answer.status, code(&answer)), (422, "limit_relays"));

    let mut r = registration();
    r["relays"] = json!([]);
    let group = |id: &str| json!({ "id": id.repeat(32), "keys": [PUSH_KEY] });
    r["groups"] = json!([group("11"), group("22"), group("33")]);
    let answer = server.ask(&alice, "PUT", DEVICE, Some(r.clone())).await;
    assert_eq!((answer.status, code(&answer)), (422, "limit_groups"));
    // As many as the limit says are taken.
    r["groups"] = json!([group("11"), group("22")]);
    assert_eq!(server.ask(&alice, "PUT", DEVICE, Some(r)).await.status, 200);
    assert_eq!(server.ask(&alice, "DELETE", DEVICE, None).await.status, 204);

    let device = |n: u32| {
        json!({ "app_id": APP, "channel": { "provider": "fcm", "token": format!("token-{n}") } })
    };
    for n in 1..=2 {
        let path = format!("/v1/devices/device-000{n}");
        assert_eq!(server.ask(&alice, "PUT", &path, Some(device(n))).await.status, 200);
    }
    let third = server.ask(&alice, "PUT", "/v1/devices/device-0003", Some(device(3))).await;
    assert_eq!((third.status, code(&third)), (422, "limit_devices"));
    // Renewing is not adding.
    assert_eq!(server.ask(&alice, "PUT", "/v1/devices/device-0001", Some(device(1))).await.status, 200);
}

#[tokio::test]
async fn a_body_above_the_limit_is_refused_in_the_servers_own_words() {
    let server = start_with("[server]\nmax_body_bytes = 2000\n").await;
    let alice = Keys::generate();
    let mut r = registration();
    r["app_version"] = json!("x".repeat(3000));
    let answer = server.ask(&alice, "PUT", DEVICE, Some(r)).await;
    assert_eq!((answer.status, code(&answer)), (413, "payload_too_large"), "{}", answer.body);
}

#[tokio::test]
async fn the_same_phone_under_a_new_identity() {
    let server = start().await;
    let (before, after) = (Keys::generate(), Keys::generate());
    server.ask(&before, "PUT", DEVICE, Some(registration())).await;
    server.ask(&after, "PUT", "/v1/devices/phone-0002", Some(registration())).await;

    assert_eq!(server.ask(&before, "GET", DEVICE, None).await.status, 404);
    assert_eq!(server.ask(&after, "GET", "/v1/devices/phone-0002", None).await.status, 200);
    assert_eq!(server.store.counts().await.unwrap().devices, 1);
}

#[tokio::test]
async fn a_test_push_says_only_what_it_is() {
    let server = start_with("[limits]\ntest_per_hour = 2\n").await;
    server
        .fcm_answers(ResponseTemplate::new(200).set_body_json(json!({ "name": "m/1" })))
        .await;
    let alice = Keys::generate();
    let test = format!("{DEVICE}/test");

    // No device yet.
    assert_eq!(server.ask(&alice, "POST", &test, None).await.status, 404);

    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    let answer = server.ask(&alice, "POST", &test, None).await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(answer.body["outcome"], "delivered");
    assert_eq!(answer.body["trace"], json!(answer.request_id));

    let pushes = server.pushes().await;
    assert_eq!(pushes.len(), 1);
    let message = &pushes[0]["message"];
    assert_eq!(message["token"], "token-of-the-phone");
    // No text: the phone shows a test push in its own words.
    assert_eq!(
        message["data"],
        json!({ "v": "2", "type": "test", "trace": answer.body["trace"] })
    );
    assert!(message.get("notification").is_none());

    let get = server.ask(&alice, "GET", DEVICE, None).await;
    assert_eq!(get.body["last_outcome"], "delivered");
    assert!(get.body["last_push_at"].as_u64().unwrap().abs_diff(now()) <= 2);

    assert_eq!(server.ask(&alice, "POST", &test, None).await.status, 200);
    let third = server.ask(&alice, "POST", &test, None).await;
    assert_eq!((third.status, code(&third)), (429, "rate_limited"));
    let wait: u64 = third.retry_after.unwrap().parse().unwrap();
    assert!((3500..=3600).contains(&wait), "{wait}");
    assert_eq!(server.pushes().await.len(), 2);
}

#[tokio::test]
async fn the_same_phone_under_a_new_device_id_has_no_test_pushes_of_its_own() {
    let server = start_with("[limits]\ntest_per_hour = 2\n").await;
    server
        .fcm_answers(ResponseTemplate::new(200).set_body_json(json!({ "name": "m/1" })))
        .await;
    let alice = Keys::generate();

    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    for _ in 0..2 {
        let answer = server.ask(&alice, "POST", &format!("{DEVICE}/test"), None).await;
        assert_eq!(answer.status, 200, "{}", answer.body);
    }

    // The same token, registered again as another device.
    let renamed = "/v1/devices/phone-0002";
    assert_eq!(server.ask(&alice, "PUT", renamed, Some(registration())).await.status, 200);
    let third = server.ask(&alice, "POST", &format!("{renamed}/test"), None).await;
    assert_eq!((third.status, code(&third)), (429, "rate_limited"), "{}", third.body);
    assert_eq!(server.pushes().await.len(), 2);

    // Another phone has an allowance of its own.
    let mut other = registration();
    other["channel"]["token"] = json!("token-of-the-tablet");
    let tablet = "/v1/devices/tablet-0001";
    assert_eq!(server.ask(&alice, "PUT", tablet, Some(other)).await.status, 200);
    assert_eq!(server.ask(&alice, "POST", &format!("{tablet}/test"), None).await.status, 200);
}

#[tokio::test]
async fn a_test_push_to_a_busy_service_is_tried_once_and_says_so() {
    let server = start().await;
    server
        .fcm_answers(ResponseTemplate::new(503).set_body_json(json!({
            "error": { "status": "UNAVAILABLE", "message": "busy",
                       "details": [{ "errorCode": "UNAVAILABLE" }] }
        })))
        .await;
    let alice = Keys::generate();
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;

    let answer = server.ask(&alice, "POST", &format!("{DEVICE}/test"), None).await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(answer.body["outcome"], "retry");
    assert_eq!(server.pushes().await.len(), 1, "whoever asked is waiting: no second attempt");
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["last_outcome"], "retry");
}

fn fcm_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "name": "m/1" }))
}

fn fcm_refuses(http: u16, status: &str, code: &str) -> ResponseTemplate {
    ResponseTemplate::new(http).set_body_json(json!({
        "error": { "status": status, "message": "why", "details": [{ "errorCode": code }] }
    }))
}

fn fcm_token_gone() -> ResponseTemplate {
    fcm_refuses(404, "NOT_FOUND", "UNREGISTERED")
}

#[tokio::test]
async fn a_token_the_push_service_gave_up_on_is_marked_and_a_new_one_clears_the_mark() {
    let server = start().await;
    // FCM knows a token when asked, and has none to push to.
    server.fcm_says_of_a_token(fcm_ok()).await;
    server.fcm_answers(fcm_token_gone()).await;
    let alice = Keys::generate();
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;

    let answer = server.ask(&alice, "POST", &format!("{DEVICE}/test"), None).await;
    assert_eq!(answer.body["outcome"], "dead_token", "{}", answer.body);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "dead_token");

    let mut renewed = registration();
    renewed["channel"]["token"] = json!("a-new-token");
    server.ask(&alice, "PUT", DEVICE, Some(renewed)).await;
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "active");
}

#[tokio::test]
async fn a_token_the_push_service_does_not_know_is_refused_and_nothing_is_kept() {
    for refusal in [
        fcm_refuses(400, "INVALID_ARGUMENT", "INVALID_ARGUMENT"),
        fcm_token_gone(),
        fcm_refuses(403, "PERMISSION_DENIED", "SENDER_ID_MISMATCH"),
    ] {
        let server = start().await;
        server.fcm_says_of_a_token(refusal).await;
        let alice = Keys::generate();

        let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
        assert_eq!((put.status, code(&put)), (422, "token_invalid"), "{}", put.body);
        assert_eq!(server.store.counts().await.unwrap().devices, 0);
        assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.status, 404);
        assert_eq!(server.counters.tokens_invalid.get(), 1);

        // FCM was asked as it is asked to push, and told to send nothing.
        assert_eq!(
            server.checks().await,
            [json!({
                "validate_only": true,
                "message": { "token": "token-of-the-phone", "data": { "v": "2", "type": "test" } },
            })]
        );
        assert!(server.pushes().await.is_empty());
    }
}

#[tokio::test]
async fn a_token_is_asked_about_once_and_not_again_with_every_renewal() {
    let server = start().await;
    server.fcm_says_of_a_token(fcm_ok()).await;
    let alice = Keys::generate();

    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(put.status, 200, "{}", put.body);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "active");
    assert!(server.vouched_at(&alice, "phone-0001").await.is_some());
    assert_eq!(server.checks().await.len(), 1);

    // The renewal of every week: the same token, and FCM is not asked.
    for _ in 0..2 {
        let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
        assert_eq!(put.status, 200, "{}", put.body);
    }
    assert_eq!(server.checks().await.len(), 1);

    // A new token is a new question.
    let mut renewed = registration();
    renewed["channel"]["token"] = json!("a-new-token");
    assert_eq!(server.ask(&alice, "PUT", DEVICE, Some(renewed)).await.status, 200);
    let checks = server.checks().await;
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[1]["message"]["token"], "a-new-token");
    assert!(server.pushes().await.is_empty(), "nothing was sent to any phone");
    assert_eq!(server.counters.tokens_invalid.get(), 0);
}

#[tokio::test]
async fn a_push_service_that_cannot_say_does_not_stop_a_registration() {
    let server = start().await;
    server
        .fcm_says_of_a_token_times(2, fcm_refuses(503, "UNAVAILABLE", "UNAVAILABLE"))
        .await;
    // The answer of FCM once it is back.
    server.fcm_says_of_a_token(fcm_ok()).await;
    let alice = Keys::generate();

    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(put.status, 200, "{}", put.body);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "active");
    assert_eq!(server.vouched_at(&alice, "phone-0001").await, None, "nobody vouched for it");
    assert_eq!(server.checks().await.len(), 1, "asked once: whoever registers is waiting");

    // Not vouched for, the token is asked about with the next registration,
    // until the push service can say.
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(server.vouched_at(&alice, "phone-0001").await, None);
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert!(server.vouched_at(&alice, "phone-0001").await.is_some());
    assert_eq!(server.checks().await.len(), 3);

    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(server.checks().await.len(), 3, "and never after that");
}

/// A service account Google refuses, or a project id that is wrong, is the
/// server's trouble. No token is refused for it.
#[tokio::test]
async fn trouble_of_the_servers_own_with_the_push_service_refuses_no_token() {
    // No answer is mounted for the address of pushes: a bare 404, as from
    // a project that does not exist.
    let server = start().await;
    let alice = Keys::generate();
    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(put.status, 200, "{}", put.body);
    assert_eq!(server.vouched_at(&alice, "phone-0001").await, None);
    assert_eq!(server.counters.tokens_invalid.get(), 0);
}

#[tokio::test]
async fn a_push_that_is_taken_vouches_for_a_token_nobody_could_vouch_for() {
    let server = start().await;
    server.fcm_says_of_a_token(fcm_refuses(503, "UNAVAILABLE", "UNAVAILABLE")).await;
    server.fcm_answers(fcm_ok()).await;
    let alice = Keys::generate();
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(server.vouched_at(&alice, "phone-0001").await, None);

    let test = server.ask(&alice, "POST", &format!("{DEVICE}/test"), None).await;
    assert_eq!(test.body["outcome"], "delivered", "{}", test.body);
    assert!(server.vouched_at(&alice, "phone-0001").await.is_some());

    // The first push decided: the renewal asks nobody.
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(server.checks().await.len(), 1);
}

#[tokio::test]
async fn a_device_marked_dead_comes_back_with_the_same_token_when_the_push_service_vouches_for_it() {
    let server = start().await;
    server.fcm_says_of_a_token(fcm_ok()).await;
    let alice = Keys::generate();
    let owner = alice.public_key().to_hex();
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;

    // Marked dead, rightly or not.
    server
        .store
        .record_outcome(&owner, "phone-0001", "token-of-the-phone", "dead_token", now())
        .await
        .unwrap();
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "dead_token");

    // The app registers the token it has, as it does when it is opened.
    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(put.status, 200, "{}", put.body);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "active");
    assert_eq!(server.checks().await.len(), 2, "a dead token is asked about again");
}

#[tokio::test]
async fn a_dead_token_the_push_service_still_does_not_know_stays_dead() {
    let server = start().await;
    server.fcm_says_of_a_token_times(1, fcm_ok()).await;
    server.fcm_says_of_a_token_times(1, fcm_token_gone()).await;
    server.fcm_says_of_a_token(fcm_refuses(503, "UNAVAILABLE", "UNAVAILABLE")).await;
    let alice = Keys::generate();
    let owner = alice.public_key().to_hex();
    server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    server
        .store
        .record_outcome(&owner, "phone-0001", "token-of-the-phone", "dead_token", now())
        .await
        .unwrap();

    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!((put.status, code(&put)), (422, "token_invalid"), "{}", put.body);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "dead_token");

    // A push service that cannot say brings nothing back to life either.
    let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!(put.status, 200, "{}", put.body);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.body["state"], "dead_token");
}

/// A registration of a device of its own, with a token of its own.
fn device(n: usize) -> (String, Value) {
    (
        format!("/v1/devices/device-{n:04}"),
        json!({ "app_id": APP, "channel": { "provider": "fcm", "token": format!("token-{n}") } }),
    )
}

#[tokio::test]
async fn an_owner_registers_ten_times_a_minute_and_the_eleventh_is_told_to_come_back() {
    let server = start().await;
    let (alice, bob) = (Keys::generate(), Keys::generate());

    for n in 0..10 {
        let put = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
        assert_eq!(put.status, 200, "registration {n}: {}", put.body);
    }
    let eleventh = server.ask(&alice, "PUT", DEVICE, Some(registration())).await;
    assert_eq!((eleventh.status, code(&eleventh)), (429, "rate_limited"), "{}", eleventh.body);
    let wait: u64 = eleventh.retry_after.unwrap().parse().unwrap();
    assert!((50..=60).contains(&wait), "{wait}");
    assert_eq!(server.counters.refused_rate_owner.get(), 1);

    // It is the registrations that are counted, and those of this owner.
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.status, 200);
    let (path, body) = device(1);
    assert_eq!(server.ask(&bob, "PUT", &path, Some(body)).await.status, 200);
}

#[tokio::test]
async fn new_devices_from_one_address_are_twenty_an_hour() {
    let server = start_with("[server]\ntrusted_proxy = true\n").await;
    const HOME: &str = "203.0.113.7";

    let mut first = None;
    for n in 0..20 {
        let owner = Keys::generate();
        let (path, body) = device(n);
        let put = server.register_from(HOME, &owner, &path, body).await;
        assert_eq!(put.status, 200, "device {n}: {}", put.body);
        first.get_or_insert(owner);
    }
    let (path, body) = device(20);
    let refused = server.register_from(HOME, &Keys::generate(), &path, body.clone()).await;
    assert_eq!((refused.status, code(&refused)), (429, "rate_limited"), "{}", refused.body);
    let wait: u64 = refused.retry_after.unwrap().parse().unwrap();
    assert!((3500..=3600).contains(&wait), "{wait}");
    assert_eq!(server.counters.refused_rate_ip.get(), 1);
    assert_eq!(server.store.counts().await.unwrap().devices, 20);

    // Renewing a device the server knows is not a new device.
    let (known, same) = device(0);
    let renewed = server.register_from(HOME, &first.unwrap(), &known, same).await;
    assert_eq!(renewed.status, 200, "{}", renewed.body);

    // What a client writes of itself is not where it came from: the proxy
    // said, and its word is the only one heard.
    let owner = Keys::generate();
    let raw = body.to_string().into_bytes();
    let header = signed(&owner, "PUT", &format!("{}{path}", server.public), &raw, now());
    let disguised = server
        .send_with(
            "PUT",
            &path,
            Some(header),
            raw,
            &[("x-real-ip", HOME), ("x-forwarded-for", "198.51.100.1")],
        )
        .await;
    assert_eq!(disguised.status, 429, "{}", disguised.body);

    // Another address has an allowance of its own.
    let elsewhere = server.register_from("203.0.113.8", &Keys::generate(), &path, body).await;
    assert_eq!(elsewhere.status, 200, "{}", elsewhere.body);
}

#[tokio::test]
async fn requests_that_came_past_the_proxy_are_counted_as_one_client() {
    let server = start_with("[server]\ntrusted_proxy = true\n").await;
    for n in 0..20 {
        let (path, body) = device(n);
        let put = server.ask(&Keys::generate(), "PUT", &path, Some(body)).await;
        assert_eq!(put.status, 200, "device {n}: {}", put.body);
    }
    let (path, body) = device(20);
    let refused = server.ask(&Keys::generate(), "PUT", &path, Some(body.clone())).await;
    assert_eq!((refused.status, code(&refused)), (429, "rate_limited"), "{}", refused.body);

    // And what is no address names nobody.
    let nobody = server.register_from("not-an-address", &Keys::generate(), &path, body).await;
    assert_eq!(nobody.status, 429, "{}", nobody.body);
}

#[tokio::test]
async fn without_a_proxy_to_say_it_nobody_is_counted_by_the_address() {
    let server = start().await;
    for n in 0..25 {
        let (path, body) = device(n);
        let put = server.register_from("203.0.113.7", &Keys::generate(), &path, body).await;
        assert_eq!(put.status, 200, "device {n}: {}", put.body);
    }
    assert_eq!(server.counters.refused_rate_ip.get(), 0);
}

#[tokio::test]
async fn a_server_that_is_full_takes_no_new_device() {
    let server = start_with("[limits]\ndevices_total = 2\n").await;
    let (alice, bob, carol) = (Keys::generate(), Keys::generate(), Keys::generate());
    let (first, of_alice) = device(1);
    let (second, of_bob) = device(2);
    let (third, of_carol) = device(3);
    assert_eq!(server.ask(&alice, "PUT", &first, Some(of_alice.clone())).await.status, 200);
    assert_eq!(server.ask(&bob, "PUT", &second, Some(of_bob)).await.status, 200);

    let full = server.ask(&carol, "PUT", &third, Some(of_carol.clone())).await;
    assert_eq!((full.status, code(&full)), (422, "limit_devices_total"), "{}", full.body);
    assert_eq!(server.store.counts().await.unwrap().devices, 2);
    assert_eq!(server.counters.refused_devices_total.get(), 1);

    // Those who are registered renew as before, and a place that is left
    // is a place for somebody.
    assert_eq!(server.ask(&alice, "PUT", &first, Some(of_alice)).await.status, 200);
    assert_eq!(server.ask(&bob, "DELETE", &second, None).await.status, 204);
    assert_eq!(server.ask(&carol, "PUT", &third, Some(of_carol)).await.status, 200);
}

#[tokio::test]
async fn what_a_client_learns_before_it_registers() {
    let server = start().await;
    let info = server.send("GET", "/v1/info", None, vec![]).await;
    assert_eq!(info.status, 200);
    assert_eq!(info.body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info.body["apps"], json!([{ "id": APP, "providers": ["fcm"] }]));
    assert_eq!(
        info.body["relays"],
        json!({ "policy": "allow_list", "allowed": ["wss://node-1.veydan.net", "wss://nos.lol"] })
    );
    assert_eq!(info.body["limits"]["devices_per_pubkey"], 10);
    assert_eq!(info.body["limits"]["max_body_bytes"], 65536);
}

#[tokio::test]
async fn two_requests_of_one_second_need_a_nonce_to_be_two() {
    let server = start().await;
    let alice = Keys::generate();
    let url = format!("{}{DEVICE}", server.public);
    let at = now();

    // The same key, address, method and second, and nothing to tell them
    // apart: signed twice, it is one event with two signatures.
    let bare = |at: u64| {
        let event = EventBuilder::new(Kind::HttpAuth, "")
            .tags([
                Tag::parse(["u", url.as_str()]).unwrap(),
                Tag::parse(["method", "GET"]).unwrap(),
            ])
            .custom_created_at(Timestamp::from_secs(at))
            .finalize(&alice)
            .unwrap();
        format!("Nostr {}", STANDARD.encode(serde_json::to_vec(&event).unwrap()))
    };
    assert_eq!(server.send("GET", DEVICE, Some(bare(at)), vec![]).await.status, 404);
    let second = server.send("GET", DEVICE, Some(bare(at)), vec![]).await;
    assert_eq!((second.status, code(&second)), (401, "auth_replay"));

    // With a nonce each request is its own event.
    for _ in 0..3 {
        let header = signed(&alice, "GET", &url, b"", at);
        assert_eq!(server.send("GET", DEVICE, Some(header), vec![]).await.status, 404);
    }
}
