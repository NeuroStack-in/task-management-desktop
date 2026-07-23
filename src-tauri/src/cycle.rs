//! One capture cycle: collect the heartbeat, drain pending events (M4 adds activity rollups, M5
//! screenshot metas), and enqueue the batch to the outbox. The 300 s loop (Thread B) and the actual
//! send (`api::batch`, gated on M1 auth) drive this — kept a pure function so it is testable without
//! a tokio runtime or a network.

use crate::clock::now_epoch_ms;
use crate::heartbeat;
use crate::state::AppState;

/// Assemble one cycle from live state and persist it to the outbox (persist-before-send). Returns the
/// enqueued `batch_seq`. `activity` is empty until M4, `screenshots` until M5; the sender + watermark
/// prune land with `api::batch`.
/// Screenshots with this many failed uploads are dropped (BUILD-PLAN §5 — a permanently-failing PUT
/// must not re-declare its meta forever).
pub const SCREENSHOT_ATTEMPT_CAP: u8 = 20;

pub fn assemble_and_enqueue(state: &AppState) -> u64 {
    let events = std::mem::take(&mut *state.pending_events.lock().unwrap());
    let activity = std::mem::take(&mut *state.pending_activity.lock().unwrap());
    // Screenshots are **re-declared** each cycle (idempotent by id) until uploaded or capped.
    let screenshots = {
        let shots = state.screenshots.lock().unwrap();
        shots
            .values()
            .filter(|s| s.attempts < SCREENSHOT_ATTEMPT_CAP)
            .map(|s| s.meta.clone())
            .collect::<Vec<_>>()
    };
    let config_version = state.config.lock().unwrap().version();
    // The consent-gated location fix the sender refreshed this cycle (None when withheld/no fix).
    let location = *state.location.lock().unwrap();
    let mut outbox = state.outbox.lock().unwrap();
    let outbox_mb = outbox.backlog_bytes() as f32 / (1024.0 * 1024.0);
    // Real machine-idle signal published by the monitor thread (was hardcoded `false`, which left the
    // fleet dashboard's online/idle status permanently "not idle").
    let idle = state.idle.load(std::sync::atomic::Ordering::Relaxed);
    let hb = heartbeat::collect(env!("CARGO_PKG_VERSION"), outbox_mb, idle, location);
    outbox.enqueue_cycle(
        now_epoch_ms(),
        config_version,
        hb,
        activity,
        events,
        screenshots,
    )
}
