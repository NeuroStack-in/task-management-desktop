//! Backend transport — the sole network egress. **Thread B** (BUILD-PLAN §4): every 300 s, if signed
//! in, assemble one cycle and drain the outbox to `POST /v1/agent/batch`. **Auth-gated, not
//! timer-gated**, so the fleet table stays honest about an online-but-not-tracking agent. On each ack
//! the outbox prunes to `watermark_seq`, and screenshot bytes go **S3-direct** to the ack's
//! host-pinned `upload_urls` (the retry/backoff *is* the 300 s tick).
//!
//! Not live-verified — needs a signed-in session against the live pool.

pub mod batch;
pub mod client;
pub mod config;
pub mod projects;
pub mod tasks;
pub mod timesheet;

use std::path::Path;
use std::time::Duration;

use tauri::Manager;
use wp_agent_contract::PresignedUpload;

use crate::cycle::{self, SCREENSHOT_ATTEMPT_CAP};
use crate::monitor::screenshot::is_allowed_upload_host;
use crate::state::AppState;

const CYCLE_SECS: u64 = 300;

/// Spawn the sender loop on Tauri's async (tokio) runtime.
pub fn spawn_sender(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let http = client::api_client();
        let upload = client::upload_client();
        loop {
            tokio::time::sleep(Duration::from_secs(CYCLE_SECS)).await;
            let state = app.state::<AppState>();

            // Auth-gated: no token → skip (don't grow the outbox while signed out).
            let Some(id_token) = state.auth.id_token().await else {
                continue;
            };
            let ingest_url = state.auth.config().ingest_url.clone();

            refresh_location(&app).await;
            cycle::assemble_and_enqueue(&state);
            drain(&http, &upload, &ingest_url, &id_token, &state).await;
        }
    });
}

/// Refresh the cached device location — **consent-gated, fails closed**. With consent, capture a
/// fresh OS fix (blocking WinRT → `spawn_blocking`) for the next heartbeat; without consent, clear any
/// cached fix so a withdrawal takes effect on the very next cycle.
async fn refresh_location(app: &tauri::AppHandle) {
    let consented = app
        .state::<AppState>()
        .consent
        .load(std::sync::atomic::Ordering::Relaxed);
    let fix = if consented {
        tokio::task::spawn_blocking(crate::location::capture)
            .await
            .unwrap_or(None)
    } else {
        None
    };
    *app.state::<AppState>().location.lock().unwrap() = fix;
}

/// Send oldest-first until the outbox is empty or a send fails (then retry next tick). After each ack,
/// prune and upload that ack's screenshots.
async fn drain(
    http: &reqwest::Client,
    upload: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    state: &AppState,
) {
    loop {
        let next = { state.outbox.lock().unwrap().next_batch().cloned() };
        let Some(batch) = next else { break };

        match batch::send_batch(http, ingest_url, id_token, &batch).await {
            Ok(ack) => {
                state.outbox.lock().unwrap().prune_to(ack.watermark_seq);

                // Config rail: the ack advertises the server version; on a mismatch, pull (ETag-conditional)
                // and apply live — cadence/blur/silent + app/URL rules the monitor threads read each tick.
                let (stale, etag) = {
                    let c = state.config.lock().unwrap();
                    (c.needs_pull(ack.config_version), c.etag())
                };
                if stale {
                    match config::pull_config(http, ingest_url, id_token, etag.as_deref()).await {
                        Ok(config::ConfigPull::Fresh { config, etag }) => {
                            let v = config.version;
                            state.config.lock().unwrap().apply(config, etag);
                            tracing::info!("config applied (version {v})");
                        }
                        Ok(config::ConfigPull::NotModified) => {}
                        Err(e) => tracing::warn!("config pull failed: {e}"),
                    }
                }

                for pu in &ack.upload_urls {
                    upload_screenshot(upload, state, pu).await;
                }
            }
            Err(e) => {
                tracing::warn!("batch send failed, will retry next cycle: {e}");
                break;
            }
        }
    }
}

/// PUT one screenshot's bytes to its presigned S3 URL (host-pinned). Success deletes the local file;
/// failure bumps the attempt count and drops the shot once it hits the cap (BUILD-PLAN §5).
async fn upload_screenshot(upload: &reqwest::Client, state: &AppState, pu: &PresignedUpload) {
    let path = {
        state
            .screenshots
            .lock()
            .unwrap()
            .get(&pu.screenshot_id)
            .map(|s| s.path.clone())
    };
    let Some(path) = path else { return };

    if !is_allowed_upload_host(&pu.url) {
        tracing::warn!("rejected non-amazonaws screenshot upload host");
        return;
    }

    match put_bytes(upload, &pu.url, &path).await {
        Ok(()) => {
            state.screenshots.lock().unwrap().remove(&pu.screenshot_id);
            let _ = std::fs::remove_file(&path);
        }
        Err(e) => {
            tracing::warn!("screenshot upload failed: {e}");
            let mut shots = state.screenshots.lock().unwrap();
            if let Some(s) = shots.get_mut(&pu.screenshot_id) {
                s.attempts += 1;
                if s.attempts >= SCREENSHOT_ATTEMPT_CAP {
                    let p = s.path.clone();
                    shots.remove(&pu.screenshot_id);
                    drop(shots);
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }
}

async fn put_bytes(client: &reqwest::Client, url: &str, path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read:{e}"))?;
    let resp = client
        .put(url)
        .header("Content-Type", "image/webp")
        .body(bytes)
        .send()
        .await
        .map_err(|e| format!("network:{e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("status:{}", resp.status().as_u16()))
    }
}
