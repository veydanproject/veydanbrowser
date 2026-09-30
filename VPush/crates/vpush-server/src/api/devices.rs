//! Registration of devices: `/v1/devices/{device_id}`.
//!
//! A device is registered by its owner and by nobody else: every request is
//! signed (NIP-98), and the key that signed it is the owner. There is no
//! other way to name an owner, so nobody can register, read or remove a
//! device of another.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, State};
use axum::http::header::{AUTHORIZATION, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use vpush_proto::{
    Channel, DeviceAnswer, DevicePut, DeviceView, ErrorCode, RelayAnswer, RelayStatus, RelayView,
    TestAnswer,
};

use super::{error, now, Api, RequestId, NEW_DEVICES_PER_HOUR, PUTS_PER_MINUTE};
use crate::auth::AuthError;
use crate::delivery::{
    check_token, mask, test_push, Outcome, ProviderKind, PushProvider, Target, TokenCheck,
};
use crate::store::{Device, DeviceInput, DeviceLimits, StoreError, WatchedRelay};

const MAX_TOKEN: usize = 4096;
/// The header in which the reverse proxy says whom a request came from.
const REAL_IP: &str = "x-real-ip";

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

    fn too_often(message: impl Into<String>, wait: Duration) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: ErrorCode::RateLimited,
            message: message.into(),
            retry_after: Some(wait),
        }
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
            // Nothing is wrong with the signature; the server has no room
            // to remember one more.
            AuthError::Busy(wait) => {
                return Self::too_often("too many requests at once; come back later", wait)
            }
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

/// What to tell about a relay: whether the server agrees to watch it, and
/// if it does, how the relay is doing.
fn judge(api: &Api, url: &str) -> (String, RelayStatus, Option<String>) {
    let (url, status, detail) = api.relays.judge(url);
    if status != RelayStatus::Pending {
        return (url, status, detail);
    }
    let status = api.watch.status(&url);
    let detail = match status {
        RelayStatus::Restricted => Some("the relay does not let the push server read".to_string()),
        RelayStatus::Unreachable => Some("the push server cannot reach the relay".to_string()),
        _ => None,
    };
    (url, status, detail)
}

/// Whom a request came from, as the reverse proxy in front says. `None`
/// when there is no proxy to say it: what a client writes into a header is
/// no address.
fn client(api: &Api, headers: &HeaderMap) -> Option<IpAddr> {
    if !api.config.server.trusted_proxy {
        return None;
    }
    let said = headers
        .get(REAL_IP)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok());
    // The proxy names somebody in every request it passes on. One that
    // names nobody came past the proxy, and all such are counted as one
    // client.
    Some(said.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
}

/// A registration that may be written down.
struct Accepted {
    /// The request as the store takes it.
    input: DeviceInput,
    /// What to tell about each relay.
    relays: Vec<RelayAnswer>,
    /// The service that pushes to the device.
    provider: Arc<dyn PushProvider>,
}

fn accepted(api: &Api, asked: &Asked, put: DevicePut) -> Result<Accepted, Refusal> {
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
    let provider = api.providers.get(&put.app_id, kind).map_err(|_| {
        Refusal::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProviderDisabled,
            format!("this server does not push through {} for this app", kind.as_str()),
        )
    })?;

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

    let mut groups: Vec<String> = Vec::with_capacity(put.groups.len());
    for group in &put.groups {
        if !is_hex64(group) {
            return Err(Refusal::bad("a group id is 64 hex characters"));
        }
        if !groups.contains(group) {
            groups.push(group.clone());
        }
    }

    // A relay the server refuses is told about and left out; the rest of
    // the registration stands.
    let mut answers = Vec::with_capacity(put.relays.len());
    let mut relays: Vec<WatchedRelay> = Vec::new();
    for relay in &put.relays {
        let (url, status, detail) = judge(api, &relay.url);
        if answers.iter().any(|a: &RelayAnswer| a.url == url) {
            continue;
        }
        // Kept when the server agrees to watch the relay, however the relay
        // is doing at the moment: a relay that is down today is watched
        // again when it comes back.
        if api.relays.get(&url).is_some() {
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
        app_version: put
            .app_version
            .map(|v| v.chars().filter(|c| !c.is_control()).take(40).collect()),
        prefs: put.prefs,
        author_key,
        relays,
        groups,
        now: at,
        expires_at: at + u64::from(limits.registration_days) * 86_400,
        token_checked_at: None,
    };
    Ok(Accepted {
        input,
        relays: answers,
        provider,
    })
}

