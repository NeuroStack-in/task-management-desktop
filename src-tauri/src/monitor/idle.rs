//! Idle detection via `user-idle` (seconds since the last input). Two thresholds live downstream:
//! this per-tick bucket threshold (active vs idle second), and the UI idle **prompt** at 5 min with
//! a **hard auto-stop at 15 min** (BUILD-PLAN §4). On Wayland `user-idle` legitimately returns 0 —
//! screenshots-only there (risk #4).

/// Below this many idle seconds a tick counts as **active** in the minute bucket (BUILD-PLAN §4).
pub const IDLE_THRESHOLD_SECS: u64 = 2;

/// Seconds since the last input, or 0 if the OS can't report it (Wayland; error).
pub fn idle_seconds() -> u64 {
    user_idle::UserIdle::get_time()
        .map(|t| t.as_seconds())
        .unwrap_or(0)
}
