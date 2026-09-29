//! A client of one's own, to ask a running server by hand.
//!
//!   cargo run -p vpush-server --example probe -- <server> info
//!   cargo run -p vpush-server --example probe -- <server> put <token> [relay…]
//!   cargo run -p vpush-server --example probe -- <server> get
//!   cargo run -p vpush-server --example probe -- <server> test
//!   cargo run -p vpush-server --example probe -- <server> delete
//!
//! `<server>` is the address as clients know it: https://vpush.veydan.net
//!
//! The key it signs with is made on the first run and kept in
//! `target/probe.key`, so the commands that follow speak as the same owner.
//! It is a key for tests and owns nothing else.

use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use nostr::prelude::*;
use serde_json::{json, Value};
use vpush_server::auth::sha256_hex;

const DEVICE: &str = "probe-device-0001";

fn key_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/probe.key")
}

fn keys() -> Keys {
    let path = key_file();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(keys) = Keys::parse(text.trim()) {
            return keys;
        }
    }
    let keys = Keys::generate();
    let _ = std::fs::write(&path, keys.secret_key().to_secret_hex());
    keys
}

fn signed(keys: &Keys, method: &str, url: &str, body: &[u8]) -> String {
    let mut nonce = [0u8; 8];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce).unwrap();
    let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let mut tags = vec![
        Tag::parse(["u", url]).unwrap(),
        Tag::parse(["method", method]).unwrap(),
        Tag::parse(["nonce", nonce.as_str()]).unwrap(),
    ];
    if !body.is_empty() {
        tags.push(Tag::parse(["payload", sha256_hex(body).as_str()]).unwrap());
    }
    let event = EventBuilder::new(Kind::HttpAuth, "")
        .tags(tags)
        .finalize(keys)
        .unwrap();
    format!("Nostr {}", STANDARD.encode(serde_json::to_vec(&event).unwrap()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (server, command) = match args.as_slice() {
        [server, command, ..] => (server.trim_end_matches('/').to_string(), command.as_str()),
        _ => {
            eprintln!("usage: probe <server> info | put <token> [relay…] | get | test | delete");
            std::process::exit(2);
        }
    };
    let rest = &args[2..];
    let keys = keys();
    eprintln!("owner: {}", keys.public_key().to_bech32()?);

    let device = format!("{server}/v1/devices/{DEVICE}");
    let (method, url, body): (&str, String, Option<Value>) = match command {
        "info" => ("GET", format!("{server}/v1/info"), None),
        "get" => ("GET", device, None),
        "delete" => ("DELETE", device, None),
        "test" => ("POST", format!("{device}/test"), None),
        "put" => {
            let token = rest.first().ok_or("put needs a token")?;
            let relays: Vec<Value> = rest[1..]
                .iter()
                .map(|url| json!({ "url": url, "dm": true, "groups": true }))
                .collect();
            let body = json!({
                "app_id": std::env::var("VPUSH_PROBE_APP").unwrap_or_else(|_| "net.veydan.mobile".into()),
                "channel": { "provider": "fcm", "token": token },
                "locale": std::env::var("VPUSH_PROBE_LOCALE").unwrap_or_else(|_| "ru".into()),
                "app_version": "probe",
                "relays": relays,
                "groups": [{ "id": "11".repeat(32), "name": "Проба" }],
            });
            ("PUT", device, Some(body))
        }
        other => return Err(format!("`{other}` is not a command").into()),
    };

    let bytes = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
    let mut request = reqwest::Client::new().request(method.parse()?, &url);
    if command != "info" {
        request = request.header("authorization", signed(&keys, method, &url, &bytes));
    }
    if !bytes.is_empty() {
        request = request.header("content-type", "application/json").body(bytes);
    }
    let response = request.send().await?;
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = response.text().await?;
    eprintln!("{method} {url} -> {status}  request_id={request_id}");
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        Err(_) => println!("{text}"),
    }
    if !status.is_success() {
        std::process::exit(1);
    }
    Ok(())
}
