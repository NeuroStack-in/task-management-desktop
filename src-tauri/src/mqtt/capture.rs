//! `capture_now` — the admin-triggered, on-demand screenshot (backend `fleet::features::capture_now`).
//!
//! The backend presigns an S3 PUT for a minted `screenshot_id` and pushes
//! `{"kind":"capture_now","screenshot_id":…,"upload_url":…,"requested_by":…}` down the device's `cmd`
//! topic. This is the **most invasive action in the product**: one named person points at one named
//! employee's screen and takes a frame of it, now, outside every cadence the employee was told
//! about. So it is the most heavily guarded, and the guards live **here, on the device** — the only
//! party that can know whether capture was permissible at that instant.
//!
//! **The guard chain, in order** ([`decide`]):
//! 1. **Privacy pause wins.** A pause is a promise that nothing is recorded for its duration
//!    (PRIVACY.md §5). An admin request that could punch through it would make the pause theatre.
//! 2. **Timer gate.** "No timer → no capture. Ever." (PRIVACY.md §2, AGENT.md §1.2 — the privacy
//!    stance made structural.) An on-demand capture outside a tracked session is a *policy
//!    expansion*, not a feature, and the agent refuses to be the place it happens.
//! 3. **Consent.** Capture fails closed without it (`AppState::consent`); a timer can legitimately
//!    run un-consented, so this is reachable and is not implied by (2).
//! 4. **Exceptions carve-out.** A focused excepted app/site is never screenshotted, and nothing
//!    about it is recorded (PRIVACY.md §2/§3). Being asked by hand doesn't change that.
//! 5. **Upload host pinning** — the URL must be `https://*.amazonaws.com` before a pixel is grabbed,
//!    so a compromised backend can't redirect a frame of someone's screen (AGENT.md §5).
//!
//! **Every outcome is answered** on the `evt` topic, refusal included: the server routes the answer
//! back to `requested_by`, so "I asked and nothing happened" is never the user experience, and
//! `fleet::features::agent_events` audits the refusal alongside the request.
//!
//! **Never covert.** Success *and* refusal are written to the employee-visible
//! [`crate::privacy_log`] and pushed to the panel as [`crate::events::ADMIN_CAPTURE`].

use tauri::{Emitter, Manager};

use crate::api::client;
use crate::monitor::{active_window, screenshot};
use crate::state::AppState;
use crate::{clock, events, privacy_log, rules};

/// Why an on-demand capture did not happen. The wire value is the ack's `reason`; the backend treats
/// it as an opaque string (`agent_events::on_capture_result`) and passes it to the requester, so new
/// variants are forward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    PrivacyPause,
    NotTracking,
    NoConsent,
    Excepted,
    UploadHostRejected,
    CaptureFailed,
}

impl Refusal {
    /// The `reason` string in the ack.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::PrivacyPause => "privacy_pause",
            Refusal::NotTracking => "not_tracking",
            Refusal::NoConsent => "no_consent",
            Refusal::Excepted => "excepted",
            Refusal::UploadHostRejected => "upload_host_rejected",
            Refusal::CaptureFailed => "capture_failed",
        }
    }

    /// What the employee is told in their own log/panel. Deliberately plain, and deliberately
    /// present for refusals too — "someone asked" is the part they are entitled to know.
    pub fn employee_detail(self) -> &'static str {
        match self {
            Refusal::PrivacyPause => {
                "An administrator requested a screenshot. Declined — your privacy pause was active."
            }
            Refusal::NotTracking => {
                "An administrator requested a screenshot. Declined — no timer was running."
            }
            Refusal::NoConsent => {
                "An administrator requested a screenshot. Declined — monitoring consent is not granted."
            }
            Refusal::Excepted => {
                "An administrator requested a screenshot. Declined — the focused app is exempt from monitoring."
            }
            Refusal::UploadHostRejected => {
                "An administrator requested a screenshot. Declined — the upload destination failed validation."
            }
            Refusal::CaptureFailed => {
                "An administrator requested a screenshot, but the screen could not be captured."
            }
        }
    }
}

/// The guard chain as a pure function, so the trust-critical branches are testable without a
/// desktop, a broker or a timer. **Order is the contract**: the privacy pause is checked before
/// anything else, and the timer gate before anything is read off the screen.
pub fn decide(paused: bool, running: bool, consented: bool, excepted: bool) -> Result<(), Refusal> {
    if paused {
        return Err(Refusal::PrivacyPause);
    }
    if !running {
        return Err(Refusal::NotTracking);
    }
    if !consented {
        return Err(Refusal::NoConsent);
    }
    if excepted {
        return Err(Refusal::Excepted);
    }
    Ok(())
}

/// The `capture_now_result` ack. `requested_by` is **echoed verbatim** — it is how the server routes
/// the answer back to the person who asked, with no correlation state of its own.
pub fn ack_payload(screenshot_id: &str, requested_by: &str, refusal: Option<Refusal>) -> Vec<u8> {
    serde_json::json!({
        "kind": "capture_now_result",
        "screenshot_id": screenshot_id,
        "requested_by": requested_by,
        "accepted": refusal.is_none(),
        "reason": refusal.map(Refusal::reason),
    })
    .to_string()
    .into_bytes()
}

