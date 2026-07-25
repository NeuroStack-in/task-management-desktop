//! Event names the core emits to the webview (`listen()` on the JS side). Stable strings the UI
//! branches on — keep them in sync with `ui/src/lib/ipc.ts`.

/// The session/refresh token expired → the UI must return to the login screen (M1).
pub const AUTH_EXPIRED: &str = "auth:expired";

/// The user has been idle past the prompt threshold → offer keep/stop (M4, 5 min; hard stop 15 min).
pub const IDLE_PROMPT: &str = "monitor:idle-prompt";

/// Tracking state changed (started/stopped) → refresh the indicator.
pub const TRACKING_CHANGED: &str = "monitor:tracking-changed";

/// A screenshot capture failed where it should have worked (e.g. macOS Screen-Recording denied) →
/// the UI surfaces a "grant permission" state instead of silence (M5, risk #5).
pub const SCREENSHOT_UNAVAILABLE: &str = "monitor:screenshot-unavailable";

/// An administrator asked for an on-demand screenshot of this machine (`capture_now`). Payload: the
/// sentence to show the employee — taken **or refused**, and why. Never covert: the same line is
/// appended to the local privacy log (`privacy_log.rs`), so it survives the banner being dismissed.
pub const ADMIN_CAPTURE: &str = "privacy:admin-capture";

/// A restricted (blocked-list) app/site was focused while the timer ran. Payload: the offending
/// identifier (domain or process). The panel shows the policy warning; the violation itself was
/// already queued for the server (`PolicyViolation`, action_taken = "warned").
pub const POLICY_BLOCKED: &str = "monitor:policy-blocked";
