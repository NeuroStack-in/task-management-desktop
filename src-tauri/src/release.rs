//! **Device release** — IT handed this laptop back, so stop the clock and sign the employee out.
//!
//! Triggered from the fleet page (`Settings → Agents → Release`) when a machine is being refreshed
//! or its owner is leaving. The employee signs in again afterwards, on this laptop or a new one;
//! nothing here is a punishment or a wipe of their work.
//!
//! ## Two ways in, one behaviour
//!
//! - **MQTT `release` command** ([`crate::mqtt`]) — the fast path, and the only one that stops a
//!   *running* timer promptly rather than at the next batch.
//! - **`released` on a batch ack** ([`crate::api`]) — the durable path, for a laptop that was asleep
//!   or shut down when the button was pressed and never saw the command.
//!
//! Both land on [`stop_and_sign_out`], which is **idempotent**: an agent that already released is
//! signed out, sends no further batches, and so never sees the second trigger. Belt and braces are
//! deliberate — a release that silently missed an offline machine is the failure mode this exists to
//! prevent.
//!
//! ## Why this is not just `auth_logout`
//!
//! The ordinary sign-out ([`crate::state::AppState::reset_for_account_switch`]) **drops** the running
//! session's `TimerStopped` event and clears the outbox — correct for an account switch, where that
//! data can no longer be sent under the departing user's token and would risk folding under the next
//! person. A release is the opposite case: the token is still valid, and the release dialog promises
//! the employee that *"everything already recorded stays on their record"*.
//!
//! So the order here is **stop → flush → tear down**, never tear-down-first. The stopped session is
//! queued as a real event and pushed to the server *before* anything is cleared, so the last stretch
//! of work is filed rather than discarded.

use std::time::Duration;

use tauri::{Emitter, Manager};
use wp_agent_contract::StopReason;

use crate::state::AppState;

/// How long the final flush may take before we sign out anyway.
///
/// Bounded on purpose: a laptop being handed back may be on a dying network, and an unbounded flush
/// would leave the employee signed in indefinitely — which is the one outcome a release must not
/// produce. Losing the tail of an outbox is the lesser failure, and the server's own 00:15 close
/// resolves a session whose stop never arrived.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(20);

/// Stop the timer, flush what this agent owes, then sign out.
///
/// Safe to call twice (see the module note) and safe to call while signed out — with no token the
/// flush is skipped and the teardown still runs, leaving the agent in the clean signed-out state a
/// release is meant to produce.
pub async fn stop_and_sign_out(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();

    // 1 — Stop the clock, and **keep the event**. `reset_for_account_switch` would discard it; here
    //     it is the employee's last stretch of work and the thing the release dialog promised to
    //     preserve.
    let stopped = state
        .timer
        .lock()
        .unwrap()
        .stop(crate::clock::now_epoch_ms(), StopReason::Logout);
    if let Some(ev) = stopped {
        state.pending_events.lock().unwrap().push(ev);
        // The indicator is showing a running session that no longer exists.
        let _ = app.emit(crate::events::TRACKING_CHANGED, ());
    }

    // 2 — Flush under the still-valid token, before anything is cleared. Best-effort and bounded:
    //     see FLUSH_TIMEOUT.
    if let Some(id_token) = state.auth.id_token().await {
        let ingest_url = state.auth.config().ingest_url.clone();
        crate::cycle::assemble_and_enqueue(&state);
        let flush = crate::api::flush_outbox(&ingest_url, &id_token, &state);
        if tokio::time::timeout(FLUSH_TIMEOUT, flush).await.is_err() {
            tracing::warn!("release: final flush timed out; signing out with an unsent tail");
        }
    }

    // 3 — Tear down exactly as a sign-out does. Anything left in the outbox now has been tried and
    //     is deliberately dropped: this device is released, and holding a released employee's
    //     captures is precisely what the release was meant to stop.
    state.reset_for_account_switch();
    crate::session_state::update(|s| {
        s.consent_granted = false;
        s.resume = None;
    });
    state.auth.logout();

    // 4 — Say why. Without this the panel simply snaps back to the sign-in screen on its next poll,
    //     which reads as a crash or an expired session; the employee needs to know this was IT, that
    //     their hours are safe, and that signing in again is the expected next step.
    let _ = app.emit(crate::events::DEVICE_RELEASED, ());
    tracing::info!("release: timer stopped, work flushed, signed out");
}
