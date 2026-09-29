//! Public HTTP API.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tower_http::timeout::TimeoutLayer;
use vpush_proto::{ErrorBody, ErrorCode, ErrorDetail};

use crate::config::Config;
use crate::version;

const REQUEST_ID: &str = "x-request-id";

/// Id of one request, carried into handlers and into the answer.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

pub fn router(config: &Config) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .fallback(not_found)
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
