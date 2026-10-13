//! Screenshots captured but not yet uploaded — kept **on disk**, not only in memory.
//!
//! The pending set used to live only in `AppState::screenshots`, a `HashMap`, while the `.webp`
//! files and the metas inside persisted outbox batches survived a restart. After any restart (tray
//! Quit, Windows sign-out, an auto-update) the replayed batch got presigned URLs for frames the map
//! no longer knew, so they were skipped: never uploaded, never deleted, and the review grid showed a
//! row pointing at an S3 object that never arrived (QA audit 2026-10-08, WP-060).
//!
//! Now every tracked frame has a sidecar `<id>.json` (its meta and failed-attempt count) beside its
//! `<id>.webp`. [`restore`] rebuilds the set at startup; a `.webp` with no sidecar can never be
//! declared, so it is deleted rather than left on disk. And a frame that has not uploaded within
//! [`MAX_PENDING_AGE_MS`] is dropped — the attempt cap only counted failed PUTs, so a frame that was
//! never presigned (no entitlement, no bucket) used to be re-declared forever.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wp_agent_contract::ScreenshotMeta;

use crate::cycle::SCREENSHOT_ATTEMPT_CAP;
use crate::state::{AppState, PendingShot};

/// A frame still pending after this long is dropped: the server forgets a batch after 72 h, and a
/// frame that has not uploaded in that time is not going to.
pub const MAX_PENDING_AGE_MS: i64 = 72 * 3600 * 1000;

#[derive(Serialize, Deserialize)]
struct Sidecar {
    meta: ScreenshotMeta,
    #[serde(default)]
    attempts: u8,
}

fn sidecar_path(webp: &Path) -> PathBuf {
    webp.with_extension("json")
}

fn write_sidecar(webp: &Path, meta: &ScreenshotMeta, attempts: u8) {
    let body = Sidecar {
        meta: meta.clone(),
        attempts,
    };
    match serde_json::to_vec(&body) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(sidecar_path(webp), bytes) {
                tracing::warn!(id = %meta.id, error = %e, "screenshot sidecar not written; the frame won't survive a restart");
            }
        }
        Err(e) => tracing::warn!(id = %meta.id, error = %e, "screenshot sidecar not encoded"),
    }
}

fn remove_files(webp: &Path) {
    let _ = std::fs::remove_file(webp);
    let _ = std::fs::remove_file(sidecar_path(webp));
}

/// Start tracking a captured frame: in memory, and as a sidecar beside its file.
pub fn track(state: &AppState, meta: ScreenshotMeta, path: PathBuf) {
    write_sidecar(&path, &meta, 0);
    state.screenshots.lock().unwrap().insert(
        meta.id.clone(),
        PendingShot {
            meta,
            path,
            attempts: 0,
            declared_seq: None,
        },
    );
}

/// Track a frame the screenshot loop just finished — **if** nothing wiped the per-user state since
/// the capture began (`epoch`, read before the grab). Otherwise the frame belongs to whoever was
/// signed in before the wipe: delete it, never hand it to the next user's queue. Returns whether it
/// was kept.
///
/// The check and the insert share the `screenshots` lock that `discard_all` takes after the reset
/// bumps the epoch, so a frame either lands before the wipe (and is discarded with the rest) or sees
/// the new epoch and is deleted here (QA audit 2026-10-08, WP-443).
pub fn track_if_current(state: &AppState, epoch: u64, meta: ScreenshotMeta, path: PathBuf) -> bool {
    let mut shots = state.screenshots.lock().unwrap();
    if state.epoch() != epoch {
        drop(shots);
        remove_files(&path);
        return false;
    }
    write_sidecar(&path, &meta, 0);
    shots.insert(
        meta.id.clone(),
        PendingShot {
            meta,
            path,
            attempts: 0,
            declared_seq: None,
        },
    );
    true
}

/// Forget a frame and delete its files — it uploaded, or must not.
pub fn discard(state: &AppState, id: &str) {
    if let Some(s) = state.screenshots.lock().unwrap().remove(id) {
        remove_files(&s.path);
    }
}

/// Forget every pending frame and delete the files (consent withdrawn, account switch).
pub fn discard_all(state: &AppState) {
    let drained: Vec<PendingShot> = state
        .screenshots
        .lock()
        .unwrap()
        .drain()
        .map(|(_, s)| s)
        .collect();
    for s in drained {
        remove_files(&s.path);
    }
}

/// Count a failed upload; drop the frame once it reaches the cap. The count is persisted, so the cap
/// holds across restarts too.
pub fn failed_upload(state: &AppState, id: &str) {
    let mut shots = state.screenshots.lock().unwrap();
    let Some(s) = shots.get_mut(id) else {
        return;
    };
    s.attempts = s.attempts.saturating_add(1);
    if s.attempts >= SCREENSHOT_ATTEMPT_CAP {
        let path = s.path.clone();
        shots.remove(id);
        drop(shots);
        remove_files(&path);
    } else {
        write_sidecar(&s.path, &s.meta, s.attempts);
    }
}

/// Drop frames older than [`MAX_PENDING_AGE_MS`], files and all. Called before each declaration.
pub fn expire(state: &AppState, now_ms: i64) {
    let stale: Vec<String> = state
        .screenshots
        .lock()
        .unwrap()
        .values()
        .filter(|s| is_stale(s.meta.captured_at, now_ms))
        .map(|s| s.meta.id.clone())
        .collect();
    for id in stale {
        tracing::info!(%id, "screenshot never uploaded within 72 h; dropping it");
        discard(state, &id);
    }
}

