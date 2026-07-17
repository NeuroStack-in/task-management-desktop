// Thin shim — all logic lives in the lib so integration tests can link it (BUILD-PLAN §1).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    workpulse_agent_lib::run();
}
