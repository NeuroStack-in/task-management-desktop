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
///
/// **Deliberate deviation from the LLD (owner decision, 2026-07-21).** The LLD specifies 768px in
/// three places (§33, §592, §1035) and `BUILD-PLAN.md:209` adopts it. In practice 768px is a ~40%
/// downscale of a 1920-wide display, which leaves on-screen text unreadable — so the review grid
/// could not answer the one question it exists to answer ("what was this person working on?").
/// 1280px is ~67% and legible, while still well short of shipping a native-resolution frame.
///
/// This weakens the "minimum useful fidelity" position in `PRIVACY.md:76`; the fidelity is now
/// *useful* rather than *minimum*. **The LLD should be amended to match** — do not silently revert
/// this to 768 to close the gap, and do not raise it further without the same conversation.
const MAX_WIDTH: u32 = 1280;
/// Lossy WebP quality (0–100). Not specified by the LLD (which fixes only the width); raised from
/// 75 alongside the width bump, since a sharper downscale is wasted on a soft encode.
const WEBP_QUALITY: f32 = 85.0;

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
    let Some((monitors, dir)) = displays_and_dir() else {
        return Vec::new();
    };
    monitors
        .into_iter()
        .enumerate()
        .filter_map(|(i, m)| process_monitor(&m, app, blur_level, captured_at, i as u8, &dir))
        .collect()
}

/// Capture **the primary display only**, through the exact same pipeline as [`capture_all`]
/// (downscale → blur → WebP → pHash → temp file + content hash).
///
/// This exists for the admin-triggered on-demand capture (`mqtt::capture`), which is handed exactly
/// **one** presigned URL: capturing every monitor there would spool frames of screens nobody can
/// upload, which is the opposite of data minimization (PRIVACY.md §4). Periodic capture keeps taking
/// every display — it gets one URL per declared meta.
pub fn capture_primary(
    app: &str,
    blur_level: u8,
    captured_at: i64,
) -> Option<(ScreenshotMeta, PathBuf)> {
    let (monitors, dir) = displays_and_dir()?;
    let primary = monitors.into_iter().next()?;
    process_monitor(&primary, app, blur_level, captured_at, 0, &dir)
}

/// Connected displays (**primary first**) plus the prepared, hardened spool directory — the shared
/// prologue of both capture entry points. `None` when the OS yields no monitors or the spool can't
/// be created.
fn displays_and_dir() -> Option<(Vec<xcap::Monitor>, PathBuf)> {
    // **Every failure below is logged, never swallowed.** These four `?`s used to be bare `.ok()?`,
    // so an empty result was indistinguishable from a denied macOS grant — and the UI said exactly
    // that on every platform. A Windows agent then spent a day reporting "grant screen-recording
    // permission" for a failure that had nothing to do with permissions, with no log to contradict
    // it. If capture stops, the reason must be in the log.
    let mut monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "capture: could not enumerate displays");
            return None;
        }
    };
    if monitors.is_empty() {
        tracing::error!("capture: the OS reported no connected displays");
        return None;
    }
    // Primary display first, so it is always `display = 0` ("Monitor 1"); the index is then the
    // stable physical position (a monitor that fails to capture leaves a gap rather than renumbering).
    monitors.sort_by_key(|m| !m.is_primary().unwrap_or(false));
    let dir = screenshots_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        // The spool path is CWD-relative unless `WP_STATE_DIR` is set, so an installed build whose
        // working directory isn't writable fails right here — silently, before any pixel is read.
        tracing::error!(dir = %dir.display(), error = %e, "capture: could not create the screenshot spool");
        return None;
    }
    // Lock the buffer down (once per process) so *other* users on the machine can't read or swap the
    // pending images. This complements the upload-time SHA-256 check: hashing catches the monitored
    // user's own scripts (same identity as the agent), ACLs stop everyone else.
    HARDEN_ONCE.call_once(|| harden_dir(&dir));
    Some((monitors, dir))
}

static HARDEN_ONCE: std::sync::Once = std::sync::Once::new();

