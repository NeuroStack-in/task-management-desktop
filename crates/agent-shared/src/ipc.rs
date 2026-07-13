//! Agent-internal IPC (core ↔ capture-helper ↔ tray). Never sent to the backend.
//!
//! Transport is a local socket / named pipe (implemented in `agentd::session_manager`); these
//! are just the message shapes.

use serde::{Deserialize, Serialize};
use wp_agent_contract::{ActivityRollup, AgentEvent, ScreenshotMeta, TrackingConfig};

/// Core service → capture helper.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CoreToHelper {
    /// Apply new capture config live (cadence/blur/silent), no restart.
    ApplyConfig(TrackingConfig),
    /// Pause/resume capture (tray "pause", quiet hours, timer-not-running gate).
    SetPaused(bool),
    /// Updated app/URL category rules synced from the server.
    UpdateRules(Vec<CategoryRule>),
}

/// Capture helper → core service (everything the helper produces flows to the outbox).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HelperToCore {
    Activity(ActivityRollup),
    Event(AgentEvent),
    /// Screenshot metadata; the raw bytes are handed off separately for S3-direct upload.
    Screenshot {
        meta: ScreenshotMeta,
        tmp_path: String,
    },
}

/// Tray UI → core service.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TrayCommand {
    Pause,
    Resume,
    StartTimer {
        task_id: String,
        project_id: String,
        description: String,
    },
    StopTimer,
    /// User granted/acknowledged monitoring consent.
    ConsentGranted,
}

/// One app/URL → category rule, synced from the server and applied on-device (edge-first).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CategoryRule {
    /// Process name or domain/glob.
    pub matcher: String,
    pub category: wp_agent_contract::Category,
}
