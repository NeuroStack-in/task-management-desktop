//! Event names the core emits to the webview (`listen()` on the JS side). Stable strings the UI
//! branches on — keep them in sync with `ui/src/lib/ipc.ts`.

/// The session/refresh token expired → the UI must return to the login screen (M1).
pub const AUTH_EXPIRED: &str = "auth:expired";

/// The user has been idle past the prompt threshold → offer keep/stop (M4, 5 min; hard stop 15 min).
pub const IDLE_PROMPT: &str = "monitor:idle-prompt";

/// Tracking state changed (started/stopped) → refresh the indicator.
pub const TRACKING_CHANGED: &str = "monitor:tracking-changed";
