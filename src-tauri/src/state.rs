//! Shared, Tauri-managed application state. Interior mutability via `Mutex` — one process, no IPC.
//! The monitor gates capture on `timer.is_running()` (BUILD-PLAN §4).

use std::sync::Mutex;

use crate::config::ConfigCache;
use crate::outbox::Outbox;
use crate::timer::TimerEngine;

pub struct AppState {
    pub timer: Mutex<TimerEngine>,
    pub outbox: Mutex<Outbox>,
    pub config: Mutex<ConfigCache>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            timer: Mutex::new(TimerEngine::default()),
            outbox: Mutex::new(Outbox::new()),
            config: Mutex::new(ConfigCache::default()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
