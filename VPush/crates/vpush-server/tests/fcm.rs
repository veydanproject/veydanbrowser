//! The FCM client against a server that plays Google.

#[path = "support/key.rs"]
mod key;

use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::signature::{RsaKeyPair, UnparsedPublicKey, RSA_PKCS1_2048_8192_SHA256};
use serde_json::{json, Value};
use vpush_proto::{Payload, PushType};
use vpush_server::delivery::fcm::{FcmClient, ServiceAccount};
use vpush_server::delivery::retry::{deliver, RetryPolicy};
use vpush_server::delivery::{Message, Outcome, PushProvider, Target};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SEND: &str = "/v1/projects/veydan-test/messages:send";

fn account(server: &MockServer) -> ServiceAccount {
    ServiceAccount::parse(
        &json!({
            "type": "service_account",
            "project_id": "veydan-test",
            "private_key_id": "key-1",
            "private_key": key::TEST_RSA_KEY,
            "client_email": "push@veydan-test.iam.gserviceaccount.com",
            "token_uri": format!("{}/token", server.uri()),
        })
        .to_string(),
    )
    .unwrap()
}

fn client(server: &MockServer) -> FcmClient {
    FcmClient::new(account(server), &server.uri()).unwrap()
}

fn token_answer(value: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "access_token": value, "expires_in": 3600, "token_type": "Bearer"
    }))
}

async fn mount_token(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_answer("access-1"))
        .mount(server)
        .await;
}

fn fcm_error(http: u16, status: &str, code: Option<&str>) -> ResponseTemplate {
    let mut error = json!({ "code": http, "message": "why", "status": status });
    if let Some(code) = code {
        error["details"] = json!([{
            "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError",
            "errorCode": code
        }]);
    }
    ResponseTemplate::new(http).set_body_json(json!({ "error": error }))
}

fn sent_ok() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({ "name": "projects/veydan-test/messages/0:1" }))
}

fn target() -> Target {
    Target {
        token: "device-token".to_string(),
    }
}

fn message() -> Message {
    let mut payload = Payload::new(PushType::Test);
    payload.trace = Some("abcd1234".into());
    Message {
        payload,
        fallback: None,
        collapse_key: Some("test".into()),
        ttl: Duration::from_secs(60),
        urgent: true,
    }
}

/// A push about a direct message that carries the event, with the other
/// form ready for a service that will not take it.
fn message_with_event() -> Message {
    let mut payload = Payload::new(PushType::Dm);
    payload.event = Some(r#"{"id":"e1","kind":1059,"content":"sealed"}"#.into());
    payload.trace = Some("abcd1234".into());
    let mut fallback = Payload::new(PushType::Dm);
    fallback.event_id = Some("e1".into());
    fallback.relay = Some("wss://node-1.veydan.net".into());
    fallback.trace = Some("abcd1234".into());
    Message {
        payload,
        fallback: Some(fallback),
        collapse_key: Some("dm".into()),
        ttl: Duration::from_secs(60),
        urgent: true,
    }
}

fn fast() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 3,
        base: Duration::from_millis(20),
        max_wait: Duration::from_secs(2),
    }
}

async fn requests_to(server: &MockServer, to: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == to)
        .collect()
}

#[tokio::test]
async fn push_is_delivered_and_is_data_only() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    let attempt = client(&server).send(&target(), &message()).await;
    assert_eq!(attempt.outcome, Outcome::Delivered, "{attempt:?}");
    assert_eq!(attempt.http_status, Some(200));

    let sent = requests_to(&server, SEND).await;
    let body: Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(
        body,
        json!({ "message": {
            "token": "device-token",
            "data": { "v": "2", "type": "test", "trace": "abcd1234" },
            "android": { "priority": "HIGH", "ttl": "60s", "collapse_key": "test" }
        }})
    );
}

#[tokio::test]
async fn one_access_token_serves_many_pushes() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    let client = client(&server);
    let (target, message) = (target(), message());
    let sends = (0..20).map(|_| client.send(&target, &message));
    let mut all = Vec::new();
    for send in sends {
        all.push(send);
    }
    // All at once: the first one fetches the token, the rest wait for it.
    let results = futures_all(all).await;
    assert!(results.iter().all(|a| a.outcome == Outcome::Delivered));
    assert_eq!(requests_to(&server, "/token").await.len(), 1);
    assert_eq!(requests_to(&server, SEND).await.len(), 20);
}

