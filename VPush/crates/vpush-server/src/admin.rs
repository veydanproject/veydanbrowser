//! Admin socket: how `vpush ctl` talks to the running server.
//!
//! A unix socket, not a port: whoever may open the file may steer the server,
//! and nobody else can reach it. One JSON line in, one JSON line out.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::delivery::retry::{self, RetryPolicy};
use crate::delivery::{mask, Message, ProviderKind, Providers, Target};
use crate::store::{Device, Store};
use crate::relay::Watch;
use crate::logging::{LogControl, LogSpec};
use crate::version;
use vpush_proto::{Payload, PushType};

/// One connection may send this much in all; `vpush ctl` sends a line or two.
const MAX_LINE: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Health,
    LogShow,
    LogSet {
        spec: String,
        ttl_secs: Option<u64>,
    },
    LogReset,
    /// One push to one token, past every filter: shows whether pushes arrive.
    TestPush {
        app: String,
        provider: ProviderKind,
        token: String,
        /// Absent with `body`: a silent push.
        title: Option<String>,
        body: Option<String>,
    },
    /// The devices of one owner, and what is watched for each.
    Devices { owner: String },
    /// How many devices and owners there are.
    Stats,
    /// The relays on the line, and how each is doing.
    Relays,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    fn ok(data: impl Serialize) -> Self {
        match serde_json::to_value(data) {
            Ok(v) => Self {
                ok: true,
                data: Some(v),
                error: None,
            },
            Err(e) => Self::err(e.to_string()),
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

impl Request {
    /// For the log. The request itself may carry a token, and is not logged.
    fn name(&self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::LogShow => "log_show",
            Self::LogSet { .. } => "log_set",
            Self::LogReset => "log_reset",
            Self::TestPush { .. } => "test_push",
            Self::Devices { .. } => "devices",
            Self::Stats => "stats",
            Self::Relays => "relays",
        }
    }
}

/// A device as `vpush ctl devices` shows it: everything but the token,
/// which is shown as its mark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceLine {
    pub device_id: String,
    pub app_id: String,
    pub provider: String,
    pub token: String,
    pub locale: String,
    pub app_version: Option<String>,
    pub state: String,
    pub dm: bool,
    pub groups: bool,
    pub author_mark: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: u64,
    pub last_push_at: Option<u64>,
    pub last_outcome: Option<String>,
    pub relays: Vec<String>,
    pub watched_groups: Vec<String>,
}

impl From<Device> for DeviceLine {
    fn from(d: Device) -> Self {
        Self {
            token: mask(&d.token),
            relays: d
                .relays
                .iter()
                .map(|r| {
                    let what = match (r.dm, r.groups) {
                        (true, true) => "dm+groups",
                        (true, false) => "dm",
                        (false, true) => "groups",
                        (false, false) => "nothing",
                    };
                    format!("{} ({what})", r.url)
                })
                .collect(),
            watched_groups: d
                .groups
                .iter()
                .map(|g| match &g.name {
                    Some(name) => format!("{} {name}", &g.id[..g.id.len().min(12)]),
                    None => g.id[..g.id.len().min(12)].to_string(),
                })
                .collect(),
            device_id: d.device_id,
            app_id: d.app_id,
            provider: d.provider,
            locale: d.locale,
            app_version: d.app_version,
            state: d.state,
            dm: d.prefs.dm,
            groups: d.prefs.groups,
            author_mark: d.author_key.is_some(),
            created_at: d.created_at,
            updated_at: d.updated_at,
            expires_at: d.expires_at,
            last_push_at: d.last_push_at,
            last_outcome: d.last_outcome,
        }
    }
}

/// What the admin commands act on.
#[derive(Clone)]
pub struct AdminState {
    pub log: Arc<LogControl>,
    pub providers: Arc<Providers>,
    pub store: Arc<dyn Store>,
    pub watch: Arc<Watch>,
}

/// The socket file; removed when dropped.
pub struct AdminSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl Drop for AdminSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl AdminSocket {
    /// Takes the socket path. A file left by a dead server is replaced; a
    /// socket somebody answers on means a second server, and that is an error.
    pub async fn bind(path: &Path) -> io::Result<Self> {
        if path.exists() {
            if UnixStream::connect(path).await.is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("another vpush answers on {}", path.display()),
                ));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    /// Answers until `shutdown` resolves.
    pub async fn run(self, state: AdminState, shutdown: impl std::future::Future<Output = ()>) {
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, _)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = serve_connection(stream, state).await {
                                tracing::debug!(error = %e, "admin connection ended with an error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "admin socket: accept failed");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                },
            }
        }
    }
}

async fn serve_connection(stream: UnixStream, state: AdminState) -> io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read.take(MAX_LINE as u64)).lines();
    // One connection may ask several things, one after another.
    while let Some(line) = tokio::time::timeout(IO_TIMEOUT, lines.next_line())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no request in time"))??
    {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => handle(request, &state).await,
            Err(e) => Response::err(format!("not a request: {e}")),
        };
        let mut out = serde_json::to_vec(&response)?;
        out.push(b'\n');
        write.write_all(&out).await?;
    }
    Ok(())
}

