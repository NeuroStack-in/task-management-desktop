//! The webview's only entry into the core (Tauri `#[command]`s). DTOs use camelCase so there is no
//! snake↔camel transform layer at the boundary (BUILD-PLAN §3).
//!
//! M0: timer control + identity, wired to real `AppState`. M1 adds `auth_cmds`; M3 the
//! project→task selector + mandatory description + meeting mode; M4/M5 monitor + screenshot status.

pub mod panel;

use tauri::State;
use wp_agent_contract::StopReason;

use crate::auth::AuthStatus;
use crate::clock::now_epoch_ms;
use crate::state::AppState;

// ---- auth (M1) ----

/// Sign in with Cognito `USER_PASSWORD_AUTH`. May return a `newPasswordSession` if the account was
/// admin-created and must set a password (then call `auth_complete_new_password`).
#[tauri::command]
pub async fn auth_login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<AuthStatus, String> {
    state.auth.login(&username, &password).await
}

#[tauri::command]
pub async fn auth_complete_new_password(
    state: State<'_, AppState>,
    username: String,
    new_password: String,
    session: String,
) -> Result<AuthStatus, String> {
    state
        .auth
        .complete_new_password(&username, &new_password, &session)
        .await
}

#[tauri::command]
pub fn auth_logout(state: State<'_, AppState>) {
    state.auth.logout();
}

#[tauri::command]
pub fn auth_status(state: State<'_, AppState>) -> AuthStatus {
    state.auth.status()
}

// ---- consent (M5 / PRIVACY.md) ----

/// Grant or revoke monitoring consent. Capture (activity + screenshots) is gated on this and
/// **defaults off** — so nothing is captured until the user consents.
#[tauri::command]
pub fn set_consent(state: State<'_, AppState>, granted: bool) {
    state
        .consent
        .store(granted, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
pub fn consent_status(state: State<'_, AppState>) -> bool {
    state.consent.load(std::sync::atomic::Ordering::Relaxed)
}

// ---- updater (M7) ----

/// Manually check for a newer signed release (respects `auto_update` for whether it installs).
#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let auto = state.config.lock().unwrap().get().tracking.auto_update;
    crate::updater::check_and_maybe_install(&app, auto).await
}

// ---- timer (M0) ----

/// Start the timer. M3 threads `description` + optional `task_id`/`project_id` (meeting mode) once
/// the contract PR lands (§6); today they are required, matching the deployed contract.
#[tauri::command]
pub fn timer_start(
    state: State<'_, AppState>,
    session_id: String,
    task_id: String,
    project_id: String,
    description: String,
) -> Result<(), String> {
    let ts = now_epoch_ms();
    let event = {
        let mut timer = state.timer.lock().unwrap();
        timer
            .start(session_id, task_id, project_id, description, ts)
            .map_err(|e| format!("timer:{e}"))?
    };
    // Buffered for the next cycle's `enqueue_cycle` drain (BUILD-PLAN §4).
    state.pending_events.lock().unwrap().push(event);
    Ok(())
}

/// Stop the running timer (user-initiated). Idle/logout/shutdown stops come from the monitor.
#[tauri::command]
pub fn timer_stop(state: State<'_, AppState>) -> Result<(), String> {
    let ts = now_epoch_ms();
    let event = { state.timer.lock().unwrap().stop(ts, StopReason::User) };
    if let Some(ev) = event {
        state.pending_events.lock().unwrap().push(ev);
    }
    Ok(())
}

/// Whether a session is currently running (drives the UI timer + capture gate).
#[tauri::command]
pub fn timer_status(state: State<'_, AppState>) -> bool {
    state.timer.lock().unwrap().is_running()
}

/// This install's stable agent id (diagnostics; the fleet row key on the backend).
#[tauri::command]
pub fn agent_id(state: State<'_, AppState>) -> String {
    state.outbox.lock().unwrap().agent_id().to_string()
}
