//! The command surface kishore's tray panel calls (`lib/agent.ts`). Serde field names are
//! **snake_case** to match `ui/src/lib/types.ts` (`policy_version`, `next_cycle_secs`, …). Wired to
//! the real single-process state — consent, timer, config, screenshots, pause — with identity from
//! the Cognito claims. `tasks`/`activity`/`sessions` have no source yet (the panel degrades: no task
//! list, empty sessions) and are served by their own commands returning empty.

use serde::Serialize;
use tauri::State;
use wp_agent_contract::{Cadence, StopReason, TrackingConfig};

use crate::clock::now_epoch_ms;
use crate::state::AppState;

fn now_ms() -> i64 {
    now_epoch_ms()
}

// ── consent ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConsentStateDto {
    pub granted: bool,
    pub policy_version: u64,
    pub captured: Vec<String>,
}

/// What the agent records, as the consent record states it. **Must match the payload**: the second
/// line said "Foreground app / website category" while every activity rollup carries the full
/// foreground window title — document names, mail subjects, chat names (QA audit 2026-10-08,
/// WP-210). Whether the notice is shown before consent is recorded is the panel's call, and an
/// owner decision; this list is what it would say.
fn disclosure() -> Vec<String> {
    [
        "Activity counts (keystroke & mouse totals — never the keys themselves)",
        "The app in the foreground and its window title (for example a document, email subject or \
         chat name), and its category under your organization's rules",
        "Periodic screenshots while your timer runs (blurred per your organization's policy)",
        "Device location (GPS / OS positioning) while your timer runs",
        "Attendance and timer events",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn read_consent(state: &AppState) -> ConsentStateDto {
    ConsentStateDto {
        granted: state.consent.load(std::sync::atomic::Ordering::Relaxed),
        policy_version: state.config.lock().unwrap().get().rules.version,
        captured: disclosure(),
    }
}

#[tauri::command]
pub fn get_consent_state(state: State<'_, AppState>) -> ConsentStateDto {
    read_consent(&state)
}

#[tauri::command]
pub fn grant_consent(state: State<'_, AppState>, policy_version: u64) -> ConsentStateDto {
    state
        .consent
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // Persisted, so the monitoring notice is a first-run step rather than a toll on every launch.
    // The version is recorded for audit — which disclosure was accepted — not used to re-prompt.
    crate::session_state::update(|s| {
        s.consent_granted = true;
        s.consent_policy_version = policy_version;
    });
    read_consent(&state)
}

// ── capture indicator ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CaptureStateDto {
    pub capturing: bool,
    pub screenshots: bool,
    pub next_cycle_secs: u32,
}

/// What the panel's capture indicator reports.
///
/// **`capturing` must mirror the gates the capture loops actually use** (`monitor::run` for activity,
/// `monitor::spawn_screenshots` for frames) — it drives the green `LiveDot`, which PRIVACY.md §5
/// requires to be lit whenever capture is running.
///
/// It used to be derived from the screenshot **cadence** alone, which is not a term in the activity
/// gate at all. So an org that set cadence to `Off` (a reasonable "no screenshots" policy) while
/// leaving activity monitoring on captured keystroke/click counts and the foreground app/title/URL
/// with the indicator **dark** — the employee saw nothing running while their activity shipped in
/// every batch. That is the precise failure the persistent indicator exists to prevent.
///
/// So the flag is the **union**: lit when *anything* is being captured. `screenshots` stays
/// screenshot-specific (it feeds the cadence countdown), and both now honour the entitlement flags
/// the loops enforce, not just the cadence.
#[tauri::command]
pub fn capture_state(state: State<'_, AppState>) -> CaptureStateDto {
    let running = state.timer.lock().unwrap().is_running();
    let consent = state.consent.load(std::sync::atomic::Ordering::Relaxed);
    let paused = state.pause.lock().unwrap().is_paused(now_ms());
    let policy_known = state
        .config_known
        .load(std::sync::atomic::Ordering::Relaxed);
    let cfg = state.config.lock().unwrap();
    let c = cfg.get();
    let cadence = c.tracking.cadence;
    // The conditions both loops share — including that the org's policy is known (WP-209).
    let gated = running && consent && !paused && policy_known;
    // Screenshots additionally need a live cadence AND the org's `monitoring.screenshots`
    // entitlement (`monitor::screenshot_gate_open`); activity needs `monitoring.activity`, no
    // cadence at all, and a session where it can be measured — none on Wayland (`monitor::run`).
    let screenshots = gated && !matches!(cadence, Cadence::Off) && c.tracking.screenshots_enabled;
    let activity = gated
        && c.tracking.activity_enabled
        && crate::monitor::session::session_info().can_track_windows;
    let capturing = screenshots || activity;
    let interval = cadence.interval_secs().unwrap_or(0);
    // The countdown is the *screenshot* cadence, so it keys off `screenshots` — not `capturing`,
    // which can now be true on an activity-only policy that never takes a frame.
    let next_cycle_secs = if screenshots && interval > 0 {
        interval - ((now_ms() / 1000) as u32 % interval)
    } else {
        0
    };
    CaptureStateDto {
        capturing,
        screenshots,
        next_cycle_secs,
    }
}

// ── config ───────────────────────────────────────────────────────────────────

/// The effective tracking config (contract `TrackingConfig` — serializes with the exact fields the
/// panel's `TrackingConfig` type reads; `cadence` is lowercase, matching `Cadence`).
#[tauri::command]
pub fn effective_config(state: State<'_, AppState>) -> TrackingConfig {
    state.config.lock().unwrap().get().tracking.clone()
}

// ── timer ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TimerStateDto {
    pub running: bool,
    pub task_id: Option<String>,
    /// The subtask being worked on, when one was picked. The panel shows it beneath the task.
    pub subtask_id: Option<String>,
    pub project_id: Option<String>,
    pub description: String,
    pub elapsed_secs: u64,
}

