//! WorkPulse headless core service.
//!
//! Boots the outbox + config cache + uploader, supervises capture helpers, and runs the
//! combined-batch upload cycle. Everything the helpers produce funnels through the outbox
//! (the single egress point); nothing goes network-direct.

// Slices are wired into the supervisor loop incrementally; stub helpers are dead until then.
#![allow(dead_code)]

mod features;

use features::{config_sync::ConfigCache, heartbeat, outbox::Outbox, uploader::Uploader};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    tracing::info!("workpulse-agentd starting");

    let mut outbox = Outbox::new();
    let mut config = ConfigCache::default();
    let uploader = Uploader::from_env();

    // One tick = one capture cycle (real cadence comes from `config`). Demonstrates the flow:
    // collect heartbeat → (helpers would enqueue activity/events) → build batch → upload →
    // apply ack (prune outbox to watermark, pull config on version mismatch).
    let hb = heartbeat::collect();
    let seq = outbox.enqueue_cycle(hb, config.version());
    tracing::info!(batch_seq = seq, "queued first cycle");

    if let Some(batch) = outbox.next_batch() {
        match uploader.send(batch).await {
            Ok(ack) => {
                outbox.prune_to(ack.watermark_seq);
                if ack.config_version != config.version() {
                    // config_sync would ETag-pull the new CONFIG#TRACK and apply it live.
                    tracing::info!(server = ack.config_version, "config drift → would pull");
                }
                // uploader would then PUT any screenshot bytes to ack.upload_urls (S3-direct).
            }
            Err(e) => tracing::warn!("batch send failed (will retry from outbox): {e}"),
        }
    }

    let _ = &mut config; // config is applied live by config_sync in the real loop
    tracing::info!("workpulse-agentd ready");
}
