//! One capture cycle: collect the heartbeat, drain pending events (M4 adds activity rollups, M5
//! screenshot metas), and enqueue the batch to the outbox. The 300 s loop (Thread B) and the actual
//! send (`api::batch`, gated on M1 auth) drive this — kept a pure function so it is testable without
//! a tokio runtime or a network.

use wp_agent_contract::ScreenshotMeta;

use crate::clock::now_epoch_ms;
use crate::heartbeat;
use crate::state::AppState;

/// Screenshots with this many failed uploads are dropped (BUILD-PLAN §5 — a permanently-failing PUT
/// must not re-declare its meta forever).
pub const SCREENSHOT_ATTEMPT_CAP: u8 = 20;

/// The most rollups / events / screenshot metas one batch carries. A normal cycle holds a handful
/// of each; these only bite after a stretch the sender could not assemble (offline with an expired
/// token, the buffers keep growing in memory), and they sit well under the server's per-batch limits
/// (`ingest::shared::sanity::check_counts`: 1000 / 1000 / 500), past which it refuses the whole
/// batch with a 400 — and a refused head batch used to stop all ingestion for the device (QA audit
/// 2026-10-08, WP-208). A backlog beyond them goes out as several batches, in order.
const MAX_ROLLUPS_PER_BATCH: usize = 240;
const MAX_EVENTS_PER_BATCH: usize = 500;
const MAX_SHOTS_PER_BATCH: usize = 200;

/// Whether a pending frame last declared in `declared_seq` must be declared again, given the oldest
/// batch still queued. Not while its batch is queued — that batch's ack will presign it — and again
/// once it was acked and the frame is still here (the upload failed).
fn needs_declaring(declared_seq: Option<u64>, oldest_queued: Option<u64>) -> bool {
    match (declared_seq, oldest_queued) {
        (None, _) | (Some(_), None) => true,
        (Some(declared), Some(oldest)) => declared < oldest,
    }
}

/// Take up to `n` items off the front of `v`, keeping their order.
fn take_front<T>(v: &mut Vec<T>, n: usize) -> Vec<T> {
    let n = n.min(v.len());
    v.drain(..n).collect()
}

/// Assemble one cycle from live state and persist it to the outbox (persist-before-send) — as
/// several batches when the buffers are larger than one may carry. Returns the last enqueued
/// `batch_seq`.
pub fn assemble_and_enqueue(state: &AppState) -> u64 {
    let mut events = std::mem::take(&mut *state.pending_events.lock().unwrap());
    let mut activity = std::mem::take(&mut *state.pending_activity.lock().unwrap());
    // Pending frames are declared (idempotent by id) until uploaded, capped, or too old to ever
    // upload (`pending_shots::expire`) — but only once per queued batch (`needs_declaring`).
    crate::pending_shots::expire(state, now_epoch_ms());
    let oldest_queued = state
        .outbox
        .lock()
        .unwrap()
        .next_batch()
        .map(|b| b.batch_seq);
    let mut screenshots: Vec<ScreenshotMeta> = {
        let shots = state.screenshots.lock().unwrap();
        shots
            .values()
            .filter(|s| s.attempts < SCREENSHOT_ATTEMPT_CAP)
            .filter(|s| needs_declaring(s.declared_seq, oldest_queued))
            .map(|s| s.meta.clone())
            .collect()
    };
    // …plus frames whose bytes are already in S3 (the admin-triggered `capture_now` uploads to the
    // command's own presigned URL). Their meta is **drained**, not re-declared: there is no upload
    // left to wait for, only the fold that writes the `SHOT#` row pointing at the object.
    screenshots.append(&mut state.pending_screenshot_meta.lock().unwrap());
    let config_version = state.config.lock().unwrap().version();
    // The consent-gated location fix the sender refreshed this cycle (None when withheld/no fix).
    let location = *state.location.lock().unwrap();
    let mut outbox = state.outbox.lock().unwrap();
    let outbox_mb = outbox.backlog_bytes() as f32 / (1024.0 * 1024.0);
    // Real machine-idle signal published by the monitor thread (was hardcoded `false`, which left the
    // fleet dashboard's online/idle status permanently "not idle").
    let idle = state.idle.load(std::sync::atomic::Ordering::Relaxed);
    // The timer state this agent believes in, declared every cycle so the server can reconcile a
    // session whose TimerStopped was lost (agent killed, crashed, or restarted without resuming).
    let paused = state.pause.lock().unwrap().is_paused(now_epoch_ms());
    let active_session = state.timer.lock().unwrap().active_session(paused);
    let hb = heartbeat::collect(
        env!("CARGO_PKG_VERSION"),
        outbox_mb,
        idle,
        location,
        active_session,
    );
    let captured_at = now_epoch_ms();
    let mut declared: Vec<(String, u64)> = Vec::new();
    let mut seq;
    loop {
        let shots = take_front(&mut screenshots, MAX_SHOTS_PER_BATCH);
        let shot_ids: Vec<String> = shots.iter().map(|s| s.id.clone()).collect();
        seq = outbox.enqueue_cycle(
            captured_at,
            config_version,
            hb.clone(),
            take_front(&mut activity, MAX_ROLLUPS_PER_BATCH),
            take_front(&mut events, MAX_EVENTS_PER_BATCH),
            shots,
        );
        declared.extend(shot_ids.into_iter().map(|id| (id, seq)));
        if activity.is_empty() && events.is_empty() && screenshots.is_empty() {
            break;
        }
    }
    drop(outbox);

    let mut shots = state.screenshots.lock().unwrap();
    for (id, seq) in declared {
        if let Some(s) = shots.get_mut(&id) {
            s.declared_seq = Some(seq);
        }
    }
    seq
}

#[cfg(test)]
mod tests {
    use super::{needs_declaring, take_front};

    /// WP-208: a frame is declared once per queued batch, and again only after that batch was acked
    /// without the upload landing.
    #[test]
    fn a_frame_is_not_redeclared_while_its_batch_is_queued() {
        assert!(needs_declaring(None, Some(5)), "never declared");
        assert!(
            !needs_declaring(Some(7), Some(5)),
            "its batch is still queued"
        );
        assert!(!needs_declaring(Some(5), Some(5)), "its batch is the head");
        assert!(
            needs_declaring(Some(4), Some(5)),
            "acked, still pending: declare again"
        );
        assert!(
            needs_declaring(Some(4), None),
            "queue drained, still pending"
        );
    }

    #[test]
    fn a_large_backlog_is_split_in_order() {
        let mut v: Vec<u32> = (0..5).collect();
        assert_eq!(take_front(&mut v, 2), [0, 1]);
        assert_eq!(take_front(&mut v, 10), [2, 3, 4]);
        assert!(take_front(&mut v, 3).is_empty());
    }
}