async fn handle(request: Request, state: &AdminState) -> Response {
    tracing::debug!(cmd = request.name(), "admin request");
    match request {
        Request::Health => Response::ok(version::health()),
        Request::LogShow => Response::ok(state.log.show()),
        Request::LogSet { spec, ttl_secs } => match spec.parse::<LogSpec>() {
            Err(e) => Response::err(e),
            Ok(parsed) => {
                let view = state.log.set(&parsed, ttl_secs.map(Duration::from_secs));
                tracing::info!(
                    set = %parsed,
                    ttl_secs,
                    effective = %view.effective,
                    "log levels changed"
                );
                Response::ok(view)
            }
        },
        Request::LogReset => {
            let view = state.log.reset();
            tracing::info!(effective = %view.effective, "log overrides removed");
            Response::ok(view)
        }
        Request::TestPush {
            app,
            provider,
            token,
            title,
            body,
        } => test_push(state, &app, provider, token, title, body).await,
        Request::Devices { owner } => {
            // An owner is named as people name them: npub or hex.
            let pubkey = match nostr::key::PublicKey::parse(owner.trim()) {
                Ok(key) => key.to_hex(),
                Err(_) => return Response::err("not a public key: give an npub or 64 hex characters"),
            };
            match state.store.devices_of(&pubkey).await {
                Ok(devices) => Response::ok(serde_json::json!({
                    "owner": pubkey,
                    "devices": devices.into_iter().map(DeviceLine::from).collect::<Vec<_>>(),
                })),
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::Relays => Response::ok(state.watch.health()),
        Request::Stats => match state.store.counts().await {
            Ok(counts) => Response::ok(counts),
            Err(e) => Response::err(e.to_string()),
        },
    }
}

async fn test_push(
    state: &AdminState,
    app: &str,
    kind: ProviderKind,
    token: String,
    title: Option<String>,
    body: Option<String>,
) -> Response {
    let provider = match state.providers.get(app, kind) {
        Ok(p) => p,
        Err(e) => return Response::err(e),
    };
    let trace = crate::api::next_request_id();
    let mut payload = Payload::new(PushType::Test);
    payload.title = title;
    payload.body = body;
    payload.trace = Some(trace.clone());
    let message = Message {
        payload,
        collapse_key: None,
        ttl: Duration::from_secs(60),
        urgent: true,
    };
    let target = Target { token };

    let delivery = retry::deliver(provider.as_ref(), &target, &message, RetryPolicy::default()).await;
    let last = delivery.attempts.last();
    tracing::info!(
        app,
        provider = kind.as_str(),
        token = %target.masked(),
        trace = %trace,
        outcome = ?delivery.outcome,
        attempts = delivery.attempts.len(),
        http_status = last.and_then(|a| a.http_status),
        code = last.and_then(|a| a.code.as_deref()),
        "test push"
    );
    Response::ok(serde_json::json!({
        "trace": trace,
        "outcome": delivery.outcome,
        "attempts": delivery.attempts,
    }))
}

/// The `vpush ctl` side: one request, one answer.
pub async fn call(socket: &Path, request: &Request) -> anyhow::Result<serde_json::Value> {
    let stream = tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(socket))
        .await
        .map_err(|_| anyhow::anyhow!("no answer from {}", socket.display()))?
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => anyhow::anyhow!(
                "vpush is not running (nothing answers on {})",
                socket.display()
            ),
            io::ErrorKind::PermissionDenied => anyhow::anyhow!(
                "no access to {}: run as the vpush user or with sudo",
                socket.display()
            ),
            _ => anyhow::anyhow!("{}: {e}", socket.display()),
        })?;

    let (read, mut write) = stream.into_split();
    let mut out = serde_json::to_vec(request)?;
    out.push(b'\n');
    write.write_all(&out).await?;
    write.shutdown().await?;

    let mut line = String::new();
    tokio::time::timeout(IO_TIMEOUT, BufReader::new(read).read_line(&mut line))
        .await
        .map_err(|_| anyhow::anyhow!("vpush did not answer in time"))??;
    if line.trim().is_empty() {
        anyhow::bail!("vpush closed the connection without an answer");
    }

    let response: Response = serde_json::from_str(&line)?;
    if response.ok {
        Ok(response.data.unwrap_or(serde_json::Value::Null))
    } else {
        anyhow::bail!(response.error.unwrap_or_else(|| "unknown error".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_have_a_stable_wire_form() {
        let line = serde_json::to_string(&Request::LogSet {
            spec: "relay=debug".to_string(),
            ttl_secs: Some(900),
        })
        .unwrap();
        assert_eq!(line, r#"{"cmd":"log_set","spec":"relay=debug","ttl_secs":900}"#);
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"cmd":"health"}"#).unwrap(),
            Request::Health
        );
    }
}