async fn futures_all<F: std::future::Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut set = Vec::new();
    let mut pinned: Vec<_> = futures.into_iter().map(Box::pin).collect();
    std::future::poll_fn(|cx| {
        let mut i = 0;
        while i < pinned.len() {
            match pinned[i].as_mut().poll(cx) {
                std::task::Poll::Ready(out) => {
                    set.push(out);
                    drop(pinned.remove(i));
                }
                std::task::Poll::Pending => i += 1,
            }
        }
        if pinned.is_empty() {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    set
}

#[tokio::test]
async fn token_request_is_signed_by_the_account_key() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    client(&server).send(&target(), &message()).await;

    let asked = requests_to(&server, "/token").await;
    let form = String::from_utf8(asked[0].body.clone()).unwrap();
    let fields: std::collections::HashMap<_, _> = url::form_urlencoded::parse(form.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(
        fields["grant_type"],
        "urn:ietf:params:oauth:grant-type:jwt-bearer"
    );

    let jwt: Vec<&str> = fields["assertion"].split('.').collect();
    assert_eq!(jwt.len(), 3);
    let part = |s: &str| -> Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).unwrap()).unwrap()
    };
    assert_eq!(
        part(jwt[0]),
        json!({ "alg": "RS256", "typ": "JWT", "kid": "key-1" })
    );
    let claims = part(jwt[1]);
    assert_eq!(claims["iss"], "push@veydan-test.iam.gserviceaccount.com");
    assert_eq!(claims["aud"], format!("{}/token", server.uri()));
    assert_eq!(
        claims["scope"],
        "https://www.googleapis.com/auth/firebase.messaging"
    );
    assert_eq!(
        claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap(),
        3600
    );

    // The signature is checked with the public half of the same key.
    let pem: String = key::TEST_RSA_KEY
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let pair = RsaKeyPair::from_pkcs8(&STANDARD.decode(pem).unwrap()).unwrap();
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, pair.public().as_ref())
        .verify(
            format!("{}.{}", jwt[0], jwt[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(jwt[2]).unwrap(),
        )
        .expect("signature of the token request");
}

#[tokio::test]
async fn unregistered_token_is_dead_and_is_not_retried() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(404, "NOT_FOUND", Some("UNREGISTERED")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::DeadToken);
    assert_eq!(delivery.attempts.len(), 1);
    assert_eq!(delivery.attempts[0].code.as_deref(), Some("UNREGISTERED"));
}

/// A push without an event is a few hundred bytes: FCM cannot mean its
/// size, so it means the token.
#[tokio::test]
async fn a_token_fcm_cannot_read_is_dead() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(400, "INVALID_ARGUMENT", Some("INVALID_ARGUMENT")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::DeadToken);
    assert_eq!(delivery.attempts.len(), 1, "no point in trying again");
    assert_eq!(delivery.attempts[0].http_status, Some(400));
    assert_eq!(delivery.attempts[0].code.as_deref(), Some("INVALID_ARGUMENT"));
    assert_eq!(requests_to(&server, SEND).await.len(), 1);
}

#[tokio::test]
async fn a_token_of_another_project_is_dead() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(403, "PERMISSION_DENIED", Some("SENDER_ID_MISMATCH")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message_with_event(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::DeadToken);
    assert_eq!(requests_to(&server, SEND).await.len(), 1, "the other form would fare no better");
}

/// The key FCM holds for APNs or for web push is refused there: a fault
/// of the project's settings, and none of the token's.
#[tokio::test]
async fn a_key_the_service_behind_fcm_refuses_does_not_kill_the_token() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(401, "UNAUTHENTICATED", Some("THIRD_PARTY_AUTH_ERROR")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Rejected);
    assert_eq!(delivery.attempts.len(), 1);
}

