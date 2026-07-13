//! Tray feature slices (LLD Appendix A). Each backs the Tauri `#[command]`s the webview calls;
//! none of them capture or upload — they read state from / send commands to the core over IPC.

pub mod capture_indicator;
pub mod consent;
pub mod pause;
pub mod settings_view;
pub mod timer_ui;