/// Run one `capture_now` end to end and return the ack bytes to publish on `evt_topic`.
///
/// Never returns an error: *every* path produces an ack, because a request that dissolves into
/// silence is the one outcome neither the employee nor the requester can act on.
pub async fn handle(
    app: &tauri::AppHandle,
    screenshot_id: &str,
    upload_url: &str,
    requested_by: &str,
) -> Vec<u8> {
    // Gates that need no observation of the screen come first — a paused or untracked machine must
    // not even have its foreground window read (that is metadata, and PRIVACY.md §2 says nothing is
    // recorded while paused).
    let (paused, running, consented, blur, silent) = {
        let state = app.state::<AppState>();
        let paused = state.pause.lock().unwrap().is_paused(clock::now_epoch_ms());
        let running = state.timer.lock().unwrap().is_running();
        let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
        let cfg = state.config.lock().unwrap();
        let c = cfg.get();
        (paused, running, consented, c.tracking.blur_level, c.tracking.silent)
    };
    if let Err(r) = decide(paused, running, consented, false) {
        return refuse(app, screenshot_id, requested_by, r);
    }

    // Only now look at what is on screen — and only to apply the carve-out and label the shot.
    let focus = active_window::current();
    let excepted = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        let r = &cfg.get().rules;
        focus.as_ref().is_some_and(|f| {
            let haystack = format!(
                "{} {} {}",
                f.app,
                f.title.as_deref().unwrap_or(""),
                f.url.as_deref().unwrap_or("")
            );
            rules::is_excepted(&haystack, r)
        })
    };
    if let Err(r) = decide(paused, running, consented, excepted) {
        return refuse(app, screenshot_id, requested_by, r);
    }

    // Host-pin BEFORE capturing: if we could not lawfully upload it, we do not take it.
    if !screenshot::is_allowed_upload_host(upload_url) {
        tracing::warn!("capture_now: rejected non-amazonaws upload host");
        return refuse(
            app,
            screenshot_id,
            requested_by,
            Refusal::UploadHostRejected,
        );
    }

    // One frame of the primary display, through the periodic pipeline (downscale → blur → WebP →
    // pHash → content hash).
    let app_name = focus.map(|f| f.app).unwrap_or_default();
    let cap_ts = clock::now_epoch_ms();
    let shot =
        tokio::task::spawn_blocking(move || screenshot::capture_primary(&app_name, blur, cap_ts))
            .await
            .ok()
            .flatten();
    let Some((mut meta, path)) = shot else {
        // Same surfacing as the periodic loop: a capture failure is a state the UI shows — unless
        // silent mode is on, where a visible "capture blocked" message would itself reveal monitoring.
        if !silent {
            let _ = app.emit(
                events::SCREENSHOT_UNAVAILABLE,
                events::capture_failure_hint(),
            );
        }
        return refuse(app, screenshot_id, requested_by, Refusal::CaptureFailed);
    };

    // The server minted the id and derived the S3 object key from it; the batch must declare the
    // SAME id or ingest's fold writes a row pointing at a different object.
    meta.id = screenshot_id.to_string();

    let result = put_shot(&path, upload_url, &meta.content_sha256).await;
    // The spool holds data only until it is uploaded (PRIVACY.md §4) — and on failure there is no
    // second chance to take it from (no ack presigns this id again), so it goes either way.
    let _ = std::fs::remove_file(&path);

    if let Err(e) = result {
        tracing::warn!(id = %screenshot_id, "capture_now: upload failed: {e}");
        return refuse(app, screenshot_id, requested_by, Refusal::CaptureFailed);
    }

    // The bytes are in S3; the meta still has to ride a batch for ingest's fold to write the `SHOT#`
    // row that points at them. It goes to `pending_screenshot_meta` (declared, not re-uploaded) and
    // the sender is woken so it lands in seconds rather than at the next cadence
    // (docs/TIMER-IMMEDIATE-FLUSH.md).
    {
        let state = app.state::<AppState>();
        state
            .pending_screenshot_meta
            .lock()
            .unwrap()
            .push(meta.clone());
        state.flush.notify_one();
    }

    tracing::info!(id = %screenshot_id, %requested_by, "capture_now: captured and uploaded");
    disclose(
        app,
        privacy_log::KIND_ADMIN_CAPTURE,
        "An administrator requested a screenshot of your screen, and it was taken.",
    );
    ack_payload(screenshot_id, requested_by, None)
}

/// Verify the spooled bytes are still the bytes we captured, then PUT them to the presigned URL.
///
/// The hash check mirrors `api::upload_screenshot`: a frame swapped on disk between capture and
/// upload must never reach the server as if it were what the screen showed. Presigned → **no auth
/// header**; `content-type: image/webp` must match what the server signed.
async fn put_shot(
    path: &std::path::Path,
    upload_url: &str,
    expected_sha256: &str,
) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read:{e}"))?;
    if screenshot::sha256_hex(&bytes) != expected_sha256 {
        return Err("tampered:bytes changed since capture".into());
    }
    crate::api::put_bytes(&client::upload_client(), upload_url, path).await
}

