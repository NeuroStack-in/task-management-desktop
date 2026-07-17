//! Per-minute activity aggregation — the real divergence from the reference sample, which keeps one
//! cumulative bucket per 300 s heartbeat while **our** contract wants per-minute `ActivityRollup`
//! (BUILD-PLAN §4/§5).
//!
//! A [`Bucketer`] accumulates 1 s ticks into the current minute (`epoch_ms / 60_000`), sealing the
//! open bucket on a minute boundary, on timer-stop, and on a screenshot early-flush (so a shot maps
//! to its exact minute and isn't double-counted). Each tick counts the second as **active** or
//! **idle** (`idle_secs > IDLE_THRESHOLD_SECS`), and the **invariant `active_sec + idle_sec ≤ 60`**
//! is enforced. `top_apps` is capped at [`MAX_APPS_PER_BUCKET`], top-N by seconds.

use std::collections::HashMap;

use wp_agent_contract::{ActivityRollup, AppSpan, Category};

use super::idle::IDLE_THRESHOLD_SECS;

/// Max app spans kept per minute bucket (top-N by seconds).
pub const MAX_APPS_PER_BUCKET: usize = 30;

const SECS_PER_MIN: u16 = 60;

fn minute_of(epoch_ms: i64) -> i64 {
    epoch_ms.div_euclid(60_000)
}

#[derive(Clone)]
struct AppAccum {
    title: Option<String>,
    url: Option<String>,
    category: Category,
    seconds: u32,
}

struct MinuteBucket {
    minute: i64,
    keystrokes: u32,
    mouse: u32,
    active_sec: u16,
    idle_sec: u16,
    /// Keyed by app name (a URL rule already folded into `category` upstream); last title/url win.
    apps: HashMap<String, AppAccum>,
}

impl MinuteBucket {
    fn new(minute: i64) -> Self {
        MinuteBucket {
            minute,
            keystrokes: 0,
            mouse: 0,
            active_sec: 0,
            idle_sec: 0,
            apps: HashMap::new(),
        }
    }

    fn into_rollup(self) -> ActivityRollup {
        let mut spans: Vec<AppSpan> = self
            .apps
            .into_iter()
            .map(|(app, a)| AppSpan {
                app,
                title: a.title,
                url: a.url,
                category: a.category,
                seconds: a.seconds,
            })
            .collect();
        // Top-N by seconds (desc); tie-break by app for determinism (golden tests).
        spans.sort_by(|x, y| y.seconds.cmp(&x.seconds).then_with(|| x.app.cmp(&y.app)));
        spans.truncate(MAX_APPS_PER_BUCKET);
        ActivityRollup {
            minute: self.minute,
            keystrokes: self.keystrokes,
            mouse: self.mouse,
            active_sec: self.active_sec,
            idle_sec: self.idle_sec,
            top_apps: spans,
        }
    }
}

#[derive(Default)]
pub struct Bucketer {
    current: Option<MinuteBucket>,
    sealed: Vec<ActivityRollup>,
}

