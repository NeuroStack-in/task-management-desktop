//! Windows geolocation via WinRT `Windows.Devices.Geolocation`.
//!
//! `RequestAccessAsync` reflects both the system-wide Location setting and the per-app permission, so
//! one check covers the whole consent surface — anything but `Allowed` is a refusal and yields no fix.
//!
//! **Blocking:** WinRT's `IAsyncOperation::get()` blocks the calling thread, which is why the seam
//! requires `spawn_blocking` (see the module docs). `GetGeopositionAsync` applies its own internal
//! timeout, so unlike the macOS and Linux backends this one needs no deadline of its own.

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
use wp_agent_contract::GeoLocation;

use super::Fix;

pub fn capture() -> Option<GeoLocation> {
    // Respect the Windows location setting / per-app permission. Anything but Allowed → no fix.
    match Geolocator::RequestAccessAsync().ok()?.get().ok()? {
        GeolocationAccessStatus::Allowed => {}
        _ => {
            tracing::debug!("location: Windows location access is not granted");
            return None;
        }
    }

    let locator = Geolocator::new().ok()?;
    let position = locator.GetGeopositionAsync().ok()?.get().ok()?;
    let coordinate = position.Coordinate().ok()?;
    let basic = coordinate.Point().ok()?.Position().ok()?;

    // Validated and stamped centrally, so all three backends agree on what a usable fix is.
    Fix {
        lat: basic.Latitude,
        lon: basic.Longitude,
        accuracy_m: coordinate.Accuracy().ok()?,
    }
    .into_geo()
}
