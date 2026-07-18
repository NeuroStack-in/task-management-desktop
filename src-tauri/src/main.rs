// Thin shim — all logic lives in the lib so integration tests can link it (BUILD-PLAN §1).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // Load `.env` if present (config like WP_COGNITO_CLIENT_ID). Real OS env vars still win — dotenvy
    // does not override an already-set variable. Absent .env is fine (defaults + real env).
    let _ = dotenvy::dotenv();

    // Headless dev hook: assemble a synthetic cycle, print it, and check the ≤60 invariant — no
    // window, no network (BUILD-PLAN M8). Runs before any Tauri init.
    if std::env::args().any(|a| a == "--dump-cycle") {
        workpulse_agent_lib::dump_cycle();
        return;
    }
    workpulse_agent_lib::run();
}
