//! Shared, Tauri-managed application state. Interior mutability via `Mutex` — one process, no IPC.
//! The monitor gates capture on `timer.is_running()` (BUILD-PLAN §4).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use wp_agent_contract::{ActivityRollup, AgentEvent, ScreenshotMeta, StopReason};

use crate::auth::AuthManager;
use crate::config::ConfigCache;
use crate::outbox::Outbox;
use crate::timer::TimerEngine;

/// A captured screenshot awaiting S3-direct upload. Declared in a batch (idempotent by `id`), and
/// declared again only once that batch has been acked without the upload succeeding — until the
/// presigned PUT succeeds or `attempts` hits the cap (M5).
pub struct PendingShot {
    pub meta: ScreenshotMeta,
    pub path: PathBuf,
    pub attempts: u8,
    /// The `batch_seq` that last declared this frame. While that batch is still queued the frame is
    /// not declared again: offline, every new batch used to re-declare every pending frame, so batch
    /// and outbox size grew quadratically with time offline until the server refused the batch as
    /// too large — and a refused head batch stopped all ingestion (QA audit 2026-10-08, WP-208).
    /// In memory only: after a restart each frame is declared once more, which is harmless.
    pub declared_seq: Option<u64>,
}

/// The daily privacy-pause allowance until the org's config says otherwise — the contract default
/// (15 minutes). The live value is `TrackingConfig::pause_allowance_secs`, set by an owner or admin.
pub const DEFAULT_PAUSE_ALLOWANCE_SECS: u64 =
    wp_agent_contract::config::DEFAULT_PAUSE_ALLOWANCE_SECS as u64;

/// The allowance used to be a fixed 30 minutes; a persisted budget from those builds is converted with
/// it once (see `lib.rs` startup).
pub const LEGACY_DAILY_PAUSE_SECS: u64 = 30 * 60;

/// Privacy-pause: a bounded window during which capture is suspended, drawn from a daily budget.
///
/// **The day is the user's local day, supplied by the panel.** The core has no local calendar (see
/// `session_state::ResumeTask`), so the webview passes its `YYYY-MM-DD` with every pause read and
/// request. The budget used to be an in-memory 30 minutes with no reset at all: restarting the agent
/// bought another 30, and an agent left running never got its allowance back, while the panel said
/// "It resets tomorrow" (QA audit 2026-10-08, WP-213). It is persisted (`session_state`) for the
/// same reason.
///
/// **Taken in pieces** (owner request 2026-10-15): a pause is granted up to the rest of the day's
/// allowance and ends when the person resumes — the unused part is given back — or when the
/// allowance runs out. So 5 seconds, then 50, then … all draw on one daily total. It used to be a
/// fixed "Pause 5 min" with no resume: five minutes were spent whether the pause lasted five minutes
/// or five seconds.
///
/// **What is tracked is time used, not time left**, so an admin's change to the allowance applies at
/// once: remaining = allowance − used.
#[derive(Default)]
pub struct PauseState {
    /// Epoch ms the current pause ends at the latest, or `None` when not paused.
    pub until_ms: Option<i64>,
    /// Seconds of [`Self::day`]'s allowance spent — including the whole of a pause still running,
    /// whose unused part [`Self::resume`] gives back.
    pub used_secs: u64,
    /// The local date `used_secs` belongs to; empty until the first pause is taken.
    pub day: String,
}

impl PauseState {
    pub fn is_paused(&self, now_ms: i64) -> bool {
        self.until_ms.is_some_and(|u| u > now_ms)
    }

    /// A later local day than the budget's earns the full allowance back. Only *later*: setting the
    /// clock back a day must not hand out a second allowance for the day already spent, and a
    /// malformed date is ignored rather than trusted.
    fn rolls_over(&self, today: &str) -> bool {
        crate::clock::is_local_date(today) && today > self.day.as_str()
    }

    /// Seconds of the allowance spent on `today`.
    fn used_on(&self, today: &str) -> u64 {
        if self.rolls_over(today) {
            0
        } else {
            self.used_secs
        }
    }

    /// The allowance left on `today`, given the org's daily `allowance`.
    pub fn budget_on(&self, today: &str, allowance: u64) -> u64 {
        allowance.saturating_sub(self.used_on(today))
    }

    /// Grant up to `requested` seconds of `today`'s remaining allowance and extend the pause window.
    /// Returns `(granted, granted_secs, remaining_budget_secs)`.
    pub fn request(
        &mut self,
        now_ms: i64,
        requested: u64,
        today: &str,
        allowance: u64,
    ) -> (bool, u64, u64) {
        if self.rolls_over(today) {
            self.day = today.to_string();
            self.used_secs = 0;
        }
        let granted = requested.min(allowance.saturating_sub(self.used_secs));
        if granted > 0 {
            self.used_secs += granted;
            let base = self.until_ms.filter(|u| *u > now_ms).unwrap_or(now_ms);
            self.until_ms = Some(base + (granted as i64) * 1000);
        }
        (
            granted > 0,
            granted,
            allowance.saturating_sub(self.used_secs),
        )
    }