fn view(api: &Api, device: Device) -> DeviceView {
    DeviceView {
        relays: device
            .relays
            .iter()
            .map(|r| RelayView {
                status: judge(api, &r.url).1,
                url: r.url.clone(),
                dm: r.dm,
                groups: r.groups,
            })
            .collect(),
        device_id: device.device_id,
        app_id: device.app_id,
        provider: device.provider,
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
        api.limits.registration(&asked.pubkey).map_err(|wait| {
            api.counters.refused_rate_owner.add();
            Refusal::too_often(
                format!("{PUTS_PER_MINUTE} registrations of one owner a minute; come back later"),
                wait,
            )
        })?;
        let put: DevicePut = serde_json::from_slice(&asked.body)
            .map_err(|e| Refusal::bad(format!("the body is not a registration: {e}")))?;
        let Accepted {
            mut input,
            relays,
            provider,
        } = accepted(&api, &asked, put)?;

        let stored = api
            .store
            .device(&asked.pubkey, &asked.device_id)
            .await
            .map_err(|e| Refusal::internal(e, &rid))?;
        let same_token = stored.as_ref().is_some_and(|d| d.token == input.token);

        // A device id or a token the server has not seen with this owner
        // costs a question to the push service and, when it is taken, a
        // row. Renewing what is known costs neither, and is not counted.
        if !same_token {
            if let Some(from) = client(&api, &headers) {
                api.limits.new_device(from).map_err(|wait| {
                    api.counters.refused_rate_ip.add();
                    Refusal::too_often(
                        format!(
                            "{NEW_DEVICES_PER_HOUR} new devices from one address an hour; come back later"
                        ),
                        wait,
                    )
                })?;
            }
        }

        // The push service is asked about the token, unless it has vouched
        // for this very token and has not taken its word back since: the
        // renewal of every week asks nobody.
        let vouched = stored.as_ref().is_some_and(|d| {
            same_token && d.state == "active" && d.token_checked_at.is_some()
        });
        let check = if vouched {
            None
        } else {
            let target = Target {
                token: input.token.clone(),
            };
            Some(check_token(provider.as_ref(), &target).await)
        };
        match check {
            Some(TokenCheck::Invalid) => {
                api.counters.tokens_invalid.add();
                tracing::info!(
                    request_id = %rid,
                    owner = %mask(&asked.pubkey),
                    device = %asked.device_id,
                    token = %mask(&input.token),
                    "registration refused: the push service does not know the token"
                );
                return Err(Refusal::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::TokenInvalid,
                    "the push service does not know this token; ask it for a new one and register again",
                ));
            }
            Some(TokenCheck::Valid) => input.token_checked_at = Some(input.now),
            // The service could not say. The device is registered, and the
            // first push to it tells.
            Some(TokenCheck::Unknown) | None => {}
        }

        let answer = DeviceAnswer {
            device_id: input.device_id.clone(),
            expires_at: input.expires_at,
            relays,
        };
        // For the log, of what the store is about to take.
        let (owner, token) = (mask(&input.pubkey), mask(&input.token));
        let (app, watched, groups) = (input.app_id.clone(), input.relays.len(), input.groups.len());
        let limits = DeviceLimits {
            per_owner: api.config.limits.devices_per_pubkey,
            total: api.config.limits.devices_total,
        };
        match api.store.put_device(input, limits).await {
            Ok(()) => {
                tracing::info!(
                    request_id = %rid,
                    %owner,
                    device = %answer.device_id,
                    %app,
                    provider = provider.kind().as_str(),
                    %token,
                    token_check = check.map_or("vouched for before", TokenCheck::as_str),
                    relays = watched,
                    refused_relays = answer.relays.len() - watched,
                    groups,
                    "device registered"
                );
                api.watch.plan_changed();
                Ok(answer)
            }
            Err(StoreError::TooManyDevices) => Err(Refusal::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::LimitDevices,
                format!(
                    "more than {} devices of one owner",
                    api.config.limits.devices_per_pubkey
                ),
            )),
            Err(StoreError::Full) => {
                api.counters.refused_devices_total.add();
                Err(Refusal::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::LimitDevicesTotal,
                    "this server takes no more devices",
                ))
            }
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
        if was {
            api.watch.plan_changed();
        }
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

        api.limits.test(&device.token).map_err(|wait| {
            Refusal::too_often(
                format!(
                    "{} test pushes an hour; come back later",
                    api.config.limits.test_per_hour
                ),
                wait,
            )
        })?;

        let kind: ProviderKind = device
            .provider
            .parse()
            .map_err(|e: String| Refusal::internal(e, &rid))?;
        let provider = api.providers.get(&device.app_id, kind).map_err(|e| {
            Refusal::new(StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::ProviderDisabled, e)
        })?;

        let target = Target {
            token: device.token.clone(),
        };
        let delivery = test_push(provider.as_ref(), &target, &rid).await;
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
            .record_outcome(&asked.pubkey, &asked.device_id, &target.token, outcome, now())
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
    fn no_room_for_one_more_signature_is_too_many_requests_not_a_bad_signature() {
        let refusal = Refusal::from(AuthError::Busy(Duration::from_secs(40)));
        assert_eq!(refusal.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(refusal.code, ErrorCode::RateLimited);
        assert_eq!(refusal.retry_after, Some(Duration::from_secs(40)));
        assert_eq!(Refusal::from(AuthError::Replay).status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn group_ids_and_author_keys() {
        assert!(is_hex64(&"ab".repeat(32)));
        for bad in ["", &"AB".repeat(32), &"ab".repeat(31), &"zz".repeat(32)] {
            assert!(!is_hex64(bad), "{bad}");
        }
    }
}
