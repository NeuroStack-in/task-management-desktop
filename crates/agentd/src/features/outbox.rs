//! Offline-first outbox — the single egress buffer. Every batch is persisted **before** send
//! and pruned only on watermark ack, so retries and 72h-offline replays are safe by
//! construction. `(agent_id, batch_seq)` is the idempotency key.
//!
//! Skeleton: in-memory `VecDeque`. Production: SQLCipher (`rusqlite` + sqlcipher), ≥72h /
//! ~1GB cap with oldest-first eviction, resumable multipart via saved part ETags.

use agent_shared::contract::{BatchEnvelope, Heartbeat};
use std::collections::VecDeque;

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

    /// Assemble one capture cycle's envelope, assign the next `batch_seq`, persist it.
    /// (Activity/events/screenshot-meta are appended from the helper stream in the real loop.)
    pub fn enqueue_cycle(&mut self, heartbeat: Heartbeat, config_version: u64) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queue.push_back(BatchEnvelope {
            agent_id: self.agent_id.clone(),
            batch_seq: seq,
            captured_at: 0, // real loop stamps a monotonic epoch-ms
            config_version,
            heartbeat,
            activity: Vec::new(),
            events: Vec::new(),
            screenshots: Vec::new(),
        });
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

/// Stable per-install agent id. Production: created at enrollment, stored in the OS keychain.
fn agent_id() -> String {
    std::env::var("WP_AGENT_ID").unwrap_or_else(|_| "dev-agent".to_string())
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
        }
    }

    #[test]
    fn seqs_increment_and_prune_by_watermark() {
        let mut ob = Outbox::new();
        assert_eq!(ob.enqueue_cycle(hb(), 0), 1);
        assert_eq!(ob.enqueue_cycle(hb(), 0), 2);
        assert_eq!(ob.enqueue_cycle(hb(), 0), 3);
        ob.prune_to(2); // server accepted up to seq 2
        assert_eq!(ob.next_batch().unwrap().batch_seq, 3);
    }
}
