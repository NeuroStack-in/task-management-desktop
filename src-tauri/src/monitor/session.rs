//! Session / display-server capability report the UI reads to show the honest-degradation banner.
//!
//! **Field names are load-bearing.** In the reference sample the Rust DTO and its TS reader disagreed
//! (`can_track_windows`/`limitation` vs `limitationMessage`/`sessionType`), so the Wayland banner
//! could never fire. The `#[test]` below pins the serialized names — both compilers are blind to this
//! bug class, so this test is the only guard (BUILD-PLAN §3/§M6).

use serde::Serialize;

/// What the agent can actually capture on this OS/session.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// False on Wayland (and where the OS forbids foreground-window inspection).
    pub can_track_windows: bool,
    /// `"x11"` | `"wayland"` | `"windows"` | `"macos"`.
    pub display_server: String,
    /// Human-readable reason capture is limited, or `None` when unrestricted.
    pub limitation: Option<String>,
}

/// Probe the current session. On Wayland `user-idle`/`device_query`/`x-win` legitimately return
/// nothing (an OS limit, not a crate gap) — so we ship **screenshots-only** there and say so (risk #4).
pub fn session_info() -> SessionInfo {
    let ds = display_server();
    let wayland = ds == "wayland";
    SessionInfo {
        can_track_windows: !wayland,
        display_server: ds,
        limitation: wayland.then(|| {
            "Wayland: input counts and per-app tracking are unavailable (OS limit); screenshots only."
                .to_string()
        }),
    }
}

fn display_server() -> String {
    if cfg!(target_os = "windows") {
        "windows".into()
    } else if cfg!(target_os = "macos") {
        "macos".into()
    } else if cfg!(target_os = "linux") {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || matches!(std::env::var("XDG_SESSION_TYPE").as_deref(), Ok("wayland"));
        if wayland {
            "wayland".into()
        } else {
            "x11".into()
        }
    } else {
        std::env::consts::OS.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact guard the sample lacked: the serialized names must be camelCase, and the old-shape
    /// names must be absent — otherwise the UI's Wayland/degradation banner silently never fires.
    #[test]
    fn dto_field_names_are_camelcase_and_stable() {
        let s = SessionInfo {
            can_track_windows: false,
            display_server: "wayland".into(),
            limitation: Some("x".into()),
        };
        let j = serde_json::to_value(&s).unwrap();
        assert!(j.get("canTrackWindows").is_some());
        assert!(j.get("displayServer").is_some());
        assert!(j.get("limitation").is_some());
        // The old/mismatched shapes must NOT appear.
        assert!(j.get("can_track_windows").is_none());
        assert!(j.get("limitationMessage").is_none());
        assert!(j.get("sessionType").is_none());
    }
}
