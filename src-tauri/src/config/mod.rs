//! Effective agent config cache. Holds the last-applied [`AgentConfig`] (tracking prefs + app/URL
//! rules). The sender compares its `version` to the batch ack and ETag-pulls on mismatch, applying
//! the new config live with no restart (BUILD-PLAN §4; contract `config.rs`).

use wp_agent_contract::AgentConfig;

#[derive(Default)]
pub struct ConfigCache {
    current: AgentConfig,
}

impl ConfigCache {
    /// The opaque change token the batch ack carries (aggregates tracking + rules versions).
    pub fn version(&self) -> u64 {
        self.current.version
    }

    pub fn get(&self) -> &AgentConfig {
        &self.current
    }

    /// The server advertised a different version on the ack → pull.
    pub fn needs_pull(&self, server_version: u64) -> bool {
        server_version != self.current.version
    }

    /// Apply a freshly pulled config (live — no restart).
    pub fn apply(&mut self, new: AgentConfig) {
        self.current = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_only_on_version_mismatch() {
        let c = ConfigCache::default(); // version 0
        assert!(!c.needs_pull(0));
        assert!(c.needs_pull(5));
    }
}