fn read_timer(state: &AppState) -> TimerStateDto {
    let s = state.timer.lock().unwrap().snapshot(now_ms());
    TimerStateDto {
        running: s.running,
        task_id: s.task_id,
        subtask_id: s.subtask_id,
        project_id: s.project_id,
        description: s.description,
        elapsed_secs: s.elapsed_secs,
    }
}

#[tauri::command]
pub fn timer_state(state: State<'_, AppState>) -> TimerStateDto {
    read_timer(&state)
}

/// Start a session against a project, with the user's free-text description ("what are you working
/// on?"). `task_id` is optional — omitted for the project/ad-hoc flow the panel uses; when present it
/// attributes to a specific task. The emitted `TimerStarted` carries all three to the backend fold.
#[tauri::command]
pub fn start_timer(
    state: State<'_, AppState>,
    project_id: Option<String>,
    description: Option<String>,
    task_id: Option<String>,
    // The subtask the user picked, when the task has a breakdown. `task_id` above remains the
    // PARENT — see `TimerEngine::start`.
    subtask_id: Option<String>,
) -> Result<TimerStateDto, String> {
    // **Self time tracking is a contributor permission** (`TimeTrackSelf`, bit 110), deliberately
    // not held by Owner or Admin. `POST /v1/agent/batch` does not check it, so the agent was the
    // only way past it — any role could record sessions, activity and screenshots for itself (QA
    // audit 2026-10-08, WP-214). An unreadable claim is not a refusal: the server is the boundary.
    if state.auth.can_track_time() == Some(false) {
        return Err("timer:not-permitted".into());
    }
    let ts = now_ms();
    let session_id = uuid::Uuid::new_v4().to_string();
    // Surface a start that was refused (already running) instead of swallowing it and returning the
    // *old* session — otherwise the panel shows the previous task as if the new one started.
    let event = {
        let mut t = state.timer.lock().unwrap();
        t.start(
            session_id,
            task_id.unwrap_or_default(),
            subtask_id.unwrap_or_default(),
            project_id.unwrap_or_default(),
            description.unwrap_or_default(),
            ts,
        )
        .map_err(|e| format!("timer:{e}"))?
    };
    state.pending_events.lock().unwrap().push(event);
    state.flush.notify_one(); // flush now — don't wait for the 300 s cycle (LLD §4)
                              // A manual start takes over — drop any armed auto-resume so it can't later restart a stale task.
    state.auto_resume.lock().unwrap().take();
    Ok(read_timer(&state))
}

