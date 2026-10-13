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

pub mod appearance;
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

use crate::cycle;
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
            //
            // **Never longer than 5 min**, whatever the cadence. The server's reaper closes a running
            // session once the user's newest device has been silent for 15 min (three missed 300 s
            // cycles), so a 20-minute custom cadence had every session "abandoned" between two
            // healthy heartbeats, and a 10-minute one after a single failed send (QA audit
            // 2026-10-08, WP-061). Shorter cycles only ship the same captures sooner.
            let cycle_secs = {
                let cadence = app
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .get()
                    .tracking
                    .cadence;
                heartbeat_secs(cadence.interval_secs().map(u64::from))
            };
            // Wake on whichever comes first: the cadence-aligned heartbeat cycle, or a timer transition
            // asking for an immediate flush. Either way we assemble + drain exactly the same — except
            // that only the periodic cycle waits for a fresh location fix (see `refresh_location`).
            let immediate = tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(cycle_secs)) => false,
                _ = flush.notified() => true,
            };
            let state = app.state::<AppState>();

            // A startup restore that failed on the network (booted before Wi-Fi) used to be the only
            // attempt, so the agent stayed signed out until someone re-typed a password (WP-059).
            // Retried here every cycle; with no stored token (an explicit sign-out) it is a no-op.
            if !state.auth.is_authenticated() && state.auth.restore().await {
                tracing::info!("session restored on retry");
                state.adopt_signed_in();
            }

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
                    // The session is really over (Cognito refused the refresh token — signed out),
                    // not just unreachable this cycle. Stop the clock, keeping the stop event: both
                    // capture loops gate on the timer, so this is what stops recording behind the
                    // sign-in screen. The queue stays this user's (`outbox_owner`) — sent when they
                    // sign back in, dropped if someone else does (QA audit 2026-10-08, WP-057).
                    if !state.auth.is_authenticated() {
                        state.end_session_locally();
                        crate::session_state::update(|s| {
                            s.consent_granted = false;
                            s.resume = None;
                        });
                        let _ = app.emit(crate::events::TRACKING_CHANGED, ());
                    }
                    let _ = app.emit(crate::events::AUTH_EXPIRED, ());
                }
                continue;
            };
            was_signed_in = true;
            let ingest_url = state.auth.config().ingest_url.clone();

            // Whose queue is this? Settled before anything is assembled or sent: the sign-in paths
            // adopt right after setting the tokens, and this cycle can land in between (WP-206).
            state.adopt_signed_in();

            refresh_location(&app, !immediate).await;
            cycle::assemble_and_enqueue(&state);
            let released_at = drain(&http, &upload, &ingest_url, &id_token, &state).await;

            // The durable half of a device release: this agent was offline when IT pressed the
            // button and so never saw the MQTT command, and the ack it just got is how it finds
            // out. Dropped here rather than inside `drain` because the teardown flushes through
            // the same path (see `release::stop_and_sign_out`).
            // `is_unhandled` is the guard that keeps a signed-back-in employee signed in: the
            // fleet row stays released, so this same instant arrives on every batch forever.
            if crate::release::is_unhandled(released_at) {
                crate::release::stop_and_sign_out(&app, released_at).await;
            }
        }
    });
}

