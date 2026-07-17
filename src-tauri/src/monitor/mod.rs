//! Activity monitor — **timer-gated** (BUILD-PLAN §4). Kept FREE of Tauri types so a headless daemon
//! can be split back out later without a rewrite (§0).
//!
//! M0 ships the privacy-invariant input counters (`input`) with its test, plus the module seams. M4
//! wires the dedicated 1 s OS thread: `device_query` (cumulative kb/mouse deltas, uint32-wrap +
//! `SPIKE_CAP`), `user-idle` (idle secs), and every 5th tick `x-win` (foreground app/title/url →
//! `rules::classify` → `AppSpan`), folded into per-minute `bucket`s. `reflect()` starts/stops that
//! thread idempotently on `TimerEngine::is_running()`.

pub mod active_window;
pub mod bucket;
pub mod idle;
pub mod input;
pub mod screenshot;
pub mod session;