    /// End the pause now and give back the part not used. Returns the seconds given back.
    ///
    /// Whole seconds only, rounded down: a fraction of a second paused is spent, never gained, so
    /// pausing and resuming cannot add to the allowance.
    pub fn resume(&mut self, now_ms: i64) -> u64 {
        let Some(until) = self.until_ms.take() else {
            return 0;
        };
        if until <= now_ms {
            return 0;
        }
        let refund = ((until - now_ms) / 1000) as u64;
        self.used_secs = self.used_secs.saturating_sub(refund);
        refund
    }
}

/// The idle stretch the panel's "still working?" banner is about (WP-212).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleWindow {
    /// The last input before the idle stretch (epoch ms) — where a "Stop" on the banner ends the
    /// session, so the idle minutes are not billed.
    pub since_ms: i64,
    /// When input came back, if it has.
    pub returned_ms: Option<i64>,
}

/// How long after coming back the banner's Stop still means "I wasn't working". Long enough to read
/// it and click; past this the person has evidently been working again, and the stop is "now".
pub const IDLE_STOP_GRACE_MS: i64 = 2 * 60 * 1000;

impl IdleWindow {
    /// Where a banner Stop at `now` should end the session.
    pub fn stop_at(&self, now_ms: i64) -> i64 {
        match self.returned_ms {
            Some(back) if now_ms - back > IDLE_STOP_GRACE_MS => now_ms,
            _ => self.since_ms,
        }
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
    /// Bumped by [`AppState::reset_for_account_switch`] **before** it clears anything. The capture
    /// loops note it when they open a minute bucket or start a screenshot, and drop that work if it
    /// moved before they hand it over. A wipe used to clear the queues while the monitor still held
    /// the open minute and the screenshot loop a frame mid-encode; both landed in the queues a moment
    /// later and rode the next user's first batch (QA audit 2026-10-08, WP-205, WP-443).
    pub capture_epoch: AtomicU64,
    /// The user this process last adopted as the queue's owner — and the lock that serialises
    /// [`AppState::adopt_signed_in`]. The sign-in command, the sender and the panel's resume claim
    /// can all reach it in the same second; two of them racing through the reset could stop a timer
    /// the new user had just started. Held in memory as well as in `session.json`, so a file that
    /// can't be written never makes every cycle wipe the current user's queue again.
    adopted_owner: Mutex<Option<String>>,
    /// An org config has been applied — pulled this run, or the last one restored from disk
    /// (`api::config::restore`). Capture waits for it, so a first run never captures under the
    /// contract's built-in defaults before the org's policy is known (WP-209).
    pub config_known: AtomicBool,
    /// Bumped to make the MQTT connection drop and re-enrol — the signed-in user belongs to a
    /// different organization than the device credential was minted for (`mqtt`, WP-215).
    pub mqtt_rebind: tokio::sync::watch::Sender<u64>,
    /// The idle stretch the "still working?" prompt is about, while the timer runs (WP-212).
    pub idle_window: Mutex<Option<IdleWindow>>,
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
            capture_epoch: AtomicU64::new(0),
            adopted_owner: Mutex::new(None),
            config_known: AtomicBool::new(false),
            mqtt_rebind: tokio::sync::watch::channel(0).0,
            idle_window: Mutex::new(None),
        }
    }

    /// The current [`Self::capture_epoch`].
    pub fn epoch(&self) -> u64 {
        self.capture_epoch.load(Ordering::SeqCst)
    }
}

impl AppState {
    /// Stop the timer and **keep** its `TimerStopped`, so the session is filed rather than left open
    /// on the server. Returns whether a session was running. Capture stops with it — both capture
    /// loops gate on `timer.is_running()`.
    pub fn stop_keeping_event(&self, reason: StopReason) -> bool {
        let stopped = self
            .timer
            .lock()
            .unwrap()
            .stop(crate::clock::now_epoch_ms(), reason);
        match stopped {
            Some(ev) => {
                self.pending_events.lock().unwrap().push(ev);
                *self.idle_window.lock().unwrap() = None;
                crate::session_state::update(|s| s.open = None);
                true
            }
            None => false,
        }
    }

