//! Agent error surface. UI-facing errors serialize to a stable `domain:reason` string (e.g.
//! `auth:expired`) so the webview can branch on `error.split(':')` without parsing prose.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("auth:expired")]
    AuthExpired,
    #[error("timer:already_running")]
    TimerAlreadyRunning,
    #[error("timer:not_running")]
    TimerNotRunning,
    #[error("io:{0}")]
    Io(String),
}

/// Tauri commands return `Result<T, String>`; convert at that boundary.
impl From<AgentError> for String {
    fn from(e: AgentError) -> String {
        e.to_string()
    }
}
