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

// A screenshot capture that produces nothing is **no longer surfaced to the employee.** It used to
// emit `monitor:screenshot-unavailable` with a per-platform sentence, but a capture failure is not
// something the person at the keyboard can act on — the message told them to forward the log to their
// admin — and it fired on transient blips that recover on their own. The reason is logged for the
// admin by `monitor::screenshot` (every failure path, plus a `catch_unwind` around the grab); there
// is deliberately no user-facing event for it.

/// An administrator asked for an on-demand screenshot of this machine (`capture_now`). Payload: the
/// sentence to show the employee — taken **or refused**, and why. Never covert: the same line is
/// appended to the local privacy log (`privacy_log.rs`), so it survives the banner being dismissed.
pub const ADMIN_CAPTURE: &str = "privacy:admin-capture";

/// A restricted (blocked-list) app/site was focused while the timer ran. Payload: the offending
/// identifier (domain or process). The panel shows the policy warning; the violation itself was
/// already queued for the server (`PolicyViolation`, action_taken = "warned").
pub const POLICY_BLOCKED: &str = "monitor:policy-blocked";
