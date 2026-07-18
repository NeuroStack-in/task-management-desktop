//! The one time source. `captured_at` **and** the monitor's minute buckets must derive from a
//! single clock, or a skewed device files activity in the wrong minute and mis-keys a `TimeEntry`
//! (BUILD-PLAN risk #9).
//!
//! M0: local epoch-ms. M2 anchors this to the server's `Date` header (monotonic-anchored
//! server-offset) — do the offset **here**, and never call `SystemTime::now()` elsewhere.

use std::time::{SystemTime, UNIX_EPOCH};

/// Epoch milliseconds. (M2: apply the server offset.)
pub fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
