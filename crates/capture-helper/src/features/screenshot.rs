//! The ONLY slice that touches pixels. Captures all displays at the configured cadence,
//! downscales to ~768px WebP, applies the admin blur level, and perceptual-hash-dedups
//! near-identical consecutive frames (keep one + duration). Emits `ScreenshotMeta` + a temp path;
//! the bytes go S3-direct via the core (never held downstream as a full-res frame).
//!
//! Skeleton stub; production uses `xcap` + `image`/`webp` + a pHash.

use agent_shared::contract::ScreenshotMeta;

/// Capture one frame for `app`, returning its metadata + a temp file path for the WebP bytes.
/// Returns `None` when the frame is a pHash duplicate of the previous one (deduped at the edge).
pub fn capture(app: &str, blur_level: u8) -> Option<(ScreenshotMeta, String)> {
    // TODO(xcap): grab displays → downscale 768px WebP → blur → pHash; skip if dup.
    let _ = (app, blur_level);
    None
}
