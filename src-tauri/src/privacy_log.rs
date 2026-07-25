//! The **employee-visible** local privacy log — "what happened on my machine, and when".
//!
//! PRIVACY.md §5 promises a transparency view and "no silent access, everything leaves a trail".
//! Until now the agent had nowhere on the *employee's* side to leave that trail: today's sessions
//! come back from the server, and every other signal (violations, idle prompts) was an ephemeral
//! banner. That was survivable while every capture was periodic and disclosed up front — it is not
//! survivable for the **admin-triggered on-demand capture** (`mqtt::capture`), where a frame of this
//! person's screen is taken because someone else asked for it. A covert version of that feature is
//! not a feature we are willing to ship, so it writes here, and the panel reads it back.
//!
//! Shape: newline-delimited JSON beside the outbox (`state_dir()/privacy-log.jsonl`), capped at
//! [`MAX_ENTRIES`]. Not a secret and not tamper-proof — it is a *disclosure* surface, and the
//! server-side audit trail (`monitoring.capture_now.*`) is the authoritative record. A missing,
//! unreadable or corrupt file degrades to "no entries", never to an error: a damaged log must not
//! stop the agent, and it must not stop a capture being refused.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Entries kept on disk. Small on purpose: this is a recent-activity view for a person, not an
/// archive. Oldest entries fall off the front.
const MAX_ENTRIES: usize = 200;

/// An admin asked for an on-demand screenshot and the agent took it.
pub const KIND_ADMIN_CAPTURE: &str = "admin_capture";
/// An admin asked for an on-demand screenshot and the agent **refused** (the guard chain). Recorded
/// as loudly as a success: the employee is entitled to know it was attempted.
pub const KIND_ADMIN_CAPTURE_REFUSED: &str = "admin_capture_refused";

/// One line of the log. `detail` is a short human sentence for the panel — never window titles,
/// URLs or anything else PRIVACY.md §4 keeps out of logs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalEvent {
    /// Epoch ms (`clock::now_epoch_ms`).
    pub ts: i64,
    pub kind: String,
    pub detail: String,
}

fn path() -> PathBuf {
    crate::outbox::state_dir().join("privacy-log.jsonl")
}

/// Append one entry. Best-effort but **logged** — a silent failure here would turn the transparency
/// promise into a lie that only shows up long after the fact.
pub fn record(kind: &str, detail: impl Into<String>) {
    let ev = LocalEvent {
        ts: crate::clock::now_epoch_ms(),
        kind: kind.to_string(),
        detail: detail.into(),
    };
    if let Err(e) = append_at(&path(), ev) {
        tracing::warn!("could not write the local privacy log ({e})");
    }
}

/// The most recent entries, **newest first**, at most `limit`.
pub fn recent(limit: usize) -> Vec<LocalEvent> {
    let mut all = read_at(&path());
    all.reverse();
    all.truncate(limit);
    all
}

/// Drop the whole log (account switch — see `AppState::reset_for_account_switch`).
pub fn clear() {
    let _ = std::fs::remove_file(path());
}

/// Read every well-formed line, oldest first. Corrupt lines are skipped rather than failing the
/// read: one bad write must not hide every entry around it.
fn read_at(p: &Path) -> Vec<LocalEvent> {
    let Ok(text) = std::fs::read_to_string(p) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LocalEvent>(l).ok())
        .collect()
}

/// Append + cap. Rewrites the whole file once the cap is exceeded — the entries are tiny and bounded
/// by [`MAX_ENTRIES`], so a rewrite is cheaper than the bookkeeping a rolling file would need.
fn append_at(p: &Path, ev: LocalEvent) -> std::io::Result<()> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut all = read_at(p);
    all.push(ev);
    if all.len() > MAX_ENTRIES {
        all.drain(..all.len() - MAX_ENTRIES);
    }
    let mut out = String::new();
    for e in &all {
        // Serialization of a struct of owned scalars cannot fail; skip rather than panic if it ever does.
        if let Ok(line) = serde_json::to_string(e) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    std::fs::write(p, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("wp-privacy-log-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        p.join("privacy-log.jsonl")
    }

    fn ev(ts: i64, detail: &str) -> LocalEvent {
        LocalEvent {
            ts,
            kind: KIND_ADMIN_CAPTURE.to_string(),
            detail: detail.to_string(),
        }
    }

    #[test]
    fn appends_and_reads_back_in_order() {
        let p = tmp("append");
        append_at(&p, ev(1, "first")).unwrap();
        append_at(&p, ev(2, "second")).unwrap();
        let all = read_at(&p);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].detail, "first");
        assert_eq!(all[1].ts, 2);
    }

    #[test]
    fn caps_at_max_entries_dropping_the_oldest() {
        let p = tmp("cap");
        for i in 0..(MAX_ENTRIES as i64 + 5) {
            append_at(&p, ev(i, "x")).unwrap();
        }
        let all = read_at(&p);
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0].ts, 5, "the oldest five fell off the front");
    }

    /// A corrupt line must not hide the entries around it — the log degrades, never errors.
    #[test]
    fn corrupt_lines_are_skipped_not_fatal() {
        let p = tmp("corrupt");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            "{not json\n{\"ts\":7,\"kind\":\"admin_capture\",\"detail\":\"ok\"}\n\n",
        )
        .unwrap();
        let all = read_at(&p);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].ts, 7);
    }

    #[test]
    fn a_missing_log_is_empty_not_an_error() {
        assert!(read_at(&tmp("missing")).is_empty());
    }
}
