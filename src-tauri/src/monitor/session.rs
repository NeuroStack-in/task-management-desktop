//! Session / screen-lock detection and the capability report the UI reads. M4 wires screen-lock
//! (`windows-sys` `OpenInputDesktop`, and the macOS/Linux equivalents).
//!
//! `SessionInfo` is the DTO the UI reads to show the honest-degradation banner (e.g. Wayland can't
//! track input/windows — risk #4). **Its field names are load-bearing**: in the reference sample the
//! Rust DTO and the TS reader disagreed, so the Wayland banner could never fire. M6 adds the
//! `#[test]` asserting the serialized field names against a hand-checked TS type (BUILD-PLAN §3) —
//! the only guard, since both compilers are blind to this bug class.

use serde::Serialize;

/// What the agent can actually capture on this OS/session, surfaced to the UI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// False on Wayland and where the OS forbids foreground-window inspection.
    pub can_track_windows: bool,
    /// `"x11"` | `"wayland"` | `"windows"` | `"macos"`.
    pub display_server: String,
    /// Human-readable reason capture is limited, or `None` when unrestricted.
    pub limitation: Option<String>,
}

/// Current session capabilities. M4 fills this in per-OS; M0 reports the unrestricted default.
pub fn session_info() -> SessionInfo {
    SessionInfo {
        can_track_windows: true,
        display_server: std::env::consts::OS.to_string(),
        limitation: None,
    }
}
