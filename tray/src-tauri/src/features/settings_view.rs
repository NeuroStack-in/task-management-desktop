//! Read-only view of the *effective* tracking config the core is running (cadence, blur level,
//! retention, silent flag). The user can see how they're being monitored but cannot change it — the
//! config is admin-owned and arrives over the CONFIG rail. Surfacing it is a transparency requirement.

use agent_shared::contract::TrackingConfig;

#[tauri::command]
pub fn effective_config() -> TrackingConfig {
    // TODO(ipc): return the core's current TrackingConfig (the same one capture-helper applies).
    TrackingConfig::default()
}
