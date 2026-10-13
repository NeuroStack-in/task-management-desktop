//! Append-only JSONL persistence for the outbox — one `BatchEnvelope` per line (BUILD-PLAN §2:
//! `VecDeque` → `queue/batches.jsonl`, **not** SQLCipher). Persist-before-send: a batch is on disk
//! before it is ever sent, so a crash/kill/offline stretch replays it on next start; a prune rewrites
//! the file to exactly the un-acked remainder.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use wp_agent_contract::BatchEnvelope;

/// Replace `path` with `bytes` **atomically**: write a sibling temp file, flush it to disk, then
/// rename it over the target. A crash or power cut leaves either the old file or the new one —
/// never a truncated mix.
///
/// The queue rewrite (every prune) and the `.seq` sidecar used to truncate in place, so a power cut
/// mid-prune emptied the queue, and a torn `.seq` read back as 0 and restarted seqs at 1 — which the
/// server de-duplicates away: the very 2026-07-21 loss the sidecar exists to prevent (QA audit
/// 2026-10-08, WP-445). Shared with `session_state`, whose file had the same flaw.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub struct JsonlStore {
    path: PathBuf,
}

impl JsonlStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Sidecar holding the highest `batch_seq` ever assigned.
    ///
    /// The queue file cannot answer this: a prune rewrites it to the *un-acked remainder*, so once
    /// the server has acked everything the file is empty and the highest-seq-ever is gone with it.
    /// The counter therefore lives beside it and is never truncated.
    fn seq_path(&self) -> PathBuf {
        self.path.with_extension("seq")
    }

    /// Highest `batch_seq` ever assigned, or 0 when unknown (absent/corrupt — never fatal).
    pub fn load_watermark(&self) -> u64 {
        fs::read_to_string(self.seq_path())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Record the highest assigned seq. Best-effort: a failure here costs a duplicate seq after a
    /// restart, which is exactly the bug this guards, so it is logged loudly by the caller.
    pub fn save_watermark(&self, seq: u64) -> io::Result<()> {
        write_atomic(&self.seq_path(), seq.to_string().as_bytes())
    }

    /// Read every persisted envelope (empty if the file is absent or a line is corrupt — a corrupt
    /// tail line is skipped, never fatal).
    pub fn load(&self) -> Vec<BatchEnvelope> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect()
    }

    /// Append one envelope (persist-before-send).
    pub fn append(&self, env: &BatchEnvelope) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(env).map_err(io::Error::other)?;
        writeln!(file, "{line}")
    }

    /// Rewrite the file to exactly `remaining` (called after a prune drops acked batches) —
    /// atomically, see [`write_atomic`].
    pub fn rewrite(&self, remaining: &VecDeque<BatchEnvelope>) -> io::Result<()> {
        let mut out = Vec::new();
        for env in remaining {
            let line = serde_json::to_string(env).map_err(io::Error::other)?;
            writeln!(out, "{line}")?;
        }
        write_atomic(&self.path, &out)
    }

    /// On-disk backlog size in bytes (feeds `Heartbeat.outbox_mb`).
    pub fn size_bytes(&self) -> u64 {
        fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }
}
