//! The running server: what starts, in which order, and how it stops.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use crate::admin::{AdminSocket, AdminState};
use crate::api::Api;
use crate::config::Config;
use crate::delivery::Providers;
use crate::relays::RelayPolicy;
use crate::store::{SqliteStore, Store};
use crate::logging::{self, LogControl};
use crate::{api, version};

/// Runs until SIGINT or SIGTERM.
pub async fn serve(config_path: PathBuf) -> anyhow::Result<()> {
    let config = Config::load(&config_path)?;
    let format = config.log_format().map_err(anyhow::Error::msg)?;
    let base = config.log_spec().map_err(anyhow::Error::msg)?;

    let log = logging::init(format, base);
    log.spawn_expiry();
    version::mark_started();

    tracing::info!(
        version = version::VERSION,
        git_sha = version::GIT_SHA,
        built_at = version::built_at(),
        config = %config_path.display(),
        "vpush starts"
    );
    for (key, value) in config.summary() {
        tracing::info!(key, value, "config");
    }

    let providers = Arc::new(Providers::from_config(&config)?);
    for (app, kinds) in providers.summary() {
        tracing::info!(app, providers = kinds, "app");
    }

    let relays = Arc::new(RelayPolicy::from_config(&config).map_err(anyhow::Error::msg)?);
    let sqlite = SqliteStore::open(&config.store.path).await?;
    tracing::info!(
        path = %config.store.path.display(),
        schema = sqlite.schema_version().await?,
        devices = sqlite.counts().await?.devices,
        "database opened"
    );
    let store: Arc<dyn Store> = Arc::new(sqlite.clone());
    tokio::spawn(forget_expired(Arc::clone(&store)));

    let admin = AdminSocket::bind(&config.admin.socket)
        .await
        .with_context(|| format!("admin socket {}", config.admin.socket.display()))?;
    let listener = TcpListener::bind(config.listen_addr())
        .await
        .with_context(|| format!("listen on {}", config.server.listen))?;
    tracing::info!(
        listen = %listener.local_addr()?,
        admin_socket = %config.admin.socket.display(),
        "ready"
    );

    let (stop_tx, stop_rx) = watch::channel(false);
    tokio::spawn(watch_signals(stop_tx, config_path, Arc::clone(&log)));

    let admin_task = tokio::spawn(admin.run(
        AdminState {
            log,
            providers: Arc::clone(&providers),
            store: Arc::clone(&store),
        },
        stopped(stop_rx.clone()),
    ));

    let api = Api::new(Arc::new(config), store, providers, relays);
    axum::serve(listener, api::router(api))
        .with_graceful_shutdown(stopped(stop_rx))
        .await
        .context("http server")?;
    let _ = admin_task.await;
    sqlite.close().await;

    tracing::info!("vpush stopped");
    Ok(())
}

/// Once an hour, forgets the registrations nobody renewed.
async fn forget_expired(store: Arc<dyn Store>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        tick.tick().await;
        match store.purge_expired(api::now()).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(devices = n, "expired registrations forgotten"),
            Err(e) => tracing::error!(error = %e, "cannot forget expired registrations"),
        }
    }
}

async fn stopped(mut rx: watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            return;
        }
    }
}

/// SIGINT and SIGTERM stop the server; SIGHUP re-reads the config.
async fn watch_signals(stop: watch::Sender<bool>, config_path: PathBuf, log: Arc<LogControl>) {
    let (Ok(mut int), Ok(mut term), Ok(mut hup)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) else {
        tracing::error!("cannot listen for signals; stop the server with SIGKILL");
        return;
    };
    loop {
        tokio::select! {
            _ = int.recv() => { tracing::info!(signal = "SIGINT", "stopping"); break; }
            _ = term.recv() => { tracing::info!(signal = "SIGTERM", "stopping"); break; }
            _ = hup.recv() => reload(&config_path, &log),
        }
    }
    let _ = stop.send(true);
}

/// Applies what can change without a restart; today that is the log section.
fn reload(config_path: &Path, log: &LogControl) {
    match Config::load(config_path).and_then(|c| {
        c.log_spec().map_err(|e| crate::config::ConfigError {
            path: config_path.to_path_buf(),
            problems: vec![e],
        })
    }) {
        Ok(base) => {
            let view = log.set_base(base);
            tracing::info!(
                base = %view.base,
                effective = %view.effective,
                "config re-read: log levels applied, the rest needs a restart"
            );
        }
        Err(e) => tracing::error!(error = %e, "config re-read failed, nothing changed"),
    }
}