/// The heartbeat interval for a screenshot cadence (`None` = screenshots off): the cadence, capped
/// at [`DEFAULT_CYCLE_SECS`] so the server never sees a gap its 15-minute reaper reads as abandoned.
fn heartbeat_secs(cadence_secs: Option<u64>) -> u64 {
    cadence_secs
        .unwrap_or(DEFAULT_CYCLE_SECS)
        .clamp(1, DEFAULT_CYCLE_SECS)
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
                    config::apply(&state, *config, etag);
                    tracing::info!("config poller applied version {v}");
                }
                Ok(config::ConfigPull::NotModified) => {
                    // The cached config *is* the server's — the ETag matched.
                    state
                        .config_known
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Err(e) => {
                    note_auth_rejection(&state, &e);
                    tracing::debug!("config poll skipped: {e}");
                }
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
///
/// **A new fix only on the periodic cycle (`fresh_fix`), and never longer than `FIX_TIMEOUT`.** Every
/// wake used to await a fix first — including the immediate flush after a Start or Stop, which LLD
/// §4 wants on the wire in seconds — and the Windows backend sets no deadline of its own (WinRT's
/// internal one is about a minute). An immediate flush now carries the last fix, and only re-checks
/// the gates so a stopped, withdrawn or paused capture still clears it at once (QA audit
/// 2026-10-08, WP-449).
async fn refresh_location(app: &tauri::AppHandle, fresh_fix: bool) {
    let state = app.state::<AppState>();

    let running = state.timer.lock().unwrap().is_running();
    let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
    let paused = state
        .pause
        .lock()
        .unwrap()
        .is_paused(crate::clock::now_epoch_ms());

    if !(running && consented && !paused) {
        *state.location.lock().unwrap() = None;
        return;
    }
    if !fresh_fix {
        return;
    }
    // A blocking OS call can't be cancelled; past the deadline its answer is simply not waited for.
    let capture = tokio::task::spawn_blocking(crate::location::capture);
    let fix = tokio::time::timeout(crate::location::FIX_TIMEOUT + LOCATION_GRACE, capture)
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
    *state.location.lock().unwrap() = fix;
}

/// Slack on top of the backends' own `FIX_TIMEOUT`, so a backend that honours it is never cut off a
/// moment before its answer.
const LOCATION_GRACE: Duration = Duration::from_secs(2);

/// Push everything the outbox owes, once, on demand — used by the release teardown to file the
/// employee's last stretch of work **before** anything is cleared ([`crate::release`]).
///
/// Its own clients rather than the sender loop's: this runs from whichever task handled the release
/// (an MQTT command, or the ack that carried it), which has no access to those. Any `released` the
/// server repeats on these acks is ignored — we are already releasing.
pub async fn flush_outbox(ingest_url: &str, id_token: &str, state: &AppState) {
    let http = client::api_client();
    let upload = client::upload_client();
    let _ = drain(&http, &upload, ingest_url, id_token, state).await;
}

/// Send oldest-first until the outbox is empty or a send fails (then retry next tick). After each ack,
/// prune and upload that ack's screenshots.
///
/// Returns **true when the server said this device has been released**, so the caller can stop and
/// sign out. Reported rather than acted on here: the teardown re-enters this very function to flush
/// the final `TimerStopped`, and doing that from inside the drain loop would recurse.
async fn drain(
    http: &reqwest::Client,
    upload: &reqwest::Client,
    ingest_url: &str,
    id_token: &str,
    state: &AppState,
) -> i64 {
    let mut released_at = 0i64;
    loop {
        let next = { state.outbox.lock().unwrap().next_batch().cloned() };
        let Some(batch) = next else { break };

        match batch::send_batch(http, ingest_url, id_token, &batch).await {
            Ok(ack) => {
                state.outbox.lock().unwrap().prune_to(ack.watermark_seq);

                // The batch was still accepted and pruned above — the release rides an ordinary
                // ack precisely so this send is not wasted. Keep draining: everything already
                // queued should reach the server before the teardown clears it.
                released_at = released_at.max(ack.released_at);

                // Config rail: the ack advertises the server version; on a mismatch, pull (ETag-conditional)
                // and apply live — cadence/blur/silent + app/URL rules the monitor threads read each tick.
                // Also whenever no org policy is known yet (first run, or a new user's sign-in), so
                // capture is held for as short a time as possible (WP-209).
                let (stale, etag) = {
                    let c = state.config.lock().unwrap();
                    let unknown = !state
                        .config_known
                        .load(std::sync::atomic::Ordering::Relaxed);
                    (c.needs_pull(ack.config_version) || unknown, c.etag())
                };
                if stale {
                    match config::pull_config(http, ingest_url, id_token, etag.as_deref()).await {
                        Ok(config::ConfigPull::Fresh { config, etag }) => {
                            let v = config.version;
                            config::apply(state, *config, etag);
                            tracing::info!("config applied (version {v})");
                        }
                        Ok(config::ConfigPull::NotModified) => state
                            .config_known
                            .store(true, std::sync::atomic::Ordering::Relaxed),
                        Err(e) => {
                            note_auth_rejection(state, &e);
                            tracing::warn!("config pull failed: {e}");
                        }
                    }
                }

                for pu in &ack.upload_urls {
                    upload_screenshot(upload, state, pu).await;
                }
            }
            Err(e) => {
                // **The server refused this batch's content** (400/413/422). It will refuse it on
                // every retry, and everything queued behind it — heartbeats, timer events, activity
                // — used to wait forever: one poisoned batch stopped all ingestion for the device
                // until someone deleted the queue file (QA audit 2026-10-08, WP-208). It keeps its
                // place for `REFUSED_BATCH_GRACE_MS` — so a server regression refusing everything is
                // fixed before any data goes — and is then dropped, loudly. The batch caps in
                // `cycle` keep the agent from producing the size-based refusals that made this real.
                if let Some(status) = batch::rejected_status(&e) {
                    let now = crate::clock::now_epoch_ms();
                    let give_up =
                        give_up_on(&mut REFUSED_HEAD.lock().unwrap(), batch.batch_seq, now);
                    if give_up {
                        tracing::error!(
                            batch_seq = batch.batch_seq,
                            status,
                            "the server has refused this batch for an hour; dropping it so the \
                             rest of the queue can go: {e}"
                        );
                        state.outbox.lock().unwrap().prune_to(batch.batch_seq);
                        continue;
                    }
                    tracing::error!(batch_seq = batch.batch_seq, status, "batch refused: {e}");
                    break;
                }
                note_auth_rejection(state, &e);
                tracing::warn!("batch send failed, will retry next cycle: {e}");
                break;
            }
        }
    }
    released_at
}

/// How long a batch the server refuses keeps its place at the head of the queue before it is
/// dropped — long enough that a server-side regression refusing every batch is fixed before any
/// agent discards data, short enough that a genuinely poisoned batch stops blocking the same day.
const REFUSED_BATCH_GRACE_MS: i64 = 60 * 60 * 1000;

/// The head batch the server last refused, and since when (epoch ms). In memory: a restart gives a
/// refused batch a fresh grace period, which only errs toward keeping data.
static REFUSED_HEAD: std::sync::Mutex<Option<(u64, i64)>> = std::sync::Mutex::new(None);

/// Whether to stop retrying batch `seq`, refused again at `now`.
fn give_up_on(refused: &mut Option<(u64, i64)>, seq: u64, now: i64) -> bool {
    match *refused {
        Some((head, since)) if head == seq => now - since >= REFUSED_BATCH_GRACE_MS,
        _ => {
            *refused = Some((seq, now));
            false
        }
    }
}

/// A 401 means the cached ID token is dead whatever its local expiry says: make the next
/// `id_token()` refresh instead of re-sending it every cycle (WP-442).
fn note_auth_rejection(state: &AppState, err: &str) {
    if err == "auth:expired" {
        state.auth.invalidate_id_token();
    }
}

/// A webview-supplied id, checked before it becomes a URL path segment.
///
/// Ids are server-minted ULIDs, but they reach these calls from the webview, and `format!` put them
/// in the path verbatim — a value with `/../` or `?` in it re-targeted a request carrying the user's
/// bearer token at another route on the same API (QA audit 2026-10-08, WP-444). Only RFC 3986
/// unreserved characters pass, and never a dot-only segment.
pub(crate) fn path_id<'a>(op: &str, id: &'a str) -> Result<&'a str, String> {
    let ok = !id.is_empty()
        && !id.chars().all(|c| c == '.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'));
    if ok {
        Ok(id)
    } else {
        Err(format!("{op}:invalid_id"))
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
                crate::pending_shots::discard(state, &pu.screenshot_id);
                return;
            }
        }
        Err(e) => {
            tracing::warn!(id = %pu.screenshot_id, "screenshot file unreadable, skipping: {e}");
            return;
        }
    }

    match put_bytes(upload, &pu.url, &path).await {
        Ok(()) => crate::pending_shots::discard(state, &pu.screenshot_id),
        Err(e) => {
            tracing::warn!("screenshot upload failed: {e}");
            crate::pending_shots::failed_upload(state, &pu.screenshot_id);
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

#[cfg(test)]
mod drain_tests {
    use super::{give_up_on, path_id, REFUSED_BATCH_GRACE_MS};

    /// WP-208: a refused head batch blocks only for the grace period, then gives way; a different
    /// batch being refused starts its own clock.
    #[test]
    fn a_refused_batch_gives_way_after_the_grace_period() {
        let mut refused = None;
        assert!(!give_up_on(&mut refused, 7, 1_000), "first refusal: retry");
        assert!(!give_up_on(
            &mut refused,
            7,
            1_000 + REFUSED_BATCH_GRACE_MS - 1
        ));
        assert!(give_up_on(&mut refused, 7, 1_000 + REFUSED_BATCH_GRACE_MS));
        assert!(
            !give_up_on(&mut refused, 8, 9_999_999),
            "a new head starts over"
        );
    }

    /// WP-444: ids become path segments only if they can't re-target the request.
    #[test]
    fn only_plain_ids_become_path_segments() {
        for ok in [
            "01JABCDEF",
            "k-1",
            "s_2",
            "a.b",
            "8dff7416-23c7-480e-98dd-10c959bab9ea",
        ] {
            assert_eq!(path_id("op", ok), Ok(ok));
        }
        for bad in [
            "", ".", "..", "a/b", "../x", "p1?x=1", "p1#f", "a b", "p%2F1",
        ] {
            assert!(path_id("op", bad).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod heartbeat_tests {
    use super::heartbeat_secs;

    /// WP-061: a long screenshot cadence must not stretch the heartbeat past the reaper's window.
    #[test]
    fn the_heartbeat_never_waits_longer_than_five_minutes() {
        assert_eq!(heartbeat_secs(Some(180)), 180);
        assert_eq!(heartbeat_secs(Some(600)), 300);
        assert_eq!(heartbeat_secs(Some(20 * 60)), 300);
        assert_eq!(heartbeat_secs(None), 300);
    }
}
