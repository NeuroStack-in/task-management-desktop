//! Holds the effective `TrackingConfig`; pulls a new one when the batch ack reports a higher
//! version (ETag-conditional GET of `CONFIG#TRACK`), and applies it live — no restart. The new
//! config is pushed to capture helpers over IPC (`CoreToHelper::ApplyConfig`).

use agent_shared::contract::TrackingConfig;

#[derive(Default)]
pub struct ConfigCache {
    current: TrackingConfig,
}

impl ConfigCache {
    pub fn version(&self) -> u64 {
        self.current.version
    }

    pub fn get(&self) -> &TrackingConfig {
        &self.current
    }

    /// True when the server advertises a newer version than we hold.
    pub fn needs_pull(&self, server_version: u64) -> bool {
        server_version > self.current.version
    }

    /// Apply a freshly-pulled config (would then be broadcast to helpers over IPC).
    pub fn apply(&mut self, new: TrackingConfig) {
        self.current = new;
    }
}
