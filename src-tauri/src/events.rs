//! Event names the core emits to the webview (`listen()` on the JS side). Stable strings the UI
//! branches on — keep them in sync with `ui/src/lib/ipc.ts`.

/// The session/refresh token expired → the UI must return to the login screen (M1).
pub const AUTH_EXPIRED: &str = "auth:expired";

/// **IT released this device** — the timer was stopped, the outbox flushed, and the agent signed out
/// ([`crate::release`]). The UI must say so rather than just showing the login screen: an unexplained
/// bounce to sign-in reads as a crash, when in fact this was deliberate, the employee's hours are
/// safe, and signing in again (here or on a replacement laptop) is the expected next step.
pub const DEVICE_RELEASED: &str = "device:released";

/// The user has been idle past the prompt threshold → offer keep/stop (M4, 5 min; hard stop 15 min).
pub const IDLE_PROMPT: &str = "monitor:idle-prompt";

/// Tracking state changed (started/stopped) → refresh the indicator.
pub const TRACKING_CHANGED: &str = "monitor:tracking-changed";

/// A screenshot capture failed where it should have worked → the UI surfaces the failure instead of
/// silence (M5, risk #5). **Payload: the sentence to show the employee** (see
/// [`capture_failure_hint`]).
///
/// It used to carry `()`, and the UI hardcoded a macOS "grant screen-recording permission" message
/// for every platform. Windows has no such permission, so a Windows agent spent a day instructing
/// its user to grant something that does not exist while the real cause went unlogged.
pub const SCREENSHOT_UNAVAILABLE: &str = "monitor:screenshot-unavailable";

/// What to tell the employee when capture produced nothing, per platform.
///
/// Only macOS gates screen capture behind a permission grant, so only macOS gets the "grant
/// permission" instruction. Everywhere else the honest answer is that capture failed and the reason
/// is in the log — which now actually contains one (`monitor::screenshot` logs every failure path).
pub fn capture_failure_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Screen capture is blocked by macOS. Grant Screen Recording to WorkPulse in System Settings → Privacy & Security, then restart the agent."
    } else if cfg!(target_os = "linux") {
        "Screen capture failed. On Wayland this usually means the screen-sharing portal denied the request. The agent log has the details."
    } else {
        "Screen capture failed, so no screenshots are being recorded. The agent log has the reason — send it to your admin if this keeps happening."
    }
}

/// An administrator asked for an on-demand screenshot of this machine (`capture_now`). Payload: the
/// sentence to show the employee — taken **or refused**, and why. Never covert: the same line is
/// appended to the local privacy log (`privacy_log.rs`), so it survives the banner being dismissed.
pub const ADMIN_CAPTURE: &str = "privacy:admin-capture";

/// A restricted (blocked-list) app/site was focused while the timer ran. Payload: the offending
/// identifier (domain or process). The panel shows the policy warning; the violation itself was
/// already queued for the server (`PolicyViolation`, action_taken = "warned").
pub const POLICY_BLOCKED: &str = "monitor:policy-blocked";
