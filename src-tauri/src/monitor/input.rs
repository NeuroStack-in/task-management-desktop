//! Keyboard/mouse activity — **counts only**. What leaves the process is a number; the contract
//! accepts no key value, so a keystroke's identity can never reach the backend or a log.
//!
//! M4 uses a **`device_query` 1 s poller** (not `rdev` — polling dodges rdev's blocking listener and
//! the macOS main-thread trap; BUILD-PLAN §2/§4). A poller is an **estimate**, not a hook: it misses
//! fast bursts and `SPIKE_CAP` truncates the rest (risk #3) — never label the number an exact
//! "keystroke count". The poller reads *how many* keys are down this tick (via `get_keys().len()`);
//! it never inspects or retains which keys, so no identity is stored.

use device_query::{DeviceQuery, DeviceState};

/// Per-tick delta ceiling — a poll spike can't inflate a minute unboundedly (BUILD-PLAN §4).
pub const SPIKE_CAP: u32 = 1000;

/// A `device_query`-backed input sampler. Lives on the monitor's dedicated OS thread (its handle is
/// not `Send`, which is exactly why capture runs on a thread, off the async reactor).
pub struct InputSampler {
    device: DeviceState,
    prev_pos: (i32, i32),
    started: bool,
}

impl InputSampler {
    pub fn new() -> Self {
        InputSampler {
            device: DeviceState::new(),
            prev_pos: (0, 0),
            started: false,
        }
    }

    /// A per-tick `(keyboard, mouse)` activity estimate. Keyboard = number of keys down this tick
    /// (identities dropped immediately); mouse = movement-this-tick + buttons down. Both `SPIKE_CAP`ed.
    pub fn sample(&mut self) -> (u32, u32) {
        let kb = self.device.get_keys().len() as u32;

        let m = self.device.get_mouse();
        let moved = (m.coords.0 - self.prev_pos.0).abs() + (m.coords.1 - self.prev_pos.1).abs();
        // Skip the first sample's "movement" — prev_pos starts at (0,0), not the real cursor.
        let move_hit = if self.started && moved > 0 { 1 } else { 0 };
        let clicks = m.button_pressed.iter().filter(|&&b| b).count() as u32;
        self.prev_pos = m.coords;
        self.started = true;

        (kb.min(SPIKE_CAP), (move_hit + clicks).min(SPIKE_CAP))
    }
}

impl Default for InputSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-minute input counters — the "counts only" accumulator (kept for the privacy-invariant test;
/// the live path folds deltas straight into `bucket::Bucketer`).
#[derive(Default)]
pub struct InputCounters {
    keystrokes: u32,
    mouse: u32,
}

impl InputCounters {
    /// A key was pressed. Intentionally takes no argument — the identity never enters the counter.
    pub fn on_keystroke(&mut self) {
        self.keystrokes = self.keystrokes.saturating_add(1);
    }

    pub fn on_mouse(&mut self) {
        self.mouse = self.mouse.saturating_add(1);
    }

    /// Read + reset for a minute rollup.
    pub fn take_minute(&mut self) -> (u32, u32) {
        let out = (self.keystrokes, self.mouse);
        self.keystrokes = 0;
        self.mouse = 0;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_and_reset() {
        let mut c = InputCounters::default();
        c.on_keystroke();
        c.on_keystroke();
        c.on_mouse();
        assert_eq!(c.take_minute(), (2, 1));
        assert_eq!(c.take_minute(), (0, 0)); // reset after fold
    }
}
