//! Backend transport — the sole network egress. **Thread B** (BUILD-PLAN §4): every 300 s, if signed
//! in, assemble one cycle and drain the outbox to `POST /v1/agent/batch`. **Auth-gated, not
//! timer-gated**, so the fleet table stays honest about an online-but-not-tracking agent. On each ack
//! the outbox prunes to `watermark_seq` (the retry/backoff *is* the 300 s tick).
//!
//! Not live-verified — needs a signed-in session against the live pool.

pub mod batch;
pub mod client;

use std::time::Duration;

use tauri::Manager;

use crate::cycle;
use crate::state::AppState;

const CYCLE_SECS: u64 = 300;

/// Spawn the sender loop on Tauri's async (tokio) runtime.
pub fn spawn_sender(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let http = client::api_client();
        loop {
            tokio::time::sleep(Duration::from_secs(CYCLE_SECS)).await;
            let state = app.state::<AppState>();

            // Auth-gated: no token → skip (don't grow the outbox while signed out).
            let Some(id_token) = state.auth.id_token().await else {
                continue;
            };
            let ingest_url = state.auth.config().ingest_url.clone();

            // One cycle: heartbeat + drained events → outbox (M4 adds activity, M5 screenshots).
            cycle::assemble_and_enqueue(&state);
            drain(&http, &ingest_url, &id_token, &state).await;
        }
    });
}

/// Send oldest-first until the outbox is empty or a send fails (then retry next tick).
async fn drain(http: &reqwest::Client, ingest_url: &str, id_token: &str, state: &AppState) {
    loop {
        let next = { state.outbox.lock().unwrap().next_batch().cloned() };
        let Some(batch) = next else { break };

        match batch::send_batch(http, ingest_url, id_token, &batch).await {
            Ok(ack) => {
                state.outbox.lock().unwrap().prune_to(ack.watermark_seq);
                if state.config.lock().unwrap().needs_pull(ack.config_version) {
                    // TODO(M2): GET /v1/agent/config (ETag) → apply live.
                    // TODO(M5): PUT screenshot bytes to ack.upload_urls (host-pinned).
                }
            }
            Err(e) => {
                tracing::warn!("batch send failed, will retry next cycle: {e}");
                break;
            }
        }
    }
}
