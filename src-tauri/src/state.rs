//! Shared, Tauri-managed application state. Interior mutability via `Mutex` — one process, no IPC.
//! The monitor gates capture on `timer.is_running()` (BUILD-PLAN §4).

use std::sync::Mutex;

use wp_agent_contract::AgentEvent;

use crate::auth::AuthManager;
use crate::config::ConfigCache;
use crate::outbox::Outbox;
use crate::timer::TimerEngine;

pub struct AppState {
    pub timer: Mutex<TimerEngine>,
    pub outbox: Mutex<Outbox>,
    pub config: Mutex<ConfigCache>,
    /// Timer/attendance/policy events awaiting the next cycle's `enqueue_cycle` drain (BUILD-PLAN §4).
    pub pending_events: Mutex<Vec<AgentEvent>>,
    /// Cognito session + single-flight refresh (interior mutability — not behind the outer Mutex).
    pub auth: AuthManager,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            timer: Mutex::new(TimerEngine::default()),
            outbox: Mutex::new(Outbox::new()),
            config: Mutex::new(ConfigCache::default()),
            pending_events: Mutex::new(Vec::new()),
            auth: AuthManager::new(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
