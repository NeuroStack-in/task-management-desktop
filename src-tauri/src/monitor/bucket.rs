//! Per-minute activity aggregation — the real divergence from the reference sample, which keeps one
//! cumulative bucket per 300 s heartbeat while **our** contract wants per-minute `ActivityRollup`
//! (BUILD-PLAN §4/§5). M4 implements:
//!
//! - `MinuteBucket` keyed `epoch_ms / 60_000`.
//! - each 1 s tick: `idle_secs > IDLE_THRESHOLD_SECS` → `idle_sec += 1`, else `active_sec += 1`.
//!   **Invariant: `active_sec + idle_sec ≤ 60`** — golden-tested.
//! - seal on the minute boundary, on timer-stop, and on a screenshot early-flush (so a shot maps to
//!   its exact minute and isn't double-counted).
//! - `top_apps` capped at `MAX_APPS_PER_BUCKET`, top-N by seconds.

/// Max app spans kept per minute bucket (top-N by seconds).
pub const MAX_APPS_PER_BUCKET: usize = 30;
