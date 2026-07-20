//! Screenshot capture — timer-gated, jittered, **consent-gated, fails-closed** (BUILD-PLAN §4/§5).
//! Pipeline (per display): `xcap` grab → downscale to ≤768px → optional blur (`blur_level → sigma`) →
//! **lossy WebP** (the `webp` crate; `image` 0.25's WebP encoder is lossless-only) → pHash
//! (`image_hasher`). Bytes go to a temp file and upload **S3-direct** via the ack's presigned
//! `upload_urls`; only the `ScreenshotMeta` rides the batch.
//!
//! **Every connected display is captured** — a dual-monitor machine yields one shot per monitor, all
//! sharing the same `captured_at`/`bucket_minute` so they ride the SAME batch and map to the same
//! activity minute (each is a distinct row: unique `id` + its own pHash). macOS needs a
//! Screen-Recording (TCC) grant; a denial yields an empty result and the caller surfaces a
//! "grant permission" state rather than silently collecting nothing (risk #5).

use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use wp_agent_contract::ScreenshotMeta;

/// Max WebP width; taller shots keep aspect.
const MAX_WIDTH: u32 = 768;
/// Lossy WebP quality (0–100).
const WEBP_QUALITY: f32 = 75.0;

/// Map a blur level to a Gaussian sigma. 0 = no blur.
fn blur_sigma(blur_level: u8) -> f32 {
    blur_level as f32 * 4.0
}

/// Downscale dims so width ≤ `MAX_WIDTH`, preserving aspect.
fn scaled_dims(w: u32, h: u32) -> (u32, u32) {
    if w <= MAX_WIDTH || w == 0 {
        (w, h)
    } else {
        let nh = ((h as u64 * MAX_WIDTH as u64) / w as u64) as u32;
        (MAX_WIDTH, nh.max(1))
    }
}

/// Host-pin: only ever PUT screenshot bytes to an **https `*.amazonaws.com`** URL (BUILD-PLAN §5) —
/// a presigned URL that isn't S3 is a redirect to somewhere it shouldn't go.
pub fn is_allowed_upload_host(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split(['/', '?'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    host == "amazonaws.com" || host.ends_with(".amazonaws.com")
}

/// Capture **every** connected display, process each to WebP, write temp files. Returns one
/// `(ScreenshotMeta, PathBuf)` per successfully-captured monitor (dual-monitor → two entries), all
/// sharing `captured_at`/`bucket_minute`. Empty when nothing could be captured (no monitor, macOS
/// grant denied, encode/IO error) — the caller surfaces the "grant permission" state on empty.
///
/// One monitor failing to capture doesn't sink the others: each is processed independently and only
/// its own `None` is dropped.
pub fn capture_all(app: &str, blur_level: u8, captured_at: i64) -> Vec<(ScreenshotMeta, PathBuf)> {
    let Ok(mut monitors) = xcap::Monitor::all() else {
        return Vec::new();
    };
    // Primary display first, so it is always `display = 0` ("Monitor 1"); the index is then the
    // stable physical position (a monitor that fails to capture leaves a gap rather than renumbering).
    monitors.sort_by_key(|m| !m.is_primary().unwrap_or(false));
    let dir = screenshots_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }
    monitors
        .into_iter()
        .enumerate()
        .filter_map(|(i, m)| process_monitor(&m, app, blur_level, captured_at, i as u8, &dir))
        .collect()
}

/// Grab + process one display. `None` on capture/encode/IO failure for this monitor alone.
fn process_monitor(
    monitor: &xcap::Monitor,
    app: &str,
    blur_level: u8,
    captured_at: i64,
    display: u8,
    dir: &Path,
) -> Option<(ScreenshotMeta, PathBuf)> {
    let rgba = monitor.capture_image().ok()?;

    let mut img = image::DynamicImage::ImageRgba8(rgba);
    let (w, h) = scaled_dims(img.width(), img.height());
    if (w, h) != (img.width(), img.height()) {
        img = img.resize_exact(w, h, FilterType::Triangle);
    }
    if blur_level > 0 {
        img = img.blur(blur_sigma(blur_level));
    }

    // pHash on the processed image (dedup accounting server-side).
    let phash = image_hasher::HasherConfig::new()
        .to_hasher()
        .hash_image(&img)
        .to_base64();

    // Lossy WebP encode.
    let rgba = img.to_rgba8();
    let webp_bytes =
        webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height()).encode(WEBP_QUALITY);

    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("{id}.webp"));
    std::fs::write(&path, &*webp_bytes).ok()?;

    let meta = ScreenshotMeta {
        id,
        captured_at,
        app: app.to_string(),
        phash,
        blur_level,
        bucket_minute: captured_at.div_euclid(60_000),
        display,
    };
    Some((meta, path))
}

fn screenshots_dir() -> PathBuf {
    std::env::var_os("WP_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".agent-state"))
        .join("screenshots")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_sigma_zero_is_none() {
        assert_eq!(blur_sigma(0), 0.0);
        assert!(blur_sigma(3) > 0.0);
    }

    #[test]
    fn scaled_dims_preserve_aspect_and_cap_width() {
        assert_eq!(scaled_dims(1920, 1080), (768, 432));
        assert_eq!(scaled_dims(640, 480), (640, 480)); // already under cap
    }

    #[test]
    fn host_pin_only_allows_https_amazonaws() {
        assert!(is_allowed_upload_host(
            "https://wp-screenshots-dev.s3.ap-south-1.amazonaws.com/k?sig=x"
        ));
        assert!(!is_allowed_upload_host(
            "http://wp-screenshots-dev.s3.amazonaws.com/k"
        )); // not https
        assert!(!is_allowed_upload_host("https://evil.com/k")); // not amazonaws
        assert!(!is_allowed_upload_host("https://amazonaws.com.evil.com/k")); // suffix trick
    }
}