    /// A sign-out, or a session that expired: end what is **personal to the session** — the timer
    /// (its stop event kept), consent, the pause window, the resume hand-off, the transparency log —
    /// but keep the queued captures. They belong to `outbox_owner`, and the next sign-in decides:
    /// the same person sends them, anyone else has them wiped first ([`Self::adopt_signed_in`]).
    ///
    /// The day's pause **budget** is kept with them: signing out and back in used to hand out a
    /// fresh 30 minutes (WP-213). A different user signing in gets their own
    /// ([`Self::reset_for_account_switch`]).
    pub fn end_session_locally(&self) {
        self.stop_keeping_event(StopReason::Logout);
        *self.location.lock().unwrap() = None;
        self.pause.lock().unwrap().until_ms = None;
        *self.auto_resume.lock().unwrap() = None;
        crate::privacy_log::clear();
        self.consent.store(false, Ordering::Relaxed);
        self.idle.store(false, Ordering::Relaxed);
    }

    /// After any successful sign-in: keep the queued captures if they are this person's, wipe them
    /// (and everything else per-user) if they are someone else's, and record the new owner.
    ///
    /// Idempotent and cheap when the owner is unchanged (one file read), so it is also called right
    /// before anything that sends or resumes per-user data — the sender's cycle and the panel's
    /// resume claim. Calling it only from the sign-in commands left a window between the tokens
    /// being set and the reset in which the sender could ship, or the panel auto-start, the previous
    /// user's queue under the new user (QA audit 2026-10-08, WP-206).
    pub fn adopt_signed_in(&self) {
        let Some(sub) = self.auth.subject() else {
            return;
        };
        let mut adopted = self.adopted_owner.lock().unwrap();
        if adopted.as_deref() == Some(sub.as_str()) {
            return;
        }
        // The file is consulted only until this process has adopted someone.
        let previous = adopted
            .clone()
            .or_else(|| crate::session_state::load().outbox_owner);
        if previous.as_deref() == Some(sub.as_str()) {
            *adopted = Some(sub);
            return;
        }
        if previous.is_some() {
            tracing::info!(
                "a different user signed in; dropping the previous user's queued captures"
            );
            self.reset_for_account_switch();
            // The previous user's org policy must not be confirmed for this one by a matching ETag:
            // the token is a bare version number, so two orgs can share it (WP-209).
            crate::api::config::forget_etag(self);
            crate::session_state::update(|s| {
                s.consent_granted = false;
                s.resume = None;
            });
        }
        crate::session_state::update(|s| s.outbox_owner = Some(sub.clone()));
        *adopted = Some(sub);
        // The MQTT identity is per organization; a user of another one must not be reachable — or
        // captured, or signed out — through the previous organization's commands (WP-215).
        crate::mqtt::rebind_if_tenant_changed(self);
    }

