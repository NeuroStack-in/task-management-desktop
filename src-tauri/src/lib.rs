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
pub mod location;
pub mod monitor;
pub mod mqtt;
pub mod outbox;
pub mod privacy_log;
pub mod rules;
pub mod session_state;
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

/// `--dump-cycle` (BUILD-PLAN M8): feed the bucketer 5 synthetic minutes, print the resulting cycle
/// (heartbeat + rollups) as JSON, and assert the `active_sec + idle_sec ≤ 60` invariant. Headless.
pub fn dump_cycle() {
    use monitor::bucket::Bucketer;

    let mut b = Bucketer::new();
    let base: i64 = 1_700_000_000_000; // fixed epoch ms for a stable dump
    for m in 0..5i64 {
        for s in 0..60i64 {
            let now = base + (m * 60 + s) * 1000;
            let idle = if s < 45 { 0 } else { 5 }; // 45 active + 15 idle seconds
            b.tick(now, idle, 1, 1);
        }
    }
    b.seal();
    let rollups = b.take_sealed();
    let hb = heartbeat::collect(env!("CARGO_PKG_VERSION"), 0.0, false, None);

    let cycle = serde_json::json!({ "heartbeat": hb, "activity": rollups });
    println!("{}", serde_json::to_string_pretty(&cycle).unwrap());

    for r in &rollups {
        assert!(
            r.active_sec + r.idle_sec <= 60,
            "invariant violated: {} + {}",
            r.active_sec,
            r.idle_sec
        );
    }
    eprintln!("dump-cycle: {} rollups, active+idle ≤ 60 ✓", rollups.len());
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
            commands::update_status,
            commands::update_install,
            commands::set_auto_start,
            commands::get_auto_start,
            // Panel (kishore's tray UI) command surface.
            commands::panel::get_consent_state,
            commands::panel::grant_consent,
            commands::panel::capture_state,
            commands::panel::effective_config,
            commands::panel::timer_state,
            commands::panel::start_timer,
            commands::panel::stop_timer,
            commands::panel::request_pause,
            commands::panel::pause_state,
            commands::panel::identity,
            commands::panel::list_projects,
            commands::panel::list_tasks,
            commands::panel::list_sessions,
            commands::panel::take_pending_resume,
            commands::panel::privacy_log,
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
            // Consent first and synchronously: the monitor and screenshot threads start just below
            // and gate on this flag, so restoring it after they spawn would leave a window in which
            // a consented user is briefly treated as not consented (fail-closed, but it would drop
            // the first cycle) — and, worse, would race the panel into showing the notice again.
            {
                let persisted = crate::session_state::load();
                app.state::<AppState>().consent.store(
                    persisted.restore_consent(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                tracing::info!(
                    "consent restored at startup: {}",
                    persisted.restore_consent()
                );
            }

            tauri::async_runtime::spawn(async move {
                let restored = handle.state::<AppState>().auth.restore().await;
                tracing::info!("auth restore at startup: {restored}");
                // Send the first heartbeat right after a resumed session so the device reappears in the
                // fleet within seconds of launch, not after a full cadence interval.
                if restored {
                    handle.state::<AppState>().flush.notify_one();
                }
            });
            api::spawn_sender(app.handle().clone());
            // The fast config rail: a 60 s conditional (ETag) pull so an admin's policy change lands
            // within a minute instead of waiting up to a full screenshot cadence (10 min) for the
            // next batch-cycle version check. The batch cycle remains the backstop.
            api::spawn_config_poller(app.handle().clone());
            // The MQTT downlink (MQTT-MIGRATION Phase 3): enrolls the device once (first signed-in
            // launch), then holds a mutual-TLS connection to IoT Core for pushed commands + presence.
            // Signals only — batches and config stay on the HTTP rails above, which remain the
            // backstop whenever this connection is down.
            mqtt::spawn(app.handle().clone());
            monitor::spawn(app.handle().clone());
            monitor::spawn_screenshots(app.handle().clone());

            // Autostart policy (was: unconditional `enable()` every launch, which silently forced
            // launch-at-login and re-overrode the user's choice on every start). Now:
            //   • Windows — the installer's "Launch at startup" checkbox writes the registry Run key;
            //     respect it, never override.
            //   • macOS/Linux — no install wizard, so default autostart ON on the FIRST run only. The
            //     user can flip it afterward via `set_auto_start`, and that choice sticks.
            // `WP_NO_AUTOSTART` opts out entirely (dev).
            #[cfg(desktop)]
            if std::env::var_os("WP_NO_AUTOSTART").is_none() {
                use tauri_plugin_autostart::ManagerExt;
                let cfg_dir = app.path().app_config_dir().ok();
                let marker = cfg_dir.as_ref().map(|d| d.join("autostart.initialized"));
                let first_run = marker.as_ref().map(|m| !m.exists()).unwrap_or(false);
                if first_run {
                    // Initial launch-at-login state:
                    //  • Windows — honor the installer's prompt, recorded in `autostart.choice`
                    //    (1/0). Absent (portable/dev) → leave off.
                    //  • macOS/Linux — no install wizard, so default ON.
                    // The autostart PLUGIN writes the actual OS entry either way, so the state always
                    // agrees with `get_auto_start` (no NSIS-vs-plugin registry-format drift).
                    let enable = if cfg!(target_os = "windows") {
                        cfg_dir
                            .as_ref()
                            .and_then(|d| std::fs::read_to_string(d.join("autostart.choice")).ok())
                            .map(|s| s.trim() == "1")
                            .unwrap_or(false)
                    } else {
                        true
                    };
                    if enable {
                        let _ = app.autolaunch().enable();
                    }
                    if let Some(m) = &marker {
                        if let Some(parent) = m.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::write(m, b"1");
                    }
                }
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

                // …and re-check on a slow cadence so a long-running agent adopts a published release
                // without a restart. Reads `auto_update` fresh each pass, so a policy change takes
                // effect on the next tick. Same `WP_NO_UPDATE` gate as the startup check above.
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    const EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
                    loop {
                        tokio::time::sleep(EVERY).await;
                        let auto = h
                            .state::<AppState>()
                            .config
                            .lock()
                            .unwrap()
                            .get()
                            .tracking
                            .auto_update;
                        match updater::check_and_maybe_install(&h, auto).await {
                            Ok(available) => {
                                tracing::info!("periodic update check: available={available}")
                            }
                            Err(e) => tracing::info!("periodic update check skipped/failed: {e}"),
                        }
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
        // Close the running session on quit (tray Quit / Ctrl-C / SIGTERM all funnel to
        // ExitRequested), stopping the timer with `Shutdown` so the day isn't left open
        // (BUILD-PLAN.md:97). Idempotent.
        //
        // The session is deliberately **not** cleared here. Tokens live in the OS keyring and refresh
        // is automatic (ENROLLMENT.md:11); `setup()` calls `auth.restore()` at startup for exactly
        // this reason. Signing out on every quit made that restore dead code and forced a fresh login
        // on each launch — for an agent that autostarts at login, that is the difference between
        // "monitoring resumes" and "monitoring silently doesn't". Sign-out is a user action
        // (`auth_logout`), not a side effect of closing the window.
        if let RunEvent::ExitRequested { .. } = event {
            let state = app_handle.state::<AppState>();
            // Best-effort clean MQTT presence: queue `{"online":false}` + DISCONNECT so the fleet
            // flips offline immediately; if the process dies first, the broker's Last Will delivers
            // the same payload after the keepalive window.
            mqtt::shutdown(&state);
            let ts = clock::now_epoch_ms();
            // Bound separately so the timer's MutexGuard is dropped before `state` goes out of
            // scope at the end of the block — an inline `if let` keeps the temporary alive too long.
            // Remember what was running *before* stopping it, so reopening can pick the same task
            // back up. Read under the same lock scope as the stop so the two cannot disagree.
            let resume = {
                let mut t = state.timer.lock().unwrap();
                let snap = t.snapshot(ts);
                let resume = snap.running.then(|| crate::session_state::ResumeTask {
                    task_id: snap.task_id.clone().unwrap_or_default(),
                    project_id: snap.project_id.clone().unwrap_or_default(),
                    description: snap.description.clone(),
                    stopped_at_ms: ts,
                });
                let stopped = t.stop(ts, StopReason::Shutdown);
                if let Some(ev) = stopped {
                    state.pending_events.lock().unwrap().push(ev);
                }
                resume
            };
            // Persist that `TimerStopped` before the process dies. `pending_events` is an in-memory
            // Vec that only reaches the durable outbox via `assemble_and_enqueue`, which the sender
            // normally drives on its cycle — but nothing here waits for the sender, and the runtime
            // is about to go away. Without this the stop event was simply lost, so the server never
            // learned the session ended and the web UI showed "Recording" indefinitely.
            //
            // Enqueue is synchronous and writes to `queue/batches.jsonl`, so the stop survives the
            // exit and ships on the next launch even when quitting offline. (Sending it *now* isn't
            // possible: the async sender can't be awaited from this handler.) The server-side
            // stale-session reaper is what covers the harder cases — a crash or power loss, where
            // this handler never runs at all.
            let seq = crate::cycle::assemble_and_enqueue(&state);
            tracing::info!(batch_seq = seq, "shutdown: queued the final batch (timer stop)");

            // The session is still *closed* on the server (the TimerStopped event above): the offline
            // period must not be billed, since nothing was captured during it. Reopening starts a
            // fresh session on the same task, and today's total comes from the folded entries.
            crate::session_state::update(|s| s.resume = resume);
        }
    });
}
