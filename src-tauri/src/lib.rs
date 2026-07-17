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
pub mod util;
pub mod window_size;

use state::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, RunEvent, WindowEvent,
};
use wp_agent_contract::StopReason;

fn focus_panel<M: Manager<tauri::Wry>>(app: &M) {
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Build and run the Tauri app.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app = tauri::Builder::default()
        // single-instance must be registered first: a second launch focuses the running one.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_panel(app);
        }))
        .plugin(tauri_plugin_shell::init())
        // Persists + restores the panel window size/position (debounced) — M6 window-size.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::auth_login,
            commands::auth_complete_new_password,
            commands::auth_logout,
            commands::auth_status,
            commands::set_consent,
            commands::consent_status,
            commands::timer_start,
            commands::timer_stop,
            commands::timer_status,
            commands::agent_id,
            commands::check_for_updates,
        ])
        // Minimize-to-tray: closing the panel hides it; the agent keeps running behind the tray.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            // Resume a keyring session, then start the sender (Thread B) + monitor (A) + screenshots (C).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let restored = handle.state::<AppState>().auth.restore().await;
                tracing::info!("auth restore at startup: {restored}");
            });
            api::spawn_sender(app.handle().clone());
            monitor::spawn(app.handle().clone());
            monitor::spawn_screenshots(app.handle().clone());

            // Register autostart (agent should relaunch at login). `WP_NO_AUTOSTART` opts out in dev.
            #[cfg(desktop)]
            if std::env::var_os("WP_NO_AUTOSTART").is_none() {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
            }

            // M7: check for a signed update at startup (no-op without a pubkey; `WP_NO_UPDATE` skips).
            if std::env::var_os("WP_NO_UPDATE").is_none() {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let auto = h
                        .state::<AppState>()
                        .config
                        .lock()
                        .unwrap()
                        .get()
                        .tracking
                        .auto_update;
                    match updater::check_and_maybe_install(&h, auto).await {
                        Ok(available) => tracing::info!("update check: available={available}"),
                        Err(e) => tracing::info!("update check skipped/failed: {e}"),
                    }
                });
            }

            // Tray: menu + a tooltip the monitor keeps in sync with tracking state.
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
                "show" => focus_panel(app),
                _ => {}
            })
            .build(app)?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the WorkPulse agent");

    app.run(|app_handle, event| {
        // Auto-sign-out on quit (tray Quit / Ctrl-C / SIGTERM all funnel to ExitRequested). Bounded:
        // stop the timer with `Shutdown` and clear the session/keyring. Idempotent.
        if let RunEvent::ExitRequested { .. } = event {
            let state = app_handle.state::<AppState>();
            let ts = clock::now_epoch_ms();
            if let Some(ev) = state.timer.lock().unwrap().stop(ts, StopReason::Shutdown) {
                state.pending_events.lock().unwrap().push(ev);
            }
            state.auth.logout();
        }
    });
}
