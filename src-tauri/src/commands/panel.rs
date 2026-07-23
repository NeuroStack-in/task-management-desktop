//! The command surface kishore's tray panel calls (`lib/agent.ts`). Serde field names are
//! **snake_case** to match `ui/src/lib/types.ts` (`policy_version`, `next_cycle_secs`, …). Wired to
//! the real single-process state — consent, timer, config, screenshots, pause — with identity from
//! the Cognito claims. `tasks`/`activity`/`sessions` have no source yet (the panel degrades: no task
//! list, empty sessions) and are served by their own commands returning empty.

use serde::Serialize;
use tauri::State;
use wp_agent_contract::{Cadence, StopReason, TrackingConfig};

use crate::clock::now_epoch_ms;
use crate::state::AppState;

fn now_ms() -> i64 {
    now_epoch_ms()
}

// ── consent ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConsentStateDto {
    pub granted: bool,
    pub policy_version: u64,
    pub captured: Vec<String>,
}

fn disclosure() -> Vec<String> {
    [
        "Activity counts (keystroke & mouse totals — never the keys themselves)",
        "Foreground app / website category",
        "Periodic screenshots (blurred per your organization's policy)",
        "Device location (GPS / OS positioning) while monitoring is on",
        "Attendance and timer events",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn read_consent(state: &AppState) -> ConsentStateDto {
    ConsentStateDto {
        granted: state.consent.load(std::sync::atomic::Ordering::Relaxed),
        policy_version: state.config.lock().unwrap().get().rules.version,
        captured: disclosure(),
    }
}

#[tauri::command]
pub fn get_consent_state(state: State<'_, AppState>) -> ConsentStateDto {
    read_consent(&state)
}

#[tauri::command]
pub fn grant_consent(state: State<'_, AppState>, policy_version: u64) -> ConsentStateDto {
    state
        .consent
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // Persisted, so the monitoring notice is a first-run step rather than a toll on every launch.
    // The version is recorded for audit — which disclosure was accepted — not used to re-prompt.
    crate::session_state::update(|s| {
        s.consent_granted = true;
        s.consent_policy_version = policy_version;
    });
    read_consent(&state)
}

// ── capture indicator ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CaptureStateDto {
    pub capturing: bool,
    pub screenshots: bool,
    pub next_cycle_secs: u32,
}

#[tauri::command]
pub fn capture_state(state: State<'_, AppState>) -> CaptureStateDto {
    let running = state.timer.lock().unwrap().is_running();
    let consent = state.consent.load(std::sync::atomic::Ordering::Relaxed);
    let paused = state.pause.lock().unwrap().is_paused(now_ms());
    let cfg = state.config.lock().unwrap();
    let cadence = cfg.get().tracking.cadence;
    let screenshots = !matches!(cadence, Cadence::Off);
    let capturing = running && consent && !paused && screenshots;
    let interval = cadence.interval_secs().unwrap_or(0);
    let next_cycle_secs = if capturing && interval > 0 {
        interval - ((now_ms() / 1000) as u32 % interval)
    } else {
        0
    };
    CaptureStateDto {
        capturing,
        screenshots,
        next_cycle_secs,
    }
}

// ── config ───────────────────────────────────────────────────────────────────

/// The effective tracking config (contract `TrackingConfig` — serializes with the exact fields the
/// panel's `TrackingConfig` type reads; `cadence` is lowercase, matching `Cadence`).
#[tauri::command]
pub fn effective_config(state: State<'_, AppState>) -> TrackingConfig {
    state.config.lock().unwrap().get().tracking.clone()
}

// ── timer ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TimerStateDto {
    pub running: bool,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
    pub description: String,
    pub elapsed_secs: u64,
}

fn read_timer(state: &AppState) -> TimerStateDto {
    let s = state.timer.lock().unwrap().snapshot(now_ms());
    TimerStateDto {
        running: s.running,
        task_id: s.task_id,
        project_id: s.project_id,
        description: s.description,
        elapsed_secs: s.elapsed_secs,
    }
}

#[tauri::command]
pub fn timer_state(state: State<'_, AppState>) -> TimerStateDto {
    read_timer(&state)
}

/// Start a session against a project, with the user's free-text description ("what are you working
/// on?"). `task_id` is optional — omitted for the project/ad-hoc flow the panel uses; when present it
/// attributes to a specific task. The emitted `TimerStarted` carries all three to the backend fold.
#[tauri::command]
pub fn start_timer(
    state: State<'_, AppState>,
    project_id: Option<String>,
    description: Option<String>,
    task_id: Option<String>,
) -> Result<TimerStateDto, String> {
    let ts = now_ms();
    let session_id = uuid::Uuid::new_v4().to_string();
    // Surface a start that was refused (already running) instead of swallowing it and returning the
    // *old* session — otherwise the panel shows the previous task as if the new one started.
    let event = {
        let mut t = state.timer.lock().unwrap();
        t.start(
            session_id,
            task_id.unwrap_or_default(),
            project_id.unwrap_or_default(),
            description.unwrap_or_default(),
            ts,
        )
        .map_err(|e| format!("timer:{e}"))?
    };
    state.pending_events.lock().unwrap().push(event);
    state.flush.notify_one(); // flush now — don't wait for the 300 s cycle (LLD §4)
    Ok(read_timer(&state))
}

