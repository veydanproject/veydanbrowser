//! Public HTTP API.

mod devices;

use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
use crate::counters::Counters;
use crate::delivery::Providers;
use crate::limit::Limiter;
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
    pub limits: Arc<Limiters>,
    pub counters: Arc<Counters>,
}

impl Api {
    pub fn new(
        config: Arc<Config>,
        store: Arc<dyn Store>,
        providers: Arc<Providers>,
        relays: Arc<RelayPolicy>,
        watch: Arc<Watch>,
        counters: Arc<Counters>,
    ) -> Self {
        Self {
            auth: Arc::new(Nip98::new(&config.public_url)),
            limits: Arc::new(Limiters::new(config.limits.test_per_hour)),
            counters,
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

/// Registrations one owner may make in a minute. An app registers when
/// something changed and once in seven days; ten a minute is a burst of
/// changes and more, and every one past that is somebody trying the server.
pub const PUTS_PER_MINUTE: usize = 10;

/// Devices the server did not know that may be registered from one address
/// in an hour. Each of them costs a question to the push service and a row.
pub const NEW_DEVICES_PER_HOUR: usize = 20;

/// How often what may be asked.
pub struct Limiters {
    /// Test pushes, by the token they go to. A token is one phone, whatever
    /// it is registered as: the same token under a new device id has no
    /// allowance of its own.
    tests: Limiter<[u8; 32]>,
    /// Registrations, by their owner.
    puts: Limiter<String>,
    /// Registrations of a device the server did not know, by the address
    /// they came from.
    new_devices: Limiter<IpAddr>,
}

impl Limiters {
    const MINUTE: Duration = Duration::from_secs(60);
    const HOUR: Duration = Duration::from_secs(3600);
    /// Keys each limit remembers at one time. Past this many a key that is
    /// not remembered is told to come back later.
    const MAX_KEYS: usize = 100_000;

    pub fn new(tests_per_hour: u32) -> Self {
        Self {
            tests: Limiter::new(Self::HOUR, tests_per_hour as usize, Self::MAX_KEYS),
            puts: Limiter::new(Self::MINUTE, PUTS_PER_MINUTE, Self::MAX_KEYS),
            new_devices: Limiter::new(Self::HOUR, NEW_DEVICES_PER_HOUR, Self::MAX_KEYS),
        }
    }

    /// Counts one more test push to the token, or says after how long to
    /// come back.
    pub fn test(&self, token: &str) -> Result<(), Duration> {
        // Remembered by its hash: a token may be thousands of bytes long.
        let digest = ring::digest::digest(&ring::digest::SHA256, token.as_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(digest.as_ref());
        self.tests.take(key)
    }

    /// Counts one more registration of the owner.
    pub fn registration(&self, owner: &str) -> Result<(), Duration> {
        self.puts.take(owner.to_string())
    }

    /// Counts one more new device from the address.
    pub fn new_device(&self, from: IpAddr) -> Result<(), Duration> {
        self.new_devices.take(network(from))
    }
}

/// What is counted as one client. An IPv6 address is counted by its first
/// 64 bits: the rest is the client's to choose, a new one for every request
/// if it likes.
fn network(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(_) => address,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let prefix = u128::from(v6) & !u128::from(u64::MAX);
                IpAddr::V6(Ipv6Addr::from(prefix))
            }
        },
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
    fn the_test_limit_is_per_token() {
        let limits = Limiters::new(2);
        limits.test("token-of-the-phone").unwrap();
        limits.test("token-of-the-phone").unwrap();
        let wait = limits.test("token-of-the-phone").unwrap_err();
        assert!(wait <= Limiters::HOUR && wait > Duration::from_secs(3590), "{wait:?}");
        limits.test("token-of-the-tablet").unwrap();
    }

    #[test]
    fn an_owner_registers_ten_times_a_minute_and_no_more() {
        let limits = Limiters::new(3);
        for _ in 0..PUTS_PER_MINUTE {
            limits.registration("alice").unwrap();
        }
        let wait = limits.registration("alice").unwrap_err();
        assert!(wait <= Limiters::MINUTE && wait > Duration::from_secs(50), "{wait:?}");
        limits.registration("bob").unwrap();
    }

    #[test]
    fn new_devices_are_counted_by_the_address_and_an_ipv6_network_is_one_address() {
        let limits = Limiters::new(3);
        let from = |address: &str| limits.new_device(address.parse().unwrap());
        for _ in 0..NEW_DEVICES_PER_HOUR {
            from("203.0.113.7").unwrap();
        }
        let wait = from("203.0.113.7").unwrap_err();
        assert!(wait <= Limiters::HOUR && wait > Duration::from_secs(3590), "{wait:?}");
        from("203.0.113.8").unwrap();
        // The same address, written as IPv6 writes an IPv4 one.
        assert!(from("::ffff:203.0.113.7").is_err());

        // Whoever holds a /64 has as many addresses as it likes, and one allowance.
        for host in 0..NEW_DEVICES_PER_HOUR {
            from(&format!("2001:db8:1:2::{host:x}")).unwrap();
        }
        assert!(from("2001:db8:1:2:ffff:ffff:ffff:ffff").is_err());
        from("2001:db8:1:3::1").unwrap();
    }
}
