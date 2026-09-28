// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

//! A session = a signer + a live ingress loop + an outbox pump. It exists
//! only while the runtime holds signing keys; the runtime restarts it when
//! the signer or the relay pool changes.

use async_trait::async_trait;
use messenger_core::traits::{SystemClock, UiEvent};
use messenger_core::{Ack, Context, DmInbound, Effect, Envelope, Handler, Outbound, PubKey, Result, Timestamp, Transport};
use messenger_core::Clock;
use messenger_ingress::{filters, Dispatcher, EffectSink, IngressLoop, Outbox};
use messenger_store::Store;
use messenger_transport::RelayPool;
use nostr::key::Keys;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub const UI_EVENT_INBOUND_DM: &str = "inbound.dm";

/// Effects go to the outbox (with an immediate pump) and to the UI channel.
pub struct RuntimeSink {
    pub pool: Arc<RelayPool>,
    pub outbox: Outbox,
    pub ui: broadcast::Sender<UiEvent>,
}

#[async_trait]
impl EffectSink for RuntimeSink {
    async fn send(&self, out: Outbound) -> Result<Ack> {
        // Persist first so a crash between here and the relay ack cannot
        // lose the request; then try right away.
        self.outbox.enqueue(out).await?;
        self.outbox.pump(self.pool.as_ref()).await?;
        Ok(Ack { accepted_by: vec![], rejected_by: vec![] })
    }

    fn emit(&self, event: UiEvent) {
        let _ = self.ui.send(event);
    }

    fn notify(&self, title: String, body: Option<String>, chat_id: Option<String>) {
        let _ = self.ui.send(UiEvent {
            name: "notify".into(),
            payload: serde_json::json!({ "title": title, "body": body, "chat_id": chat_id }),
        });
    }
}

/// Stage 3 placeholder: surfaces DMs to the UI feed. Stage 5 replaces it
/// with the real DM handler (relationship matrix, storage, chats).
pub struct DebugDmHandler;

#[async_trait]
impl Handler<DmInbound> for DebugDmHandler {
    async fn handle(&self, msg: DmInbound, ctx: &Context) -> Result<Vec<Effect>> {
        let historical = msg.created_at < ctx.session_started_at;
        let (kind, text) = match Envelope::parse(&msg.content) {
            Ok(env) => (env.t.clone(), env.as_text().map(String::from)),
            Err(_) => ("raw".to_string(), Some(msg.content.clone())),
        };
        Ok(vec![Effect::Emit(UiEvent {
            name: UI_EVENT_INBOUND_DM.into(),
            payload: serde_json::json!({
                "sender": msg.sender.as_hex(),
                "rumor_id": msg.rumor_id.as_hex(),
                "created_at": msg.created_at.secs(),
                "historical": historical,
                "type": kind,
                "text": text,
                "source": msg.envelope.source,
            }),
        })])
    }
}

pub struct Session {
    pub keys: Keys,
    pub started_at: Timestamp,
    ingress: IngressLoop,
    pump: JoinHandle<()>,
}

impl Session {
    /// Subscribe to the inbox, start the ingress loop and the outbox pump.
    pub async fn start(
        store: Store,
        pool: Arc<RelayPool>,
        keys: Keys,
        outbox: Outbox,
        ui: broadcast::Sender<UiEvent>,
        dispatcher: Arc<Dispatcher>,
    ) -> Result<Self> {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let started_at = clock.now();
        let me = PubKey::parse(&keys.public_key().to_hex()).expect("valid pubkey");

        let since = Timestamp(started_at.secs() - filters::DM_LIVE_MARGIN_SECS);
        pool.send(Outbound::Subscribe {
            id: messenger_core::SubId(filters::SUB_DM_LIVE.into()),
            filter: filters::dm_inbox(&me, since),
            scope: messenger_core::Scope::Own,
        })
        .await?;

        let sink: Arc<dyn EffectSink> = Arc::new(RuntimeSink { pool: pool.clone(), outbox: outbox.clone(), ui });
        let ctx = Context { my_pubkey: me, session_started_at: started_at, clock };
        let ingress = IngressLoop::spawn(pool.events(), store, Some(keys.clone()), dispatcher, ctx, sink);

        let pump_pool = pool.clone();
        let pump = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if let Err(e) = outbox.pump(pump_pool.as_ref()).await {
                    eprintln!("messenger outbox: pump failed: {e}");
                }
            }
        });
        Ok(Self { keys, started_at, ingress, pump })
    }

    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        self.ingress.stats.snapshot()
    }

    pub fn stop(&self) {
        self.ingress.abort();
        self.pump.abort();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}
