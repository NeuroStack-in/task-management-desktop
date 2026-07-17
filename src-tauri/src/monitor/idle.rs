//! Idle detection. M4: `user-idle` for seconds-since-last-input. Two thresholds live downstream:
//! this per-tick bucket threshold (active vs idle second), and the UI idle **prompt** at 5 min with
//! a **hard auto-stop at 15 min** (BUILD-PLAN §4).

/// Below this many idle seconds a tick counts as **active** in the minute bucket (BUILD-PLAN §4).
pub const IDLE_THRESHOLD_SECS: u64 = 2;

/// Seconds since the last input. M4: `user-idle`. On Wayland this legitimately returns 0 (no global
/// input API) — screenshots-only there (risk #4).
pub fn idle_seconds() -> u64 {
    0 // TODO(M4): user-idle
}
