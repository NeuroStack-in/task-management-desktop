//! Linux geolocation via **GeoClue2** over D-Bus (`org.freedesktop.GeoClue2`).
//!
//! GeoClue is the freedesktop standard and the only positioning source a desktop Linux app should
//! use: it owns the Wi-Fi/3G/GPS backends, applies the user's privacy policy, and is what GNOME and
//! KDE themselves consume. Talking to a location backend directly would bypass the consent surface
//! the user actually manages.
//!
//! ## The flow
//!
//! 1. `Manager.GetClient()` → a per-caller client object.
//! 2. Set `DesktopId` — **mandatory**. GeoClue matches it against its allow-list and refuses to
//!    `Start()` a client that hasn't identified itself.
//! 3. Set `RequestedAccuracyLevel = EXACT`. GeoClue may still hand back something coarser; whatever it
//!    returns is what we report, with its own accuracy radius.
//! 4. `Start()`, then poll the client's `Location` property until it stops being the null path.
//! 5. Read the location object's properties, then `Stop()` — always, so GeoClue can power down its
//!    backends instead of holding a GPS awake behind us.
//!
//! ## Why polling rather than the `LocationUpdated` signal
//!
//! The signal is the idiomatic choice for a long-lived subscriber. This caller is the opposite: a
//! one-shot inside a send cycle that must finish or give up within [`FIX_TIMEOUT`]. Polling a property
//! with a deadline expresses that directly, whereas a signal stream would need a timeout wrapper
//! around it anyway — and `Location` is already populated when a previous cycle warmed it, so the
//! common case returns on the first read without waiting for any signal at all.
//!
//! **Fails closed.** No system bus, no GeoClue service, a refused `Start()`, or no fix inside the
//! deadline all yield `None`. On a headless box or a locked-down distro that is the normal answer, so
//! it is logged at debug — an error every 3 minutes for a service the machine simply doesn't run is
//! log spam, not a diagnostic.

use std::time::{Duration, Instant};

use wp_agent_contract::GeoLocation;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use super::{Fix, FIX_TIMEOUT};

const SERVICE: &str = "org.freedesktop.GeoClue2";
const MANAGER_PATH: &str = "/org/freedesktop/GeoClue2/Manager";
const MANAGER_IFACE: &str = "org.freedesktop.GeoClue2.Manager";
const CLIENT_IFACE: &str = "org.freedesktop.GeoClue2.Client";
const LOCATION_IFACE: &str = "org.freedesktop.GeoClue2.Location";

/// Identifies the agent to GeoClue. Must match the installed `.desktop` file for distros that check
/// the allow-list against it; the Tauri bundle ships `WorkPulse.desktop`.
const DESKTOP_ID: &str = "WorkPulse";

/// GeoClue's `AccuracyLevel` enum — 8 = EXACT (street level or better).
const ACCURACY_EXACT: u32 = 8;

/// How often to re-read the `Location` property while waiting for a first fix.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub fn capture() -> Option<GeoLocation> {
    match read_fix() {
        Ok(fix) => fix.and_then(Fix::into_geo),
        Err(e) => {
            // Debug, not error: a machine with no GeoClue is a supported configuration, and this runs
            // on every cycle. See the module note.
            tracing::debug!(error = %e, "location: no fix from GeoClue");
            None
        }
    }
}

fn read_fix() -> zbus::Result<Option<Fix>> {
    // GeoClue lives on the SYSTEM bus, not the session bus — a session-bus connection here simply
    // never finds the service.
    let conn = Connection::system()?;

    let manager = Proxy::new(&conn, SERVICE, MANAGER_PATH, MANAGER_IFACE)?;
    let client_path: OwnedObjectPath = manager.call("GetClient", &())?;

    let client = Proxy::new(&conn, SERVICE, &client_path, CLIENT_IFACE)?;
    // Both writes must precede Start(): GeoClue validates DesktopId at start time and rejects a
    // client that set it afterwards.
    client.set_property("DesktopId", DESKTOP_ID)?;
    client.set_property("RequestedAccuracyLevel", ACCURACY_EXACT)?;

    client.call::<_, _, ()>("Start", &())?;
    let fix = poll_for_fix(&conn, &client);
    // Stop unconditionally — including on the timeout path. Leaving the client started keeps GeoClue's
    // backends (and possibly a GPS radio) awake for a fix nobody is waiting for any more.
    let _ = client.call::<_, _, ()>("Stop", &());
    fix
}

/// Re-read `Location` until it names a real object or the deadline passes.
fn poll_for_fix(conn: &Connection, client: &Proxy<'_>) -> zbus::Result<Option<Fix>> {
    let deadline = Instant::now() + FIX_TIMEOUT;
    loop {
        let path: OwnedObjectPath = client.get_property("Location")?;
        // "/" is GeoClue's null object path — the client is started but has no fix yet.
        if path.as_str() != "/" {
            return read_location(conn, &path).map(Some);
        }
        if Instant::now() >= deadline {
            tracing::debug!("location: GeoClue produced no fix within the deadline");
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn read_location(conn: &Connection, path: &OwnedObjectPath) -> zbus::Result<Fix> {
    let loc = Proxy::new(conn, SERVICE, path, LOCATION_IFACE)?;
    Ok(Fix {
        lat: loc.get_property("Latitude")?,
        lon: loc.get_property("Longitude")?,
        accuracy_m: loc.get_property("Accuracy")?,
    })
}
