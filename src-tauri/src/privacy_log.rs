//! The **employee-visible** local privacy log — what remains of it.
//!
//! Older builds wrote the admin-triggered on-demand captures (`mqtt::capture`) to
//! `state_dir()/privacy-log.jsonl` and the panel read them back. The owner removed that on-device
//! disclosure on 2026-09-02 (see `mqtt::capture`), so nothing writes the log and nothing reads it:
//! the `privacy_log` command, its writer and the panel card that was never rendered are gone (QA
//! audit 2026-10-08, WP-451). The server-side audit trail (`monitoring.capture_now.*`) is the record.
//!
//! What stays is [`clear`]: a file an older build left behind is the previous user's record of their
//! machine, and an account switch must still not hand it to the next person.

use std::path::PathBuf;

fn path() -> PathBuf {
    crate::outbox::state_dir().join("privacy-log.jsonl")
}

/// Delete the log an older build may have written (account switch, sign-out — see
/// `AppState::reset_for_account_switch`).
pub fn clear() {
    let _ = std::fs::remove_file(path());
}
