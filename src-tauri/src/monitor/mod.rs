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
/// No user input for this long marks the machine **idle** in the fleet heartbeat. Independent of the
/// timer and shorter than the prompt/auto-stop thresholds: the heartbeat answers "is anyone at this
/// machine right now", not "should we stop the timer".
const HEARTBEAT_IDLE_SECS: u64 = 60;
/// Hard-stop the timer after this much continuous idle (no productive time is invented).
const AUTO_STOP_SECS: u64 = 900;
/// A tick gap beyond this means the process wasn't running — the machine slept, hibernated, or the
/// lid shut. The loop ticks every 1 s, so this is generous enough that scheduler jitter, a busy
/// machine, or a slow disk can never look like a suspend.
const SUSPEND_GAP_MS: i64 = 90_000;
/// How often the running session's liveness note is rewritten to disk. Bounds how much of a
/// crashed session's time is unknowable: the record can be at most this stale.
const ALIVE_HEARTBEAT_TICKS: u64 = 15;

/// The last instant the agent can prove it was awake, given this tick's `now` and the gap that
/// preceded it.
///
/// Trivial arithmetic, named because the subtraction is the entire point: get it backwards and the
/// timer stops at the wake instead of the sleep, billing the night.
fn last_wake_ms(now: i64, gap: i64) -> i64 {
    now - gap
}

/// What the idle thresholds say to do this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleAction {
    /// Nothing to do.
    None,
    /// Cross the prompt threshold — ask whether they are still there.
    Prompt,
    /// Past the hard limit — stop the timer.
    Stop,
}

