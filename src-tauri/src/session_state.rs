//! The small pieces of agent state that must outlive a restart: **monitoring consent** and **the
//! task that was running when the app closed**.
//!
//! Everything else the panel shows is either derived from the server (today's sessions) or
//! deliberately ephemeral. These two were held only in memory, which is why closing the agent asked
//! for the monitoring notice again and dropped the running task on the floor.
//!
//! Plain JSON beside the outbox, not the OS keyring: neither value is a secret, and the keyring is
//! reserved for the refresh token. A missing, unreadable or corrupt file degrades to defaults —
//! consent `false`, nothing to resume — which is the fail-closed direction for both.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The task that was running when the agent last closed, so it can be picked back up.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeTask {
    pub task_id: String,
    /// The subtask that was running. `#[serde(default)]` so a state file written before subtasks
    /// existed still loads — a missing field there must degrade to "the task itself", not refuse to
    /// parse and lose the resume entirely.
    #[serde(default)]
    pub subtask_id: String,
    pub project_id: String,
    pub description: String,
    /// Epoch ms when the agent closed on this task.
    ///
    /// Stored as an instant, not a local date, because **the core has no local calendar**. Rust
    /// here has exactly one clock (`clock::now_epoch_ms`, UTC ms) and the `time` crate is built
    /// without `local-offset`; the webview is what knows the user's local day, which is why
    /// `list_sessions` already takes the date as a parameter rather than deriving it. Deciding
    /// "is this still today?" therefore belongs to the panel, and it makes that call before
    /// resuming.
    pub stopped_at_ms: i64,
}

/// A session that was running when this file was last written — the crash-recovery record.
///
/// The clean exits (tray Quit, Windows sign-out, auto-update) all close the session themselves.
/// This exists for the endings where **no code of ours runs at all**: a panic, Task Manager, a
/// power cut, a battery dying. Nothing can emit a `TimerStopped` at the moment of such an ending,
/// so the agent instead leaves a note on disk while it runs and reads it back on the next launch.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenSession {
    pub session_id: String,
    /// Epoch ms the session began — the contract requires it on `TimerStopped` so a session
    /// crossing midnight folds into one row rather than two orphaned halves.
    pub started_at: i64,
    /// Epoch ms of the last tick that saw this session alive.
    ///
    /// **This, not the recovery time, is what the session is closed at.** The moment of a crash is
    /// unknowable; the last moment the agent was demonstrably running is not. Closing at "now"
    /// would bill every hour the machine spent switched off, and closing at `started_at` would
    /// discard work that really happened. Refreshed on a cadence, so the error is bounded by that
    /// interval rather than by how long the machine stayed down.
    pub last_alive_ms: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    /// Whether the monitoring notice has been accepted.
    pub consent_granted: bool,
    /// The rules version in effect when consent was given. Recorded for audit only — see the note on
    /// [`SessionState::restore_consent`] about why it does not currently force a re-prompt.
    #[serde(default)]
    pub consent_policy_version: u64,
    #[serde(default)]
    pub resume: Option<ResumeTask>,
    /// The session running as of the last heartbeat — see [`OpenSession`]. `None` whenever the
    /// timer is stopped, and cleared by every clean exit, so a value here on startup means the
    /// previous run ended without one.
    #[serde(default)]
    pub open: Option<OpenSession>,
}

impl SessionState {
    /// Consent as it should be applied at startup.
    ///
    /// Returns the stored flag as-is. It deliberately does **not** re-prompt when
    /// `consent_policy_version` differs from the current rules version: the disclosure the user
    /// actually agreed to (`commands::panel::disclosure`) is a fixed list, and the rules version
    /// bumps whenever an owner edits app/URL rules — so re-prompting on a version change would
    /// re-ask for consent to the *same* disclosure, training people to click through it.
    ///
    /// **If the disclosure ever becomes dynamic** — driven by org policy rather than that fixed list
    /// — this is the place to compare versions and fall back to `false`, and it should be.
    pub fn restore_consent(&self) -> bool {
        self.consent_granted
    }

    /// The task the agent closed on, if any.
    ///
    /// Whether it is still *today's* — and so whether it may auto-start — is decided by the panel,
    /// which owns the local calendar. Auto-starting a timer is an act, not a display: reopening on
    /// Wednesday must never silently resume, and bill, Monday evening's task.
    pub fn pending_resume(&self) -> Option<&ResumeTask> {
        self.resume.as_ref()
    }
}

fn path() -> PathBuf {
    crate::outbox::state_dir().join("session.json")
}

/// Read persisted state. Any failure (absent, unreadable, corrupt) yields defaults — never an error:
/// a damaged file must not stop the agent from starting.
pub fn load() -> SessionState {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Write state. Best-effort but **logged**: a silent failure here is exactly the class of bug that
/// made the refresh token never persist, and the symptom (state lost on the *next* launch) shows up
/// far from the cause.
pub fn save(s: &SessionState) {
    let p = path();
    if let Some(parent) = p.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("could not create the agent state dir ({e}); session state not saved");
            return;
        }
    }
    match serde_json::to_string(s) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&p, json) {
                tracing::warn!("could not persist session state ({e})");
            }
        }
        Err(e) => tracing::warn!("could not serialise session state ({e})"),
    }
}

/// Update the stored state through `f`, preserving whatever the caller does not touch.
///
/// Read-modify-write rather than write-whole: consent and the resume task are written by different
/// code paths at different times, and a blind overwrite from one would erase the other.
pub fn update(f: impl FnOnce(&mut SessionState)) {
    let mut s = load();
    f(&mut s);
    save(&s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_resume(stopped_at_ms: i64) -> SessionState {
        SessionState {
            consent_granted: true,
            consent_policy_version: 3,
            resume: Some(ResumeTask {
                task_id: "t1".into(),
                subtask_id: String::new(),
                project_id: "p1".into(),
                description: "Writing the fold".into(),
                stopped_at_ms,
            }),
            open: None,
        }
    }

    #[test]
    fn carries_the_instant_the_agent_closed_on() {
        let s = with_resume(1_784_592_000_000);
        assert_eq!(s.pending_resume().unwrap().stopped_at_ms, 1_784_592_000_000);
    }

    #[test]
    fn nothing_to_resume_is_not_an_error() {
        let s = SessionState::default();
        assert!(s.pending_resume().is_none());
        assert!(!s.restore_consent());
    }

    /// Consent survives a restart — the whole point of persisting it.
    #[test]
    fn consent_round_trips() {
        let s = with_resume(1_784_592_000_000);
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert!(back.restore_consent());
    }

    /// Older files predate the newer fields; they must load rather than reset consent to false.
    #[test]
    fn older_files_without_the_new_fields_still_load() {
        let back: SessionState = serde_json::from_str(r#"{"consent_granted":true}"#).unwrap();
        assert!(back.restore_consent());
        assert_eq!(back.consent_policy_version, 0);
        assert!(back.resume.is_none());
    }

    /// A corrupt file must degrade to fail-closed defaults, not propagate an error.
    #[test]
    fn corrupt_json_degrades_to_defaults() {
        let back: Result<SessionState, _> = serde_json::from_str("{not json");
        assert!(back.is_err());
        let fallback = back.unwrap_or_default();
        assert!(!fallback.restore_consent());
        assert!(fallback.resume.is_none());
    }
}
