//! Offline-first outbox — the single egress buffer. Every batch is persisted **before** send and
//! pruned only on watermark ack, so retries and long offline replays are safe by construction.
//! `(agent_id, batch_seq)` is the idempotency key.
//!
//! M2: durable **append-only `queue/batches.jsonl`** (BUILD-PLAN §2). On start the queue **resumes**
//! from disk (so `batch_seq` stays gapless across a crash/kill and the server dedups replays); a
//! prune rewrites the file to the un-acked remainder. The stable per-install `agent_id` replaced the
//! old `"dev-agent"` default that collided every dev machine on the live table.

mod store;

use std::collections::VecDeque;
use std::path::PathBuf;

use wp_agent_contract::{ActivityRollup, AgentEvent, BatchEnvelope, Heartbeat, ScreenshotMeta};

use store::JsonlStore;

pub struct Outbox {
    agent_id: String,
    next_seq: u64,
    queue: VecDeque<BatchEnvelope>,
    store: JsonlStore,
}

impl Outbox {
    pub fn new() -> Self {
        Self::with_store(agent_id(), JsonlStore::new(outbox_path()))
    }

    /// Build an outbox over a specific store, **resuming** whatever is already persisted. `next_seq`
    /// continues past the highest seq ever assigned so ids never repeat across restarts.
    ///
    /// The high-water mark comes from the **sidecar**, not from the queue. Deriving it from
    /// `queue.back()` alone was a silent data-loss bug: a prune rewrites the queue to the un-acked
    /// remainder, so a clean restart (everything acked) found an empty file and restarted at 1.
    /// The server dedups `(agent_id, batch_seq)` and — deliberately — skips the SQS enqueue for a
    /// seq it has already recorded while still presigning the screenshot uploads. Net effect: the
    /// agent re-sent seq 1..n, got `200 OK` every time, uploaded image bytes to S3, and **none of
    /// it was ever folded**. No error surfaced on either side; the day's activity, screenshots and
    /// locations simply never appeared.
    ///
    /// `max` of the two so an install that predates the sidecar still resumes correctly from its
    /// queue instead of replaying seqs the server has already seen.
    pub fn with_store(agent_id: String, store: JsonlStore) -> Self {
        let queue: VecDeque<BatchEnvelope> = store.load().into();
        let from_queue = queue.back().map(|b| b.batch_seq).unwrap_or(0);
        let next_seq = from_queue.max(store.load_watermark()) + 1;
        Outbox {
            agent_id,
            next_seq,
            queue,
            store,
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// On-disk backlog size (bytes) → `Heartbeat.outbox_mb`.
    pub fn backlog_bytes(&self) -> u64 {
        self.store.size_bytes()
    }

    /// Assemble one capture cycle's envelope, assign the next `batch_seq`, **persist it before send**
    /// (BUILD-PLAN §2). Takes the full cycle — heartbeat + drained rollups/events/screenshot metas.
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
        // Bump the durable high-water mark before the batch is even persisted: burning a seq costs
        // nothing (the server dedups on it, gaps are fine), whereas reusing one silently discards a
        // whole cycle server-side. See `with_store`.
        if let Err(e) = self.store.save_watermark(seq) {
            tracing::error!("outbox: failed to persist seq watermark {seq}: {e}");
        }
        let env = BatchEnvelope {
            agent_id: self.agent_id.clone(),
            batch_seq: seq,
            captured_at,
            config_version,
            heartbeat,
            activity,
            events,
            screenshots,
        };
        if let Err(e) = self.store.append(&env) {
            tracing::error!("outbox: failed to persist batch {seq}: {e}");
        }
        self.queue.push_back(env);
        seq
    }

    /// The oldest un-acked batch to send next.
    pub fn next_batch(&self) -> Option<&BatchEnvelope> {
        self.queue.front()
    }

    /// Drop every un-acked batch on an **account switch**: they hold the previous user's captures and
    /// must never be sent under a different user's token. `next_seq` (and its durable watermark) is
    /// preserved, so the sequence never repeats and the server keeps deduping correctly.
    pub fn clear(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        self.queue.clear();
        if let Err(e) = self.store.rewrite(&self.queue) {
            tracing::error!("outbox: failed to clear queue on account switch: {e}");
        }
    }

    /// Drop every batch with `batch_seq <= watermark_seq` (durably accepted), then rewrite the file
    /// to the remainder.
    pub fn prune_to(&mut self, watermark_seq: u64) {
        let before = self.queue.len();
        while self
            .queue
            .front()
            .is_some_and(|b| b.batch_seq <= watermark_seq)
        {
            self.queue.pop_front();
        }
        if self.queue.len() != before {
            if let Err(e) = self.store.rewrite(&self.queue) {
                tracing::error!("outbox: failed to rewrite after prune: {e}");
            }
        }
    }
}

impl Default for Outbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Local agent-state dir — identity, session, screenshot spool, outbox queue and logs.
///
/// `pub(crate)` so `session_state` writes beside the outbox instead of re-deriving the path — two
/// copies of this rule would drift, and the symptom would be state silently saved to one directory
/// and read from another.
///
/// ## Why this is not just `.agent-state`
///
/// It used to be exactly that: a **CWD-relative** path. A process's working directory is not its
/// install directory, and nothing guarantees it is writable. Launched from the Windows `Run` key at
/// login — which is how "launch at login" works, and which carries no working directory — the agent
/// inherits `C:\Windows\System32`. Creating `.agent-state` there is refused, so the screenshot spool
/// could not be made, capture produced nothing, and the UI reported "screen capture failed" with no
/// explanation. The file log lands in the same directory, so **the log that would have named the
/// cause could not be written either**. Running the same build by double-clicking it worked, which
/// made it look like an intermittent capture fault rather than a path problem.
///
/// See [`resolve_state_dir`] for the order and why migration comes before correctness.
pub(crate) fn state_dir() -> PathBuf {
    let exe_legacy = exe_dir().map(|d| d.join(LEGACY_DIR));
    let cwd_legacy = PathBuf::from(LEGACY_DIR);
    resolve_state_dir(
        std::env::var_os("WP_STATE_DIR").map(PathBuf::from),
        exe_legacy.filter(|p| p.is_dir()),
        Some(cwd_legacy).filter(|p| p.is_dir()),
        app_data_dir(),
        exe_dir().map(|d| d.join(LEGACY_DIR)),
    )
}

const LEGACY_DIR: &str = ".agent-state";

/// The directory the resolution rules pick from, given the facts. Pure, so the precedence is
/// testable without touching the filesystem or the environment.
///
/// Order, and the reasoning:
/// 1. **`WP_STATE_DIR`** — an explicit instruction always wins.
/// 2. **An existing `.agent-state` beside the executable** — an install that already has one keeps
///    it. This is migration, and it comes first for a reason: the directory holds `agent_id`, so
///    moving an existing install to a new path would enrol it as a *second* device and strand the
///    old one in the fleet, plus abandon any queued batches that had not uploaded.
/// 3. **An existing `.agent-state` in the working directory** — the same courtesy for developer
///    checkouts, which have accumulated one from `cargo run`.
/// 4. **The per-user application-data directory** — where a fresh install belongs, and the whole
///    point of the change: writable regardless of how the process was launched.
/// 5. **Beside the executable** — only if the platform gave us no app-data path at all.
fn resolve_state_dir(
    env_override: Option<PathBuf>,
    exe_legacy: Option<PathBuf>,
    cwd_legacy: Option<PathBuf>,
    app_data: Option<PathBuf>,
    exe_fallback: Option<PathBuf>,
) -> PathBuf {
    env_override
        .or(exe_legacy)
        .or(cwd_legacy)
        .or(app_data)
        .or(exe_fallback)
        // Only reachable if the OS reports no executable path and no home — keep the old behaviour
        // rather than panicking, since losing the agent is worse than losing the ideal location.
        .unwrap_or_else(|| PathBuf::from(LEGACY_DIR))
}

/// The directory the running executable lives in — **not** the working directory, which is the whole
/// bug this module now guards against.
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(std::path::Path::to_path_buf)
}

