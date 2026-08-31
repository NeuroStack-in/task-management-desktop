//! Idle detection via `user-idle` (seconds since the last input). Two thresholds live downstream:
//! this per-tick bucket threshold (active vs idle second), and the UI idle **prompt** at 5 min with
//! a **hard auto-stop at 15 min** (BUILD-PLAN §4). On Wayland `user-idle` legitimately returns 0 —
//! screenshots-only there (risk #4).

/// Below this many idle seconds a tick counts as **active** in the minute bucket.
///
/// **Was 2 seconds, which measured typing rather than working.** At that threshold a second counted
/// as active only if the keyboard or mouse had been touched within the previous two — so reading a
/// document, thinking, watching a demo, or being on a call all recorded as idle. Measured against
/// real days it kept `active_sec` at roughly a third of the time people were actually on the clock,
/// and since the server divides by that to get Utilization, every score in the product was dragged
/// down by an instrumentation artefact rather than by anyone's work.
///
/// A minute is the smallest span that does not punish thought. Someone at their desk touches an
/// input device far more often than once a minute; someone who has left does not, and the two
/// controls that actually decide absence are unchanged and much longer — the idle prompt at 5
/// minutes and the hard auto-stop at 15.
///
/// The cost is deliberate and bounded: up to a minute of real absence can be counted as active.
/// That is the right side to err on. Over-counting a minute nudges a number; under-counting
/// two-thirds of a working day made the whole score meaningless.
pub const IDLE_THRESHOLD_SECS: u64 = 60;

/// Seconds since the last input, or 0 if the OS can't report it (Wayland; error).
pub fn idle_seconds() -> u64 {
    user_idle::UserIdle::get_time()
        .map(|t| t.as_seconds())
        .unwrap_or(0)
}