#[tauri::command]
pub fn stop_timer(state: State<'_, AppState>) -> TimerStateDto {
    let ts = now_ms();
    let ev = state.timer.lock().unwrap().stop(ts, StopReason::User);
    if let Some(e) = ev {
        state.pending_events.lock().unwrap().push(e);
        state.flush.notify_one(); // a stop should reflect as fast as a start
    }
    read_timer(&state)
}

// ── privacy pause ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PauseGrantDto {
    pub granted: bool,
    pub granted_secs: u64,
    pub remaining_budget_secs: u64,
}

/// The live pause window, so the panel can re-attach to a pause it didn't start.
#[derive(Serialize)]
pub struct PauseStateDto {
    pub paused: bool,
    /// Seconds left on the current pause, 0 when not paused.
    pub remaining_secs: u64,
    pub remaining_budget_secs: u64,
}

/// Read the current pause without consuming budget.
///
/// The core owns the authoritative pause window (`state.rs`), but until now only `request_pause`
/// existed — so a webview reload lost the countdown while the core stayed paused, and the panel
/// under-reported. The UI must never track this locally: the budget is enforced core-side.
#[tauri::command]
pub fn pause_state(state: State<'_, AppState>) -> PauseStateDto {
    let now = now_ms();
    let p = state.pause.lock().unwrap();
    let remaining_secs = p
        .until_ms
        .filter(|u| *u > now)
        // Round up: a 500 ms remainder is still a paused second, and truncating would report 0
        // while capture is genuinely still suspended.
        .map(|u| ((u - now) as f64 / 1000.0).ceil() as u64)
        .unwrap_or(0);
    PauseStateDto {
        paused: p.is_paused(now),
        remaining_secs,
        remaining_budget_secs: p.budget_secs,
    }
}

#[tauri::command]
pub fn request_pause(state: State<'_, AppState>, requested_secs: u64) -> PauseGrantDto {
    let (granted, granted_secs, remaining_budget_secs) = state
        .pause
        .lock()
        .unwrap()
        .request(now_ms(), requested_secs);
    PauseGrantDto {
        granted,
        granted_secs,
        remaining_budget_secs,
    }
}

// ── projects (backend-fed) ───────────────────────────────────────────────────

/// The signed-in user's projects for the "Select project" picker — fetched live from
/// `GET /v1/projects`. Empty (not an error) when signed out, so the panel just shows no projects.
#[tauri::command]
pub async fn list_projects(
    state: State<'_, AppState>,
) -> Result<Vec<crate::api::projects::ProjectDto>, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Ok(Vec::new());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    crate::api::projects::fetch_projects(&client, &ingest_url, &id_token).await
}

/// The signed-in user's assigned tasks (with titles) for the task picker — `GET /v1/me/tasks`.
/// Empty when signed out.
#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
) -> Result<Vec<crate::api::tasks::TaskDto>, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Ok(Vec::new());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    crate::api::tasks::fetch_tasks(&client, &ingest_url, &id_token).await
}

// ── today's sessions (backend-fed) ───────────────────────────────────────────

/// The signed-in user's folded time entries for `date` (the client's local `YYYY-MM-DD`), aggregated
/// per (project, description) — the panel's "Today's sessions". Reads back this agent's own timer
/// events after they fold server-side (`GET /v1/me/timesheet/today`). Empty when signed out.
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    date: String,
) -> Result<Vec<crate::api::timesheet::SessionDto>, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Ok(Vec::new());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    crate::api::timesheet::fetch_today(&client, &ingest_url, &id_token, &date).await
}

// ── resume after restart ─────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingResumeDto {
    pub task_id: String,
    pub project_id: String,
    pub description: String,
    /// Epoch ms the agent closed on this task. The panel compares it against its own local calendar
    /// and only resumes when it is still the same day — the core has no local timezone.
    pub stopped_at_ms: i64,
}

/// The task the agent was running when it last closed, **claimed once**.
///
/// Taking-and-clearing rather than a plain read is deliberate. This drives an auto-start, so a value
/// left in place would resume again on the next launch, and again after that — a task the user
/// stopped days ago quietly restarting forever. Claiming it means one restart resumes one session.
///
/// Returning it does not start anything: the panel decides, because only it knows whether
/// `stopped_at_ms` is still today.
#[tauri::command]
pub fn take_pending_resume(state: State<'_, AppState>) -> Option<PendingResumeDto> {
    // Never resume on top of a running timer — a start would be rejected anyway, and this makes the
    // ordering explicit rather than relying on that.
    if state.timer.lock().unwrap().is_running() {
        return None;
    }
    let mut out = None;
    crate::session_state::update(|s| {
        out = s.resume.take().map(|r| PendingResumeDto {
            task_id: r.task_id,
            project_id: r.project_id,
            description: r.description,
            stopped_at_ms: r.stopped_at_ms,
        });
    });
    out
}

// ── identity ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct IdentityDto {
    pub name: String,
    pub email: String,
    pub avatar_url: String,
}

/// Who this device reports as — the person's name + email from the Cognito ID-token claims (the
/// `name`/`email` claims, never the `username`/`sub` UUID). `None` when signed out (the panel hides
/// the avatar rather than inventing a person).
#[tauri::command]
pub fn identity(state: State<'_, AppState>) -> Option<IdentityDto> {
    let (name, email) = state.auth.identity()?;
    Some(IdentityDto {
        name,
        email,
        avatar_url: String::new(),
    })
}
