//! Registration of devices: `/v1/devices/{device_id}`.
//!
//! A device is registered by its owner and by nobody else: every request is
//! signed (NIP-98), and the key that signed it is the owner. There is no
//! other way to name an owner, so nobody can register, read or remove a
//! device of another.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, State};
use axum::http::header::{AUTHORIZATION, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use vpush_proto::{
    Channel, DeviceAnswer, DevicePut, DeviceView, ErrorCode, GroupWatch, Payload, PushType,
    RelayAnswer, RelayStatus, RelayView, TestAnswer,
};

use super::{error, now, Api, RequestId};
use crate::auth::AuthError;
use crate::delivery::retry::{self, RetryPolicy};
use crate::delivery::{mask, Message, Outcome, ProviderKind, Target};
use crate::store::{Device, DeviceInput, StoreError, WatchedRelay};
use crate::texts::Texts;

const MAX_TOKEN: usize = 4096;
const MAX_GROUP_NAME: usize = 64;

/// A refusal, on its way to become an answer.
struct Refusal {
    status: StatusCode,
    code: ErrorCode,
    message: String,
    retry_after: Option<Duration>,
}

impl Refusal {
    fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after: None,
        }
    }

    fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, ErrorCode::BadRequest, message)
    }

    fn internal(e: impl std::fmt::Display, request_id: &str) -> Self {
        // What went wrong inside is for the log, not for the one who asked.
        tracing::error!(request_id, error = %e, "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "the server failed; the request may be repeated",
        )
    }

    fn answer(self, request_id: &str) -> Response {
        let mut response = error(self.status, self.code, self.message, request_id.to_string());
        if let Some(after) = self.retry_after {
            if let Ok(value) = HeaderValue::from_str(&after.as_secs().max(1).to_string()) {
                response.headers_mut().insert(RETRY_AFTER, value);
            }
        }
        response
    }
}

impl From<AuthError> for Refusal {
    fn from(e: AuthError) -> Self {
        let (code, message) = match e {
            AuthError::Missing => (
                ErrorCode::AuthMissing,
                "the request is not signed: no `Authorization: Nostr …` header".to_string(),
            ),
            AuthError::Invalid(why) => (ErrorCode::AuthInvalid, why),
            AuthError::Expired => (
                ErrorCode::AuthExpired,
                "the signature is too old or from the future; compare the clocks".to_string(),
            ),
            AuthError::Replay => (
                ErrorCode::AuthReplay,
                "this signature was used already".to_string(),
            ),
            AuthError::UrlMismatch => (
                ErrorCode::AuthUrlMismatch,
                "the request was signed for another address".to_string(),
            ),
        };
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }
}

/// What every request to a device starts with.
struct Asked {
    /// The owner: the key that signed the request.
    pubkey: String,
    device_id: String,
    body: Bytes,
}

