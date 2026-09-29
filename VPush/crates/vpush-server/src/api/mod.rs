//! Public HTTP API.

mod devices;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tower_http::timeout::TimeoutLayer;
use vpush_proto::{AppInfo, ErrorBody, ErrorCode, ErrorDetail, Info, Limits};

use crate::auth::Nip98;
use crate::config::Config;
use crate::delivery::Providers;
use crate::relays::RelayPolicy;
use crate::store::Store;
use crate::version;
use crate::relay::Watch;

const REQUEST_ID: &str = "x-request-id";

/// Id of one request, carried into handlers and into the answer.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

/// What the handlers work with.
#[derive(Clone)]
pub struct Api {
    pub config: Arc<Config>,
    pub store: Arc<dyn Store>,
    pub providers: Arc<Providers>,
    pub relays: Arc<RelayPolicy>,
    /// How the relays are doing, and the way to say that what is watched changed.
    pub watch: Arc<Watch>,
    pub auth: Arc<Nip98>,
    pub tests: Arc<TestLimiter>,
}

impl Api {
    pub fn new(
        config: Arc<Config>,
        store: Arc<dyn Store>,
        providers: Arc<Providers>,
        relays: Arc<RelayPolicy>,
        watch: Arc<Watch>,
    ) -> Self {
        Self {
            auth: Arc::new(Nip98::new(&config.public_url)),
            tests: Arc::new(TestLimiter::new(config.limits.test_per_hour)),
            config,
            store,
            providers,
            relays,
            watch,
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How many test pushes a device has asked for in the last hour.
pub struct TestLimiter {
    per_hour: u32,
    asked: Mutex<HashMap<(String, String), Vec<Instant>>>,
}

impl TestLimiter {
    const HOUR: Duration = Duration::from_secs(3600);

    pub fn new(per_hour: u32) -> Self {
        Self {
            per_hour,
            asked: Mutex::new(HashMap::new()),
        }
    }

    /// Counts one more request, or says after how long to come back.
    pub fn take(&self, pubkey: &str, device_id: &str) -> Result<(), Duration> {
        let now = Instant::now();
        let mut asked = self.asked.lock().unwrap();
        if asked.len() >= 4096 {
            asked.retain(|_, times| {
                times.retain(|t| now.duration_since(*t) < Self::HOUR);
                !times.is_empty()
            });
        }
        let times = asked
            .entry((pubkey.to_string(), device_id.to_string()))
            .or_default();
        times.retain(|t| now.duration_since(*t) < Self::HOUR);
        if times.len() >= self.per_hour as usize {
            let oldest = times.iter().min().copied().unwrap_or(now);
            return Err(Self::HOUR.saturating_sub(now.duration_since(oldest)));
        }
        times.push(now);
        Ok(())
    }
}

pub fn router(api: Api) -> Router {
    let config = api.config.clone();
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/info", get(info))
        .route(
            "/v1/devices/{device_id}",
            put(devices::put).get(devices::get).delete(devices::delete),
        )
        .route("/v1/devices/{device_id}/test", post(devices::test))
        .fallback(not_found)
        .with_state(api)
        .layer(DefaultBodyLimit::max(config.server.max_body_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(config.server.request_timeout_secs),
        ))
        .layer(middleware::from_fn(access_log))
}

async fn healthz() -> Json<vpush_proto::Health> {
    Json(version::health())
}

async fn info(State(api): State<Api>) -> Json<Info> {
    let limits = &api.config.limits;
    Json(Info {
        version: version::VERSION.to_string(),
        apps: api
            .providers
            .summary()
            .into_iter()
            .map(|(id, providers)| AppInfo {
                id,
                providers: providers.split(", ").map(str::to_string).collect(),
            })
            .collect(),
        relays: vpush_proto::RelayPolicy {
            policy: api.config.relays.policy.clone(),
            allowed: api.relays.urls(),
        },
        limits: Limits {
            devices_per_pubkey: limits.devices_per_pubkey,
            relays_per_device: limits.relays_per_device,
            groups_per_device: limits.groups_per_device,
            registration_days: limits.registration_days,
            test_per_hour: limits.test_per_hour,
            max_body_bytes: api.config.server.max_body_bytes as u32,
        },
    })
}

async fn not_found(request: Request) -> Response {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();
    error(StatusCode::NOT_FOUND, ErrorCode::NotFound, "no such route", id)
}

pub fn error(
    status: StatusCode,
    code: ErrorCode,
    message: impl Into<String>,
    request_id: String,
) -> Response {
    let body = ErrorBody {
        error: ErrorDetail {
            code,
            message: message.into(),
            server_time: (code == ErrorCode::AuthExpired).then(now),
        },
        request_id,
    };
    (status, Json(body)).into_response()
}

/// Ids only have to tell requests apart in a log, so a counter seeded by the
/// start time is enough.
pub fn next_request_id() -> String {
    static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seed = *SEED.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    });
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Spread the bits so neighbours do not look alike.
    let mixed = (seed ^ n).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("{:08x}", (mixed >> 32) as u32)
}

/// One INFO line per request, and the request id on the way in and out.
async fn access_log(mut request: Request, next: Next) -> Response {
    let id = next_request_id();
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    request.extensions_mut().insert(RequestId(id.clone()));

    let started = Instant::now();
    let mut response = next.run(request).await;
    let status = response.status().as_u16();
    let ms = started.elapsed().as_millis() as u64;

    // The health check is asked every few seconds; it would drown the rest.
    if path == "/healthz" {
        tracing::debug!(request_id = %id, %method, path, status, ms, "request");
    } else {
        tracing::info!(request_id = %id, %method, path, status, ms, "request");
    }

    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(REQUEST_ID, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_limit_is_per_device() {
        let limiter = TestLimiter::new(2);
        limiter.take("alice", "phone").unwrap();
        limiter.take("alice", "phone").unwrap();
        let wait = limiter.take("alice", "phone").unwrap_err();
        assert!(wait <= TestLimiter::HOUR && wait > Duration::from_secs(3590), "{wait:?}");
        limiter.take("alice", "tablet").unwrap();
        limiter.take("bob", "phone").unwrap();
    }
}
