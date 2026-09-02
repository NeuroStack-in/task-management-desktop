//! `capture_now` — the admin-triggered, on-demand screenshot (backend `fleet::features::capture_now`).
//!
//! The backend presigns an S3 PUT for a minted `screenshot_id` and pushes
//! `{"kind":"capture_now","screenshot_id":…,"upload_url":…,"requested_by":…}` down the device's `cmd`
//! topic. The agent captures the primary display and uploads it.
//!
//! ## Owner policy, 2026-09-02 — capture-now is unconditional
//!
//! This path used to be the most heavily guarded action in the product: a chain of privacy gates
//! (privacy pause → timer running → consent → focused-app exception) refused the capture unless a
//! tracked, consented session was live, and every request — taken **or** refused — was disclosed to
//! the employee on-device.
//!
//! **The owner removed those gates.** An admin capture-now now takes a frame whenever it is
//! technically possible, regardless of timer, consent or pause, and the employee is shown nothing on
//! the device. What remains here are the two guards that are *not* about employee privacy:
//!
//! 1. **Upload host pinning** — the URL must be `https://*.amazonaws.com` before a pixel is grabbed,
//!    so a compromised backend can't redirect a frame of someone's screen (AGENT.md §5). This is a
//!    security control, not a consent one, and it stays.
//! 2. **Capture success** — if the screen genuinely can't be read, that is reported as such.
//!
//! Every outcome is still answered on the `evt` topic so the requester learns what happened, and the
//! **server-side audit trail** (`fleet::capture_now` writes a `security` entry) is unchanged — the
//! org keeps its own record of who asked for what. What is gone is the *employee-facing* disclosure.
//!
//! To restore a gate, reintroduce a check before [`put_shot`] in [`handle`] and return the matching
//! [`Refusal`]; the wire/ack shape already carries arbitrary reasons.

use tauri::Manager;

use crate::api::client;
use crate::clock;
use crate::monitor::{active_window, screenshot};
use crate::state::AppState;

/// Why an on-demand capture did not happen. The wire value is the ack's `reason`; the backend treats
/// it as an opaque string (`agent_events::on_capture_result`) and passes it to the requester, so new
/// variants are forward-compatible.
///
/// The privacy-gate variants were removed with the 2026-09-02 policy change (see the module docs);
/// only the two technical failures remain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    UploadHostRejected,
    CaptureFailed,
}

impl Refusal {
    /// The `reason` string in the ack.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::UploadHostRejected => "upload_host_rejected",
            Refusal::CaptureFailed => "capture_failed",
        }
    }
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
    // **No privacy gate** (owner policy 2026-09-02): the pause / timer / consent / exception chain
    // that used to stand here is gone. The only thing read up front is the blur level, so the shot
    // goes through the same downscale/blur pipeline as a periodic one.
    let blur = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        cfg.get().tracking.blur_level
    };

    // The foreground window is read only to *label* the shot now — it no longer gates anything.
    let focus = active_window::current();

    // Host-pin BEFORE capturing: if we could not lawfully upload it, we do not take it. This is a
    // security control (a compromised backend must not redirect a frame of the screen), not a
    // consent one, so it survives the policy change.
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
        // A capture failure is reported to the requester via the ack, but **not surfaced on the
        // device** — a "screenshot unavailable" banner would reveal that a capture was attempted,
        // which the covert-capture policy exists to prevent.
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

    // **No employee-facing disclosure** (owner policy 2026-09-02): the success used to be written to
    // the local privacy log and pushed to the panel. The server-side audit trail is unchanged.
    tracing::info!(id = %screenshot_id, %requested_by, "capture_now: captured and uploaded");
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

/// Log the refusal (locally, for the operator) and build the ack. **No employee-facing disclosure**
/// — the refusal is answered to the requester on the `evt` topic and audited server-side, but the
/// device stays silent (owner policy 2026-09-02). `app` is unused now that nothing is emitted, kept
/// so a gate that needs to disclose can be reintroduced without threading it back through.
fn refuse(_app: &tauri::AppHandle, screenshot_id: &str, requested_by: &str, r: Refusal) -> Vec<u8> {
    tracing::info!(
        id = %screenshot_id,
        %requested_by,
        reason = r.reason(),
        "capture_now: refused"
    );
    ack_payload(screenshot_id, requested_by, Some(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ack is a wire contract with `fleet::features::agent_events` — pin it. `requested_by` is
    /// echoed (the server routes on it) and `reason` is null on success, never absent.
    #[test]
    fn refusal_ack_is_pinned() {
        let v: serde_json::Value =
            serde_json::from_slice(&ack_payload("s1", "u7", Some(Refusal::UploadHostRejected)))
                .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "capture_now_result",
                "screenshot_id": "s1",
                "requested_by": "u7",
                "accepted": false,
                "reason": "upload_host_rejected",
            })
        );
        let failed = ack_payload("s2", "u7", Some(Refusal::CaptureFailed));
        let v: serde_json::Value = serde_json::from_slice(&failed).unwrap();
        assert_eq!(v["reason"], "capture_failed");
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
    /// therefore never be a payload it has to guess about. Only the two technical failures remain
    /// after the 2026-09-02 policy change.
    #[test]
    fn every_refusal_reports_a_reason() {
        for r in [Refusal::UploadHostRejected, Refusal::CaptureFailed] {
            let v: serde_json::Value =
                serde_json::from_slice(&ack_payload("s", "u", Some(r))).unwrap();
            assert_eq!(v["accepted"], false);
            assert!(v["reason"].is_string(), "{r:?} must carry a reason");
        }
    }
}
