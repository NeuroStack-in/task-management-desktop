//! The webview's only entry into the core (Tauri `#[command]`s). DTOs use camelCase so there is no
//! snake↔camel transform layer at the boundary (BUILD-PLAN §3).
//!
//! M0: timer control + identity, wired to real `AppState`. M1 adds `auth_cmds`; M3 the
//! project→task selector + mandatory description + meeting mode; M4/M5 monitor + screenshot status.

pub mod panel;

use tauri::State;
use wp_agent_contract::StopReason;

use crate::auth::AuthStatus;
use crate::clock::now_epoch_ms;
use crate::state::AppState;

// ---- autostart / launch-at-login (M6) ----

/// Enable or disable launch-at-login. On Windows the installer's "Launch at startup" checkbox sets
/// the initial state and this changes it afterward; on macOS/Linux (no install wizard) this is the
/// only control. Writes the OS autostart entry via the autostart plugin.
#[tauri::command]
pub fn set_auto_start(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    let r = if enabled { m.enable() } else { m.disable() };
    r.map_err(|e| format!("autostart change failed: {e}"))
}

/// The actual OS launch-at-login state — drives the settings toggle so it reflects reality, not a
/// cached guess.
#[tauri::command]
pub fn get_auto_start(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("autostart query failed: {e}"))
}

// ---- auth (M1) ----

/// Sign in with Cognito `USER_PASSWORD_AUTH`. May return a `newPasswordSession` if the account was
/// admin-created and must set a password (then call `auth_complete_new_password`).
#[tauri::command]
pub async fn auth_login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<AuthStatus, String> {
    let status = state.auth.login(&username, &password).await?;
    // A signed-in agent should appear in the fleet **now**, not a full cadence (up to 5–10 min) later:
    // wake the sender so it sends the first heartbeat immediately. Harmless if the login returned a
    // new-password challenge — the sender still auth-gates on a real token.
    if status.signed_in {
        state.flush.notify_one();
    }
    Ok(status)
}

/// Sign in with **Google** via the Hosted UI. Opens the system browser and catches the redirect on a
/// localhost loopback (native OAuth + PKCE); resolves to the same `AuthStatus` a password login does.
/// Works for an invited user signing in and for a brand-new user (who then lands on onboarding).
#[tauri::command]
pub async fn auth_login_google(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AuthStatus, String> {
    use tauri_plugin_shell::ShellExt;
    let status = state
        .auth
        .login_google(move |url| {
            // `shell().open` is deprecated in favour of tauri-plugin-opener, but the shell plugin is
            // already initialized here and pulling in a second plugin just to open one URL isn't
            // worth it. It still opens the OS default browser, which is exactly what the flow needs.
            #[allow(deprecated)]
            app.shell()
                .open(url.to_string(), None)
                .map_err(|e| format!("auth:oauth: couldn't open the browser ({e})"))
        })
        .await?;
    if status.signed_in {
        state.flush.notify_one();
    }
    Ok(status)
}

/// Abandon a Google sign-in that is still waiting on the browser.
///
/// Not `async`: it takes a lock and drops a sender, and making it async would let a second cancel
/// interleave with the first for no benefit. Returns whether anything was actually waiting.
#[tauri::command]
pub fn auth_cancel_google(state: State<'_, AppState>) -> bool {
    state.auth.cancel_oauth()
}

#[tauri::command]
pub async fn auth_complete_new_password(
    state: State<'_, AppState>,
    username: String,
    new_password: String,
    session: String,
) -> Result<AuthStatus, String> {
    let status = state
        .auth
        .complete_new_password(&username, &new_password, &session)
        .await?;
    if status.signed_in {
        state.flush.notify_one();
    }
    Ok(status)
}

/// Answer an outstanding MFA challenge (`SOFTWARE_TOKEN_MFA` / `SMS_MFA`) with the user's code.
///
/// Same post-sign-in flush as the other two paths: an agent that has just authenticated should
/// appear in the fleet now, not a cadence later.
#[tauri::command]
pub async fn auth_complete_mfa(
    state: State<'_, AppState>,
    challenge: String,
    username: String,
    code: String,
    session: String,
) -> Result<AuthStatus, String> {
    let status = state
        .auth
        .complete_mfa(&challenge, &username, &code, &session)
        .await?;
    if status.signed_in {
        state.flush.notify_one();
    }
    Ok(status)
}

#[tauri::command]
pub fn auth_logout(state: State<'_, AppState>) {
    // Full teardown so the next person to sign in on this device starts clean — the timer, queued
    // captures, consent, pause budget and the resume hand-off are all reset (see state.rs). The
    // persisted flags that outlive the process are cleared here: the next user must re-consent, and
    // the previous user's "resume this task" must never auto-start under them.
    state.reset_for_account_switch();
    crate::session_state::update(|s| {
        s.consent_granted = false;
        s.resume = None;
    });
    state.auth.logout();
}

#[tauri::command]
pub fn auth_status(state: State<'_, AppState>) -> AuthStatus {
    state.auth.status()
}

// ---- external links ----

/// The WorkPulse web app — where an organization is created, invites are sent, and the full
/// dashboard lives. The agent panel is a small fixed window with no navigation of its own, so links
/// out of it (creating an org after an `auth:oauth:no_org` sign-in, "forgot password", the dashboard)
/// go here.
const WEBSITE_URL: &str = "https://workpulse-ns.vercel.app";

/// Open the WorkPulse web app in the user's **system browser** — never the webview, which is the
/// panel itself. Routed through the core rather than an in-webview `<a href>` so the single place
/// network egress happens stays Rust-side, matching the CSP contract (`default-src 'self'`).
#[tauri::command]
pub fn open_website(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    #[allow(deprecated)] // same rationale as `auth_login_google`: the shell plugin is already here.
    app.shell()
        .open(WEBSITE_URL.to_string(), None)
        .map_err(|e| format!("open:website: couldn't open the browser ({e})"))
}

// ---- consent (M5 / PRIVACY.md) ----

/// Grant or revoke monitoring consent. Capture (activity + screenshots) is gated on this and
/// **defaults off** — so nothing is captured until the user consents.
#[tauri::command]
pub fn set_consent(state: State<'_, AppState>, granted: bool) {
    state
        .consent
        .store(granted, std::sync::atomic::Ordering::Relaxed);
    if !granted {
        // Turning monitoring off must also drop frames already captured but not yet uploaded —
        // otherwise the last shots keep leaving the device after the user believes capture is off
        // (they are re-declared every cycle until uploaded). Mirrors the location-on-withdrawal
        // behaviour in api/mod.rs.
        let mut shots = state.screenshots.lock().unwrap();
        for s in shots.values() {
            let _ = std::fs::remove_file(&s.path);
        }
        shots.clear();
    }
    // Persist **both** directions. A revoke that only lived in memory would come back granted on the
    // next launch — silently resuming capture the user had switched off, which is the worst possible
    // direction for this flag to fail in.
    crate::session_state::update(|s| s.consent_granted = granted);
}

#[tauri::command]
pub fn consent_status(state: State<'_, AppState>) -> bool {
    state.consent.load(std::sync::atomic::Ordering::Relaxed)
}

// ---- updater (M7) ----

/// What the panel shows about updates: the running version, and whether a newer one exists.
///
/// `checked` distinguishes "we asked and there is nothing newer" from "we could not ask" — offline,
/// or a build with no public key. Without it both render as *up to date*, which is a claim we
/// haven't earned and the one an employee would rely on before assuming they have the latest fix.
#[derive(serde::Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub checked: bool,
    pub latest: Option<String>,
    /// Present only when the check failed; the panel shows it as a quiet hint, not an alarm.
    pub error: Option<String>,
}