impl Bucketer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Account one 1 s tick. Rolls the bucket if the minute changed, then records active/idle + input
    /// counts for this second. `kb`/`mouse` are the per-tick deltas already wrap/spike-capped by the
    /// caller ([`super::input`]).
    pub fn tick(&mut self, now_ms: i64, idle_secs: u64, kb: u32, mouse: u32) {
        let minute = minute_of(now_ms);
        self.roll_to(minute);
        let b = self
            .current
            .get_or_insert_with(|| MinuteBucket::new(minute));

        b.keystrokes = b.keystrokes.saturating_add(kb);
        b.mouse = b.mouse.saturating_add(mouse);
        // Invariant: never let a bucket exceed 60 counted seconds.
        if b.active_sec + b.idle_sec < SECS_PER_MIN {
            if idle_secs > IDLE_THRESHOLD_SECS {
                b.idle_sec += 1;
            } else {
                b.active_sec += 1;
            }
        }
    }

    /// Attribute `seconds` of foreground time to `app` (with its classified category) in the current
    /// minute. Sampled every Nth tick by the caller (`WINDOW_SAMPLE_EVERY`).
    pub fn sample_app(
        &mut self,
        now_ms: i64,
        app: String,
        title: Option<String>,
        url: Option<String>,
        category: Category,
        seconds: u32,
    ) {
        let minute = minute_of(now_ms);
        self.roll_to(minute);
        let b = self
            .current
            .get_or_insert_with(|| MinuteBucket::new(minute));
        let entry = b.apps.entry(app).or_insert(AppAccum {
            title: title.clone(),
            url: url.clone(),
            category,
            seconds: 0,
        });
        entry.title = title;
        entry.url = url;
        entry.category = category;
        entry.seconds = entry.seconds.saturating_add(seconds);
    }

    /// Seal the open bucket (timer-stop / screenshot early-flush). No-op if none is open.
    pub fn seal(&mut self) {
        if let Some(b) = self.current.take() {
            self.sealed.push(b.into_rollup());
        }
    }

    /// Drain the sealed rollups for the next cycle's batch.
    pub fn take_sealed(&mut self) -> Vec<ActivityRollup> {
        std::mem::take(&mut self.sealed)
    }

    fn roll_to(&mut self, minute: i64) {
        if let Some(b) = &self.current {
            if b.minute != minute {
                let done = self.current.take().unwrap();
                self.sealed.push(done.into_rollup());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN0: i64 = 28_800_000 * 60_000; // an arbitrary far-future minute in ms (well past midnight)

    #[test]
    fn a_full_minute_seals_with_60_seconds_split_active_idle() {
        let mut b = Bucketer::new();
        // 40 active seconds (idle 0), 20 idle seconds (idle above threshold), all in minute 0.
        for s in 0..40 {
            b.tick(MIN0 + s * 1000, 0, 1, 0);
        }
        for s in 40..60 {
            b.tick(MIN0 + s * 1000, IDLE_THRESHOLD_SECS + 1, 0, 1);
        }
        // Crossing into the next minute rolls (seals) minute 0.
        b.tick(MIN0 + 60_000, 0, 0, 0);

        let sealed = b.take_sealed();
        assert_eq!(sealed.len(), 1);
        let r = &sealed[0];
        assert_eq!(r.active_sec, 40);
        assert_eq!(r.idle_sec, 20);
        assert!(r.active_sec + r.idle_sec <= 60, "the ≤60 invariant");
        assert_eq!(r.keystrokes, 40);
        assert_eq!(r.mouse, 20);
    }

    #[test]
    fn never_exceeds_60_even_with_extra_ticks() {
        let mut b = Bucketer::new();
        for s in 0..90 {
            // 90 ticks all stamped inside the SAME minute (clock stall / double-tick).
            b.tick(MIN0 + (s % 60) * 1000, 0, 0, 0);
        }
        b.seal();
        let r = &b.take_sealed()[0];
        assert!(
            r.active_sec + r.idle_sec <= 60,
            "capped at 60, got {}",
            r.active_sec + r.idle_sec
        );
    }

    #[test]
    fn top_apps_accumulate_and_cap_top_n_by_seconds() {
        let mut b = Bucketer::new();
        // App "code" gets 3 samples (30s), "chrome" 1 (5s); plus 40 filler apps of 1s each.
        b.sample_app(MIN0, "code".into(), None, None, Category::Productive, 10);
        b.sample_app(MIN0, "code".into(), None, None, Category::Productive, 20);
        b.sample_app(MIN0, "chrome".into(), None, None, Category::Neutral, 5);
        for i in 0..40 {
            b.sample_app(MIN0, format!("filler{i}"), None, None, Category::Neutral, 1);
        }
        b.seal();
        let r = &b.take_sealed()[0];
        assert_eq!(r.top_apps.len(), MAX_APPS_PER_BUCKET, "capped");
        assert_eq!(r.top_apps[0].app, "code");
        assert_eq!(r.top_apps[0].seconds, 30, "samples accumulate");
        assert_eq!(r.top_apps[1].app, "chrome");
    }

    #[test]
    fn crossing_a_minute_boundary_yields_two_rollups() {
        let mut b = Bucketer::new();
        b.tick(MIN0 + 30_000, 0, 1, 0); // minute 0
        b.tick(MIN0 + 90_000, 0, 1, 0); // minute 1 → seals minute 0
        b.seal(); // seal minute 1
        let sealed = b.take_sealed();
        assert_eq!(sealed.len(), 2);
        assert_eq!(sealed[0].minute + 1, sealed[1].minute);
    }

    #[test]
    fn midnight_is_just_another_minute_boundary() {
        // Ticks straddling UTC midnight (day 0 23:59:30 → day 1 00:00:30) must seal two consecutive
        // minutes — buckets key on epoch minute, so midnight isn't special (BUILD-PLAN M8).
        let midnight_ms: i64 = 86_400_000; // 1970-01-02 00:00:00 UTC
        let mut b = Bucketer::new();
        b.tick(midnight_ms - 30_000, 0, 1, 0); // 23:59:30 (day 0)
        b.tick(midnight_ms + 30_000, 0, 1, 0); // 00:00:30 (day 1) → seals the previous minute
        b.seal();
        let sealed = b.take_sealed();
        assert_eq!(sealed.len(), 2);
        assert_eq!(sealed[0].minute + 1, sealed[1].minute);
        assert_eq!(sealed[1].minute, midnight_ms / 60_000);
    }
}
