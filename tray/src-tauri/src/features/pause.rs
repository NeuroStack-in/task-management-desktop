//! Bounded pause. The user can request a privacy pause (e.g. a personal break); the core grants it
//! up to an admin-configured maximum, suspends capture for that window, and records the pause as an
//! auditable event. The tray only *requests* — the core decides the allowed duration and enforces it.

use serde::Serialize;

#[derive(Serialize)]
pub struct PauseGrant {
    /// Whether the pause was granted (admins can disable pausing entirely).
    pub granted: bool,
    /// Actual granted duration, clamped to the org's maximum.
    pub granted_secs: u64,
    /// Remaining pause budget for the day after this grant.
    pub remaining_budget_secs: u64,
}

#[tauri::command]
pub fn request_pause(requested_secs: u64) -> PauseGrant {
    // TODO(ipc): send TrayCommand::Pause; the core clamps to policy, suspends capture (SetPaused to
    // helpers), emits an audit event, and returns the actual grant. Resume via TrayCommand::Resume.
    let _ = requested_secs;
    PauseGrant { granted: false, granted_secs: 0, remaining_budget_secs: 0 }
}