/// Decide the idle action from the timer state alone.
///
/// **The parameter list is the point.** This deliberately cannot see monitoring consent, the
/// capture pause, or the org's activity entitlement, because for a long time the code that did this
/// job sat *inside* a branch gated on all three — so a user who declined consent, or an org on a
/// plan without `monitoring.activity`, got no idle prompt and no auto-stop at all. Their timers ran
/// unbounded. One real org spent a day like that and recorded 3h41m of tracked time for someone
/// with three seconds of input.
///
/// Capture is about what we may observe. This is about what we may bill. Keep them apart: if a
/// future edit needs a capture flag in here, the answer is that it doesn't.
fn idle_action(running: bool, idle_secs: u64, already_prompted: bool) -> IdleAction {
    if !running {
        return IdleAction::None;
    }
    if idle_secs >= AUTO_STOP_SECS {
        IdleAction::Stop
    } else if idle_secs >= IDLE_PROMPT_SECS && !already_prompted {
        IdleAction::Prompt
    } else {
        IdleAction::None
    }
}
/// Re-warn (and re-report) the same restricted identifier at most once per this window. One
/// YouTube session is one violation with one warning — not sixty per minute; a *different*
/// restricted site during the cooldown still fires immediately (the map is per-identifier).
const VIOLATION_COOLDOWN_MS: i64 = 5 * 60 * 1000;

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

            // Read the foreground window once — used both for the exception carve-out and as the
            // capture's app label. Fetched before the config lock so no syscall runs under it.
            let focus = active_window::current();

            // Gates (scoped so `state` isn't held across the blocking capture).
            let (go, blur, excepted, silent) = {
                let state = app.state::<AppState>();
                let running = state.timer.lock().unwrap().is_running();
                let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
                // Privacy pause (panel PauseCard) suspends capture until the grant expires.
                let paused = state.pause.lock().unwrap().is_paused(clock::now_epoch_ms());
                let cfg = state.config.lock().unwrap();
                let c = cfg.get();
                // Screenshots need both a live cadence AND the org's `monitoring.screenshots`
                // entitlement (mirrored into `screenshots_enabled`). Disabling the feature stops
                // capture without disturbing the admin's chosen interval; activity/heartbeats are
                // gated separately and keep running.
                let on = !matches!(c.tracking.cadence, wp_agent_contract::Cadence::Off)
                    && c.tracking.screenshots_enabled;
                // Privacy exceptions carve-out (§14 / PRIVACY.md §2): a focused excepted app/site is
                // never screenshotted. Match on the full app+title+url haystack, like the span path.
                let excepted = focus.as_ref().is_some_and(|f| {
                    let haystack = format!(
                        "{} {} {}",
                        f.app,
                        f.title.as_deref().unwrap_or(""),
                        f.url.as_deref().unwrap_or("")
                    );
                    crate::rules::is_excepted(&haystack, &c.rules)
                });
                (
                    running && consented && on && !paused,
                    c.tracking.blur_level,
                    excepted,
                    c.tracking.silent,
                )
            };
            if !go || excepted {
                continue;
            }

            let app_name = focus.map(|f| f.app).unwrap_or_default();
            let cap_ts = clock::now_epoch_ms();
            // One shot per connected display (dual-monitor → two); all share this cycle's timestamp.
            let shots = tokio::task::spawn_blocking(move || {
                screenshot::capture_all(&app_name, blur, cap_ts)
            })
            .await
            .unwrap_or_default();

            if shots.is_empty() {
                // Capture failed where it shouldn't. The *reason* is always logged by
                // `screenshot::capture_all`; normally we also surface an employee-facing sentence.
                // Under silent mode (owner policy) it is logged only, never shown — a visible
                // "capture blocked" message would itself reveal that monitoring is happening.
                if !silent {
                    let _ = app.emit(
                        events::SCREENSHOT_UNAVAILABLE,
                        events::capture_failure_hint(),
                    );
                }
            } else {
                let state = app.state::<AppState>();
                let mut store = state.screenshots.lock().unwrap();
                for (meta, path) in shots {
                    store.insert(
                        meta.id.clone(),
                        crate::state::PendingShot {
                            meta,
                            path,
                            attempts: 0,
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
    // Was CAPTURE active last tick (all four gate conditions), not "was the timer running" — the
    // two are different, and conflating them is what disabled the idle auto-stop for anyone who
    // hadn't consented. Named for what it actually gates: sealing the open minute bucket.
    let mut was_capturing = false;
    // Wall-clock of the previous tick, for suspend detection. Seeded with now so the first pass
    // after launch can't read as a 55-year gap.
    let mut last_tick_ms = crate::clock::now_epoch_ms();
    let mut alive_tick: u64 = 0;
    let mut idle_prompted = false;
    // identifier → last time it was warned/reported (the violation debounce).
    let mut last_violation: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();

    loop {
        thread::sleep(Duration::from_secs(1));
        let state = app.state::<AppState>();

        // Capture only while the timer runs **and** monitoring consent is granted (PRIVACY.md —
        // consent-gated, fails closed). Time tracking can run without consent; activity capture can't.
        let running = state.timer.lock().unwrap().is_running();
        let consented = state.consent.load(std::sync::atomic::Ordering::Relaxed);
        // Privacy pause (panel PauseCard) suspends activity capture until the grant expires.
        let paused = state.pause.lock().unwrap().is_paused(clock::now_epoch_ms());

        // Idle for the fleet heartbeat is sampled every tick, independent of the capture gate below:
        // a signed-in machine can be idle whether or not a timer is running. Published for
        // `cycle::assemble_and_enqueue` to fold into the next heartbeat. Reused inside the gate so the
        // OS is queried once per tick.
        let idle = idle::idle_seconds();
        state.idle.store(
            idle >= HEARTBEAT_IDLE_SECS,
            std::sync::atomic::Ordering::Relaxed,
        );

        // The org's `monitoring.activity` entitlement (mirrored into `activity_enabled`) gates
        // activity capture only — the heartbeat above (device liveness) and the timer keep running,
        // so disabling activity still reports the device without app/input rollups.
        let activity_enabled = state.config.lock().unwrap().get().tracking.activity_enabled;

        // Needed by the idle auto-stop below, which runs whether or not capture does.
        let now = clock::now_epoch_ms();

        // ── Woke from suspend ───────────────────────────────────────────────────────────────
        //
        // This loop ticks once a second, so a gap materially larger than that means the process was
        // not running: the machine slept, hibernated, or someone shut the lid. Windows announces
        // some of those through `WM_POWERBROADCAST` and tao surfaces none of them, so the wall
        // clock is the signal that actually works here — and it has the advantage of catching every
        // variant at once, including a hard freeze that no power event would describe.
        //
        // **Backdated to the last tick, never to now.** Those hours were not worked. Stopping at
        // `now` would bill the entire time the laptop spent shut, which is precisely the outcome
        // this exists to prevent; `last_tick_ms` is the most recent instant the agent can prove it
        // was awake.
        //
        // Idle detection cannot cover this on its own: `user-idle` reports seconds since the last
        // input, and after a wake that number may be reset by the unlock keystroke — so a machine
        // that slept for nine hours can come back looking freshly active.
        let gap = now - last_tick_ms;
        last_tick_ms = now;
        if running && gap > SUSPEND_GAP_MS {
            let ev = state
                .timer
                .lock()
                .unwrap()
                .stop(last_wake_ms(now, gap), StopReason::Idle);
            if let Some(e) = ev {
                state.pending_events.lock().unwrap().push(e);
                state.flush.notify_one();
            }
            bucketer.seal();
            was_capturing = false;
            idle_prompted = false;
            let _ = app.emit(events::TRACKING_CHANGED, ());
            crate::session_state::update(|s| s.open = None);
            tracing::info!(
                gap_secs = gap / 1000,
                "woke from suspend; stopped the timer at the last awake tick"
            );
            continue;
        }

        if running && consented && !paused && activity_enabled {
            let (kb, mouse) = sampler.sample();
            bucketer.tick(now, idle, kb, mouse);

            if tick.is_multiple_of(WINDOW_SAMPLE_EVERY) {
                if let Some(f) = active_window::current() {
                    let (rules, silent) = {
                        let cfg = state.config.lock().unwrap();
                        let c = cfg.get();
                        (c.rules.clone(), c.tracking.silent)
                    };

                    // Restricted-list enforcement (LLD §14) — timer-gated by this whole branch,
                    // and checked BEFORE the untracked filter (the three rule lists are
                    // orthogonal: an untracked app can still be restricted). URL when the
                    // platform yields one, title as the fallback signal, process name for apps.
                    let haystack = format!(
                        "{} {} {}",
                        f.app,
                        f.title.as_deref().unwrap_or(""),
                        f.url.as_deref().unwrap_or("")
                    );
                    if let Some((kind, identifier)) = crate::rules::blocked_match(&haystack, &rules)
                    {
                        let due = last_violation
                            .get(&identifier)
                            .is_none_or(|t| now - t >= VIOLATION_COOLDOWN_MS);
                        if due {
                            last_violation.insert(identifier.clone(), now);
                            // Report first (the manager's flag), then warn (the employee's toast):
                            // `action_taken: warned` must be true by the time the batch leaves.
                            state.pending_events.lock().unwrap().push(
                                wp_agent_contract::AgentEvent::PolicyViolation {
                                    ts: now,
                                    kind: kind.to_string(),
                                    identifier: identifier.clone(),
                                    action_taken: "warned".to_string(),
                                },
                            );
                            state.flush.notify_one();
                            // The visible warning: surface the panel and tell the webview which
                            // site/app tripped the policy. Deliberately shown, not focus-stolen —
                            // the point is "you were seen", not yanking the keyboard mid-keystroke.
                            // Suppressed under silent mode (owner policy): the violation is still
                            // reported to the server (the PolicyViolation event above), but the
                            // employee is shown nothing.
                            if !silent {
                                if let Some(w) = app.get_webview_window("panel") {
                                    let _ = w.show();
                                }
                                let _ = app.emit(events::POLICY_BLOCKED, identifier);
                            }
                        }
                    }

                    // Privacy exceptions carve-out (§14 / PRIVACY.md §2): while an excepted app/site
                    // is focused, record nothing about it — no activity span (the screenshot loop
                    // applies the same carve-out to its capture). Orthogonal to `is_untracked`, so
                    // check it independently on the full app+title+url haystack.
                    // Idle-gated, exactly as `tick` gates `active_sec`. Without this the foreground
                    // app accrued WINDOW_SAMPLE_EVERY seconds every sample regardless of whether
                    // anyone was there, while `active_sec` counted none of them — so an app left
                    // focused while the machine sat idle produced more span seconds than active
                    // seconds. The server divides one by the other (Q, and the dashboard's
                    // productive share), which is how a real day rendered as **108%**.
                    //
                    // Same threshold as `tick`, deliberately: two different definitions of "active"
                    // in one bucket is what caused this.
                    let at_keyboard = idle <= idle::IDLE_THRESHOLD_SECS;
                    let excepted = crate::rules::is_excepted(&haystack, &rules);
                    if at_keyboard && !excepted && !crate::rules::is_untracked(&f.app, &rules) {
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

            tick = tick.wrapping_add(1);
            was_capturing = true;
        } else if was_capturing {
            // Capture just stopped: seal the open minute so its partial rollup isn't lost.
            bucketer.seal();
            was_capturing = false;
        }

        // ── Idle protection — a property of the TIMER, not of capture ────────────────────────
        //
        // This block used to live inside the capture gate above, so the 5-minute prompt and the
        // 15-minute hard stop only ran for a user who had granted monitoring consent, had capture
        // un-paused, AND whose org had the activity entitlement on. For everyone else the timer ran
        // unbounded: no prompt, no auto-stop, hours accruing with nothing watching them.
        //
        // That is exactly backwards. The auto-stop exists so a timer left running overnight doesn't
        // bill a day nobody worked — a guarantee about *time*, which every timer needs and which
        // matters MOST for the people capture isn't watching. A real org showed 3h41m of tracked
        // time against 3 seconds of input for one person, and five people with timer sessions and
        // no activity rows at all: unmonitored timers that should have stopped after 15 minutes.
        //
        // `idle` is read every tick above, outside the gate, so this needs nothing capture provides.
        match idle_action(running, idle, idle_prompted) {
            IdleAction::Stop => {
                let ev = state.timer.lock().unwrap().stop(now, StopReason::Idle);
                if let Some(e) = ev {
                    state.pending_events.lock().unwrap().push(e);
                    state.flush.notify_one(); // an idle auto-stop should reflect as fast as a manual one
                }
                // Seal whatever capture had open. A no-op when the gate was shut — there is no
                // bucket — which is why this is safe to call on a path capture never reached.
                bucketer.seal();
                was_capturing = false;
                let _ = app.emit(events::TRACKING_CHANGED, ());
                idle_prompted = false;
            }
            IdleAction::Prompt => {
                let _ = app.emit(events::IDLE_PROMPT, idle);
                idle_prompted = true;
            }
            // Below the prompt threshold (or no timer) ⇒ re-arm, so the next idle stretch in the
            // same session prompts again rather than being swallowed by the first one's flag.
            IdleAction::None => {
                if !running || idle < IDLE_PROMPT_SECS {
                    idle_prompted = false;
                }
            }
        }

        // ── The crash-recovery note ─────────────────────────────────────────────────────────
        //
        // Every ending the agent can *observe* closes the session itself. This covers the ones it
        // cannot: a panic, Task Manager, a power cut. The running session is written to disk with
        // the time it was last seen alive, and `lifecycle::recover_open_session` closes it at that
        // instant on the next launch — so an unclean exit costs at most one heartbeat of accuracy
        // instead of leaving the session open indefinitely.
        alive_tick = alive_tick.wrapping_add(1);
        if alive_tick.is_multiple_of(ALIVE_HEARTBEAT_TICKS) {
            let open = state.timer.lock().unwrap().active_session(false).map(|a| {
                crate::session_state::OpenSession {
                    session_id: a.session_id,
                    started_at: a.started_at,
                    last_alive_ms: now,
                }
            });
            // Written on both edges: a stopped timer must CLEAR the note, or a session that ended
            // cleanly hours ago would be "recovered" and closed a second time after a later crash.
            crate::session_state::update(|s| s.open = open);
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
mod idle_action_tests {
    use super::*;

    /// An eight-hour sleep must close the timer at the moment it went to sleep, not at the wake.
    ///
    /// The sign here is the whole feature. Backwards, and a laptop shut at 18:00 and opened at
    /// 09:00 bills the entire night as worked time — silently, since every number involved is real.
    #[test]
    fn a_suspend_is_backdated_to_the_last_awake_tick() {
        // Woke at t=8h having last ticked at t=0.
        let eight_hours = 8 * 60 * 60 * 1000;
        let now = 1_700_000_000_000 + eight_hours;
        assert_eq!(last_wake_ms(now, eight_hours), 1_700_000_000_000);
        // The stop instant must precede the wake, never follow it.
        assert!(last_wake_ms(now, eight_hours) < now);
    }

    /// Normal ticks must never be mistaken for a suspend — a false positive stops a timer while
    /// someone is working, which is worse than the bug it guards against.
    #[test]
    fn ordinary_jitter_is_not_a_suspend() {
        // A 1 s loop under load: even a very late tick is nowhere near the threshold.
        for gap_ms in [1_000, 2_000, 10_000, 60_000] {
            assert!(
                gap_ms < SUSPEND_GAP_MS,
                "{gap_ms}ms must not read as a suspend"
            );
        }
        // A real sleep is unmistakable.
        for gap_ms in [5 * 60_000, 60 * 60_000, 8 * 60 * 60_000] {
            assert!(gap_ms > SUSPEND_GAP_MS, "{gap_ms}ms must read as a suspend");
        }
    }

    /// The regression this function exists for.
    ///
    /// There is no capture flag in the signature, so the old failure — auto-stop unreachable for an
    /// unconsented user or a plan without `monitoring.activity` — cannot be expressed here at all.
    /// That is the guarantee; these cases just pin the thresholds.
    #[test]
    fn a_running_timer_stops_after_the_hard_idle_limit() {
        assert_eq!(idle_action(true, AUTO_STOP_SECS, false), IdleAction::Stop);
        assert_eq!(
            idle_action(true, AUTO_STOP_SECS + 3600, true),
            IdleAction::Stop
        );
        // Stopping outranks prompting: past the limit there is nothing left to ask.
        assert_eq!(idle_action(true, AUTO_STOP_SECS, true), IdleAction::Stop);
    }

    #[test]
    fn the_prompt_fires_once_then_stays_quiet() {
        assert_eq!(
            idle_action(true, IDLE_PROMPT_SECS, false),
            IdleAction::Prompt
        );
        // Already asked — don't nag every second for the next ten minutes.
        assert_eq!(
            idle_action(true, IDLE_PROMPT_SECS + 60, true),
            IdleAction::None
        );
    }

    #[test]
    fn an_active_user_is_left_alone() {
        assert_eq!(idle_action(true, 0, false), IdleAction::None);
        assert_eq!(
            idle_action(true, IDLE_PROMPT_SECS - 1, false),
            IdleAction::None
        );
    }

    /// No timer ⇒ nothing to stop and nothing to ask, however long the machine has been idle.
    #[test]
    fn a_stopped_timer_is_never_acted_on() {
        for idle in [0, IDLE_PROMPT_SECS, AUTO_STOP_SECS, AUTO_STOP_SECS * 10] {
            assert_eq!(
                idle_action(false, idle, false),
                IdleAction::None,
                "idle={idle}"
            );
            assert_eq!(
                idle_action(false, idle, true),
                IdleAction::None,
                "idle={idle}"
            );
        }
    }

    /// The thresholds must stay ordered, or the prompt becomes unreachable.
    #[test]
    fn the_prompt_comes_before_the_stop() {
        // Runtime bindings so the comparison isn't const-folded (`clippy::assertions_on_constants`).
        let (prompt, stop) = (IDLE_PROMPT_SECS, AUTO_STOP_SECS);
        assert!(
            prompt < stop,
            "prompt {prompt}s must come before stop {stop}s"
        );
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
