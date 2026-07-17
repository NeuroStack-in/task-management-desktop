//! Activity monitor — **timer-gated** (BUILD-PLAN §4). Kept FREE of Tauri types in its leaves
//! (`bucket`, `input`, `idle`, `active_window`, `rules`) so a headless daemon can be split back out
//! later without a rewrite (§0); only this `mod` touches the `AppHandle`.
//!
//! **Thread A** — one dedicated OS thread, 1 s tick. Capture runs only while the timer is on (the
//! internal gate *is* `reflect`'s idempotent start/stop). Each tick: `user-idle` → idle secs;
//! `device_query` → kb/mouse deltas; every `WINDOW_SAMPLE_EVERY`th tick `x-win` → app/title →
//! `rules::classify_focus` → `AppSpan`. Ticks fold into per-minute `bucket`s; sealed rollups drain to
//! `AppState::pending_activity` for the next batch cycle. An idle **prompt** fires at 5 min and a
//! **hard auto-stop** at 15 min.

pub mod active_window;
pub mod bucket;
pub mod idle;
pub mod input;
pub mod screenshot;
pub mod session;

use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use wp_agent_contract::StopReason;

use crate::state::AppState;
use crate::{clock, events};
use bucket::Bucketer;
use input::InputSampler;

/// Sample the foreground window every Nth tick (cheaper than every second).
const WINDOW_SAMPLE_EVERY: u64 = 5;
/// Prompt the user after this much continuous idle.
const IDLE_PROMPT_SECS: u64 = 300;
/// Hard-stop the timer after this much continuous idle (no productive time is invented).
const AUTO_STOP_SECS: u64 = 900;
/// Anti-evasion jitter around the screenshot cadence (BUILD-PLAN §4).
const SCREENSHOT_JITTER_SECS: i64 = 60;

/// Spawn Thread A. Runs for the app's lifetime; capture is gated internally on `timer.is_running()`.
pub fn spawn(app: AppHandle) {
    thread::spawn(move || run(app));
}

/// Spawn **Thread C** — the jittered screenshot loop (BUILD-PLAN §4, timer + consent + cadence gated,
/// fails closed). Runs on the async runtime; the blocking grab/encode goes to `spawn_blocking`.
pub fn spawn_screenshots(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let interval = {
                let state = app.state::<AppState>();
                let cadence = state.config.lock().unwrap().get().tracking.cadence;
                cadence.interval_secs()
            };
            let Some(base) = interval.map(|s| s as i64) else {
                // Cadence::Off → no screenshots; re-check in a minute.
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            };
            let now = clock::now_epoch_ms();
            let jitter = (now % (2 * SCREENSHOT_JITTER_SECS + 1)) - SCREENSHOT_JITTER_SECS;
            tokio::time::sleep(Duration::from_secs((base + jitter).max(1) as u64)).await;

            // Gates (scoped so `state` isn't held across the blocking capture).
            let (go, blur) = {
                let state = app.state::<AppState>();
                let running = state.timer.lock().unwrap().is_running();
                let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
                let cfg = state.config.lock().unwrap();
                let t = &cfg.get().tracking;
                let on = !matches!(t.cadence, wp_agent_contract::Cadence::Off);
                (running && consented && on, t.blur_level)
            };
            if !go {
                continue;
            }

            let app_name = active_window::current().map(|f| f.app).unwrap_or_default();
            let cap_ts = clock::now_epoch_ms();
            let shot =
                tokio::task::spawn_blocking(move || screenshot::capture(&app_name, blur, cap_ts))
                    .await
                    .ok()
                    .flatten();

            match shot {
                Some((meta, path)) => {
                    app.state::<AppState>().screenshots.lock().unwrap().insert(
                        meta.id.clone(),
                        crate::state::PendingShot {
                            meta,
                            path,
                            attempts: 0,
                        },
                    );
                }
                // Capture failed where it shouldn't (e.g. macOS grant denied) — surface, not silence.
                None => {
                    let _ = app.emit(events::SCREENSHOT_UNAVAILABLE, ());
                }
            }
        }
    });
}

fn run(app: AppHandle) {
    let mut bucketer = Bucketer::new();
    let mut sampler = InputSampler::new();
    let mut tick: u64 = 0;
    let mut was_running = false;
    let mut idle_prompted = false;

    loop {
        thread::sleep(Duration::from_secs(1));
        let state = app.state::<AppState>();

        // Capture only while the timer runs **and** monitoring consent is granted (PRIVACY.md —
        // consent-gated, fails closed). Time tracking can run without consent; activity capture can't.
        let running = state.timer.lock().unwrap().is_running();
        let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
        if running && consented {
            let now = clock::now_epoch_ms();
            let idle = idle::idle_seconds();
            let (kb, mouse) = sampler.sample();
            bucketer.tick(now, idle, kb, mouse);

            if tick.is_multiple_of(WINDOW_SAMPLE_EVERY) {
                if let Some(f) = active_window::current() {
                    let rules = state.config.lock().unwrap().get().rules.clone();
                    if !crate::rules::is_untracked(&f.app, &rules) {
                        let cat = crate::rules::classify_focus(&f.app, &rules);
                        bucketer.sample_app(
                            now,
                            f.app,
                            f.title,
                            f.url,
                            cat,
                            WINDOW_SAMPLE_EVERY as u32,
                        );
                    }
                }
            }

            if idle >= AUTO_STOP_SECS {
                // Hard stop: seal the open bucket and end the session with `Idle`. The timer is now
                // off, so `was_running` clears — no redundant seal next tick.
                let ev = state.timer.lock().unwrap().stop(now, StopReason::Idle);
                if let Some(e) = ev {
                    state.pending_events.lock().unwrap().push(e);
                }
                bucketer.seal();
                let _ = app.emit(events::TRACKING_CHANGED, ());
                idle_prompted = false;
                was_running = false;
            } else {
                if idle >= IDLE_PROMPT_SECS && !idle_prompted {
                    let _ = app.emit(events::IDLE_PROMPT, idle);
                    idle_prompted = true;
                } else if idle < IDLE_PROMPT_SECS {
                    idle_prompted = false;
                }
                was_running = true;
            }
            tick = tick.wrapping_add(1);
        } else if was_running {
            // Timer just stopped: seal the open minute so its partial rollup isn't lost.
            bucketer.seal();
            was_running = false;
            idle_prompted = false;
        }

        let sealed = bucketer.take_sealed();
        if !sealed.is_empty() {
            state.pending_activity.lock().unwrap().extend(sealed);
        }
    }
}
