//! The webview's only entry into the core (Tauri `#[command]`s). DTOs use camelCase so there is no
//! snake↔camel transform layer at the boundary (BUILD-PLAN §3).
//!
//! M0: timer control + identity, wired to real `AppState`. M1 adds `auth_cmds`; M3 the
//! project→task selector + mandatory description + meeting mode; M4/M5 monitor + screenshot status.

use tauri::State;
use wp_agent_contract::StopReason;

use crate::clock::now_epoch_ms;
use crate::state::AppState;

/// Start the timer. M3 threads `description` + optional `task_id`/`project_id` (meeting mode) once
/// the contract PR lands (§6); today they are required, matching the deployed contract.
#[tauri::command]
pub fn timer_start(
    state: State<'_, AppState>,
    session_id: String,
    task_id: String,
    project_id: String,
) -> Result<(), String> {
    let ts = now_epoch_ms();
    let event = {
        let mut timer = state.timer.lock().unwrap();
        timer
            .start(session_id, task_id, project_id, ts)
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
