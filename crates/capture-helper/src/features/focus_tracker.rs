//! Foreground app/window/URL tracking: focus-change events + a 1s poll, folded into `AppSpan`s
//! (app/title/URL + duration), classified on-device by `rule_classifier`.
//!
//! Skeleton stub; production uses `active-win-pos-rs`.

/// The current foreground target (before classification).
pub struct Focus {
    pub app: String,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Read the current foreground window. Skeleton returns `None`.
pub fn current() -> Option<Focus> {
    // TODO(active-win-pos-rs): app name, window title, and (for browsers) the foreground URL.
    None
}
