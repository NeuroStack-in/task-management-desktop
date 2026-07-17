//! WorkPulse tray — the employee-facing surface of the desktop agent.
//!
//! This is the ONLY process a user interacts with. It shows the privacy/consent state and the
//! live capture indicator, exposes the personal timer (start/stop/switch task), and lets the user
//! request a bounded pause. It holds **no capture capability and no network egress** — it is a thin
//! remote control that talks to `agentd` over local IPC (`agent_shared::ipc`). Everything the tray
//! shows is state the core owns; every action the tray takes is a `TrayCommand` sent to the core.
//!
//! Slices (LLD Appendix A) live under `features/` and back the Tauri `#[command]`s the webview calls.

mod features;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

/// Shared handle to the core connection. Today a stub; production wraps the IPC channel to `agentd`.
pub struct CoreLink;

impl CoreLink {
    fn connect() -> Self {
        // TODO(ipc): open the local socket/named-pipe to agentd and start the state subscription.
        CoreLink
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(CoreLink::connect())
        .setup(|app| {
            // The panel window starts hidden; clicking the tray icon toggles it.
            let toggle = MenuItem::with_id(app, "toggle", "Show WorkPulse", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &quit])?;

            // The icon must be set here. This is the only tray icon: `app.trayIcon` is
            // deliberately absent from tauri.conf.json, because Tauri auto-builds one from
            // that config (app.rs) and we'd end up with two icons — a menu-less duplicate
            // alongside this one.
            TrayIconBuilder::with_id("main")
                .icon(
                    app.default_window_icon()
                        .expect("bundle.icon is configured in tauri.conf.json")
                        .clone(),
                )
                .tooltip("WorkPulse")
                .menu(&menu)
                // Left click toggles the panel (the expected gesture); the menu is on right click.
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => toggle_panel(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_panel(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            features::consent::get_consent_state,
            features::consent::grant_consent,
            features::capture_indicator::capture_state,
            features::settings_view::effective_config,
            features::pause::request_pause,
            features::timer_ui::timer_state,
            features::timer_ui::start_timer,
            features::timer_ui::stop_timer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WorkPulse tray");
}

/// Show/hide the panel window (invoked from the tray menu).
fn toggle_panel<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(win) = app.get_webview_window("panel") {
        let visible = win.is_visible().unwrap_or(false);
        if visible {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}
