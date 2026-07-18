//! Foreground window → app / title via `x-win`, sampled every `WINDOW_SAMPLE_EVERY`th tick and
//! classified into an `AppSpan` (BUILD-PLAN §4). Partial on Wayland (risk #4); needs Accessibility
//! for titles on macOS (risk #5). `x-win` reports no URL, so `url` is `None` — browser-URL
//! extraction is a later refinement.

/// A foreground focus observation.
#[derive(Clone, Debug, Default)]
pub struct Focus {
    pub app: String,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// The current foreground window, or `None` when the OS forbids it / nothing is focused.
pub fn current() -> Option<Focus> {
    let w = x_win::get_active_window().ok()?;
    let app = if !w.info.exec_name.is_empty() {
        w.info.exec_name
    } else {
        w.info.name
    };
    if app.is_empty() {
        return None;
    }
    Some(Focus {
        app,
        title: (!w.title.is_empty()).then_some(w.title),
        url: None,
    })
}
