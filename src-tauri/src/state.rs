//! Shared, Tauri-managed application state. Interior mutability via `Mutex` — one process, no IPC.
//! The monitor gates capture on `timer.is_running()` (BUILD-PLAN §4).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use wp_agent_contract::{ActivityRollup, AgentEvent, ScreenshotMeta, StopReason};

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
    /// Signalled on any timer transition so the sender loop flushes **immediately** instead of
    /// waiting for the 300 s cycle (LLD §4: "post TimerStarted immediately"). One permit suffices —
    /// a burst of transitions collapses to a single extra flush.
    pub flush: Arc<Notify>,
    /// Sealed per-minute rollups from the monitor thread, awaiting the next cycle's drain (M4).
    pub pending_activity: Mutex<Vec<ActivityRollup>>,
    /// Captured screenshots keyed by id, awaiting upload (M5).
    pub screenshots: Mutex<HashMap<String, PendingShot>>,
    /// Screenshot metadata for frames whose **bytes are already in S3**, awaiting declaration in the
    /// next batch. Today that is only the admin-triggered `capture_now`, which uploads to the
    /// command's own presigned URL rather than to one the ack hands back — so the meta still has to
    /// ride a batch (that is what makes ingest's fold write the `SHOT#` row pointing at the object),
    /// but it must **not** sit in `screenshots` waiting for an upload that already happened.
    /// Drained by `cycle::assemble_and_enqueue`, like `pending_events`.
    pub pending_screenshot_meta: Mutex<Vec<ScreenshotMeta>>,
    /// Monitoring consent. **Defaults false → capture fails closed** (PRIVACY.md); the tray grants it.
    pub consent: AtomicBool,
    /// Latest machine-idle signal for the fleet heartbeat (true = no user input for the idle window).
    /// Published by the monitor thread every tick, **independent of the timer**, so the fleet table
    /// reflects an idle-but-online machine. Read by `cycle::assemble_and_enqueue` into `Heartbeat.idle`.
    pub idle: AtomicBool,
    /// Privacy-pause window + budget (the panel's "Pause 5 min").
    pub pause: Mutex<PauseState>,
    /// Latest device location fix, refreshed each cycle **only while consent is granted** (the sender
    /// clears it when consent is withdrawn). `None` = no fix; it rides the next heartbeat when present.
    pub location: Mutex<Option<wp_agent_contract::GeoLocation>>,
    /// Cognito session + single-flight refresh (interior mutability — not behind the outer Mutex).
    pub auth: AuthManager,
    /// Live MQTT client + status topic, set while a broker connection is up — so the exit hook can
    /// publish a clean `{"online":false}` (mqtt::shutdown). **Device-level, like config**: the
    /// credential and connection are per-install, so `reset_for_account_switch` leaves this alone.
    pub mqtt: Mutex<Option<crate::mqtt::MqttHandle>>,
    /// The session target remembered when the timer was stopped **by the agent** (idle cut-off,
    /// suspend/lid, or lock) — never by the user. The monitor restarts this same work on the next
    /// keyboard/mouse input. `None` when there is nothing to resume. Per-user, so it's cleared on an
    /// account switch. See `monitor`'s auto-resume block.
    pub auto_resume: Mutex<Option<AutoResume>>,
}

/// A remembered timer target for auto-resume-on-activity (see `AppState::auto_resume`).
#[derive(Clone, Debug)]
pub struct AutoResume {
    pub task_id: String,
    pub subtask_id: String,
    pub project_id: String,
    pub description: String,
    /// When the timer stopped (epoch ms). Bounds how long the offer stays open — a short break
    /// resumes; an overnight sleep does not (that would restart a stale task, maybe on a new day).
    pub stopped_at: i64,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            timer: Mutex::new(TimerEngine::default()),
            outbox: Mutex::new(Outbox::new()),
            config: Mutex::new(ConfigCache::default()),
            pending_events: Mutex::new(Vec::new()),
            flush: Arc::new(Notify::new()),
            pending_activity: Mutex::new(Vec::new()),
            screenshots: Mutex::new(HashMap::new()),
            pending_screenshot_meta: Mutex::new(Vec::new()),
            consent: AtomicBool::new(false),
            idle: AtomicBool::new(false),
            pause: Mutex::new(PauseState::default()),
            location: Mutex::new(None),
            auth: AuthManager::new(),
            mqtt: Mutex::new(None),
            auto_resume: Mutex::new(None),
        }
    }
}

impl AppState {
    /// Wipe all **per-user** runtime state on an account switch (sign-out), so one user's timer,
    /// queued captures, consent and session can never bleed into the next person who signs in on
    /// this device. Device-level config/idle re-sync from the server/OS and are left alone.
    ///
    /// The stopped session's `TimerStopped` event is intentionally **dropped**, not queued: it can no
    /// longer be sent under the signing-out user's token, and the server's 00:15 close resolves the
    /// still-open day. Keeping it would only risk folding it under the *next* user.
    pub fn reset_for_account_switch(&self) {
        // End the running session locally so the next user never inherits it.
        let _ = self
            .timer
            .lock()
            .unwrap()
            .stop(crate::clock::now_epoch_ms(), StopReason::Logout);

        // Drop buffered-but-unsent captures and any queued batches — all the previous user's data.
        self.pending_events.lock().unwrap().clear();
        self.pending_activity.lock().unwrap().clear();
        {
            let mut shots = self.screenshots.lock().unwrap();
            for s in shots.values() {
                let _ = std::fs::remove_file(&s.path);
            }
            shots.clear();
        }
        self.pending_screenshot_meta.lock().unwrap().clear();
        self.outbox.lock().unwrap().clear();
        *self.location.lock().unwrap() = None;
        *self.pause.lock().unwrap() = PauseState::default();
        // The next person must not have their timer auto-started onto the previous user's task.
        *self.auto_resume.lock().unwrap() = None;

        // The transparency log is this user's own record of what was captured on their machine; the
        // next person to sign in must not inherit (or be able to read) it.
        crate::privacy_log::clear();

        // Re-require consent for the next person: capture fails closed until they accept the notice.
        self.consent
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.idle.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
