//! Consent gate. On first run (and whenever monitoring policy changes) the user must see exactly
//! what is captured and acknowledge it before the core is allowed to start capture. The tray shows
//! the disclosure; the core enforces that no capture begins until `granted` is true.
//!
//! `silent` mode (admin-configured, LLD CONFIG) changes the *copy* — the user is still informed the
//! device is managed — but consent acknowledgement is still recorded.

use serde::Serialize;

#[derive(Serialize)]
pub struct ConsentState {
    /// Whether the user has acknowledged the current monitoring disclosure.
    pub granted: bool,
    /// The disclosure version acknowledged; a policy change bumps this and re-prompts.
    pub policy_version: u32,
    /// Human-readable list of what is captured, shown verbatim in the disclosure.
    pub captured: Vec<&'static str>,
}

fn disclosure() -> Vec<&'static str> {
    vec![
        "Activity counts (keystroke & mouse totals — never the keys themselves)",
        "Foreground app / website category",
        "Periodic screenshots (blurred per your organization's policy)",
        "Attendance and timer events",
    ]
}

#[tauri::command]
pub fn get_consent_state() -> ConsentState {
    // TODO(ipc): read the persisted consent record from the core.
    ConsentState {
        granted: false,
        policy_version: 1,
        captured: disclosure(),
    }
}

#[tauri::command]
pub fn grant_consent(policy_version: u32) -> ConsentState {
    // TODO(ipc): send TrayCommand::ConsentGranted to the core, which persists the acknowledged
    // policy_version and unblocks capture. Returned state reflects the acknowledgement.
    ConsentState {
        granted: true,
        policy_version,
        captured: disclosure(),
    }
}
