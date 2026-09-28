// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! Headless messenger over `MessengerRuntime`. Second participant for smoke
//! tests and living proof that the runtime works without a UI.
//!
//! ```text
//! messenger-cli [--data-dir DIR] keygen [--password PW]
//! messenger-cli [--data-dir DIR] import <nsec|ncryptsec> <secret> [--password PW]
//! messenger-cli [--data-dir DIR] whoami
//! messenger-cli [--data-dir DIR] relays
//! messenger-cli [--data-dir DIR] relay-add <wss-url> [--key API_KEY]
//! messenger-cli [--data-dir DIR] send <npub|hex> <text…>
//! messenger-cli [--data-dir DIR] tail
//! ```
//!
//! Secrets live in `<data-dir>/secrets.json` in plaintext: development only.

use messenger_core::MessengerConfig;
use messenger_runtime::MessengerRuntime;
use messenger_testkit::FileSecretStore;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn usage() -> ! {
    eprintln!(
        "usage: messenger-cli [--data-dir DIR] <keygen [--password PW] | import <nsec|ncryptsec> <secret> [--password PW] \
         | whoami | relays | relay-add <url> [--key K] | send <to> <text…> | tail>"
    );
    std::process::exit(2)
}

fn take_flag(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    if i + 1 >= args.len() {
        usage();
    }
    let v = args.remove(i + 1);
    args.remove(i);
    Some(v)
}

#[tokio::main]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir = take_flag(&mut args, "--data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./messenger-cli-data"));
    let password = take_flag(&mut args, "--password");
    let api_key = take_flag(&mut args, "--key");
    if args.is_empty() {
        usage();
    }
    let cmd = args.remove(0);

    let config = MessengerConfig::new(data_dir.clone());
    let secrets = Arc::new(FileSecretStore::open(data_dir.join("secrets.json")).await.unwrap_or_else(die));
    let rt = MessengerRuntime::start(config, secrets).await.unwrap_or_else(die);

    match cmd.as_str() {
        "keygen" => {
            let pw = password.unwrap_or_else(|| "cli-dev-password".into());
            let created = rt.identity().create(&pw).await.unwrap_or_else(die);
            rt.refresh_signer().await.unwrap_or_else(die);
            println!("npub      {}", created.identity.npub);
            println!("pubkey    {}", created.identity.pubkey.as_hex());
            println!("ncryptsec {}", created.ncryptsec);
        }
        "import" => {
            if args.len() < 2 {
                usage();
            }
            let kind = args[0].as_str();
            let secret = args[1].as_str();
            let id = match kind {
                "nsec" => rt.identity().import_nsec(secret).await,
                "ncryptsec" => rt.identity().import_ncryptsec(secret, &password.unwrap_or_default()).await,
                _ => usage(),
            }
            .unwrap_or_else(die);
            rt.refresh_signer().await.unwrap_or_else(die);
            println!("imported {}", id.npub);
        }
        "whoami" => match rt.identity().get().await.unwrap_or_else(die) {
            Some(id) => println!("{}\n{}", id.npub, id.pubkey.as_hex()),
            None => println!("(no identity — run keygen or import)"),
        },
        "relays" => {
            wait_connect(&rt).await;
            for r in rt.relays().list().await.unwrap_or_else(die) {
                println!(
                    "{:<12} {:<9} {:<8} {} {}",
                    format!("{:?}", r.state).to_lowercase(),
                    r.source,
                    r.auth_type.unwrap_or_else(|| "-".into()),
                    if r.enabled { "on " } else { "off" },
                    r.url
                );
            }
        }
        "relay-add" => {
            if args.is_empty() {
                usage();
            }
            let v = rt.relays().add_user(&args[0], api_key).await.unwrap_or_else(die);
            println!("added {} ({})", v.url, v.auth_type.unwrap_or_else(|| "no auth".into()));
        }
        "send" => {
            if args.len() < 2 {
                usage();
            }
            let to = args.remove(0);
            let text = args.join(" ");
            wait_connect(&rt).await;
            let id = rt.send_text_dm(&to, &text).await.unwrap_or_else(die);
            // Give the outbox pump a moment to report.
            tokio::time::sleep(Duration::from_millis(500)).await;
            let pending = rt.outbox().pending().await.unwrap_or(0);
            println!("queued {id} (pending in outbox: {pending})");
        }
        "tail" => {
            wait_connect(&rt).await;
            let st = rt.status().await.unwrap_or_else(die);
            eprintln!(
                "listening as session={} relays={}/{} — Ctrl-C to stop",
                st.session_active, st.relays_connected, st.relays_total
            );
            let mut rx = rt.ui_events();
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        Ok(e) => println!("{} {}", e.name, e.payload),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => eprintln!("(lagged {n})"),
                        Err(_) => break,
                    },
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        }
        _ => usage(),
    }
    rt.shutdown().await;
}

async fn wait_connect(rt: &MessengerRuntime) {
    for _ in 0..40 {
        let st = rt.status().await.unwrap_or_else(die);
        if st.relays_connected > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    eprintln!("warning: no relay connected yet");
}

fn die<T>(e: messenger_core::MessengerError) -> T {
    eprintln!("error: {e}");
    std::process::exit(1)
}
