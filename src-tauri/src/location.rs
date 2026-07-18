//! Native device geolocation for the heartbeat (precise GPS / OS positioning).
//!
//! **Consent-gated + fails closed.** The caller invokes this only while monitoring consent is
//! granted; and if the OS location service is off or access is denied, it returns `None` — no fix,
//! never an approximation. Windows uses WinRT `Windows.Devices.Geolocation`; macOS (CoreLocation) and
//! Linux (GeoClue) are not wired yet and return `None` — a documented cross-platform gap, consistent
//! with the rest of the capability matrix (AGENT.md).
//!
//! Blocking: WinRT `IAsyncOperation::get()` blocks, so this must be called from `spawn_blocking`,
//! never on the async runtime thread.

use wp_agent_contract::GeoLocation;

#[cfg(windows)]
pub fn capture() -> Option<GeoLocation> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};

    use crate::clock::now_epoch_ms;

    // Respect the Windows location setting / per-app permission. Anything but Allowed → no fix.
    match Geolocator::RequestAccessAsync().ok()?.get().ok()? {
        GeolocationAccessStatus::Allowed => {}
        _ => return None,
    }

    let locator = Geolocator::new().ok()?;
    let position = locator.GetGeopositionAsync().ok()?.get().ok()?;
    let coordinate = position.Coordinate().ok()?;
    let basic = coordinate.Point().ok()?.Position().ok()?;

    Some(GeoLocation {
        lat: basic.Latitude,
        lon: basic.Longitude,
        accuracy_m: coordinate.Accuracy().ok()?,
        captured_at: now_epoch_ms(),
    })
}

#[cfg(not(windows))]
pub fn capture() -> Option<GeoLocation> {
    // macOS (CoreLocation) / Linux (GeoClue) not yet wired — no fix rather than a guess.
    None
}
