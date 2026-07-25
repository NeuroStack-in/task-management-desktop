//! Backend transport — the sole network egress. **Thread B** (BUILD-PLAN §4). Two send triggers:
//! the **heartbeat cycle** fires on the owner's **screenshot cadence** (3/5/10 min; `Off` → 5 min
//! fallback) and carries the heartbeat + that window's activity/screenshot-meta/location; **timer
//! transitions** fire out-of-band (a start/stop flushes immediately, seconds not minutes). Either
//! trigger assembles one cycle and drains the outbox to `POST /v1/agent/batch`. **Auth-gated, not
//! timer-gated**, so the fleet table stays honest about an online-but-not-tracking agent. On each ack
//! the outbox prunes to `watermark_seq`, and screenshot bytes go **S3-direct** to the ack's
//! host-pinned `upload_urls`.
//!
//! Not live-verified — needs a signed-in session against the live pool.

pub mod batch;
pub mod client;
pub mod config;
pub mod enroll;
pub mod projects;
pub mod tasks;
pub mod timesheet;

use std::path::Path;
use std::time::Duration;

use tauri::{Emitter, Manager};
use wp_agent_contract::PresignedUpload;

use crate::cycle::{self, SCREENSHOT_ATTEMPT_CAP};
use crate::monitor::screenshot::is_allowed_upload_host;
use crate::state::AppState;

/// Fallback heartbeat interval when the owner set cadence to `Off` (no screenshots) — the fleet still
/// needs periodic heartbeats, so we don't go silent.
const DEFAULT_CYCLE_SECS: u64 = 300;

/// Interval for `spawn_config_poller` — one conditional (ETag) config GET per minute; a 304 except
/// in the minute after an admin changed the policy.
const CONFIG_POLL_SECS: u64 = 60;

/// Spawn the sender loop on Tauri's async (tokio) runtime.
///
/// Two send triggers, deliberately different:
/// - **Heartbeat cycle** — periodic, its interval **the owner's screenshot cadence** (3/5/10 min): the
///   batch carries the heartbeat + that window's activity/screenshot-meta/location, sent at the end of
///   the window. Cadence `Off` falls back to 5 min so fleet health still flows.
/// - **Timer transitions** — out-of-band: a start/stop flips `flush`, so `TimerStarted`
///   (start + project + task) / `TimerStopped` (stop time) reach the backend in seconds, not on the
///   cycle (LLD §4).
pub fn spawn_sender(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let http = client::api_client();
        let upload = client::upload_client();
        // Cloned once so `select!` can await it without re-borrowing `AppState` each tick.
        let flush = app.state::<AppState>().flush.clone();
        // Tracks the last observed session so a sign-out can be reported once, as an edge.
        let mut was_signed_in = false;
        loop {
            // The heartbeat interval tracks the current screenshot cadence, read fresh each cycle so a
            // policy change takes effect on the next window.
            let cycle_secs = {
                let cadence = app
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .get()
                    .tracking
                    .cadence;
                cadence
                    .interval_secs()
                    .map(u64::from)
                    .unwrap_or(DEFAULT_CYCLE_SECS)
            };
            // Wake on whichever comes first: the cadence-aligned heartbeat cycle, or a timer transition
            // asking for an immediate flush. Either way we assemble + drain exactly the same.
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(cycle_secs)) => {}
                _ = flush.notified() => {}
            }
            let state = app.state::<AppState>();

            // Auth-gated: no token → skip (don't grow the outbox while signed out).
            //
            // `id_token()` also drives the `auth:expired` edge (BUILD-PLAN.md M1: "401 →
            // `auth:expired`"). A 401 on refresh makes `AuthManager` log itself out, so the token
            // going from Some to None is precisely the moment the session died — as opposed to the
            // user signing out, which lands here too but only after the UI already moved. Emitting on
            // the edge (not every idle cycle) keeps it a one-shot the panel can act on.
            let Some(id_token) = state.auth.id_token().await else {
                if was_signed_in {
                    was_signed_in = false;
                    let _ = app.emit(crate::events::AUTH_EXPIRED, ());
                }
                continue;
            };
            was_signed_in = true;
            let ingest_url = state.auth.config().ingest_url.clone();

            refresh_location(&app).await;
            cycle::assemble_and_enqueue(&state);
            drain(&http, &upload, &ingest_url, &id_token, &state).await;
        }
    });
}