fn device_id_ok(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn asked(
    api: &Api,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    device_id: String,
    body: Result<Bytes, BytesRejection>,
) -> Result<Asked, Refusal> {
    let body = body.map_err(|e| {
        let too_large = e.status() == StatusCode::PAYLOAD_TOO_LARGE;
        Refusal::new(
            e.status(),
            if too_large { ErrorCode::PayloadTooLarge } else { ErrorCode::BadRequest },
            if too_large {
                format!("the body is above {} bytes", api.config.server.max_body_bytes)
            } else {
                "the body cannot be read".to_string()
            },
        )
    })?;
    if !device_id_ok(&device_id) {
        return Err(Refusal::bad(
            "the device id is 8 to 64 letters, digits, `_` or `-`",
        ));
    }
    let header = match headers.get(AUTHORIZATION) {
        None => None,
        Some(value) => Some(value.to_str().map_err(|_| {
            Refusal::from(AuthError::Invalid("the header is not text".into()))
        })?),
    };
    let path_and_query = uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or_else(|| uri.path());
    let pubkey = api
        .auth
        .check(header, method.as_str(), path_and_query, &body, now())?;
    Ok(Asked {
        pubkey,
        device_id,
        body,
    })
}

/// Names are shown on a lock screen: one line, no control characters, not long.
fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned: String = cleaned.chars().take(MAX_GROUP_NAME).collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn clean_locale(locale: Option<&str>) -> String {
    let locale = locale.unwrap_or("en").trim();
    let ok = (2..=16).contains(&locale.len())
        && locale
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-' || b == b'_');
    if ok { locale.to_string() } else { "en".to_string() }
}

/// The request as the store takes it, and what to tell about each relay.
fn accepted(
    api: &Api,
    asked: &Asked,
    put: DevicePut,
) -> Result<(DeviceInput, Vec<RelayAnswer>), Refusal> {
    let limits = &api.config.limits;

    let kind: ProviderKind = put
        .channel
        .provider()
        .parse()
        .map_err(Refusal::bad)?;
    if !api.config.apps.contains_key(&put.app_id) {
        return Err(Refusal::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::UnknownApp,
            format!("this server does not serve the app `{}`", put.app_id.chars().take(100).collect::<String>()),
        ));
    }
    if api.providers.get(&put.app_id, kind).is_err() {
        return Err(Refusal::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProviderDisabled,
            format!("this server does not push through {} for this app", kind.as_str()),
        ));
    }

    let (token, channel_json) = match &put.channel {
        Channel::Fcm { token } => (token.clone(), None),
        Channel::Apns { token, .. } => (
            token.clone(),
            Some(serde_json::to_string(&put.channel).unwrap_or_default()),
        ),
        Channel::Unifiedpush { endpoint, .. } => (
            endpoint.clone(),
            Some(serde_json::to_string(&put.channel).unwrap_or_default()),
        ),
    };
    if token.trim().is_empty() || token.len() > MAX_TOKEN {
        return Err(Refusal::bad("the token is empty or too long"));
    }

    if put.relays.len() > limits.relays_per_device as usize {
        return Err(Refusal::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::LimitRelays,
            format!("more than {} relays", limits.relays_per_device),
        ));
    }
    if put.groups.len() > limits.groups_per_device as usize {
        return Err(Refusal::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::LimitGroups,
            format!("more than {} groups", limits.groups_per_device),
        ));
    }

    let author_key = match put.author_key.as_deref() {
        None => None,
        Some(key) if is_hex64(key) => Some(key.to_string()),
        Some(_) => return Err(Refusal::bad("author_key is 64 hex characters")),
    };

    let mut groups: Vec<GroupWatch> = Vec::with_capacity(put.groups.len());
    for group in &put.groups {
        if !is_hex64(&group.id) {
            return Err(Refusal::bad("a group id is 64 hex characters"));
        }
        if groups.iter().any(|g| g.id == group.id) {
            continue;
        }
        groups.push(GroupWatch {
            id: group.id.clone(),
            name: group.name.as_deref().and_then(clean_name),
        });
    }

    // A relay the server refuses is told about and left out; the rest of
    // the registration stands.
    let mut answers = Vec::with_capacity(put.relays.len());
    let mut relays: Vec<WatchedRelay> = Vec::new();
    for relay in &put.relays {
        let (url, status, detail) = api.relays.judge(&relay.url);
        if answers.iter().any(|a: &RelayAnswer| a.url == url) {
            continue;
        }
        if matches!(status, RelayStatus::Ok | RelayStatus::Pending) {
            relays.push(WatchedRelay {
                url: url.clone(),
                dm: relay.dm,
                groups: relay.groups,
            });
        }
        answers.push(RelayAnswer { url, status, detail });
    }

    let at = now();
    let input = DeviceInput {
        pubkey: asked.pubkey.clone(),
        device_id: asked.device_id.clone(),
        app_id: put.app_id,
        provider: kind.as_str().to_string(),
        token,
        channel_json,
        locale: clean_locale(put.locale.as_deref()),
        app_version: put
            .app_version
            .map(|v| v.chars().filter(|c| !c.is_control()).take(40).collect()),
        prefs: put.prefs,
        author_key,
        relays,
        groups,
        now: at,
        expires_at: at + u64::from(limits.registration_days) * 86_400,
    };
    Ok((input, answers))
}

