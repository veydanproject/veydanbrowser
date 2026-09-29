//! The examples of `spec/golden` are read by the types, and written back
//! the same. A change of a type that breaks the protocol fails here.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use vpush_proto::{
    Channel, DeviceAnswer, DevicePut, ErrorBody, ErrorCode, Payload, Prefs, PushType, RelayStatus,
};

fn golden(name: &str) -> String {
    let path = format!("{}/../../spec/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Reads the example, and checks that writing it gives the example back.
fn round_trip<T: DeserializeOwned + Serialize>(name: &str) -> T {
    let text = golden(name);
    let value: T = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let written = serde_json::to_value(&value).unwrap();
    let expected: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written, expected, "{name} is not written back as it was read");
    value
}

#[test]
fn registration() {
    let put: DevicePut = round_trip("device_put.json");
    assert_eq!(put.app_id, "net.veydan.mobile");
    assert_eq!(
        put.channel,
        Channel::Fcm {
            token: "fcm-token-of-the-device".into()
        }
    );
    assert_eq!(put.locale.as_deref(), Some("ru-RU"));
    assert_eq!(put.relays.len(), 2);
    assert!(!put.relays[1].groups);
    assert_eq!(put.groups[0].name.as_deref(), Some("Команда"));
    assert_eq!(put.groups[1].name, None);
    assert_eq!(put.author_key.unwrap().len(), 64);
}

#[test]
fn the_least_a_registration_may_say() {
    // Not written back the same: what was left out is filled in.
    let put: DevicePut = serde_json::from_str(&golden("device_put_minimal.json")).unwrap();
    assert_eq!(put.prefs, Prefs { dm: true, groups: true });
    assert!(put.relays.is_empty());
    assert!(put.groups.is_empty());
    assert_eq!(put.locale, None);
    assert_eq!(put.author_key, None);
}

#[test]
fn a_relay_is_watched_for_everything_unless_said_otherwise() {
    let put: DevicePut = serde_json::from_str(
        r#"{"app_id":"a","channel":{"provider":"fcm","token":"t"},
            "relays":[{"url":"wss://r.example.org"}]}"#,
    )
    .unwrap();
    assert!(put.relays[0].dm && put.relays[0].groups);
}

#[test]
fn what_a_newer_client_adds_does_not_break_an_older_server() {
    let put: DevicePut = serde_json::from_str(
        r#"{"app_id":"a","channel":{"provider":"fcm","token":"t"},"from_the_future":{"x":1}}"#,
    )
    .unwrap();
    assert_eq!(put.app_id, "a");
}

#[test]
fn answer_to_a_registration() {
    let answer: DeviceAnswer = round_trip("device_answer.json");
    assert_eq!(answer.relays[0].status, RelayStatus::Pending);
    assert_eq!(answer.relays[1].status, RelayStatus::NotAllowed);
    assert!(answer.relays[1].detail.is_some());
}

#[test]
fn refusal_with_the_servers_time() {
    let body: ErrorBody = round_trip("error_auth_expired.json");
    assert_eq!(body.error.code, ErrorCode::AuthExpired);
    assert_eq!(body.error.server_time, Some(1_790_000_000));
}

#[test]
fn pushes() {
    let dm: Payload = round_trip("push_dm.json");
    assert_eq!(dm.kind, PushType::Dm);
    assert!(!dm.is_silent());

    let group: Payload = round_trip("push_group.json");
    assert_eq!(group.kind, PushType::Group);
    assert_eq!(group.count, Some(3));
    assert_eq!(group.to_data()["count"], "3");
}
