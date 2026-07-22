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

/// One `splitmix64` step → a uniform `f64` in `[0, 1)`. Cheap, no-dep PRNG; good enough for
/// anti-evasion jitter (not cryptographic). Seeded per shot from `clock::entropy_seed()`.
fn rand01(seed: u64) -> f64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 53 bits → the f64 mantissa range, so the result is uniform in [0, 1).
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// When to take the next screenshot: **exactly one per `interval`-second window, at a uniformly
/// random instant inside that window** (org owner picks the interval, e.g. 10 min; the moment within
/// each 10-min window is random and unpredictable — the anti-evasion property).
///
/// `next_window` is the window index we intend to shoot in; it's clamped so we never target the past
/// (e.g. after a config change or a slow capture). When the target window is the one we're already
/// inside, the random offset is drawn from the **remaining** part of that window, so it stays uniform
/// and never lands before `now`. Pure and deterministic in its inputs → unit-tested below.
///
/// Returns `(sleep_ms, shot_window)`; the caller sleeps `sleep_ms`, captures, then sets
/// `next_window = shot_window + 1` to advance to the following window.
fn schedule_shot(now_ms: i64, interval_secs: i64, next_window: i64, rand01: f64) -> (i64, i64) {
    let win = (interval_secs * 1000).max(1);
    let cur = now_ms.div_euclid(win);
    let target_window = next_window.max(cur);
    let win_start = target_window * win;
    // Inside the current window, only the time left counts; a full window is available otherwise.
    let (lo, span) = if target_window == cur {
        let elapsed = now_ms - win_start; // 0 <= elapsed < win
        (elapsed, win - elapsed)
    } else {
        (0, win)
    };
    let offset = lo + (rand01 * span as f64) as i64;
    let target = win_start + offset;
    ((target - now_ms).max(0), target_window)
}

/// Spawn Thread A. Runs for the app's lifetime; capture is gated internally on `timer.is_running()`.
pub fn spawn(app: AppHandle) {
    thread::spawn(move || run(app));
}

