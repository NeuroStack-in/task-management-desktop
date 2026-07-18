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

/// Privacy-pause: a bounded window during which capture is suspended, drawn from a daily budget.
pub struct PauseState {
    /// Epoch ms the current pause ends, or `None` when not paused.
    pub until_ms: Option<i64>,
    /// Remaining pause budget (seconds).
    pub budget_secs: u64,
}

impl Default for PauseState {
    fn default() -> Self {
        PauseState {
            until_ms: None,
            budget_secs: 30 * 60,
        }
    }
}

impl PauseState {
    pub fn is_paused(&self, now_ms: i64) -> bool {
        self.until_ms.is_some_and(|u| u > now_ms)
    }

    /// Grant up to `requested` seconds from the budget and extend the pause window. Returns
    /// `(granted, granted_secs, remaining_budget_secs)`.
    pub fn request(&mut self, now_ms: i64, requested: u64) -> (bool, u64, u64) {
        let granted = requested.min(self.budget_secs);
        if granted > 0 {
            self.budget_secs -= granted;
            let base = self.until_ms.filter(|u| *u > now_ms).unwrap_or(now_ms);
            self.until_ms = Some(base + (granted as i64) * 1000);
        }
        (granted > 0, granted, self.budget_secs)
    }
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
    /// Privacy-pause window + budget (the panel's "Pause 5 min").
    pub pause: Mutex<PauseState>,
    /// Latest device location fix, refreshed each cycle **only while consent is granted** (the sender
    /// clears it when consent is withdrawn). `None` = no fix; it rides the next heartbeat when present.
    pub location: Mutex<Option<wp_agent_contract::GeoLocation>>,
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
            pause: Mutex::new(PauseState::default()),
            location: Mutex::new(None),
            auth: AuthManager::new(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