/// Stop the running timer.
///
/// `discard_idle` is the idle banner's Stop ("No activity for 5 min — still working?"): the person
/// is saying they weren't, so the session ends at the last input before the idle stretch rather than
/// at the click — unless they have evidently been back at work for a while since
/// (`IdleWindow::stop_at`). Stopping at the click billed the idle minutes (QA audit 2026-10-08,
/// WP-212).
#[tauri::command]
pub fn stop_timer(state: State<'_, AppState>, discard_idle: Option<bool>) -> TimerStateDto {
    let now = now_ms();
    let window = state.idle_window.lock().unwrap().take();
    let ts = match window {
        Some(w) if discard_idle == Some(true) => w.stop_at(now),
        _ => now,
    };
    let ev = state.timer.lock().unwrap().stop(ts, StopReason::User);
    if let Some(e) = ev {
        state.pending_events.lock().unwrap().push(e);
        state.flush.notify_one(); // a stop should reflect as fast as a start
    }
    // A deliberate stop must not auto-resume — clear any target armed by an earlier idle/suspend stop.
    state.auto_resume.lock().unwrap().take();
    read_timer(&state)
}

// ── privacy pause ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PauseGrantDto {
    pub granted: bool,
    pub granted_secs: u64,
    pub remaining_budget_secs: u64,
}

/// The live pause window, so the panel can re-attach to a pause it didn't start.
#[derive(Serialize)]
pub struct PauseStateDto {
    pub paused: bool,
    /// Seconds left on the current pause, 0 when not paused.
    pub remaining_secs: u64,
    pub remaining_budget_secs: u64,
    /// The org's daily allowance, so the panel can say "of 15 min" (0 = pausing is off).
    pub allowance_secs: u64,
}

/// The org's daily pause allowance from the applied config (owner request 2026-10-15).
fn pause_allowance(state: &AppState) -> u64 {
    u64::from(
        state
            .config
            .lock()
            .unwrap()
            .get()
            .tracking
            .pause_allowance_secs,
    )
}

/// Save the pause day, use and window so a restart neither refills the allowance nor drops a pause.
fn persist_pause(p: &crate::state::PauseState) {
    let (day, used, until) = (p.day.clone(), p.used_secs, p.until_ms);
    crate::session_state::update(|s| {
        s.pause_day = day;
        s.pause_used_secs = Some(used);
        s.pause_until_ms = until;
        s.pause_budget_secs = None;
    });
}

fn pause_dto(p: &crate::state::PauseState, now: i64, today: &str, allowance: u64) -> PauseStateDto {
    let remaining_secs = p
        .until_ms
        .filter(|u| *u > now)
        // Round up: a 500 ms remainder is still a paused second, and truncating would report 0
        // while capture is genuinely still suspended.
        .map(|u| ((u - now) as f64 / 1000.0).ceil() as u64)
        .unwrap_or(0);
    PauseStateDto {
        paused: p.is_paused(now),
        remaining_secs,
        remaining_budget_secs: p.budget_on(today, allowance),
        allowance_secs: allowance,
    }
}

/// Read the current pause without consuming budget.
///
/// The core owns the authoritative pause window (`state.rs`), but until now only `request_pause`
/// existed — so a webview reload lost the countdown while the core stayed paused, and the panel
/// under-reported. The UI must never track this locally: the budget is enforced core-side.
///
/// `local_date` is the panel's `YYYY-MM-DD`: the budget is per local day (`PauseState`), and the
/// core has no local calendar to tell when that day ends.
#[tauri::command]
pub fn pause_state(state: State<'_, AppState>, local_date: Option<String>) -> PauseStateDto {
    let now = now_ms();
    let today = local_date.unwrap_or_default();
    let allowance = pause_allowance(&state);
    let p = state.pause.lock().unwrap();
    pause_dto(&p, now, &today, allowance)
}

