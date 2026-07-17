//! WorkPulse desktop agent — **ONE Tauri process** (BUILD-PLAN §0/§1, which amends LLD Appendix A's
//! 3-process split). The Rust core owns capture, the timer, the outbox, config, and the sole network
//! egress; a Preact webview (`ui/`) is the surface.
//!
//! **Design invariant (§0):** `monitor/` stays free of Tauri types, so a headless daemon can be
//! split back out later (to regain tamper-resistance / survive-logout) without a rewrite.
//!
//! M0 stands up the structure + the four surviving real slices (`outbox`, `timer`, `monitor::input`,
//! `rules`) with their tests. Auth (M1), the heartbeat/upload rail (M2), the timer+selector UI (M3),
//! the 1 s monitor thread (M4), screenshots (M5), shell/tray polish (M6), and the updater (M7) land
//! with their milestones — see `docs/BUILD-PLAN.md`.
#![allow(dead_code)] // M0 scaffold: several slices are seams filled in M1–M8.

pub mod api;
pub mod auth;
pub mod clock;
pub mod commands;
pub mod config;
pub mod cycle;
pub mod error;
pub mod events;
pub mod heartbeat;
pub mod lifecycle;
pub mod monitor;
pub mod outbox;
pub mod rules;
pub mod state;
pub mod timer;
pub mod updater;
pub mod window_size;

use state::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

/// Build and run the Tauri app.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::auth_login,
            commands::auth_complete_new_password,
            commands::auth_logout,
            commands::auth_status,
            commands::timer_start,
            commands::timer_stop,
            commands::timer_status,
            commands::agent_id,
        ])
        .setup(|app| {
            // Try to resume a session from the keyring, then start the 300 s sender loop (Thread B).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let restored = handle.state::<AppState>().auth.restore().await;
                tracing::info!("auth restore at startup: {restored}");
            });
            api::spawn_sender(app.handle().clone());

            // Thread A: the 1 s activity monitor (timer-gated internally).
            monitor::spawn(app.handle().clone());

            // Tray: menu + tooltip. M6 makes the tooltip reflect tracking/idle and adds
            // minimize-to-tray + auto-sign-out on quit.
            let show = MenuItem::with_id(app, "show", "Show WorkPulse", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let mut tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .tooltip("WorkPulse");
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.on_menu_event(|app, event| match event.id.as_ref() {
                "quit" => app.exit(0),
                "show" => {
                    if let Some(w) = app.get_webview_window("panel") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                _ => {}
            })
            .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the WorkPulse agent");
}