/// Spawn the fast config poller — the "reflect settings changes ASAP" rail.
///
/// The batch cycle already pulls config on a version mismatch, but its interval *is* the screenshot
/// cadence, so a policy change could sit unapplied for up to 10 minutes. This loop closes that gap
/// with a 60 s conditional pull. It is auth-gated like the sender (no token → skip quietly) and
/// deliberately fire-and-forget: a failed poll waits for the next tick, and the batch-cycle pull
/// remains the backstop — two readers of the same ETag'd endpoint can't disagree.
pub fn spawn_config_poller(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let http = client::api_client();
        loop {
            tokio::time::sleep(Duration::from_secs(CONFIG_POLL_SECS)).await;
            let state = app.state::<AppState>();
            let Some(id_token) = state.auth.id_token().await else {
                continue;
            };
            let ingest_url = state.auth.config().ingest_url.clone();
            let etag = state.config.lock().unwrap().etag();
            match config::pull_config(&http, &ingest_url, &id_token, etag.as_deref()).await {
                Ok(config::ConfigPull::Fresh { config, etag }) => {
                    let v = config.version;
                    state.config.lock().unwrap().apply(*config, etag);
                    tracing::info!("config poller applied version {v}");
                }
                Ok(config::ConfigPull::NotModified) => {}
                Err(e) => tracing::debug!("config poll skipped: {e}"),
            }
        }
    });
}

/// Refresh the cached device location — **timer-gated, consent-gated, pause-aware; fails closed.**
///
/// All three gates must hold, and any one of them failing clears the cached fix so the change takes
/// effect on the very next cycle rather than leaking a stale position into the next heartbeat.
///
/// **Timer-gating is the load-bearing one** (owner decision, 2026-07-21). Location previously rode
/// the heartbeat, which runs whenever the agent is signed in and consented — so it recorded where
/// the employee was at 21:00 on a Sunday with no timer running. That is the difference between
/// *"where work happened"* and *"where this person is"*, and only the first is what the product
/// claims to measure. It now matches the screenshot loop, which has always been timer-gated
/// (`monitor::spawn_screenshots`), so no capture in the agent outlives the working session.
///
/// The privacy pause is honoured for the same reason it suspends screenshots: a bounded window in
/// which nothing is recorded is worthless if one of the three capture paths keeps going.
async fn refresh_location(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();

    let running = state.timer.lock().unwrap().is_running();
    let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
    let paused = state
        .pause
        .lock()
        .unwrap()
        .is_paused(crate::clock::now_epoch_ms());

    let fix = if running && consented && !paused {
        tokio::task::spawn_blocking(crate::location::capture)
            .await
            .unwrap_or(None)
    } else {
        None
    };
    *state.location.lock().unwrap() = fix;
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
                            state.config.lock().unwrap().apply(*config, etag);
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
    let expected = {
        state
            .screenshots
            .lock()
            .unwrap()
            .get(&pu.screenshot_id)
            .map(|s| (s.path.clone(), s.meta.content_sha256.clone()))
    };
    let Some((path, expected_sha256)) = expected else {
        return;
    };

    if !is_allowed_upload_host(&pu.url) {
        tracing::warn!("rejected non-amazonaws screenshot upload host");
        return;
    }

    // Tamper-evidence: the bytes on disk must still be the bytes we captured. If a script replaced the
    // buffered image, its SHA-256 no longer matches what we committed at capture — so we **refuse to
    // upload the forged image** and drop it, rather than let a fabricated screenshot reach the server.
    match std::fs::read(&path) {
        Ok(bytes) => {
            let actual = crate::monitor::screenshot::sha256_hex(&bytes);
            if actual != expected_sha256 {
                tracing::warn!(
                    id = %pu.screenshot_id,
                    "screenshot bytes changed on disk since capture — refusing to upload (tampered)"
                );
                state.screenshots.lock().unwrap().remove(&pu.screenshot_id);
                let _ = std::fs::remove_file(&path);
                return;
            }
        }
        Err(e) => {
            tracing::warn!(id = %pu.screenshot_id, "screenshot file unreadable, skipping: {e}");
            return;
        }
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

/// PUT a spooled file's bytes to a presigned URL. **No auth header** — the signature is in the URL —
/// and `Content-Type` must match what the server signed. Shared with the on-demand `capture_now`
/// path (`mqtt::capture`), which uploads to the command's own URL instead of an ack's.
pub(crate) async fn put_bytes(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
) -> Result<(), String> {
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
