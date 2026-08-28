//! App lifecycle — closing the running session so a quit never leaves a timer open on the server.
//!
//! There are two ways this process ends, and only one of them is a Tauri event.
//!
//! **A quit** (tray Quit / Ctrl-C / SIGTERM) funnels through `RunEvent::ExitRequested`, which is
//! where this used to live inline.
//!
//! **An auto-update** does not. `tauri-plugin-updater` runs the Windows installer and then calls
//! `std::process::exit(0)` directly — the event loop never sees it, so `ExitRequested` never fires.
//! The plugin's `on_before_exit` hook is the only chance to act, and the agent was passing an empty
//! closure to it. So an agent that updated itself while a timer was running left that timer open:
//! no `TimerStopped` reached the server, the web UI showed "Recording" indefinitely, and the task
//! the employee was on was forgotten instead of offered back on relaunch.
//!
//! Hence one function, called from both.

use tauri::{AppHandle, Manager};

use crate::state::AppState;
use wp_agent_contract::StopReason;

/// Close the running session and persist it. **Idempotent** — safe if the process ends twice over
/// (a quit that races the updater), because `Timer::stop` returns `None` when nothing is running.
///
/// Deliberately synchronous: both callers are on a path where the runtime is about to disappear and
/// nothing can be awaited. `assemble_and_enqueue` writes to `queue/batches.jsonl` on this thread,
/// so the stop survives the exit and ships on the next launch even if the machine is offline.
///
/// The session is **not** signed out. Tokens live in the OS keyring and `auth.restore()` picks them
/// up at startup; signing out here would force a fresh login on every update, which for an agent
/// that autostarts is the difference between "monitoring resumes" and "monitoring silently doesn't".
pub fn close_session_for_exit(app: &AppHandle, reason: &'static str) {
    let state = app.state::<AppState>();

    // Best-effort clean MQTT presence: queue `{"online":false}` + DISCONNECT so the fleet flips
    // offline immediately; if the process dies first, the broker's Last Will delivers the same
    // payload after the keepalive window.
    crate::mqtt::shutdown(&state);

    let ts = crate::clock::now_epoch_ms();
    // Bound separately so the timer's MutexGuard is dropped before `state` goes out of scope.
    // Remember what was running *before* stopping it, so reopening can offer the same task back.
    // Read under the same lock scope as the stop so the two cannot disagree.
    let resume = {
        let mut t = state.timer.lock().unwrap();
        let snap = t.snapshot(ts);
        let resume = snap.running.then(|| crate::session_state::ResumeTask {
            task_id: snap.task_id.clone().unwrap_or_default(),
            project_id: snap.project_id.clone().unwrap_or_default(),
            description: snap.description.clone(),
            stopped_at_ms: ts,
        });
        let stopped = t.stop(ts, StopReason::Shutdown);
        if let Some(ev) = stopped {
            state.pending_events.lock().unwrap().push(ev);
        }
        resume
    };

    // Persist that `TimerStopped` before the process dies. `pending_events` is an in-memory Vec
    // that only reaches the durable outbox via `assemble_and_enqueue`, which the sender normally
    // drives on its cycle — but nothing here waits for the sender, and the runtime is about to go
    // away. Without this the stop event is simply lost, the server never learns the session ended,
    // and the web UI shows "Recording" indefinitely.
    let seq = crate::cycle::assemble_and_enqueue(&state);
    tracing::info!(
        batch_seq = seq,
        reason,
        resuming = resume.is_some(),
        "shutdown: queued the final batch (timer stop)"
    );

    // The session is still *closed* on the server (the TimerStopped above): the offline period must
    // not be billed, since nothing was captured during it. Reopening starts a fresh session on the
    // same task, and today's total comes from the folded entries.
    crate::session_state::update(|s| s.resume = resume);
}
