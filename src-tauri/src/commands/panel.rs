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
    let _ = policy_version; // the version is advisory; consent is a single on/off flag
    state
        .consent
        .store(true, std::sync::atomic::Ordering::Relaxed);
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
) -> TimerStateDto {
    let ts = now_ms();
    let session_id = uuid::Uuid::new_v4().to_string();
    let ev = {
        let mut t = state.timer.lock().unwrap();
        t.start(
            session_id,
            task_id.unwrap_or_default(),
            project_id.unwrap_or_default(),
            description.unwrap_or_default(),
            ts,
        )
        .ok()
    };
    if let Some(e) = ev {
        state.pending_events.lock().unwrap().push(e);
    }
    read_timer(&state)
}

#[tauri::command]
pub fn stop_timer(state: State<'_, AppState>) -> TimerStateDto {
    let ts = now_ms();
    let ev = state.timer.lock().unwrap().stop(ts, StopReason::User);
    if let Some(e) = ev {
        state.pending_events.lock().unwrap().push(e);
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

// ── identity ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct IdentityDto {
    pub name: String,
    pub email: String,
    pub avatar_url: String,
}

/// Who this device reports as — from the Cognito ID-token claims. `None` when signed out (the panel
/// hides the avatar rather than inventing a person).
#[tauri::command]
pub fn identity(state: State<'_, AppState>) -> Option<IdentityDto> {
    let s = state.auth.status();
    if !s.signed_in {
        return None;
    }
    let email = s.username.clone().unwrap_or_default();
    let name = display_name(&email);
    Some(IdentityDto {
        name,
        email,
        avatar_url: String::new(),
    })
}

/// A friendly display name from an email local-part (`alex.morgan@…` → `Alex Morgan`).
fn display_name(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    if local.is_empty() {
        return email.to_string();
    }
    local
        .split(['.', '_', '-'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_titlecases_the_local_part() {
        assert_eq!(display_name("alex.morgan@acme.test"), "Alex Morgan");
        assert_eq!(display_name("owner@acme.test"), "Owner");
    }
}