/// Log the refusal (locally + for the operator), disclose it to the employee, and build the ack.
fn refuse(app: &tauri::AppHandle, screenshot_id: &str, requested_by: &str, r: Refusal) -> Vec<u8> {
    tracing::info!(
        id = %screenshot_id,
        %requested_by,
        reason = r.reason(),
        "capture_now: refused"
    );
    disclose(
        app,
        privacy_log::KIND_ADMIN_CAPTURE_REFUSED,
        r.employee_detail(),
    );
    ack_payload(screenshot_id, requested_by, Some(r))
}

/// The employee-facing half: a durable line in their own privacy log **and** a live panel event.
/// Both, not either — the event is how they see it *now*, the log is how they see it later.
///
/// **Silent mode (owner policy) suppresses both.** When `tracking.silent` is set, an on-demand
/// capture is neither announced to the employee nor written to their local privacy log. This is the
/// deliberate covert-capture path the org owner opted into; the server still records every
/// capture-now request (`fleet::capture_now` writes a `security` audit entry), so the org keeps an
/// accountability trail even though the employee is shown nothing.
fn disclose(app: &tauri::AppHandle, kind: &str, detail: &str) {
    if app
        .state::<AppState>()
        .config
        .lock()
        .unwrap()
        .get()
        .tracking
        .silent
    {
        return;
    }
    privacy_log::record(kind, detail);
    let _ = app.emit(events::ADMIN_CAPTURE, detail.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The trust-critical branch.** A privacy pause refuses regardless of everything else — and it
    /// is checked first, so it is the reason reported even when the timer is also off.
    #[test]
    fn a_privacy_pause_refuses_and_wins_over_every_other_state() {
        assert_eq!(
            decide(true, true, true, false),
            Err(Refusal::PrivacyPause),
            "paused mid-session must refuse"
        );
        assert_eq!(
            decide(true, false, false, true),
            Err(Refusal::PrivacyPause),
            "the pause is reported first — it is the strongest promise we made"
        );
    }

    /// **The trust-critical branch.** No timer, no capture — the whole privacy stance, enforced in
    /// code rather than by configuration.
    #[test]
    fn no_running_timer_refuses() {
        assert_eq!(decide(false, false, true, false), Err(Refusal::NotTracking));
    }

    /// A timer may run without monitoring consent (time tracking and capture are separate gates), so
    /// this path is reachable and must fail closed.
    #[test]
    fn a_consented_gate_is_not_implied_by_a_running_timer() {
        assert_eq!(decide(false, true, false, false), Err(Refusal::NoConsent));
    }

    /// The exceptions carve-out survives being asked by hand: an excepted window is never recorded.
    #[test]
    fn an_excepted_foreground_window_refuses() {
        assert_eq!(decide(false, true, true, true), Err(Refusal::Excepted));
    }

    #[test]
    fn all_gates_open_accepts() {
        assert_eq!(decide(false, true, true, false), Ok(()));
    }

    /// The ack is a wire contract with `fleet::features::agent_events` — pin it. `requested_by` is
    /// echoed (the server routes on it) and `reason` is null on success, never absent.
    #[test]
    fn refusal_ack_is_pinned() {
        let v: serde_json::Value =
            serde_json::from_slice(&ack_payload("s1", "u7", Some(Refusal::PrivacyPause))).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "capture_now_result",
                "screenshot_id": "s1",
                "requested_by": "u7",
                "accepted": false,
                "reason": "privacy_pause",
            })
        );
        let not_tracking = ack_payload("s2", "u7", Some(Refusal::NotTracking));
        let v: serde_json::Value = serde_json::from_slice(&not_tracking).unwrap();
        assert_eq!(v["reason"], "not_tracking");
        assert_eq!(v["accepted"], false);
    }

    #[test]
    fn success_ack_is_pinned() {
        let v: serde_json::Value = serde_json::from_slice(&ack_payload("s3", "u7", None)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "capture_now_result",
                "screenshot_id": "s3",
                "requested_by": "u7",
                "accepted": true,
                "reason": serde_json::Value::Null,
            })
        );
    }

    /// The server's `on_capture_result` reads `accepted` with `.unwrap_or(false)` — a refusal must
    /// therefore never be a payload it has to guess about.
    #[test]
    fn every_refusal_reports_a_reason() {
        for r in [
            Refusal::PrivacyPause,
            Refusal::NotTracking,
            Refusal::NoConsent,
            Refusal::Excepted,
            Refusal::UploadHostRejected,
            Refusal::CaptureFailed,
        ] {
            let v: serde_json::Value =
                serde_json::from_slice(&ack_payload("s", "u", Some(r))).unwrap();
            assert_eq!(v["accepted"], false);
            assert!(v["reason"].is_string(), "{r:?} must carry a reason");
            assert!(!r.employee_detail().is_empty(), "{r:?} must be disclosable");
        }
    }
}
