//! Shared, Tauri-managed application state. Interior mutability via `Mutex` — one process, no IPC.
//! The monitor gates capture on `timer.is_running()` (BUILD-PLAN §4).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use wp_agent_contract::{ActivityRollup, AgentEvent, ScreenshotMeta};

use crate::auth::AuthManager;
use crate::config::ConfigCache;
use crate::outbox::Outbox;
use crate::timer::TimerEngine;

/// A captured screenshot awaiting S3-direct upload. Re-declared in each batch (idempotent by `id`)
/// until the presigned PUT succeeds or `attempts` hits the cap (M5).
pub struct PendingShot {
    pub meta: ScreenshotMeta,
    pub path: PathBuf,
    pub attempts: u8,
}

pub struct AppState {
    pub timer: Mutex<TimerEngine>,
    pub outbox: Mutex<Outbox>,
    pub config: Mutex<ConfigCache>,
    /// Timer/attendance/policy events awaiting the next cycle's `enqueue_cycle` drain (BUILD-PLAN §4).
    pub pending_events: Mutex<Vec<AgentEvent>>,
    /// Sealed per-minute rollups from the monitor thread, awaiting the next cycle's drain (M4).
    pub pending_activity: Mutex<Vec<ActivityRollup>>,
    /// Captured screenshots keyed by id, awaiting upload (M5).
    pub screenshots: Mutex<HashMap<String, PendingShot>>,
    /// Monitoring consent. **Defaults false → capture fails closed** (PRIVACY.md); the tray grants it.
    pub consent: AtomicBool,
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
            pending_activity: Mutex::new(Vec::new()),
            screenshots: Mutex::new(HashMap::new()),
            consent: AtomicBool::new(false),
            auth: AuthManager::new(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
