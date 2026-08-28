//! Self-update — **M7**. Signed **GitHub Releases** (Ed25519/minisign via `tauri-plugin-updater`).
//! **Release builds refuse unsigned updates**: with no configured public key the check errors out
//! (`updater:no_pubkey`) and nothing is downloaded — an unsigned or mismatched artifact never
//! installs. Gated by `TrackingConfig.auto_update`.
//!
//! **One manual step before releases work:** run `cargo tauri signer generate`. Commit the PUBLIC key
//! (set `WP_UPDATER_PUBKEY`, or paste it into `PUBKEY` below) and keep the PRIVATE key a **CI secret**
//! (`TAURI_SIGNING_PRIVATE_KEY`) used to sign the release artifacts. Point releases at **our** repo
//! (below) — not the sample's.

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// The running agent version — folds into the heartbeat and gates update checks.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Where the update manifest lives — the **public downloads bucket**, not GitHub Releases.
///
/// It used to point at `github.com/NeuroStack-in/task-management-desktop/releases/latest/download/
/// latest.json`. That repo is private, so an installed agent — which has no GitHub credentials and
/// never will — got **404 on every check**. Self-update was configured, signed, and completely
/// non-functional: every upgrade meant asking each employee to re-download the installer by hand.
///
/// The same bucket already serves the installers the Download page links to, for exactly the same
/// reason, and it costs nothing extra: the release pipeline mirrors `latest.json` there alongside
/// them. Signatures still gate the install (`PUBKEY` below) — a public bucket changes *where* the
/// manifest is read from, never *whether* an artifact is trusted.
///
/// `WP_UPDATER_ENDPOINT` overrides it, so a build can be pointed at a staging manifest without a
/// recompile.
const RELEASES_ENDPOINT: &str =
    "https://wp-downloads-dev.s3.ap-south-1.amazonaws.com/agent/latest/latest.json";

fn endpoint() -> String {
    match std::env::var("WP_UPDATER_ENDPOINT") {
        Ok(v) if !v.is_empty() => v,
        _ => RELEASES_ENDPOINT.to_string(),
    }
}

/// Baked-in minisign public key — **must be the public half of the CI `TAURI_SIGNING_PRIVATE_KEY`
/// secret**, and identical to `tauri.conf.json` `plugins.updater.pubkey` (the plugin verifies each
/// artifact against it). `WP_UPDATER_PUBKEY` can override at runtime for testing. Non-empty ⇒ the
/// updater actually runs; empty short-circuits to `updater:no_pubkey` and installs nothing.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDZFNjFCQjdCNzM5MkQyNQpSV1FsTFRtM3R4dm1CcUFjK3ljNUpHMm9wcGVHbmxTZWJrcXoyY2ZjMmFGM1FaOUt0RVR0ay9XOAo=";

fn pubkey() -> String {
    match std::env::var("WP_UPDATER_PUBKEY") {
        Ok(v) if !v.is_empty() => v,
        _ => PUBKEY.to_string(),
    }
}

/// Build a configured updater, or explain why we can't.
fn updater_for(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let pk = pubkey();
    if pk.is_empty() {
        return Err("updater:no_pubkey".into());
    }
    let endpoint = url::Url::parse(&endpoint()).map_err(|e| format!("updater:url:{e}"))?;
    app.updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| format!("updater:endpoints:{e}"))?
        .pubkey(pk)
        .build()
        .map_err(|e| format!("updater:build:{e}"))
}

/// Look for a newer signed build **without installing anything** — `Some(version)` when one exists.
///
/// Separate from [`check_and_maybe_install`] because a UI needs to say *"up to date"* or *"0.1.4 is
/// available"* before the user has agreed to anything. Folding the two together, as the original
/// single entry point did, meant the only way to learn a version existed was to have already
/// installed it.
pub async fn check_only(app: &AppHandle) -> Result<Option<String>, String> {
    let update = updater_for(app)?
        .check()
        .await
        .map_err(|e| format!("updater:check:{e}"))?;
    Ok(update.map(|u| u.version))
}

/// Download and install the pending update. Errors when there is none, rather than reporting a
/// success that did nothing — a button that silently no-ops is worse than one that explains itself.
pub async fn install_now(app: &AppHandle) -> Result<String, String> {
    let update = updater_for(app)?
        .check()
        .await
        .map_err(|e| format!("updater:check:{e}"))?
        .ok_or("updater:none_available")?;
    let version = update.version.clone();
    let handle = app.clone();
    update
        // The second closure is `on_before_exit`, and it used to be `|| {}`. On Windows the plugin
        // runs the installer and then calls `std::process::exit(0)`, so `RunEvent::ExitRequested`
        // never fires — this hook is the ONLY chance to close the session. Without it an agent that
        // updated mid-timer left the session open on the server: "Recording" forever in the web UI,
        // and the employee's task forgotten rather than offered back on relaunch.
        .download_and_install(
            |_downloaded, _total| {},
            move || crate::lifecycle::close_session_for_exit(&handle, "update"),
        )
        .await
        .map_err(|e| format!("updater:install:{e}"))?;
    Ok(version)
}

/// Check GitHub Releases for a newer **signed** build. Returns whether an update is available. When
/// `auto_update` is on, it is downloaded + installed (signature verified by the plugin first). With no
/// public key configured this refuses to proceed — never an unsigned update.
pub async fn check_and_maybe_install(app: &AppHandle, auto_update: bool) -> Result<bool, String> {
    let pk = pubkey();
    if pk.is_empty() {
        return Err("updater:no_pubkey".into());
    }
    let endpoint = url::Url::parse(&endpoint()).map_err(|e| format!("updater:url:{e}"))?;

    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| format!("updater:endpoints:{e}"))?
        .pubkey(pk)
        .build()
        .map_err(|e| format!("updater:build:{e}"))?;

    let Some(update) = updater
        .check()
        .await
        .map_err(|e| format!("updater:check:{e}"))?
    else {
        return Ok(false); // already current
    };
    if !auto_update {
        tracing::info!(
            "update {} available; auto_update off — not installing",
            update.version
        );
        return Ok(true);
    }
    let handle = app.clone();
    update
        // See `install_now` — the auto-update path needs the same close, and needs it more: this
        // one fires on a 6-hour timer with nobody watching, in the middle of someone's workday.
        .download_and_install(
            |_downloaded, _total| {},
            move || crate::lifecycle::close_session_for_exit(&handle, "auto-update"),
        )
        .await
        .map_err(|e| format!("updater:install:{e}"))?;
    Ok(true)
}
