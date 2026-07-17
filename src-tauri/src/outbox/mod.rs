//! Offline-first outbox — the single egress buffer. Every batch is persisted **before** send and
//! pruned only on watermark ack, so retries and long offline replays are safe by construction.
//! `(agent_id, batch_seq)` is the idempotency key.
//!
//! M0: in-memory `VecDeque` + a **stable per-install `agent_id`** (the old `"dev-agent"` default made
//! every dev machine collide on `(agent_id, batch_seq)` against the LIVE dev table — corrupted
//! watermarks, machines pruning each other's batches; BUILD-PLAN §2). M2: append-only
//! `queue/batches.jsonl` (**not** SQLCipher — the older docs were wrong), and `agent_id` moves into
//! the OS keyring at enrollment (M1).

use std::collections::VecDeque;
use std::path::PathBuf;

use wp_agent_contract::{ActivityRollup, AgentEvent, BatchEnvelope, Heartbeat, ScreenshotMeta};

pub struct Outbox {
    agent_id: String,
    next_seq: u64,
    queue: VecDeque<BatchEnvelope>,
}

impl Outbox {
    pub fn new() -> Self {
        Outbox {
            agent_id: agent_id(),
            next_seq: 1,
            queue: VecDeque::new(),
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Assemble one capture cycle's envelope, assign the next `batch_seq`, persist it. Takes the
    /// **full cycle** — heartbeat + drained activity rollups + events + screenshot metadata
    /// (BUILD-PLAN §2). `captured_at` comes from the one server-offset clock (`crate::clock`).
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_cycle(
        &mut self,
        captured_at: i64,
        config_version: u64,
        heartbeat: Heartbeat,
        activity: Vec<ActivityRollup>,
        events: Vec<AgentEvent>,
        screenshots: Vec<ScreenshotMeta>,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queue.push_back(BatchEnvelope {
            agent_id: self.agent_id.clone(),
            batch_seq: seq,
            captured_at,
            config_version,
            heartbeat,
            activity,
            events,
            screenshots,
        });
        // M2: append the envelope to queue/batches.jsonl before returning (persist-before-send).
        seq
    }

    /// The oldest un-acked batch to send next.
    pub fn next_batch(&self) -> Option<&BatchEnvelope> {
        self.queue.front()
    }

    /// Drop every batch with `batch_seq <= watermark_seq` (durably accepted by the server).
    pub fn prune_to(&mut self, watermark_seq: u64) {
        while self
            .queue
            .front()
            .is_some_and(|b| b.batch_seq <= watermark_seq)
        {
            self.queue.pop_front();
        }
    }
}

impl Default for Outbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Local agent-state dir. M0: a dot-dir (gitignored), overridable for tests/dev. M2: Tauri's
/// per-user app-data dir.
fn state_dir() -> PathBuf {
    std::env::var_os("WP_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".agent-state"))
}

/// Stable per-install agent id. `WP_AGENT_ID` overrides for dev; otherwise a v4 UUID minted once and
/// persisted. **Never defaults to a shared constant** — that collided the live dev table. (M1 moves
/// the persisted id into the OS keyring, created at enrollment.)
fn agent_id() -> String {
    if let Ok(v) = std::env::var("WP_AGENT_ID") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    let path = state_dir().join("agent_id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let t = existing.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(&path, &id);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hb() -> Heartbeat {
        Heartbeat {
            hostname: "h".into(),
            os: "windows".into(),
            os_version: "11".into(),
            agent_version: "0.1.0".into(),
            ip: "127.0.0.1".into(),
            cpu_pct: 1.0,
            mem_pct: 1.0,
            outbox_mb: 0.0,
            idle: false,
        }
    }

    #[test]
    fn seqs_increment_and_prune_by_watermark() {
        // Pin the id so the test never touches disk or collides.
        std::env::set_var("WP_AGENT_ID", "test-agent");
        let mut ob = Outbox::new();
        assert_eq!(ob.enqueue_cycle(0, 0, hb(), vec![], vec![], vec![]), 1);
        assert_eq!(ob.enqueue_cycle(0, 0, hb(), vec![], vec![], vec![]), 2);
        assert_eq!(ob.enqueue_cycle(0, 0, hb(), vec![], vec![], vec![]), 3);
        ob.prune_to(2); // server accepted up to seq 2
        assert_eq!(ob.next_batch().unwrap().batch_seq, 3);
    }
}
