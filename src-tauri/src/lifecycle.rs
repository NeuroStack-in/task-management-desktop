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
    //
    // `open` is cleared in the same write: this exit closed the session properly, so the next launch
    // must not also recover it. A leftover record would emit a second `TimerStopped` for a session
    // already closed.
    crate::session_state::update(|s| {
        s.resume = resume;
        s.open = None;
    });
}

/// Close a session left open by an ending that ran no code of ours — a crash, a force-kill, a power
/// cut. Called once at startup, before anything can start a new timer.
///
/// Emits `TimerStopped` at the session's **last known-alive moment**, not at now. The crash instant
/// is unknowable, but the last heartbeat is a time the agent was provably running, and it is the
/// only defensible end: billing to now would charge for every hour the machine was off.
///
/// Silent and idempotent — no record, nothing to do, which is the normal case on every launch.
pub fn recover_open_session(app: &AppHandle) {
    let Some(open) = crate::session_state::load().open else {
        return;
    };
    let state = app.state::<AppState>();
    state
        .pending_events
        .lock()
        .unwrap()
        .push(wp_agent_contract::AgentEvent::TimerStopped {
            session_id: open.session_id.clone(),
            started_at: open.started_at,
            ts: open.last_alive_ms,
            // The contract has no `Crash`, and adding one is a server-side change. `Shutdown` is
            // the honest reading of what happened: the process ended without stopping the timer.
            reason: StopReason::Shutdown,
        });
    // Queue synchronously — this must survive even if the agent is closed again immediately.
    let seq = crate::cycle::assemble_and_enqueue(&state);
    let open_secs = (open.last_alive_ms - open.started_at).max(0) / 1000;
    tracing::warn!(
        batch_seq = seq,
        session_id = %open.session_id,
        open_secs,
        "recovered a session left open by an unclean exit; closed it at its last heartbeat"
    );
    crate::session_state::update(|s| s.open = None);
}
