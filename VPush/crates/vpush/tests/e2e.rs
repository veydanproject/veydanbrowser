//! The real binary, started as the server would be.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_vpush");

struct Server {
    child: Child,
    dir: PathBuf,
    port: u16,
    out: Arc<Mutex<Vec<String>>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_config(dir: &Path, port: u16, log: &str) -> PathBuf {
    write_config_with(dir, port, log, "")
}

fn write_config_with(dir: &Path, port: u16, log: &str, extra: &str) -> PathBuf {
    let path = dir.join("vpush.toml");
    std::fs::write(
        &path,
        format!(
            "public_url = \"http://localhost:{port}\"\n\
             [server]\nlisten = \"127.0.0.1:{port}\"\n\
             [admin]\nsocket = \"{}\"\n\
             [store]\npath = \"{}\"\n\
             [log]\n{log}\n{extra}\n",
            dir.join("admin.sock").display(),
            dir.join("vpush.db").display()
        ),
    )
    .unwrap();
    path
}

impl Server {
    fn start(name: &str) -> Self {
        Self::start_with(name, |_| String::new())
    }

    /// `extra` gets the directory of the server and returns more config.
    fn start_with(name: &str, extra: impl Fn(&Path) -> String) -> Self {
        // Unix socket paths are short; the system temp dir keeps them so.
        let dir = std::env::temp_dir().join(format!("vpush-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let port = free_port();
        let config = write_config_with(&dir, port, "level = \"info\"", &extra(&dir));

        let out = Arc::new(Mutex::new(Vec::new()));
        let child = Self::spawn(&config, &out);
        let server = Self {
            child,
            dir,
            port,
            out,
        };
        server.wait_for("ready");
        server
    }

    /// Starts the server. What it logs, and what it says when it falls, is
    /// gathered in `out`.
    fn spawn(config: &Path, out: &Arc<Mutex<Vec<String>>>) -> Child {
        let mut child = Command::new(BIN)
            .args(["serve", "--config"])
            .arg(config)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pipes: [Box<dyn Read + Send>; 2] = [
            Box::new(child.stdout.take().unwrap()),
            Box::new(child.stderr.take().unwrap()),
        ];
        for pipe in pipes {
            let sink = Arc::clone(out);
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    sink.lock().unwrap().push(line);
                }
            });
        }
        child
    }

