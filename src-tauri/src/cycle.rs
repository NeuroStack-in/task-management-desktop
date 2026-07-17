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
pub fn assemble_and_enqueue(state: &AppState) -> u64 {
    let events = std::mem::take(&mut *state.pending_events.lock().unwrap());
    let activity = std::mem::take(&mut *state.pending_activity.lock().unwrap());
    let config_version = state.config.lock().unwrap().version();
    let mut outbox = state.outbox.lock().unwrap();
    let outbox_mb = outbox.backlog_bytes() as f32 / (1024.0 * 1024.0);
    let hb = heartbeat::collect(env!("CARGO_PKG_VERSION"), outbox_mb, false);
    // `screenshots` fill in at M5.
    outbox.enqueue_cycle(now_epoch_ms(), config_version, hb, activity, events, vec![])
}