/// Pause capture — screenshots, activity and location; the timer keeps running — for up to
/// `requested_secs` of `local_date`'s remaining allowance. The panel asks for all of it: the pause
/// lasts until the person resumes (`resume_pause` gives the unused part back) or the allowance runs
/// out. Use is persisted, so a restart does not hand out a fresh allowance (WP-213).
#[tauri::command]
pub fn request_pause(
    state: State<'_, AppState>,
    requested_secs: u64,
    local_date: Option<String>,
) -> PauseGrantDto {
    let today = local_date.unwrap_or_default();
    let allowance = pause_allowance(&state);
    let (granted, granted_secs, remaining_budget_secs) = {
        let mut p = state.pause.lock().unwrap();
        let out = p.request(now_ms(), requested_secs, &today, allowance);
        if out.0 {
            persist_pause(&p);
        }
        out
    };
    PauseGrantDto {
        granted,
        granted_secs,
        remaining_budget_secs,
    }
}

/// End the privacy pause now, giving the unused part back to today's allowance — so a pause can be
/// taken in as many pieces as the person likes (owner request 2026-10-15).
#[tauri::command]
pub fn resume_pause(state: State<'_, AppState>, local_date: Option<String>) -> PauseStateDto {
    let now = now_ms();
    let today = local_date.unwrap_or_default();
    let allowance = pause_allowance(&state);
    let mut p = state.pause.lock().unwrap();
    p.resume(now);
    persist_pause(&p);
    pause_dto(&p, now, &today, allowance)
}

// ── session capabilities ─────────────────────────────────────────────────────

/// What this session can measure — the honest-degradation report `monitor::session` was written
/// for, which no command exposed until now. On Wayland the panel says idle can't be detected, so
/// the timer must be stopped by hand (QA audit 2026-10-08, WP-216).
#[tauri::command]
pub fn session_info() -> crate::monitor::session::SessionInfo {
    crate::monitor::session::session_info()
}

// ── projects (backend-fed) ───────────────────────────────────────────────────

/// A valid ID token for a panel read, or the error the panel keeps its last-good lists on.
///
/// The list reads used to answer `Ok([])` here. `id_token()` is also `None` when a refresh merely
/// couldn't reach Cognito (offline at the 15-minute rotation), so a network blip blanked the project
/// picker, the task workspace and Today's sessions, and the footer told the employee to ask an admin
/// for projects (QA audit 2026-10-08, WP-207). An error is what that is; the panel already reads the
/// lists only while signed in.
async fn list_token(state: &AppState) -> Result<String, String> {
    state
        .auth
        .id_token()
        .await
        .ok_or_else(|| "auth:unavailable".to_string())
}

/// A 401 on a panel read means the cached ID token is dead whatever its local expiry says — make
/// the next read refresh it (WP-442).
fn note_rejection<T>(state: &AppState, result: Result<T, String>) -> Result<T, String> {
    if matches!(&result, Err(e) if e == "auth:expired") {
        state.auth.invalidate_id_token();
    }
    result
}

/// The signed-in user's projects for the "Select project" picker — fetched live from
/// `GET /v1/projects`. An error (signed out, offline, rejected) leaves the panel's last-good list.
#[tauri::command]
pub async fn list_projects(
    state: State<'_, AppState>,
) -> Result<Vec<crate::api::projects::ProjectDto>, String> {
    let id_token = list_token(&state).await?;
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    note_rejection(
        &state,
        crate::api::projects::fetch_projects(&client, &ingest_url, &id_token).await,
    )
}

