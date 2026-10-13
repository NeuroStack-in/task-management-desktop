//! The one time source. `captured_at` **and** the monitor's minute buckets must derive from a
//! single clock, or a skewed device files activity in the wrong minute and mis-keys a `TimeEntry`
//! (BUILD-PLAN risk #9).
//!
//! **It is the local wall clock — not monotonic, and with no server offset.** The planned anchoring
//! to the server's `Date` header was never built; if it is, it belongs here, and `SystemTime::now()`
//! must still have no other call site. What guards a session against clock *steps* is the monitor:
//! a forward jump reads as a suspend (the session ends at the last tick and resumes on input), and a
//! backward step splits the session at the step so neither half is shortened (QA audit 2026-10-08,
//! WP-448 — `monitor::run`).

use std::time::{SystemTime, UNIX_EPOCH};

/// Epoch milliseconds, from the local wall clock.
pub fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Nanosecond-resolution entropy for seeding jitter — **NOT a time source**. Never key data or
/// buckets off this; it exists only so screenshot timing can't be predicted (sub-second precision an
/// employee can't observe). Kept here so `SystemTime::now()` still has exactly one call site.
pub fn entropy_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Whether `s` is shaped like the panel's local date, `YYYY-MM-DD`. The core has no local calendar,
/// so the webview supplies the user's day; this only checks the shape before it is trusted as a
/// path query, a budget key or a comparison (`YYYY-MM-DD` sorts as dates do).
pub fn is_local_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::is_local_date;

    #[test]
    fn only_a_yyyy_mm_dd_date_passes() {
        assert!(is_local_date("2026-10-09"));
        for bad in [
            "",
            "2026-1-09",
            "2026/10/09",
            "2026-10-09&x=1",
            "../2026-10",
            "２０２６-10-09",
        ] {
            assert!(!is_local_date(bad), "{bad}");
        }
    }
}
