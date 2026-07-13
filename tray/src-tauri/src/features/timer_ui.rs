//! The personal work timer. The employee starts/stops/switches the task they're working on; the tray
//! reflects the running timer and its elapsed time. Start/stop are `TrayCommand`s to the core, which
//! owns the authoritative timer state machine (`agentd::timer_engine`) and emits the Timer* events
//! that ride the batch to the backend — the tray never fabricates timer events itself.

use serde::Serialize;

#[derive(Serialize)]
pub struct TimerState {
    /// True while a timer session is running.
    pub running: bool,
    /// The task the running timer is attributed to, if any.
    pub task_id: Option<String>,
    /// Elapsed seconds in the current session.
    pub elapsed_secs: u64,
}

#[tauri::command]
pub fn timer_state() -> TimerState {
    // TODO(ipc): last timer state pushed by the core.
    TimerState { running: false, task_id: None, elapsed_secs: 0 }
}

#[tauri::command]
pub fn start_timer(task_id: Option<String>) -> TimerState {
    // TODO(ipc): send TrayCommand::StartTimer{task_id, project_id, description}; the core's
    // timer_engine transitions and emits AgentEvent::TimerStarted. Returned state echoes the core.
    TimerState { running: true, task_id, elapsed_secs: 0 }
}

#[tauri::command]
pub fn stop_timer() -> TimerState {
    // TODO(ipc): send TrayCommand::StopTimer; the core emits AgentEvent::TimerStopped{reason: User}.
    TimerState { running: false, task_id: None, elapsed_secs: 0 }
}