/// Restrict the screenshot buffer directory to the agent's own user (+ SYSTEM/Administrators on
/// Windows). Best-effort and **never fatal** — a failure here must not stop capture; it's a
/// hardening layer, not a gate.
///
/// **What it does and doesn't stop:** it denies *other* users on a shared machine. It does **not**
/// stop the monitored user's own scripts — they run as the same identity as the agent — which is
/// exactly what the upload-time content hash is for. Defence in depth, not a silver bullet.
#[cfg(windows)]
fn harden_dir(dir: &Path) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let user = std::env::var("USERNAME").unwrap_or_default();

    let mut cmd = Command::new("icacls");
    cmd.arg(dir).arg("/inheritance:r"); // drop inherited (broad) permissions
    if !user.is_empty() {
        cmd.arg("/grant:r").arg(format!("{user}:(OI)(CI)F"));
    }
    cmd.arg("/grant:r")
        .arg("SYSTEM:(OI)(CI)F")
        .arg("/grant:r")
        .arg("Administrators:(OI)(CI)F")
        .arg("/T")
        .arg("/C")
        .arg("/Q")
        .creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(o) if o.status.success() => tracing::info!("screenshot buffer directory locked down"),
        Ok(o) => tracing::warn!(
            "icacls harden failed: {}",
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => tracing::warn!("could not harden screenshot dir: {e}"),
    }
}

#[cfg(unix)]
fn harden_dir(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    // rwx for the owner only.
    if let Err(e) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
        tracing::warn!("could not set 0700 on screenshot dir: {e}");
    }
}

#[cfg(not(any(windows, unix)))]
fn harden_dir(_dir: &Path) {}

/// Lowercase-hex SHA-256 of `bytes`. Used to bind a screenshot's captured bytes to the upload so a
/// swapped file on disk is detected (tamper-evidence).
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Grab + process one display. `None` on capture/encode/IO failure for this monitor alone. The
/// returned `meta.content_sha256` is the SHA-256 of the exact WebP bytes written — the tamper-evidence
/// anchor that rides the authenticated batch and gates the upload.
fn process_monitor(
    monitor: &xcap::Monitor,
    app: &str,
    blur_level: u8,
    captured_at: i64,
    // Named `display_index`, not `display`: inside a `tracing` macro a bare `display` resolves to
    // that crate's own `field::display()` helper rather than to the local, which fails to compile
    // with a thoroughly misleading trait error.
    display_index: u8,
    dir: &Path,
) -> Option<(ScreenshotMeta, PathBuf)> {
    // On macOS this is where a denied Screen-Recording grant lands. Everywhere else it is a real
    // capture failure (driver reset, RDP/locked session, resource pressure) — which is why the
    // error text matters and a bare `.ok()?` did not.
    let rgba = match monitor.capture_image() {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(display_index, error = %e, "capture: display grab failed");
            return None;
        }
    };

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

    // Bind these exact bytes for tamper-evidence — computed here, before the file leaves our hands.
    let content_sha256 = sha256_hex(&webp_bytes);

    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("{id}.webp"));
    if let Err(e) = std::fs::write(&path, &*webp_bytes) {
        // Disk full, or the spool became unwritable after `create_dir_all` succeeded.
        tracing::error!(path = %path.display(), error = %e, "capture: could not write the frame");
        return None;
    }

    let meta = ScreenshotMeta {
        id,
        captured_at,
        app: app.to_string(),
        phash,
        blur_level,
        bucket_minute: captured_at.div_euclid(60_000),
        display: display_index,
        content_sha256,
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
        assert_eq!(scaled_dims(1920, 1080), (1280, 720)); // 16:9 stays 16:9
        assert_eq!(scaled_dims(2560, 1600), (1280, 800)); // 16:10 stays 16:10
        assert_eq!(scaled_dims(640, 480), (640, 480)); // already under cap
    }

    #[test]
    fn sha256_hex_is_stable_and_detects_a_changed_byte() {
        // Known vector: SHA-256("abc").
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Any change flips the hash — this is what catches a swapped screenshot.
        assert_ne!(sha256_hex(b"abc"), sha256_hex(b"abd"));
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
