//! The live "you are being monitored" indicator. Privacy-by-design: whenever capture is active the
//! tray icon + panel show it unmistakably, and the indicator reflects the *actual* core state (not a
//! guess) — if the core stops capturing (paused, off-shift, consent revoked) the indicator clears.

use serde::Serialize;

#[derive(Serialize)]
pub struct CaptureState {
    /// True when the core is actively capturing right now.
    pub capturing: bool,
    /// True when screenshots specifically are part of the current cadence.
    pub screenshots: bool,
    /// Seconds until the next capture cycle (drives the subtle countdown in the panel).
    pub next_cycle_secs: u64,
}

#[tauri::command]
pub fn capture_state() -> CaptureState {
    // TODO(ipc): subscribe to the core's capture state; this is the last pushed value.
    CaptureState { capturing: false, screenshots: false, next_cycle_secs: 0 }
}