/// Report update availability **without installing**. Never errors: a failed check is a state the
/// panel renders, not an exception it has to catch.
#[tauri::command]
pub async fn update_status(app: tauri::AppHandle) -> UpdateStatus {
    let current = crate::updater::current_version().to_string();
    match crate::updater::check_only(&app).await {
        Ok(latest) => UpdateStatus {
            current,
            checked: true,
            latest,
            error: None,
        },
        Err(e) => UpdateStatus {
            current,
            checked: false,
            latest: None,
            error: Some(e),
        },
    }
}

/// Install the pending update on request. Returns the version installed; the app relaunches into it.
#[tauri::command]
pub async fn update_install(app: tauri::AppHandle) -> Result<String, String> {
    crate::updater::install_now(&app).await
}

/// Manually check for a newer signed release (respects `auto_update` for whether it installs).
#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let auto = state.config.lock().unwrap().get().tracking.auto_update;
    crate::updater::check_and_maybe_install(&app, auto).await
}

// ---- timer (M0) ----

/// Start the timer. M3 threads `description` + optional `task_id`/`project_id` (meeting mode) once
/// the contract PR lands (§6); today they are required, matching the deployed contract.
#[tauri::command]
pub fn timer_start(
    state: State<'_, AppState>,
    session_id: String,
    task_id: String,
    project_id: String,
    description: String,
    // The subtask being worked on. Optional — a task with no breakdown, or a user who picked the
    // task itself, sends nothing. `task_id` stays the parent either way.
    subtask_id: Option<String>,
) -> Result<(), String> {
    let ts = now_epoch_ms();
    let event = {
        let mut timer = state.timer.lock().unwrap();
        timer
            .start(
                session_id,
                task_id,
                subtask_id.unwrap_or_default(),
                project_id,
                description,
                ts,
            )
            .map_err(|e| format!("timer:{e}"))?
    };
    // Buffered for the next cycle's `enqueue_cycle` drain (BUILD-PLAN §4), and flushed now.
    state.pending_events.lock().unwrap().push(event);
    state.flush.notify_one(); // immediate flush — the backend learns in seconds, not ~5 min (LLD §4)
    Ok(())
}

/// Stop the running timer (user-initiated). Idle/logout/shutdown stops come from the monitor.
#[tauri::command]
pub fn timer_stop(state: State<'_, AppState>) -> Result<(), String> {
    let ts = now_epoch_ms();
    let event = { state.timer.lock().unwrap().stop(ts, StopReason::User) };
    if let Some(ev) = event {
        state.pending_events.lock().unwrap().push(ev);
        state.flush.notify_one();
    }
    Ok(())
}

/// Whether a session is currently running (drives the UI timer + capture gate).
#[tauri::command]
pub fn timer_status(state: State<'_, AppState>) -> bool {
    state.timer.lock().unwrap().is_running()
}

/// This install's stable agent id (diagnostics; the fleet row key on the backend).
#[tauri::command]
pub fn agent_id(state: State<'_, AppState>) -> String {
    state.outbox.lock().unwrap().agent_id().to_string()
}
