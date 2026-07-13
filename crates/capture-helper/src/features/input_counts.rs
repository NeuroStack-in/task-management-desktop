//! Keyboard/mouse activity — **counts only**. The privacy invariant is structural: the API
//! accepts no key value, so nothing downstream can ever see one. Production registers `rdev`
//! callbacks that call `on_keystroke`/`on_mouse` and **drop the event inside the callback**.

/// Per-minute input counters. Reset each minute when folded into an `ActivityRollup`.
#[derive(Default)]
pub struct InputCounters {
    keystrokes: u32,
    mouse: u32,
}

impl InputCounters {
    /// A key was pressed. NOTE: intentionally takes no argument — the key identity is dropped
    /// at the source and never enters the process.
    pub fn on_keystroke(&mut self) {
        self.keystrokes = self.keystrokes.saturating_add(1);
    }

    /// A mouse click/move occurred.
    pub fn on_mouse(&mut self) {
        self.mouse = self.mouse.saturating_add(1);
    }

    pub fn keystrokes(&self) -> u32 {
        self.keystrokes
    }
    pub fn mouse(&self) -> u32 {
        self.mouse
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