/// Spawn **Thread C** — the screenshot loop (BUILD-PLAN §4, timer + consent + cadence gated, fails
/// closed). Runs on the async runtime; the blocking grab/encode goes to `spawn_blocking`. Cadence is
/// owner-fixed (Off/3/5/10 min); within each window the shot lands at a random instant — see
/// `schedule_shot`.
pub fn spawn_screenshots(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // The window we intend to shoot in next. Advanced past each fired shot so we never take two
        // in one window even if a re-roll would have landed later in the same one.
        let mut next_window: i64 = -1;
        loop {
            let interval = {
                let state = app.state::<AppState>();
                let cadence = state.config.lock().unwrap().get().tracking.cadence;
                cadence.interval_secs()
            };
            let Some(interval_secs) = interval.map(|s| s as i64) else {
                // Cadence::Off → no screenshots; re-check in a minute and re-anchor on resume.
                next_window = -1;
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            };
            let now = clock::now_epoch_ms();
            let (sleep_ms, shot_window) = schedule_shot(
                now,
                interval_secs,
                next_window,
                rand01(clock::entropy_seed()),
            );
            next_window = shot_window + 1;
            tokio::time::sleep(Duration::from_millis(sleep_ms.max(0) as u64)).await;

            // Gates (scoped so `state` isn't held across the blocking capture).
            let (go, blur) = {
                let state = app.state::<AppState>();
                let running = state.timer.lock().unwrap().is_running();
                let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
                // Privacy pause (panel PauseCard) suspends capture until the grant expires.
                let paused = state.pause.lock().unwrap().is_paused(clock::now_epoch_ms());
                let cfg = state.config.lock().unwrap();
                let t = &cfg.get().tracking;
                let on = !matches!(t.cadence, wp_agent_contract::Cadence::Off);
                (running && consented && on && !paused, t.blur_level)
            };
            if !go {
                continue;
            }

            let app_name = active_window::current().map(|f| f.app).unwrap_or_default();
            let cap_ts = clock::now_epoch_ms();
            // One shot per connected display (dual-monitor → two); all share this cycle's timestamp.
            let shots = tokio::task::spawn_blocking(move || {
                screenshot::capture_all(&app_name, blur, cap_ts)
            })
            .await
            .unwrap_or_default();

            if shots.is_empty() {
                // Capture failed where it shouldn't (e.g. macOS grant denied) — surface, not silence.
                let _ = app.emit(events::SCREENSHOT_UNAVAILABLE, ());
            } else {
                let state = app.state::<AppState>();
                let mut store = state.screenshots.lock().unwrap();
                for (meta, path, content_sha256) in shots {
                    store.insert(
                        meta.id.clone(),
                        crate::state::PendingShot {
                            meta,
                            path,
                            attempts: 0,
                            content_sha256,
                        },
                    );
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
        // Privacy pause (panel PauseCard) suspends activity capture until the grant expires.
        let paused = state.pause.lock().unwrap().is_paused(clock::now_epoch_ms());
        if running && consented && !paused {
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
                    state.flush.notify_one(); // an idle auto-stop should reflect as fast as a manual one
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

        // Keep the tray tooltip in sync with tracking state (M6).
        if let Some(tray) = app.tray_by_id("main") {
            let tip = if running && consented {
                "WorkPulse — tracking"
            } else if running {
                "WorkPulse — consent required"
            } else {
                "WorkPulse — idle"
            };
            let _ = tray.set_tooltip(Some(tip));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{rand01, schedule_shot};

    const MIN10: i64 = 600; // owner picks "every 10 minutes"

    /// The shot always lands inside the intended window, and never in the past.
    #[test]
    fn shot_is_within_the_window_and_never_past() {
        let win_ms = MIN10 * 1000;
        // Sweep the random value over rand01's real domain [0, 1) and several positions in window 0.
        for r_millis in 0..1000 {
            let r = r_millis as f64 / 1000.0;
            for &now in &[0i64, 1, 123_456, win_ms - 1] {
                let (sleep_ms, shot_window) = schedule_shot(now, MIN10, 0, r);
                assert!(
                    sleep_ms >= 0,
                    "never schedules in the past (r={r}, now={now})"
                );
                let target = now + sleep_ms;
                assert_eq!(shot_window, 0, "stays in window 0 while we're inside it");
                assert!(
                    (0..win_ms).contains(&target),
                    "target {target} must be within window 0 [0,{win_ms})"
                );
            }
        }
    }

    /// Advancing `next_window` past the current one schedules a full window ahead — exactly one
    /// shot per window, so consecutive shots are ~[interval range] apart, not clustered.
    #[test]
    fn one_shot_per_window_when_advancing() {
        let win_ms = MIN10 * 1000;
        // We're at t=90s into window 0 but already intend to shoot in window 1.
        let now = 90_000;
        let (sleep_ms, shot_window) = schedule_shot(now, MIN10, 1, 0.0);
        assert_eq!(shot_window, 1);
        // r=0 → the earliest instant of window 1 == win_ms.
        assert_eq!(now + sleep_ms, win_ms);
        let (sleep_ms_max, _) = schedule_shot(now, MIN10, 1, 0.999_999);
        assert!(now + sleep_ms_max < 2 * win_ms, "still within window 1");
    }

    /// A stale `next_window` (from a slow capture or a cadence change) is clamped up to the current
    /// window — we never target a window that has already elapsed.
    #[test]
    fn stale_next_window_is_clamped_to_now() {
        let now = 5 * MIN10 * 1000 + 42; // deep into window 5
        let (sleep_ms, shot_window) = schedule_shot(now, MIN10, 0 /* way behind */, 0.5);
        assert_eq!(shot_window, 5, "clamped forward to the current window");
        assert!(now + sleep_ms >= now, "never in the past");
    }

    #[test]
    fn rand01_stays_in_unit_interval() {
        for seed in [0u64, 1, 42, u64::MAX, 9_999_999_999] {
            let r = rand01(seed);
            assert!((0.0..1.0).contains(&r), "rand01({seed}) = {r} out of [0,1)");
        }
    }
}
