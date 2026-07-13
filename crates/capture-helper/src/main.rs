//! Per-user capture helper.
//!
//! Runs the capture tasks (screenshot, input counts, focus, idle) and folds them into
//! per-minute rollups + events, handed to the core's outbox over IPC. This process has **no HTTP
//! capability** — the core's `uploader` is the only egress point (structural one-call-per-cycle).

// Slices are wired into the capture loop incrementally; stub helpers are dead until then.
#![allow(dead_code)]

mod features;

use features::input_counts::InputCounters;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    tracing::info!("workpulse-capture-helper starting");

    // Counts-not-keys: the counters only ever increment; there is no code path that receives a
    // key value. (Real capture registers rdev/xcap/active-win callbacks that feed these.)
    let mut counters = InputCounters::default();
    counters.on_keystroke();
    counters.on_mouse();
    tracing::info!(
        keys = counters.keystrokes(),
        mouse = counters.mouse(),
        "captured (counts only)"
    );

    // TODO: spawn the capture tokio tasks, fold into ActivityRollup per minute, send over IPC.
    tracing::info!("workpulse-capture-helper ready");
}
