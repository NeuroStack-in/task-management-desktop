//! Active vs idle: idle at ≥ 5 min with no input; per-minute active/idle rollups. Feeds break
//! derivation (idle ≥ 15 min ⇒ break) in the core's `attendance_events`.
//!
//! Skeleton stub; production uses `user-idle`.

const IDLE_THRESHOLD_SECS: u64 = 300;

/// Whether the user is idle given seconds since last input.
pub fn is_idle(idle_secs: u64) -> bool {
    idle_secs >= IDLE_THRESHOLD_SECS
}

/// Read seconds since last input. Skeleton returns 0 (always active).
pub fn idle_seconds() -> u64 {
    // TODO(user-idle): query the OS idle time.
    0
}
