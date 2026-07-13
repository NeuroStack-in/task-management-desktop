//! Derives attendance + breaks from OS signals: session lock/unlock → login/logout attendance;
//! idle ≥ 15 min → break. Emits `AgentEvent`s into the outbox (no new agent-facing signal).
//!
//! Skeleton placeholder; production hooks OS lock/unlock notifications + the idle detector.

use agent_shared::contract::AgentEvent;

/// Map an OS lock/unlock transition to an attendance event.
pub fn on_session_lock(locked: bool, ts: i64) -> AgentEvent {
    if locked {
        AgentEvent::AttendanceLogout { ts }
    } else {
        AgentEvent::AttendanceLogin { ts }
    }
}
