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
/// Whether `released_at` is a release this agent has **not** already acted on.
///
/// The fleet row stays released once IT presses the button, so the server keeps reporting the same
/// instant on every batch — including the first batch after the employee signs back in. Acting on
/// that unconditionally is what locked them out of v0.1.20: sign in, batch, told "released", sign
/// out, about a second a cycle. Comparing against the latch makes a repeat a no-op while a later
/// release still stops the agent.
///
/// A `0` is "never released" and is never actionable.
pub fn is_unhandled(released_at: i64) -> bool {
    is_newer(released_at, crate::session_state::load().released_ack_ms)
}

/// The decision itself, split from the disk read so the rule that keeps an employee signed in is
/// unit-tested rather than only exercised by releasing a real laptop.
fn is_newer(released_at: i64, acked: i64) -> bool {
    released_at > 0 && released_at > acked
}

/// Stop the timer, flush what this agent owes, then sign out — recording `released_at` so this
/// release is never acted on twice.
pub async fn stop_and_sign_out(app: &tauri::AppHandle, released_at: i64) {
    // Latched **first**, before anything can fail. The teardown that follows signs the user out, and
    // if the latch were written last a failure in between would leave the agent signing itself out
    // on every subsequent sign-in — the exact lockout this exists to prevent. Recording a release
    // that was then only partly carried out costs at most one un-stopped timer; the other order
    // costs the employee their machine.
    crate::session_state::update(|s| s.released_ack_ms = s.released_ack_ms.max(released_at));

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

#[cfg(test)]
mod tests {
    use super::is_newer;

    /// `0` is "this device has never been released" and must never trigger a teardown — it is what
    /// every ordinary ack carries.
    #[test]
    fn a_device_that_was_never_released_is_never_torn_down() {
        assert!(!is_newer(0, 0));
        assert!(!is_newer(0, 1_700_000_000_000));
    }

    #[test]
    fn an_unhandled_release_stops_the_agent() {
        assert!(is_newer(1_700_000_000_000, 0));
    }

    /// **The lockout.** The fleet row stays released, so the server reports the same instant on
    /// every batch — including the first batch after the employee signs back in. v0.1.20 acted on
    /// it unconditionally and signed them straight out again, about a second per attempt, with no
    /// way through. Having acted on it once, the agent must ignore it thereafter.
    #[test]
    fn the_same_release_is_ignored_once_it_has_been_acted_on() {
        let at = 1_700_000_000_000;
        assert!(is_newer(at, 0), "first delivery acts");
        assert!(
            !is_newer(at, at),
            "the same release must not sign the employee out of a session they just started",
        );
    }

    /// Idempotent across both paths: MQTT and the batch ack carry the same instant, so whichever
    /// arrives second is recognised as the release already handled.
    #[test]
    fn the_second_delivery_path_is_a_no_op() {
        let at = 1_700_000_000_000;
        assert!(!is_newer(at, at));
    }

    /// A device released again later must still stop — the latch remembers one release, not "ever
    /// released".
    #[test]
    fn a_later_release_still_stops_the_agent() {
        let first = 1_700_000_000_000;
        let second = first + 60_000;
        assert!(is_newer(second, first));
    }

    /// A clock that went backwards, or an out-of-order delivery, must not re-trigger a teardown the
    /// agent has already performed.
    #[test]
    fn an_older_release_arriving_late_is_ignored() {
        assert!(!is_newer(1_699_999_000_000, 1_700_000_000_000));
    }
}