/// The theme the user chose in the web app, so the panel matches it instead of keeping a second,
/// rival preference of its own.
///
/// `None` when signed out or unreachable — the panel then keeps whatever it is already showing.
/// Deliberately not an error: a theme that failed to load is not worth an error state, and failing
/// loudly here would put a red banner on a working agent.
#[tauri::command]
pub async fn appearance(
    state: State<'_, AppState>,
) -> Result<Option<crate::api::appearance::AppearanceDto>, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Ok(None);
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    match crate::api::appearance::fetch_appearance(&client, &ingest_url, &id_token).await {
        Ok(a) => Ok(Some(a)),
        Err(e) => {
            tracing::debug!("appearance unavailable ({e}); panel keeps its current look");
            Ok(None)
        }
    }
}

/// The tasks the picker offers — the signed-in user's own, plus any unassigned work in projects
/// they belong to (`GET /v1/me/tasks?include_unassigned=true`). An error leaves the last-good list.
#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
) -> Result<Vec<crate::api::tasks::TaskDto>, String> {
    let id_token = list_token(&state).await?;
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    note_rejection(
        &state,
        crate::api::tasks::fetch_tasks(&client, &ingest_url, &id_token).await,
    )
}

/// Create a task in `project_id` from the panel and return it, so the picker can offer it (and the
/// UI select it) without waiting for the next poll. Errors when signed out (`auth:expired`) or if the
/// backend refuses (e.g. the caller's project role can't create tasks) — surfaced as the panel's
/// action error, not swallowed.
#[tauri::command]
pub async fn create_task(
    state: State<'_, AppState>,
    project_id: String,
    title: String,
) -> Result<crate::api::tasks::TaskDto, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Err("auth:expired".into());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    // **Title only, and assigned to the caller.** The removed version also took description, due
    // date, priority and an assign-to-me switch — five controls in a 320px panel for something the
    // employee is about to start timing anyway. Everything else is editable in the web app, which
    // is where task detail belongs; this exists so nobody has to leave the panel to have something
    // to time against.
    //
    // "Assign to me" resolves to the caller's own user id — the Cognito `sub`, which is exactly
    // what the backend stores as the member/assignee id (`wp_platform::auth`: `user_id = sub`).
    let assignee_id = crate::auth::token::decode_id_claims(&id_token)
        .map(|c| c.sub)
        .unwrap_or_default();
    crate::api::tasks::create_task(
        &client,
        &ingest_url,
        &id_token,
        &project_id,
        &title,
        crate::api::tasks::NewTaskFields {
            assignee_id: &assignee_id,
            ..Default::default()
        },
    )
    .await
}

/// Add a subtask under `task_id` and return it, so the picker can show it (and select it) without
/// waiting for the next poll.
///
/// Only the title is asked for: the server defaults the status to `todo` and the assignee to the
/// caller, which is exactly what an employee breaking down their own work wants.
#[tauri::command]
pub async fn create_subtask(
    state: State<'_, AppState>,
    project_id: String,
    task_id: String,
    title: String,
) -> Result<crate::api::tasks::SubtaskDto, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Err("auth:expired".into());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    crate::api::tasks::create_subtask(
        &client,
        &ingest_url,
        &id_token,
        &project_id,
        &task_id,
        title.trim(),
    )
    .await
}

/// Tick a subtask off, or move it back.
///
/// Fails with `subtask:not-yours` when the caller is a project Member and someone else added the
/// subtask (a Member edits only subtasks they created; a Lead/Manager edits any) — a rule the person
/// can act on, rather than a raw 403 the panel would have to translate.
#[tauri::command]
pub async fn set_subtask_status(
    state: State<'_, AppState>,
    project_id: String,
    task_id: String,
    subtask_id: String,
    status: String,
) -> Result<crate::api::tasks::SubtaskDto, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Err("auth:expired".into());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    crate::api::tasks::set_subtask_status(
        &client,
        &ingest_url,
        &id_token,
        &project_id,
        &task_id,
        &subtask_id,
        &status,
    )
    .await
}