fn is_stale(captured_at: i64, now_ms: i64) -> bool {
    now_ms - captured_at > MAX_PENDING_AGE_MS
}

/// Rebuild the pending set from the spool at startup. Returns how many frames were restored.
///
/// Run before the capture and sender threads start, so nothing is mid-write. A frame is restored only
/// when both files exist and it is young enough; everything else in the spool is deleted — a `.webp`
/// with no sidecar (written by an older build, or orphaned by a crash between the two writes) has no
/// meta to declare it with, and so could never leave the machine except as clutter.
pub fn restore(state: &AppState, dir: &Path, now_ms: i64) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut restored = 0usize;
    let mut webps: Vec<PathBuf> = Vec::new();
    let mut sidecars: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("webp") => webps.push(path),
            Some("json") => sidecars.push(path),
            _ => {}
        }
    }
    let mut shots = state.screenshots.lock().unwrap();
    for side in sidecars {
        let webp = side.with_extension("webp");
        let parsed: Option<Sidecar> = std::fs::read(&side)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        match parsed {
            Some(sc)
                if webp.exists()
                    && !is_stale(sc.meta.captured_at, now_ms)
                    && sc.attempts < SCREENSHOT_ATTEMPT_CAP =>
            {
                shots.insert(
                    sc.meta.id.clone(),
                    PendingShot {
                        meta: sc.meta,
                        path: webp,
                        attempts: sc.attempts,
                        declared_seq: None,
                    },
                );
                restored += 1;
            }
            _ => remove_files(&webp),
        }
    }
    for webp in webps {
        if !sidecar_path(&webp).exists() {
            let _ = std::fs::remove_file(&webp);
        }
    }
    restored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, captured_at: i64) -> ScreenshotMeta {
        ScreenshotMeta {
            id: id.into(),
            captured_at,
            app: "code.exe".into(),
            phash: "0".into(),
            blur_level: 0,
            bucket_minute: captured_at / 60_000,
            display: 0,
            content_sha256: "x".into(),
        }
    }

    fn spool() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wp-pending-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// WP-060: a tracked frame comes back after a restart; an orphan and an expired frame do not.
    #[test]
    fn a_restart_restores_pending_frames_and_clears_the_rest() {
        let dir = spool();
        let now = 1_800_000_000_000;
        let before = AppState::new();
        let kept = dir.join("kept.webp");
        std::fs::write(&kept, b"bytes").unwrap();
        track(&before, meta("kept", now - 60_000), kept.clone());
        let old = dir.join("old.webp");
        std::fs::write(&old, b"bytes").unwrap();
        track(
            &before,
            meta("old", now - MAX_PENDING_AGE_MS - 1),
            old.clone(),
        );
        let orphan = dir.join("orphan.webp");
        std::fs::write(&orphan, b"bytes").unwrap();

        let after = AppState::new();
        assert_eq!(restore(&after, &dir, now), 1);
        assert!(after.screenshots.lock().unwrap().contains_key("kept"));
        assert!(kept.exists());
        assert!(
            !old.exists() && !sidecar_path(&old).exists(),
            "expired frame removed"
        );
        assert!(
            !orphan.exists(),
            "a frame with no sidecar can never be declared"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_attempt_count_survives_a_restart_and_the_cap_still_drops() {
        let dir = spool();
        let now = 1_800_000_000_000;
        let state = AppState::new();
        let p = dir.join("s.webp");
        std::fs::write(&p, b"bytes").unwrap();
        track(&state, meta("s", now), p.clone());
        failed_upload(&state, "s");
        let after = AppState::new();
        restore(&after, &dir, now);
        assert_eq!(after.screenshots.lock().unwrap()["s"].attempts, 1);
        for _ in 1..SCREENSHOT_ATTEMPT_CAP {
            failed_upload(&after, "s");
        }
        assert!(!after.screenshots.lock().unwrap().contains_key("s"));
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// WP-443: a frame finished after a sign-out wiped the queues is deleted, not queued for the next
    /// user; one finished with nothing in between is kept.
    #[test]
    fn a_frame_finished_after_a_wipe_is_deleted() {
        let dir = spool();
        let state = AppState::new();
        let epoch = state.epoch();
        let kept = dir.join("kept.webp");
        std::fs::write(&kept, b"bytes").unwrap();
        assert!(track_if_current(
            &state,
            epoch,
            meta("kept", 1),
            kept.clone()
        ));

        let late = dir.join("late.webp");
        std::fs::write(&late, b"bytes").unwrap();
        // What `reset_for_account_switch` does first (called directly, it would also clear this
        // machine's real queue and session file).
        state
            .capture_epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(!track_if_current(
            &state,
            epoch,
            meta("late", 1),
            late.clone()
        ));
        assert!(!late.exists() && !sidecar_path(&late).exists());
        assert_eq!(state.screenshots.lock().unwrap().len(), 1, "only the first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frames_older_than_three_days_expire() {
        assert!(!is_stale(1_000, 1_000 + MAX_PENDING_AGE_MS));
        assert!(is_stale(1_000, 1_000 + MAX_PENDING_AGE_MS + 1));
    }
}
