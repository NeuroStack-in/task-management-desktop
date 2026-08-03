//! Native device geolocation for the heartbeat — one platform-standard backend per OS.
//!
//! **Consent-gated + timer-gated + fails closed.** The caller ([`crate::api::refresh_location`])
//! invokes this only while the timer runs, consent is granted, and no privacy pause is active. If the
//! OS location service is off, access is denied, or no fix arrives in time, every backend returns
//! `None` — never an approximation, never a stale value dressed as fresh. A missing fix is a fact the
//! heartbeat can carry; a guessed one is a lie about where someone worked.
//!
//! | OS | API | Permission surface |
//! |---|---|---|
//! | Windows | WinRT `Windows.Devices.Geolocation` | Settings → Privacy → Location |
//! | macOS | CoreLocation (`CLLocationManager`) | System Settings → Privacy → Location Services |
//! | Linux | GeoClue2 over D-Bus (`org.freedesktop.GeoClue2`) | agent/`geoclue.conf` allow-list |
//!
//! **Blocking by construction.** All three backends block: WinRT `IAsyncOperation::get()`, a
//! CoreLocation run loop, and a synchronous D-Bus round trip. `capture()` must therefore be called
//! from `spawn_blocking`, never on the async runtime thread — [`crate::api::refresh_location`] does.
//!
//! Each backend caps its own wait at [`FIX_TIMEOUT`]; a heartbeat that is late is worse than a
//! heartbeat with no fix, because the cycle it delays also carries the activity rollup.

use std::time::Duration;

use wp_agent_contract::GeoLocation;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::capture;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::capture;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::capture;

/// How long a backend may wait for the OS to produce a first fix before giving up.
///
/// A cold GPS/Wi-Fi fix genuinely takes seconds, so this cannot be tight — but it sits inside the
/// send cycle, so it cannot be generous either. 8 s is comfortably under the shortest cadence (3 min)
/// and under any sane HTTP timeout downstream.
#[allow(dead_code)] // Used by the macOS and Linux backends; Windows' WinRT call self-limits.
pub(crate) const FIX_TIMEOUT: Duration = Duration::from_secs(8);

/// A platform's raw reading, before it becomes a contract type. Keeping the conversion here means the
/// three backends can't disagree about what `accuracy_m` or `captured_at` mean.
#[allow(dead_code)] // Constructed by the macOS and Linux backends.
pub(crate) struct Fix {
    pub lat: f64,
    pub lon: f64,
    /// Horizontal accuracy in metres. A negative value is CoreLocation's "invalid" sentinel and is
    /// rejected by [`Fix::into_geo`] rather than shipped as a real radius.
    pub accuracy_m: f64,
}

#[allow(dead_code)]
impl Fix {
    /// Validate and stamp. `None` when the platform handed us something unusable — an out-of-range
    /// coordinate or a negative accuracy — because a malformed fix on a map is worse than no pin.
    pub fn into_geo(self) -> Option<GeoLocation> {
        if !self.lat.is_finite() || !self.lon.is_finite() || !self.accuracy_m.is_finite() {
            return None;
        }
        if !(-90.0..=90.0).contains(&self.lat) || !(-180.0..=180.0).contains(&self.lon) {
            tracing::warn!(
                lat = self.lat,
                lon = self.lon,
                "location: out-of-range fix discarded"
            );
            return None;
        }
        // CoreLocation reports a negative horizontalAccuracy when the fix is invalid; GeoClue never
        // should, but the same guard costs nothing and keeps one rule for all three.
        if self.accuracy_m < 0.0 {
            return None;
        }
        Some(GeoLocation {
            lat: self.lat,
            lon: self.lon,
            accuracy_m: self.accuracy_m,
            captured_at: crate::clock::now_epoch_ms(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plausible_fix_becomes_a_geolocation() {
        let g = Fix {
            lat: 12.9716,
            lon: 77.5946,
            accuracy_m: 18.0,
        }
        .into_geo()
        .expect("a Bengaluru fix is valid");
        assert_eq!(g.lat, 12.9716);
        assert_eq!(g.accuracy_m, 18.0);
        assert!(
            g.captured_at > 0,
            "the fix must be stamped when it is taken"
        );
    }

    /// CoreLocation's documented sentinel for "this reading is not valid" — it must never reach a map
    /// as a real accuracy radius.
    #[test]
    fn a_negative_accuracy_is_rejected() {
        assert!(Fix {
            lat: 12.0,
            lon: 77.0,
            accuracy_m: -1.0
        }
        .into_geo()
        .is_none());
    }

    #[test]
    fn out_of_range_coordinates_are_rejected() {
        assert!(Fix {
            lat: 91.0,
            lon: 0.0,
            accuracy_m: 5.0
        }
        .into_geo()
        .is_none());
        assert!(Fix {
            lat: 0.0,
            lon: -181.0,
            accuracy_m: 5.0
        }
        .into_geo()
        .is_none());
    }

    #[test]
    fn non_finite_values_are_rejected() {
        assert!(Fix {
            lat: f64::NAN,
            lon: 0.0,
            accuracy_m: 5.0
        }
        .into_geo()
        .is_none());
        assert!(Fix {
            lat: 0.0,
            lon: 0.0,
            accuracy_m: f64::INFINITY
        }
        .into_geo()
        .is_none());
    }

    /// 0,0 is a real coordinate (Null Island) and must not be special-cased away — the guard is for
    /// malformed values, not for coordinates that look suspicious.
    #[test]
    fn null_island_is_a_valid_coordinate() {
        assert!(Fix {
            lat: 0.0,
            lon: 0.0,
            accuracy_m: 10.0
        }
        .into_geo()
        .is_some());
    }
}