    /// Wipe all **per-user** runtime state, queued captures included, so one user's timer, data,
    /// consent and session can never bleed into the next person who signs in on this device.
    /// Device-level config/idle re-sync from the server/OS and are left alone.
    ///
    /// Used when the data must go: a released device (after its final flush), and a sign-in by a
    /// different user than the one who owns the queue. An ordinary sign-out uses
    /// [`Self::end_session_locally`] instead, which keeps the queue for its owner.
    pub fn reset_for_account_switch(&self) {
        // First, before anything is cleared: any capture still in flight (the open minute bucket, a
        // frame mid-encode) belongs to the previous user and must be dropped, not handed over after
        // the queues below are emptied (WP-205, WP-443).
        self.capture_epoch.fetch_add(1, Ordering::SeqCst);

        // End the running session locally so the next user never inherits it.
        let _ = self
            .timer
            .lock()
            .unwrap()
            .stop(crate::clock::now_epoch_ms(), StopReason::Logout);
        *self.idle_window.lock().unwrap() = None;

        // Drop buffered-but-unsent captures and any queued batches — all the previous user's data.
        self.pending_events.lock().unwrap().clear();
        self.pending_activity.lock().unwrap().clear();
        crate::pending_shots::discard_all(self);
        self.pending_screenshot_meta.lock().unwrap().clear();
        self.outbox.lock().unwrap().clear();
        *self.location.lock().unwrap() = None;
        *self.pause.lock().unwrap() = PauseState::default();
        crate::session_state::update(|s| {
            s.pause_day.clear();
            s.pause_budget_secs = None;
            s.pause_used_secs = None;
            s.pause_until_ms = None;
        });
        // The next person must not have their timer auto-started onto the previous user's task.
        *self.auto_resume.lock().unwrap() = None;

        // The transparency log is this user's own record of what was captured on their machine; the
        // next person to sign in must not inherit (or be able to read) it.
        crate::privacy_log::clear();

        // Re-require consent for the next person: capture fails closed until they accept the notice.
        self.consent.store(false, Ordering::Relaxed);
        self.idle.store(false, Ordering::Relaxed);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: &str = "2026-10-05";
    const TUE: &str = "2026-10-06";

    const ALLOW: u64 = 15 * 60;
    const SEC: i64 = 1000;

    /// WP-213: the allowance is per local day — spent today stays spent, tomorrow it is whole again.
    #[test]
    fn the_pause_budget_resets_on_the_next_local_day() {
        let mut p = PauseState::default();
        assert_eq!(p.request(0, 10 * 60, MON, ALLOW), (true, 10 * 60, 5 * 60));
        p.until_ms = None; // that pause ran out
        assert_eq!(p.request(0, 10 * 60, MON, ALLOW), (true, 5 * 60, 0));
        p.until_ms = None;
        assert_eq!(
            p.request(0, 60, MON, ALLOW),
            (false, 0, 0),
            "spent for Monday"
        );
        assert_eq!(p.budget_on(MON, ALLOW), 0);
        assert_eq!(p.budget_on(TUE, ALLOW), ALLOW);
        assert!(
            p.request(0, 60, TUE, ALLOW).0,
            "Tuesday has its own allowance"
        );
    }

    /// Setting the clock back a day, or a malformed date from the webview, earns nothing back.
    #[test]
    fn an_earlier_or_malformed_day_does_not_refill_the_budget() {
        let mut p = PauseState::default();
        p.request(0, ALLOW, TUE, ALLOW);
        p.until_ms = None;
        assert!(!p.request(0, 60, MON, ALLOW).0);
        assert!(!p.request(0, 60, "tomorrow", ALLOW).0);
        assert_eq!(p.budget_on("", ALLOW), 0);
    }

    /// The owner's example: 5 s, then 50 s, … — only the time actually paused is spent, until the
    /// day's 15 minutes are used.
    #[test]
    fn a_pause_can_be_taken_in_pieces() {
        let mut p = PauseState::default();
        // Pause (asks for the rest of the allowance), resume after 5 s.
        assert!(p.request(0, ALLOW, MON, ALLOW).0);
        assert_eq!(p.resume(5 * SEC), ALLOW - 5);
        assert_eq!(p.budget_on(MON, ALLOW), ALLOW - 5);
        assert!(!p.is_paused(6 * SEC));
        // Again for 50 s.
        assert!(p.request(100 * SEC, ALLOW, MON, ALLOW).0);
        p.resume(150 * SEC);
        assert_eq!(p.budget_on(MON, ALLOW), ALLOW - 55);
        // A last pause runs to the end of the allowance and stops by itself.
        let (_, granted, left) = p.request(200 * SEC, ALLOW, MON, ALLOW);
        assert_eq!((granted, left), (ALLOW - 55, 0));
        assert!(p.is_paused(200 * SEC + (granted as i64 - 1) * SEC));
        assert!(!p.is_paused(200 * SEC + granted as i64 * SEC));
        assert_eq!(
            p.resume(10_000 * SEC),
            0,
            "nothing to give back once it has run out"
        );
    }

    /// A fraction of a second paused is spent, never gained back.
    #[test]
    fn resuming_never_adds_to_the_allowance() {
        let mut p = PauseState::default();
        p.request(0, ALLOW, MON, ALLOW);
        p.resume(400); // 0.4 s in: 899.6 s unused, 899 given back — the part-second is spent
        assert_eq!(p.budget_on(MON, ALLOW), ALLOW - 1);
        assert_eq!(p.resume(500), 0, "resuming twice gives nothing");
    }

    /// An admin's change applies at once, in either direction; 0 turns pausing off.
    #[test]
    fn the_allowance_comes_from_the_org_and_applies_at_once() {
        let mut p = PauseState::default();
        p.request(0, 60, MON, ALLOW);
        p.resume(60 * SEC);
        assert_eq!(p.budget_on(MON, 30 * 60), 29 * 60, "raised to 30 min");
        assert_eq!(p.budget_on(MON, 30), 0, "lowered below what was used");
        assert_eq!(p.request(0, 60, TUE, 0), (false, 0, 0), "0 = no pausing");
    }

    /// WP-212: the banner's Stop ends the session at the last input — unless the person has clearly
    /// been back at work for a while, when it means "now".
    #[test]
    fn an_idle_banner_stop_backdates_only_while_the_stretch_is_current() {
        let w = IdleWindow {
            since_ms: 1_000,
            returned_ms: None,
        };
        assert_eq!(w.stop_at(900_000), 1_000);
        let back = IdleWindow {
            returned_ms: Some(600_000),
            ..w
        };
        assert_eq!(back.stop_at(600_000 + IDLE_STOP_GRACE_MS), 1_000);
        assert_eq!(
            back.stop_at(600_001 + IDLE_STOP_GRACE_MS),
            600_001 + IDLE_STOP_GRACE_MS
        );
    }
}