#[tokio::test]
async fn a_push_fcm_finds_too_big_is_sent_again_without_the_event() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .and(body_string_contains(r#""event":"#))
        .respond_with(fcm_error(400, "INVALID_ARGUMENT", Some("INVALID_ARGUMENT")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message_with_event(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Delivered, "{delivery:?}");
    assert_eq!(delivery.attempts.len(), 1, "the second form is a part of the same attempt");

    let sent = requests_to(&server, SEND).await;
    assert_eq!(sent.len(), 2);
    let data = |r: &Request| serde_json::from_slice::<Value>(&r.body).unwrap()["message"]["data"].clone();
    assert!(data(&sent[0]).get("event").is_some());
    assert_eq!(
        data(&sent[1]),
        json!({ "v": "2", "type": "dm", "event_id": "e1",
                "relay": "wss://node-1.veydan.net", "trace": "abcd1234" })
    );
}

/// Refused with the event, the push may be too big. Refused without it as
/// well, it is not the push that FCM cannot read.
#[tokio::test]
async fn a_token_fcm_refuses_in_both_forms_is_dead() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(400, "INVALID_ARGUMENT", Some("INVALID_ARGUMENT")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message_with_event(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::DeadToken);
    assert_eq!(delivery.attempts.len(), 1);

    let sent = requests_to(&server, SEND).await;
    assert_eq!(sent.len(), 2, "once with the event, once without, and that is the end");
    let data = |r: &Request| serde_json::from_slice::<Value>(&r.body).unwrap()["message"]["data"].clone();
    assert!(data(&sent[0]).get("event").is_some());
    assert!(data(&sent[1]).get("event").is_none());
}

/// The second form failed for a reason that says nothing of the token: the
/// first refusal may still have been about the size, and the token stays.
#[tokio::test]
async fn a_busy_service_after_a_push_that_was_too_big_does_not_kill_the_token() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .and(body_string_contains(r#""event":"#))
        .respond_with(fcm_error(400, "INVALID_ARGUMENT", Some("INVALID_ARGUMENT")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(503, "UNAVAILABLE", Some("UNAVAILABLE")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message_with_event(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Retry);
    assert!(delivery.attempts.iter().all(|a| a.outcome == Outcome::Retry), "{delivery:?}");
}

#[tokio::test]
async fn wrong_project_does_not_kill_the_token() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    // No mock for the send path: the server answers a bare 404.

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Rejected);
}

#[tokio::test]
async fn retry_after_is_obeyed() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(
            fcm_error(429, "RESOURCE_EXHAUSTED", Some("QUOTA_EXCEEDED"))
                .insert_header("retry-after", "1"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    let started = Instant::now();
    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Delivered);
    assert_eq!(delivery.attempts.len(), 2);
    assert_eq!(delivery.attempts[0].outcome, Outcome::Retry);
    assert_eq!(delivery.attempts[0].retry_after_ms, Some(1000));
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "waited {:?}, the service asked for 1s",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_wait_longer_than_we_accept_ends_the_attempts() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(
            fcm_error(429, "RESOURCE_EXHAUSTED", Some("QUOTA_EXCEEDED"))
                .insert_header("retry-after", "3600"),
        )
        .mount(&server)
        .await;

    let started = Instant::now();
    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Retry);
    assert_eq!(delivery.attempts.len(), 1);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn a_service_that_stays_down_is_given_up_on() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(fcm_error(503, "UNAVAILABLE", Some("UNAVAILABLE")))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Retry);
    assert_eq!(delivery.attempts.len(), 3);
}

#[tokio::test]
async fn refused_access_token_is_replaced() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_answer("access-old"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_answer("access-new"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .and(header("authorization", "Bearer access-old"))
        .respond_with(fcm_error(401, "UNAUTHENTICATED", None))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .and(header("authorization", "Bearer access-new"))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Delivered, "{delivery:?}");
    assert_eq!(delivery.attempts.len(), 2);
    assert_eq!(requests_to(&server, "/token").await.len(), 2);
}

#[tokio::test]
async fn an_account_google_refuses_is_our_fault_not_the_devices() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant", "error_description": "Invalid JWT Signature."
        })))
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Rejected);
    assert_eq!(delivery.attempts[0].code.as_deref(), Some("PROVIDER_AUTH"));
    assert!(requests_to(&server, SEND).await.is_empty());
}

#[tokio::test]
async fn google_being_down_is_tried_again() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_token(&server).await;
    Mock::given(method("POST"))
        .and(path(SEND))
        .respond_with(sent_ok())
        .mount(&server)
        .await;

    let delivery = deliver(&client(&server), &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Delivered);
    assert_eq!(delivery.attempts.len(), 2);
}

#[tokio::test]
async fn a_server_that_is_not_there_is_tried_again() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let account = account(&server);
    // Nothing listens on port 1.
    let client = FcmClient::new(account, "http://127.0.0.1:1").unwrap();

    let delivery = deliver(&client, &target(), &message(), fast()).await;
    assert_eq!(delivery.outcome, Outcome::Retry);
    assert_eq!(delivery.attempts[0].code.as_deref(), Some("UNREACHABLE"));
}
