//! Auto-update via the Tauri 2 signed-updater channel (staged rollout, signature-verified).
//! The check/apply is driven from the tray; this slice tracks channel + applied version.
//!
//! Skeleton placeholder; production integrates `tauri-plugin-updater`.

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