    /// Stops the server and starts it again, with the config as it is now.
    fn restart(&mut self) {
        self.signal("TERM");
        let _ = self.child.wait();
        let before = self.count("ready");
        self.child = Self::spawn(&self.config(), &self.out);
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.count("ready") == before {
            assert!(Instant::now() < deadline, "not started again:\n{}", self.lines().join("\n"));
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn config(&self) -> PathBuf {
        self.dir.join("vpush.toml")
    }

    fn lines(&self) -> Vec<String> {
        self.out.lock().unwrap().clone()
    }

    fn count(&self, needle: &str) -> usize {
        self.lines().iter().filter(|l| l.contains(needle)).count()
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.count(needle) == 0 {
            assert!(
                Instant::now() < deadline,
                "`{needle}` never appeared in:\n{}",
                self.lines().join("\n")
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn ctl(&self, args: &[&str]) -> (bool, String) {
        let out = Command::new(BIN)
            .args(["ctl", "--config"])
            .arg(self.config())
            .args(args)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    }

    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    }

    fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .arg(format!("-{name}"))
            .arg(self.child.id().to_string())
            .status()
            .unwrap();
        assert!(status.success());
    }
}

#[test]
fn health_over_http_and_over_the_socket() {
    let server = Server::start("health");

    let answer = server.get("/healthz");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains(r#""status":"ok""#), "{answer}");
    assert!(answer.to_lowercase().contains("x-request-id:"), "{answer}");

    let (ok, text) = server.ctl(&["health"]);
    assert!(ok, "{text}");
    assert!(text.contains(r#""status": "ok""#), "{text}");
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
}

#[test]
fn unknown_route_is_a_json_error_and_is_logged() {
    let server = Server::start("route");

    let answer = server.get("/nothing-here");
    assert!(answer.starts_with("HTTP/1.1 404"), "{answer}");
    assert!(answer.contains(r#""code":"not_found""#), "{answer}");

    server.wait_for("path=\"/nothing-here\"");
    // The health check stays out of the log at the default level.
    server.get("/healthz");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(server.count("path=\"/healthz\""), 0);
}

#[test]
fn log_level_changes_without_a_restart_and_comes_back() {
    let server = Server::start("loglevel");
    let debug_line = "admin request";

    server.ctl(&["health"]);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(server.count(debug_line), 0, "debug is off at the start");

    let (ok, text) = server.ctl(&["log", "set", "admin=debug", "--for", "2s"]);
    assert!(ok, "{text}");
    assert!(text.contains(r#""effective": "info,admin=debug""#), "{text}");

    server.ctl(&["health"]);
    server.wait_for(debug_line);

    server.wait_for("log override ran out");
    let (_, text) = server.ctl(&["log", "show"]);
    assert!(text.contains(r#""effective": "info""#), "{text}");
    assert!(text.contains(r#""overrides": []"#), "{text}");

    let before = server.count(debug_line);
    server.ctl(&["health"]);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(server.count(debug_line), before, "debug is off again");
}

#[test]
fn log_reset_removes_overrides() {
    let server = Server::start("logreset");

    let (ok, text) = server.ctl(&["log", "set", "trace,relay=debug"]);
    assert!(ok, "{text}");
    assert!(text.contains(r#""effective": "trace,relay=debug""#), "{text}");

    let (ok, text) = server.ctl(&["log", "reset"]);
    assert!(ok, "{text}");
    assert!(text.contains(r#""effective": "info""#), "{text}");
}

#[test]
fn bad_log_spec_is_refused_with_a_reason() {
    let server = Server::start("badspec");

    let (ok, text) = server.ctl(&["log", "set", "relay=loud"]);
    assert!(!ok);
    assert!(text.contains("`loud` is not a level"), "{text}");

    let (ok, text) = server.ctl(&["log", "set", "debug", "--for", "soon"]);
    assert!(!ok);
    assert!(text.contains("not a duration"), "{text}");
}

#[test]
fn sighup_applies_the_log_section() {
    let server = Server::start("sighup");

    write_config(&server.dir, server.port, "level = \"warn\"\n[log.targets]\nserve = \"info\"\nadmin = \"debug\"");
    server.signal("HUP");
    server.wait_for("config re-read");

    let (_, text) = server.ctl(&["log", "show"]);
    assert!(
        text.contains(r#""base": "warn,admin=debug,serve=info""#),
        "{text}"
    );
    server.wait_for("admin request");
}

#[test]
fn sighup_with_a_broken_config_changes_nothing() {
    let server = Server::start("sighupbad");

    std::fs::write(server.config(), "public_url = 5").unwrap();
    server.signal("HUP");
    server.wait_for("config re-read failed");

    let answer = server.get("/healthz");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

#[test]
fn sigterm_stops_cleanly_and_removes_the_socket() {
    let mut server = Server::start("stop");
    let socket = server.dir.join("admin.sock");
    assert!(socket.exists());

    server.signal("TERM");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = server.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "server did not stop");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}");
    assert!(!socket.exists(), "socket file left behind");
    server.wait_for("vpush stopped");
}

#[test]
fn second_server_on_the_same_socket_refuses_to_start() {
    let server = Server::start("twice");

    let out = Command::new(BIN)
        .args(["serve", "--config"])
        .arg(server.config())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("another vpush answers"), "{text}");

    // The first one is untouched.
    let (ok, text) = server.ctl(&["health"]);
    assert!(ok, "{text}");
}

#[test]
fn ctl_without_a_server_says_so() {
    let dir = std::env::temp_dir().join(format!("vpush-noserver-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(BIN)
        .args(["ctl", "--socket"])
        .arg(dir.join("admin.sock"))
        .arg("health")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("vpush is not running"), "{text}");
}

#[test]
fn check_config_lists_every_problem() {
    let dir = std::env::temp_dir().join(format!("vpush-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("vpush.toml");
    std::fs::write(
        &path,
        "public_url = \"ftp://x\"\n[server]\nlisten = \"nowhere\"\n",
    )
    .unwrap();
    let out = Command::new(BIN)
        .args(["check-config", "--config"])
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("public_url"), "{text}");
    assert!(text.contains("server.listen"), "{text}");
}

#[path = "../../vpush-server/tests/support/key.rs"]
mod key;

/// A server that plays Google, and a vpush configured to push through it.
async fn server_with_fcm(name: &str, send: wiremock::ResponseTemplate) -> (Server, wiremock::MockServer) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let google = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-1", "expires_in": 3600
        })))
        .mount(&google)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/veydan-test/messages:send"))
        .respond_with(send)
        .mount(&google)
        .await;

    let uri = google.uri();
    let server = Server::start_with(name, |dir| {
        let account = dir.join("fcm.json");
        std::fs::write(
            &account,
            serde_json::json!({
                "project_id": "veydan-test",
                "private_key": key::TEST_RSA_KEY,
                "client_email": "push@veydan-test.iam.gserviceaccount.com",
                "token_uri": format!("{uri}/token"),
            })
            .to_string(),
        )
        .unwrap();
        format!(
            "[apps.\"net.veydan.mobile\".fcm]\nservice_account = \"{}\"\nendpoint = \"{uri}\"\n",
            account.display()
        )
    });
    (server, google)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_push_goes_out_and_the_token_stays_out_of_the_log() {
    let sent = wiremock::ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({ "name": "projects/veydan-test/messages/1" }));
    let (server, google) = server_with_fcm("testpush", sent).await;
    assert!(server.count("app=\"net.veydan.mobile\" providers=\"fcm\"") == 1);

    let secret = "secret-device-token-0123456789";
    let (ok, text) = tokio::task::block_in_place(|| {
        server.ctl(&[
            "test-push",
            "--app",
            "net.veydan.mobile",
            "--token",
            secret,
        ])
    });
    assert!(ok, "{text}");
    assert!(text.contains(r#""outcome": "delivered""#), "{text}");

    let pushes: Vec<_> = google
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().ends_with("messages:send"))
        .collect();
    assert_eq!(pushes.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&pushes[0].body).unwrap();
    assert_eq!(body["message"]["token"], secret);
    assert_eq!(body["message"]["data"]["type"], "test");
    assert!(body["message"].get("notification").is_none());

    server.wait_for("test push");
    // Even with everything logged, the token itself is never written.
    tokio::task::block_in_place(|| {
        server.ctl(&["log", "set", "trace"]);
        server.ctl(&["test-push", "--app", "net.veydan.mobile", "--token", secret]);
    });
    server.wait_for("push attempt");
    let log = server.lines().join("\n");
    assert!(!log.contains(secret), "the token is in the log:\n{log}");
    assert!(!log.contains("access-1"), "the access token is in the log:\n{log}");
    assert!(!log.contains("PRIVATE KEY"), "the key is in the log:\n{log}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_push_reports_what_the_service_said() {
    let refused = wiremock::ResponseTemplate::new(404).set_body_json(serde_json::json!({
        "error": { "code": 404, "message": "Requested entity was not found.",
            "status": "NOT_FOUND",
            "details": [{ "errorCode": "UNREGISTERED" }] }
    }));
    let (server, _google) = server_with_fcm("testpushdead", refused).await;

    let (ok, text) = tokio::task::block_in_place(|| {
        server.ctl(&["test-push", "--app", "net.veydan.mobile", "--token", "gone"])
    });
    assert!(ok, "{text}");
    assert!(text.contains(r#""outcome": "dead_token""#), "{text}");
    assert!(text.contains(r#""code": "UNREGISTERED""#), "{text}");
}

#[test]
fn test_push_for_an_unknown_app_names_the_known_ones() {
    let server = Server::start("testpushapp");
    let (ok, text) = server.ctl(&["test-push", "--app", "nobody", "--token", "t"]);
    assert!(!ok);
    assert!(text.contains("no apps are configured"), "{text}");

    let (ok, text) = server.ctl(&["test-push", "--app", "a", "--provider", "pigeon", "--token", "t"]);
    assert!(!ok);
    assert!(text.contains("`pigeon` is not a provider"), "{text}");
}

#[test]
fn a_broken_service_account_stops_the_start_with_a_reason() {
    let dir = std::env::temp_dir().join(format!("vpush-badfcm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fcm.json"), "{}").unwrap();
    let config = write_config_with(
        &dir,
        free_port(),
        "level = \"info\"",
        &format!(
            "[apps.app.fcm]\nservice_account = \"{}\"\n",
            dir.join("fcm.json").display()
        ),
    );
    let out = Command::new(BIN)
        .args(["serve", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("apps.app.fcm.service_account"), "{text}");
    assert!(text.contains("not a service account file"), "{text}");
}

/// A request signed the way a client signs it (NIP-98).
fn signed(keys: &nostr::key::Keys, method: &str, url: &str, body: &[u8]) -> String {
    use base64::Engine;
    use nostr::prelude::*;
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut tags = vec![
        Tag::parse(["u", url]).unwrap(),
        Tag::parse(["method", method]).unwrap(),
        Tag::parse(["nonce", hex(Keys::generate().public_key().as_bytes()).as_str()]).unwrap(),
    ];
    if !body.is_empty() {
        let hash = ring::digest::digest(&ring::digest::SHA256, body);
        tags.push(Tag::parse(["payload", hex(hash.as_ref()).as_str()]).unwrap());
    }
    let event = EventBuilder::new(Kind::HttpAuth, "").tags(tags).finalize(keys).unwrap();
    format!(
        "Nostr {}",
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&event).unwrap())
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_on_a_gated_relay_becomes_a_push_and_the_key_of_the_relay_stays_out_of_the_log() {
    use nostr::prelude::*;
    use nostr_sdk::local_relay::LocalRelay;

    const GATE_KEY: &str = "gate-key-0123456789abcdef0123456789abcdef";

    let relay = LocalRelay::new();
    relay.run().await.unwrap();
    let relay_url = relay.url().await.to_string().trim_end_matches('/').to_string();

    let sent = wiremock::ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({ "name": "projects/veydan-test/messages/1" }));
    let google = wiremock::MockServer::start().await;
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "access-1", "expires_in": 3600
            })))
            .mount(&google)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/veydan-test/messages:send"))
            .respond_with(sent)
            .mount(&google)
            .await;
    }

    let uri = google.uri();
    let server = tokio::task::block_in_place(|| {
        Server::start_with("gated", |dir| {
            std::fs::write(dir.join("relay.key"), format!("{GATE_KEY}\n")).unwrap();
            let account = dir.join("fcm.json");
            std::fs::write(
                &account,
                serde_json::json!({
                    "project_id": "veydan-test",
                    "private_key": crate::key::TEST_RSA_KEY,
                    "client_email": "push@veydan-test.iam.gserviceaccount.com",
                    "token_uri": format!("{uri}/token"),
                })
                .to_string(),
            )
            .unwrap();
            format!(
                "[log.targets]\nnostr_sdk = \"trace\"\nnostr_relay_pool = \"trace\"\nasync_wsocket = \"trace\"\nrelay = \"trace\"\npipeline = \"trace\"\n\
                 [apps.\"net.veydan.mobile\".fcm]\nservice_account = \"{}\"\nendpoint = \"{uri}\"\n\
                 [[relays.allow]]\nurl = \"{relay_url}\"\napi_key_file = \"{}\"\n\
                 [pipeline]\nthrottle_secs = 1\n",
                account.display(),
                dir.join("relay.key").display(),
            )
        })
    });
    assert_eq!(server.count(&format!("{relay_url} (with a key)")), 1, "{}", server.lines().join("\n"));

    // A phone registers, as the app would.
    let alice = Keys::generate();
    let body = serde_json::json!({
        "app_id": "net.veydan.mobile",
        "channel": { "provider": "fcm", "token": "token-of-the-phone" },
        "locale": "en",
        "relays": [{ "url": relay_url, "dm": true, "groups": true }],
    })
    .to_string();
    let path = "/v1/devices/phone-0001";
    let answer = reqwest::Client::new()
        .put(format!("http://127.0.0.1:{}{path}", server.port))
        .header(
            "authorization",
            signed(&alice, "PUT", &format!("http://localhost:{}{path}", server.port), body.as_bytes()),
        )
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(answer.status(), 200);

    tokio::task::block_in_place(|| server.wait_for("state=\"connected\""));
    tokio::task::block_in_place(|| server.wait_for("stock taken"));

    let message = EventBuilder::new(Kind::GiftWrap, "sealed")
        .tags([Tag::public_key(alice.public_key())])
        .finalize(&Keys::generate())
        .unwrap();
    relay.add_event(message.clone()).await.unwrap();

    tokio::task::block_in_place(|| server.wait_for("outcome=\"delivered\""));
    let pushes: Vec<_> = google
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().ends_with("messages:send"))
        .collect();
    assert_eq!(pushes.len(), 1);
    let push: serde_json::Value = serde_json::from_slice(&pushes[0].body).unwrap();
    assert_eq!(push["message"]["data"]["type"], "dm");
    assert_eq!(push["message"]["data"]["title"], "Direct message");
    assert_eq!(push["message"]["data"]["event_id"], message.id.to_hex());

    // The way of the event, from the relay to the push, by one number.
    let trace = push["message"]["data"]["trace"].as_str().unwrap();
    assert!(server.count(&format!("trace=\"{trace}\"")) + server.count(&format!("trace={trace}")) >= 2);

    let (ok, text) = tokio::task::block_in_place(|| server.ctl(&["relays"]));
    assert!(ok, "{text}");
    assert!(text.contains(r#""state": "connected""#), "{text}");
    assert!(!text.contains(GATE_KEY), "{text}");

    let log = server.lines().join("\n");
    assert!(!log.contains(GATE_KEY), "the key of the relay is in the log");
    assert!(!log.contains("token-of-the-phone"), "the token is in the log");
}

/// The relays of the world are behind TLS, and the relays of the tests are
/// not. This one is: something that takes a connection and says nothing,
/// which is enough for the server to begin a TLS handshake.
#[tokio::test(flavor = "multi_thread")]
async fn a_relay_behind_tls_does_not_take_the_server_down() {
    use nostr::prelude::*;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_port = listener.local_addr().unwrap().port();
    let asked = Arc::new(Mutex::new(0u32));
    let counter = Arc::clone(&asked);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            *counter.lock().unwrap() += 1;
            std::thread::sleep(Duration::from_millis(200));
            drop(stream);
        }
    });
    let relay_url = format!("wss://localhost:{relay_port}");

    let sent = wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({ "name": "m/1" }));
    let (server, _google) = {
        // The relay of this test is put on the list of a server that pushes
        // through the mock.
        let relay_url = relay_url.clone();
        let (mut server, google) = server_with_fcm("tls-before", sent).await;
        // Started again, with the relay on its list.
        tokio::task::block_in_place(|| {
            let config = std::fs::read_to_string(server.config()).unwrap();
            std::fs::write(
                server.config(),
                format!("{config}\n[[relays.allow]]\nurl = \"{relay_url}\"\n"),
            )
            .unwrap();
            server.restart();
        });
        (server, google)
    };

    let alice = Keys::generate();
    let body = serde_json::json!({
        "app_id": "net.veydan.mobile",
        "channel": { "provider": "fcm", "token": "token-of-the-phone" },
        "relays": [{ "url": relay_url, "dm": true, "groups": true }],
    })
    .to_string();
    let path = "/v1/devices/phone-0001";
    let answer = reqwest::Client::new()
        .put(format!("http://127.0.0.1:{}{path}", server.port))
        .header(
            "authorization",
            signed(&alice, "PUT", &format!("http://localhost:{}{path}", server.port), body.as_bytes()),
        )
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(answer.status(), 200);

    tokio::task::block_in_place(|| {
        server.wait_for(&format!("watching relay={relay_url}"));
        // Long enough for a handshake to begin, fail, and begin again.
        let deadline = Instant::now() + Duration::from_secs(10);
        while *asked.lock().unwrap() == 0 {
            assert!(Instant::now() < deadline, "the server never came to the relay");
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_secs(2));
    });

    let log = server.lines().join("\n");
    assert!(!log.contains("panicked"), "{log}");
    let (ok, text) = tokio::task::block_in_place(|| server.ctl(&["health"]));
    assert!(ok, "the server is gone: {text}\n{log}");
    let (_, relays) = tokio::task::block_in_place(|| server.ctl(&["relays"]));
    assert!(relays.contains(&relay_url), "{relays}");
}
