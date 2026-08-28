//! Timer state machine: at most ONE running session (the agent holds the authoritative clock).
//! Emits `AgentEvent`s into the outbox.
//!
//! `started_at` is tracked so `TimerStopped` can carry it — **required** by the contract, because a
//! session crossing midnight otherwise orphans into two half-rows that never merge (envelope.rs).
//!
//! M3 (after the contract PR, BUILD-PLAN §6) adds a mandatory `description` and makes
//! `task_id`/`project_id` `Option` for meeting mode, plus crash-recovery of the running session.

use wp_agent_contract::{AgentEvent, StopReason};

#[derive(Default)]
pub struct TimerEngine {
    running: Option<Running>,
}

struct Running {
    session_id: String,
    task_id: String,
    /// The subtask the timer is running against, or empty when it targets the task itself.
    ///
    /// Held here, not derived at stop time, for the same reason `started_at` is: the panel may have
    /// moved on to another task by then, and the session must report what it actually ran against.
    subtask_id: String,
    project_id: String,
    started_at: i64,
    description: String,
}

impl TimerEngine {
    /// Start a session. Returns the event to enqueue, or `Err` if one is already running.
    #[allow(clippy::too_many_arguments)]
    /// `task_id` is **always the parent task**, even when `subtask_id` is set.
    ///
    /// That is the contract, and it is what keeps every server-side hours rollup — project totals,
    /// the timesheet, `time.logged_for_project`, the KPI velocity bucket — working unchanged: they
    /// all key off `task_id`. Putting the subtask id there instead would file the time against
    /// something none of them know about, and the hours would vanish from the project.
    pub fn start(
        &mut self,
        session_id: String,
        task_id: String,
        subtask_id: String,
        project_id: String,
        description: String,
        ts: i64,
    ) -> Result<AgentEvent, &'static str> {
        if self.running.is_some() {
            return Err("a session is already running");
        }
        self.running = Some(Running {
            session_id: session_id.clone(),
            task_id: task_id.clone(),
            subtask_id: subtask_id.clone(),
            project_id: project_id.clone(),
            started_at: ts,
            description: description.clone(),
        });
        Ok(AgentEvent::TimerStarted {
            session_id,
            task_id,
            project_id,
            ts,
            description,
            // Absent, not empty: the server distinguishes "targeted the task" from "targeted a
            // subtask we failed to record", and an empty string would collapse the two.
            subtask_id: none_if_empty(&subtask_id),
        })
    }

    /// Stop the running session (user/idle/logout/shutdown). Carries `started_at` for the fold.
    pub fn stop(&mut self, ts: i64, reason: StopReason) -> Option<AgentEvent> {
        self.running.take().map(|r| AgentEvent::TimerStopped {
            session_id: r.session_id,
            started_at: r.started_at,
            ts,
            reason,
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The running session as the **heartbeat** declares it, or `None` when nothing is running.
    ///
    /// `None` here is a positive statement ("no timer"), not an absence of information: the server
    /// uses it to close a session this agent no longer knows about. `paused` is supplied by the
    /// caller because the privacy pause lives outside the engine.
    pub fn active_session(&self, paused: bool) -> Option<wp_agent_contract::ActiveSession> {
        self.running
            .as_ref()
            .map(|r| wp_agent_contract::ActiveSession {
                session_id: r.session_id.clone(),
                started_at: r.started_at,
                paused,
            })
    }

    /// The running session's shape for the panel's `timer_state` command. `task_id`/`project_id` are
    /// `None` when stopped or when that field is empty (meeting/ad-hoc mode); `elapsed_secs` is the
    /// current session's length. `description` is what the user typed ("what are you working on?").
    pub fn snapshot(&self, now_ms: i64) -> TimerSnapshot {
        match &self.running {
            None => TimerSnapshot::default(),
            Some(r) => TimerSnapshot {
                running: true,
                task_id: none_if_empty(&r.task_id),
                subtask_id: none_if_empty(&r.subtask_id),
                project_id: none_if_empty(&r.project_id),
                description: r.description.clone(),
                elapsed_secs: ((now_ms - r.started_at).max(0) / 1000) as u64,
            },
        }
    }
}

/// A snapshot of the running session for the UI (`timer_state`).
#[derive(Default)]
pub struct TimerSnapshot {
    pub running: bool,
    pub task_id: Option<String>,
    /// The subtask being worked on, when the user picked one. The panel shows it under the task.
    pub subtask_id: Option<String>,
    pub project_id: Option<String>,
    pub description: String,
    pub elapsed_secs: u64,
}

fn none_if_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_session_at_a_time() {
        let mut t = TimerEngine::default();
        assert!(t
            .start(
                "s1".into(),
                "k1".into(),
                String::new(),
                "p1".into(),
                "d1".into(),
                0
            )
            .is_ok());
        assert!(t
            .start(
                "s2".into(),
                "k2".into(),
                String::new(),
                "p1".into(),
                "d2".into(),
                1
            )
            .is_err());
        assert!(t.stop(2, StopReason::User).is_some());
        assert!(!t.is_running());
        assert!(t
            .start(
                "s3".into(),
                "k3".into(),
                String::new(),
                "p1".into(),
                "d3".into(),
                3
            )
            .is_ok());
    }
}
