//! Screenshot capture — timer-gated, jittered (`Cadence::interval_secs()` ± `SCREENSHOT_JITTER_SECS`).
//! M5: `xcap` → downscale to 768px → **lossy WebP** (the `webp` crate; `image` 0.25's WebP is
//! lossless-only) → `image::imageops::blur` by `blur_level` → pHash (`image_hasher`). The `screenshots`
//! flag **fails closed** (BUILD-PLAN §4). Bytes go S3-direct via the ack's `upload_urls`; only
//! `ScreenshotMeta` rides the batch.
//!
//! macOS needs a Screen-Recording (TCC) grant; a denial must surface a "grant permission" state, not
//! silence (risk #5). `Cadence::Off` → no capture thread at all.

use wp_agent_contract::ScreenshotMeta;

/// Capture one screenshot for `app`, returning its metadata + the temp file path of the encoded
/// bytes. `None` until M5 (and when a required OS grant is denied).
pub fn capture(_app: &str, _blur_level: u8) -> Option<(ScreenshotMeta, String)> {
    None // TODO(M5): xcap → WebP + blur + pHash
}
