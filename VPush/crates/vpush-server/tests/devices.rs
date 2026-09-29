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
use vpush_server::delivery::fcm::{FcmClient, ServiceAccount};
use vpush_server::delivery::Providers;
use vpush_server::relays::RelayPolicy;
use vpush_server::store::{SqliteStore, Store};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const APP: &str = "net.veydan.mobile";
const SEND: &str = "/v1/projects/veydan-test/messages:send";

struct Server {
    base: String,
    /// The address clients sign for.
    public: String,
    http: reqwest::Client,
    store: Arc<SqliteStore>,
    google: MockServer,
}

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
    let api = Api::new(
        Arc::new(config),
        store.clone() as Arc<dyn Store>,
        Arc::new(providers),
        Arc::new(relays),
    );
    tokio::spawn(async move {
        axum::serve(listener, api::router(api)).await.unwrap();
    });

    Server {
        base: format!("http://127.0.0.1:{port}"),
        public,
        http: reqwest::Client::new(),
        store,
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
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(h) = header {
            request = request.header("authorization", h);
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

    async fn fcm_answers(&self, answer: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(SEND))
            .respond_with(answer)
            .mount(&self.google)
            .await;
    }

    async fn pushes(&self) -> Vec<Value> {
        self.google
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.url.path() == SEND)
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
}

fn registration() -> Value {
    json!({
        "app_id": APP,
        "channel": { "provider": "fcm", "token": "token-of-the-phone" },
        "locale": "ru-RU",
        "relays": [
            { "url": "wss://node-1.veydan.net", "dm": true, "groups": true },
            { "url": "WSS://NOS.LOL/", "dm": true, "groups": false },
            { "url": "wss://relay.example.org" },
            { "url": "https://not-a-relay.example.org" },
        ],
        "groups": [
            { "id": "11".repeat(32), "name": "  Команда \n разработки " },
            { "id": "22".repeat(32) },
        ],
    })
}

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
    assert_eq!(get.body["locale"], "ru-RU");
    assert_eq!(get.body["provider"], "fcm");
    // Only what the server agreed to watch is kept.
    assert_eq!(
        get.body["relays"],
        json!([
            { "url": "wss://node-1.veydan.net", "dm": true, "groups": true, "status": "pending" },
            { "url": "wss://nos.lol", "dm": true, "groups": false, "status": "pending" },
        ])
    );
    assert_eq!(get.body["groups"][0]["name"], "Команда разработки");
    assert!(get.body["groups"][1].get("name").is_none());
    // The token is the device's own business; it is not given back.
    assert!(!get.body.to_string().contains("token-of-the-phone"));

    let delete = server.ask(&alice, "DELETE", DEVICE, None).await;
    assert_eq!(delete.status, 204);
    assert_eq!(server.ask(&alice, "GET", DEVICE, None).await.status, 404);
    // Removing what is gone is not an error.
    assert_eq!(server.ask(&alice, "DELETE", DEVICE, None).await.status, 204);
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
        ("group id", json!({ "app_id": APP, "channel": { "provider": "fcm", "token": "t" },
                             "groups": [{ "id": "xyz" }] })),
        ("author key", json!({ "app_id": APP, "channel": { "provider": "fcm", "token": "t" },
                               "author_key": "short" })),
    ] {
        let answer = server.ask(&alice, "PUT", DEVICE, Some(bad)).await;
        assert_eq!((answer.status, code(&answer)), (400, "bad_request"), "{what}: {}", answer.body);
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
    r["groups"] = json!([{ "id": "11".repeat(32) }, { "id": "22".repeat(32) }, { "id": "33".repeat(32) }]);
    let answer = server.ask(&alice, "PUT", DEVICE, Some(r)).await;
    assert_eq!((answer.status, code(&answer)), (422, "limit_groups"));

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
async fn a_test_push_is_sent_in_the_language_of_the_device() {
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
    assert_eq!(
        message["data"],
        json!({
            "v": "1", "type": "test", "title": "VPush",
            "body": "Тестовое уведомление: пуши работают.",
            "trace": answer.body["trace"],
        })
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
async fn a_token_the_push_service_gave_up_on_is_marked_and_a_new_one_clears_the_mark() {
    let server = start().await;
    server
        .fcm_answers(ResponseTemplate::new(404).set_body_json(json!({
            "error": { "status": "NOT_FOUND", "message": "gone",
                       "details": [{ "errorCode": "UNREGISTERED" }] }
        })))
        .await;
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