fn view(api: &Api, device: Device) -> DeviceView {
    DeviceView {
        relays: device
            .relays
            .iter()
            .map(|r| RelayView {
                status: api.relays.judge(&r.url).1,
                url: r.url.clone(),
                dm: r.dm,
                groups: r.groups,
            })
            .collect(),
        device_id: device.device_id,
        app_id: device.app_id,
        provider: device.provider,
        locale: device.locale,
        prefs: device.prefs,
        state: device.state,
        created_at: device.created_at,
        updated_at: device.updated_at,
        expires_at: device.expires_at,
        groups: device.groups,
        last_push_at: device.last_push_at,
        last_outcome: device.last_outcome,
    }
}

fn no_device() -> Refusal {
    Refusal::new(
        StatusCode::NOT_FOUND,
        ErrorCode::NotFound,
        "no such device of this owner",
    )
}

pub async fn put(
    State(api): State<Api>,
    Extension(RequestId(rid)): Extension<RequestId>,
    Path(device_id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let done = async {
        let asked = asked(&api, &method, &uri, &headers, device_id, body)?;
        let put: DevicePut = serde_json::from_slice(&asked.body)
            .map_err(|e| Refusal::bad(format!("the body is not a registration: {e}")))?;
        let (input, relays) = accepted(&api, &asked, put)?;

        let answer = DeviceAnswer {
            device_id: input.device_id.clone(),
            expires_at: input.expires_at,
            relays,
        };
        tracing::info!(
            request_id = %rid,
            owner = %mask(&input.pubkey),
            device = %input.device_id,
            app = %input.app_id,
            provider = %input.provider,
            token = %mask(&input.token),
            relays = input.relays.len(),
            refused_relays = answer.relays.len() - input.relays.len(),
            groups = input.groups.len(),
            "device registered"
        );
        match api
            .store
            .put_device(input, api.config.limits.devices_per_pubkey)
            .await
        {
            Ok(()) => Ok(answer),
            Err(StoreError::TooManyDevices) => Err(Refusal::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::LimitDevices,
                format!(
                    "more than {} devices of one owner",
                    api.config.limits.devices_per_pubkey
                ),
            )),
            Err(e) => Err(Refusal::internal(e, &rid)),
        }
    }
    .await;
    match done {
        Ok(answer) => Json(answer).into_response(),
        Err(refusal) => refusal.answer(&rid),
    }
}

