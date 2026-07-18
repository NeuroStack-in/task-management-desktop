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
    project_id: String,
    started_at: i64,
    description: String,
}

impl TimerEngine {
    /// Start a session. Returns the event to enqueue, or `Err` if one is already running.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        session_id: String,
        task_id: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_session_at_a_time() {
        let mut t = TimerEngine::default();
        assert!(t
            .start("s1".into(), "k1".into(), "p1".into(), "d1".into(), 0)
            .is_ok());
        assert!(t
            .start("s2".into(), "k2".into(), "p1".into(), "d2".into(), 1)
            .is_err());
        assert!(t.stop(2, StopReason::User).is_some());
        assert!(!t.is_running());
        assert!(t
            .start("s3".into(), "k3".into(), "p1".into(), "d3".into(), 3)
            .is_ok());
    }
}
