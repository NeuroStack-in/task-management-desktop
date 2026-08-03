//! macOS geolocation via **CoreLocation** (`CLLocationManager`).
//!
//! CoreLocation is the only positioning API macOS sanctions, and the only one bound to the Location
//! Services privacy surface the user controls.
//!
//! ## The run-loop problem, and why this thread exists
//!
//! `CLLocationManager` is asynchronous and delivers everything — authorization changes and fixes —
//! through a **run loop**. A worker thread that never turns one is handed nothing, forever: the call
//! doesn't fail, it simply never produces a fix, which would read as "location is broken on Mac"
//! rather than "we never let CoreLocation speak".
//!
//! `capture()` is called from `spawn_blocking`, on a Tokio worker with no run loop. So this backend
//! creates the manager on that thread and pumps the run loop itself in short slices, checking the
//! manager's `location` property between them, until a fix appears or [`FIX_TIMEOUT`] passes.
//!
//! Pumping the loop rather than declaring a `CLLocationManagerDelegate` is deliberate: the delegate
//! protocol buys nothing for a one-shot read (its callbacks would only write to a cell this function
//! then polls anyway), and it would mean defining an Objective-C class to receive them. The `location`
//! property already holds the most recent fix, which is exactly what a one-shot wants.
//!
//! ## Authorization
//!
//! `requestWhenInUseAuthorization` is a no-op once the user has answered, so calling it every cycle is
//! safe. **The app must ship `NSLocationWhenInUseUsageDescription` in its `Info.plist`** — without it
//! macOS terminates the process on the authorization request rather than showing a prompt. See
//! `tauri.conf.json`.
//!
//! **Fails closed.** Services disabled, authorization denied or restricted, or no fix inside the
//! deadline all yield `None`.

use std::time::{Duration, Instant};

use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRunLoop};
use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};
use wp_agent_contract::GeoLocation;

use super::{Fix, FIX_TIMEOUT};

/// How long to let the run loop turn before checking for a fix again.
const RUN_LOOP_SLICE: Duration = Duration::from_millis(200);

pub fn capture() -> Option<GeoLocation> {
    read_fix().and_then(Fix::into_geo)
}

fn read_fix() -> Option<Fix> {
    // Safety: every call below is a plain Objective-C message send to CLLocationManager, which is
    // usable off the main thread — CoreLocation's own documentation requires only that the thread
    // have an active run loop, which is precisely what this function provides.
    unsafe {
        // No `locationServicesEnabled()` pre-check. Apple deprecated it, and the reason is that it
        // was always the wrong gate: it answers a system-wide question that the authorization status
        // below already subsumes, and calling it on the main thread can block. Services switched off
        // surface here as a non-authorized status, or simply as no fix before the deadline — both of
        // which this function already handles, and both of which fail closed.
        let manager = CLLocationManager::new();

        // Ask once; macOS shows the prompt on first call and ignores it thereafter. The status is
        // re-read after pumping, because the answer to a first-run prompt arrives on the run loop.
        manager.requestWhenInUseAuthorization();

        let deadline = Instant::now() + FIX_TIMEOUT;
        manager.startUpdatingLocation();

        let fix = loop {
            // Turn the run loop so CoreLocation can deliver authorization changes and fixes. Without
            // this the loop below would spin to the deadline against a permanently empty property.
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, RUN_LOOP_SLICE.as_secs_f64(), false);

            match manager.authorizationStatus() {
                // Terminal denials — stop immediately rather than burning the full deadline on an
                // answer that will not change while this process runs.
                CLAuthorizationStatus::Denied | CLAuthorizationStatus::Restricted => {
                    tracing::debug!("location: authorization denied or restricted");
                    break None;
                }
                // Still pending a user answer, or granted — either way keep waiting for a fix.
                _ => {}
            }

            if let Some(loc) = manager.location() {
                let coord = loc.coordinate();
                break Some(Fix {
                    lat: coord.latitude,
                    lon: coord.longitude,
                    // Negative means "invalid" in CoreLocation; `Fix::into_geo` rejects it rather
                    // than letting a sentinel become an accuracy radius on a map.
                    accuracy_m: loc.horizontalAccuracy(),
                });
            }

            if Instant::now() >= deadline {
                tracing::debug!("location: CoreLocation produced no fix within the deadline");
                break None;
            }
        };

        // Always stop: an updating manager keeps the location hardware awake, and this one is about
        // to be dropped anyway.
        manager.stopUpdatingLocation();
        fix
    }
}