pub async fn get(
    State(api): State<Api>,
    Extension(RequestId(rid)): Extension<RequestId>,
    Path(device_id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let done = async {
        let asked = asked(&api, &method, &uri, &headers, device_id, body)?;
        let device = api
            .store
            .device(&asked.pubkey, &asked.device_id)
            .await
            .map_err(|e| Refusal::internal(e, &rid))?
            .ok_or_else(no_device)?;
        Ok::<_, Refusal>(view(&api, device))
    }
    .await;
    match done {
        Ok(view) => Json(view).into_response(),
        Err(refusal) => refusal.answer(&rid),
    }
}

pub async fn delete(
    State(api): State<Api>,
    Extension(RequestId(rid)): Extension<RequestId>,
    Path(device_id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let done = async {
        let asked = asked(&api, &method, &uri, &headers, device_id, body)?;
        let was = api
            .store
            .delete_device(&asked.pubkey, &asked.device_id)
            .await
            .map_err(|e| Refusal::internal(e, &rid))?;
        tracing::info!(
            request_id = %rid,
            owner = %mask(&asked.pubkey),
            device = %asked.device_id,
            was,
            "device removed"
        );
        // Removing what is not there is what the client wanted anyway.
        Ok::<_, Refusal>(())
    }
    .await;
    match done {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(refusal) => refusal.answer(&rid),
    }
}

pub async fn test(
    State(api): State<Api>,
    Extension(RequestId(rid)): Extension<RequestId>,
    Path(device_id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let done = async {
        let asked = asked(&api, &method, &uri, &headers, device_id, body)?;
        let device = api
            .store
            .device(&asked.pubkey, &asked.device_id)
            .await
            .map_err(|e| Refusal::internal(e, &rid))?
            .ok_or_else(no_device)?;

        api.tests
            .take(&asked.pubkey, &asked.device_id)
            .map_err(|wait| Refusal {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: ErrorCode::RateLimited,
                message: format!(
                    "{} test pushes an hour; come back later",
                    api.config.limits.test_per_hour
                ),
                retry_after: Some(wait),
            })?;

        let kind: ProviderKind = device
            .provider
            .parse()
            .map_err(|e: String| Refusal::internal(e, &rid))?;
        let provider = api.providers.get(&device.app_id, kind).map_err(|e| {
            Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::ProviderDisabled, e)
        })?;

        let (title, body) = Texts::of(&device.locale).test();
        let mut payload = Payload::new(PushType::Test);
        payload.title = Some(title);
        payload.body = Some(body);
        payload.trace = Some(rid.clone());
        let message = Message {
            payload,
            collapse_key: None,
            ttl: Duration::from_secs(60),
            urgent: true,
        };
        let target = Target {
            token: device.token.clone(),
        };
        let delivery =
            retry::deliver(provider.as_ref(), &target, &message, RetryPolicy::default()).await;
        let outcome = outcome_name(delivery.outcome);
        let last = delivery.attempts.last();
        tracing::info!(
            request_id = %rid,
            trace = %rid,
            owner = %mask(&asked.pubkey),
            device = %asked.device_id,
            token = %target.masked(),
            outcome,
            attempts = delivery.attempts.len(),
            http_status = last.and_then(|a| a.http_status),
            code = last.and_then(|a| a.code.as_deref()),
            "test push"
        );
        api.store
            .record_outcome(&asked.pubkey, &asked.device_id, outcome, now())
            .await
            .map_err(|e| Refusal::internal(e, &rid))?;
        Ok::<_, Refusal>(TestAnswer {
            outcome: outcome.to_string(),
            trace: rid.clone(),
        })
    }
    .await;
    match done {
        Ok(answer) => Json(answer).into_response(),
        Err(refusal) => refusal.answer(&rid),
    }
}

pub fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Delivered => "delivered",
        Outcome::DeadToken => "dead_token",
        Outcome::Rejected => "rejected",
        Outcome::Retry => "retry",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids() {
        for ok in ["phone-0001", "a1b2c3d4", "A_b-9".repeat(2).as_str(), &"x".repeat(64)] {
            assert!(device_id_ok(ok), "{ok}");
        }
        for bad in ["", "short", &"x".repeat(65), "with space", "sl/ash", "точка-1234"] {
            assert!(!device_id_ok(bad), "{bad}");
        }
    }

    #[test]
    fn names_are_made_fit_for_a_lock_screen() {
        assert_eq!(clean_name("  Команда \n разработки\t").as_deref(), Some("Команда разработки"));
        assert_eq!(clean_name("a\u{0007}b\u{200B}c").as_deref(), Some("ab\u{200B}c"));
        assert_eq!(clean_name(" \n ").as_deref(), None);
        assert_eq!(clean_name(&"я".repeat(200)).unwrap().chars().count(), 64);
    }

    #[test]
    fn locales() {
        assert_eq!(clean_locale(Some("ru-RU")), "ru-RU");
        assert_eq!(clean_locale(Some(" en ")), "en");
        assert_eq!(clean_locale(None), "en");
        for bad in ["", "r", "ru; DROP", "../../etc", &"a".repeat(17)] {
            assert_eq!(clean_locale(Some(bad)), "en", "{bad}");
        }
    }
}