/// Per-user application data, by platform convention. Derived from the environment rather than a
/// crate: it is three lookups, and the agent already avoids dependencies it does not need.
fn app_data_dir() -> Option<PathBuf> {
    const APP: &str = "com.workpulse.agent";

    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join(APP).join("state"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library/Application Support")
                .join(APP)
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|b| b.join(APP))
    }
}

fn outbox_path() -> PathBuf {
    state_dir().join("queue").join("batches.jsonl")
}

/// Stable per-install agent id. `WP_AGENT_ID` overrides for dev; otherwise a v4 UUID minted once and
/// persisted. **Never a shared constant** — that collided the live dev table. (M1 moves the persisted
/// id into the OS keyring, created at enrollment.)
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
    use std::sync::atomic::{AtomicU64, Ordering};

    fn p(s: &str) -> Option<PathBuf> {
        Some(PathBuf::from(s))
    }

    /// The regression. Launched from the Windows `Run` key at login the process inherits
    /// `C:\Windows\System32` as its working directory, so a CWD-relative state dir could not be
    /// created: no spool, no capture, and no log to say why. Resolution must never depend on CWD
    /// unless a state dir is genuinely already there.
    #[test]
    fn a_fresh_install_uses_app_data_not_the_working_directory() {
        let got = resolve_state_dir(
            None,
            None,
            None,
            p("/appdata/wp"),
            p("/install/.agent-state"),
        );
        assert_eq!(got, PathBuf::from("/appdata/wp"));
    }

    /// Migration beats correctness: the directory holds `agent_id`, so relocating an existing
    /// install would enrol it as a second device and abandon any queued batches.
    #[test]
    fn an_existing_install_keeps_its_directory() {
        let got = resolve_state_dir(
            None,
            p("/install/.agent-state"),
            None,
            p("/appdata/wp"),
            p("/install/.agent-state"),
        );
        assert_eq!(got, PathBuf::from("/install/.agent-state"));
    }

    /// A developer checkout that already has one from `cargo run` keeps working unchanged.
    #[test]
    fn an_existing_checkout_dir_is_preferred_over_app_data() {
        let got = resolve_state_dir(None, None, p(".agent-state"), p("/appdata/wp"), None);
        assert_eq!(got, PathBuf::from(".agent-state"));
    }

    #[test]
    fn an_explicit_override_wins_over_everything() {
        let got = resolve_state_dir(
            p("/tmp/forced"),
            p("/install/.agent-state"),
            p(".agent-state"),
            p("/appdata/wp"),
            p("/install/.agent-state"),
        );
        assert_eq!(got, PathBuf::from("/tmp/forced"));
    }

    /// No app-data path (no HOME/LOCALAPPDATA) falls back beside the executable — still not CWD.
    #[test]
    fn without_app_data_it_falls_back_beside_the_executable() {
        let got = resolve_state_dir(None, None, None, None, p("/install/.agent-state"));
        assert_eq!(got, PathBuf::from("/install/.agent-state"));
    }

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
            location: None,
            active_session: None,
            reports_timer_state: true,
        }
    }

    /// A unique temp path per test invocation (no rand/time crate: pid + a process-wide counter).
    fn temp_store() -> (JsonlStore, PathBuf) {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wp-outbox-{}-{n}", std::process::id()));
        let path = dir.join("batches.jsonl");
        (JsonlStore::new(path.clone()), path)
    }

    fn enqueue(ob: &mut Outbox) -> u64 {
        ob.enqueue_cycle(0, 0, hb(), vec![], vec![], vec![])
    }

    #[test]
    fn seqs_increment_and_prune_by_watermark() {
        let (store, _p) = temp_store();
        let mut ob = Outbox::with_store("test-agent".into(), store);
        assert_eq!(enqueue(&mut ob), 1);
        assert_eq!(enqueue(&mut ob), 2);
        assert_eq!(enqueue(&mut ob), 3);
        ob.prune_to(2); // server accepted up to seq 2
        assert_eq!(ob.next_batch().unwrap().batch_seq, 3);
    }

    /// The regression that cost a day of real data (2026-07-21).
    ///
    /// Fully-acked outbox → prune empties the queue file → restart. Deriving `next_seq` from the
    /// queue alone restarted at 1, and the server dedups `(agent_id, batch_seq)`: it skips the SQS
    /// enqueue for a seq it already holds **while still presigning the screenshot uploads**. So the
    /// agent got `200 OK`, uploaded bytes to S3, pruned — and nothing was ever folded. Silent, on
    /// both sides. A seq must never be reused.
    #[test]
    fn seq_never_restarts_after_a_full_prune() {
        let (store, path) = temp_store();
        let mut ob = Outbox::with_store("test-agent".into(), store);
        enqueue(&mut ob);
        enqueue(&mut ob);
        let last = enqueue(&mut ob); // 1,2,3
        ob.prune_to(last); // server acked everything → queue file is now empty

        let mut resumed = Outbox::with_store("test-agent".into(), JsonlStore::new(path));
        assert_eq!(
            enqueue(&mut resumed),
            4,
            "seq restarted after a full prune — the server would silently drop this batch"
        );
    }

    #[test]
    fn persists_and_resumes_gapless_across_restart() {
        let (store, path) = temp_store();
        let mut ob = Outbox::with_store("test-agent".into(), store);
        enqueue(&mut ob);
        enqueue(&mut ob);
        enqueue(&mut ob); // seqs 1,2,3 on disk

        // "Restart": a fresh outbox over the SAME file resumes the queue and continues the seq.
        let mut resumed = Outbox::with_store("test-agent".into(), JsonlStore::new(path.clone()));
        assert_eq!(resumed.next_batch().unwrap().batch_seq, 1);
        assert_eq!(enqueue(&mut resumed), 4, "seq must not repeat after resume");

        // Prune rewrites the file to the remainder; a second resume sees only what's left.
        resumed.prune_to(3);
        let again = Outbox::with_store("test-agent".into(), JsonlStore::new(path));
        assert_eq!(again.next_batch().unwrap().batch_seq, 4);
    }
}
