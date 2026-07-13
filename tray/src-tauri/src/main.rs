// Hide the console window on Windows release builds (the tray is windowless by design).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    workpulse_tray_lib::run()
}