/// Change a task's **status** from the panel (To do / In progress / In review / Done / Blocked).
///
/// Fails with `task:status-not-yours` on the backend's 403: since 2026-09-01 any project member may
/// move a task between the working columns, and only **Done** and **Blocked** need a project
/// Lead/Manager (`projects::update_task`). The panel turns that into a sentence rather than showing
/// a raw 403.
#[tauri::command]
pub async fn set_task_status(
    state: State<'_, AppState>,
    project_id: String,
    task_id: String,
    status: String,
) -> Result<crate::api::tasks::TaskDto, String> {
    let Some(id_token) = state.auth.id_token().await else {
        return Err("auth:expired".into());
    };
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    crate::api::tasks::set_task_status(
        &client,
        &ingest_url,
        &id_token,
        &project_id,
        &task_id,
        &status,
    )
    .await
}

// ── today's sessions (backend-fed) ───────────────────────────────────────────

/// The signed-in user's folded time entries for `date` (the client's local `YYYY-MM-DD`), aggregated
/// per (project, task, subtask, description) — the panel's "Today's sessions". Reads back this
/// agent's own timer events after they fold server-side (`GET /v1/me/timesheet/today`). An error
/// leaves the last-good list.
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    date: String,
) -> Result<Vec<crate::api::timesheet::SessionDto>, String> {
    let id_token = list_token(&state).await?;
    let ingest_url = state.auth.config().ingest_url.clone();
    let client = crate::api::client::api_client();
    note_rejection(
        &state,
        crate::api::timesheet::fetch_today(&client, &ingest_url, &id_token, &date).await,
    )
}

// ── resume after restart ─────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingResumeDto {
    pub task_id: String,
    /// The subtask that was running, so a resume picks up exactly where it left off rather than
    /// silently reattaching to the parent task. Empty when the session targeted the task itself.
    pub subtask_id: String,
    pub project_id: String,
    pub description: String,
    /// Epoch ms the agent closed on this task. The panel compares it against its own local calendar
    /// and only resumes when it is still the same day — the core has no local timezone.
    pub stopped_at_ms: i64,
}

/// The task the agent was running when it last closed, **claimed once**.
///
/// Taking-and-clearing rather than a plain read is deliberate. This drives an auto-start, so a value
/// left in place would resume again on the next launch, and again after that — a task the user
/// stopped days ago quietly restarting forever. Claiming it means one restart resumes one session.
///
/// Returning it does not start anything: the panel decides, because only it knows whether
/// `stopped_at_ms` is still today.
#[tauri::command]
pub fn take_pending_resume(state: State<'_, AppState>) -> Option<PendingResumeDto> {
    // Settle whose state this is first. The panel claims the resume the moment it sees a session,
    // which can be before the sign-in path has reset a previous user's hand-off — and that would
    // start the previous user's task on the new user's clock (QA audit 2026-10-08, WP-206).
    state.adopt_signed_in();
    // Never resume on top of a running timer — a start would be rejected anyway, and this makes the
    // ordering explicit rather than relying on that.
    if state.timer.lock().unwrap().is_running() {
        return None;
    }
    let mut out = None;
    crate::session_state::update(|s| {
        out = s.resume.take().map(|r| PendingResumeDto {
            task_id: r.task_id,
            subtask_id: r.subtask_id,
            project_id: r.project_id,
            description: r.description,
            stopped_at_ms: r.stopped_at_ms,
        });
    });
    out
}

// ── identity ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct IdentityDto {
    pub name: String,
    pub email: String,
    pub avatar_url: String,
    /// `false` only when the role's permissions are readable and lack `TimeTrackSelf` — the panel
    /// then says so instead of offering a Start that `start_timer` refuses (WP-214).
    pub can_track_time: bool,
}

/// Who this device reports as — the person's name + email from the Cognito ID-token claims (the
/// `name`/`email` claims, never the `username`/`sub` UUID). `None` when signed out (the panel hides
/// the avatar rather than inventing a person).
#[tauri::command]
pub fn identity(state: State<'_, AppState>) -> Option<IdentityDto> {
    let (name, email) = state.auth.identity()?;
    Some(IdentityDto {
        name,
        email,
        avatar_url: String::new(),
        can_track_time: state.auth.can_track_time() != Some(false),
    })
}
