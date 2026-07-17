//! Foreground window → app / title / url. M4: `x-win` (`active-win-pos-rs`), sampled every 5th tick
//! (`WINDOW_SAMPLE_EVERY: 5`), classified via `rules::classify` into an `AppSpan`. Partial on
//! Wayland (risk #4); needs Accessibility for titles on macOS (risk #5).

/// A foreground focus observation. Serialized to the UI later with camelCase (BUILD-PLAN §3).
#[derive(Clone, Debug, Default)]
pub struct Focus {
    pub app: String,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// The current foreground window. `None` until M4 wires `x-win` (and where the OS forbids it).
pub fn current() -> Option<Focus> {
    None // TODO(M4): active-win-pos-rs
}
