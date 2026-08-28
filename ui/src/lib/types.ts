/**
 * Mirrors the serde shapes returned by the core's Tauri commands.
 *
 * Source of truth:
 *   src-tauri/src/commands/panel.rs   (snake_case DTOs — policy_version, next_cycle_secs, …)
 *   src-tauri/src/commands/mod.rs     (auth DTOs — camelCase, see AuthStatus)
 *   src-tauri/src/api/{projects,tasks,timesheet}.rs  (backend-fed rows)
 *   ../backend/crates/wp-agent-contract/src/config.rs (TrackingConfig)
 *
 * Hand-maintained, no codegen — if a DTO changes Rust-side, change it here too.
 */

/** Mirrors Rust `Cadence`: presets as strings, a custom interval as `{ custom: minutes }`. */
export type Cadence = "off" | "min3" | "min5" | "min10" | { custom: number };

/** panel.rs — ConsentStateDto */
export interface ConsentState {
  granted: boolean;
  policy_version: number;
  captured: string[];
}

/** panel.rs — CaptureStateDto */
export interface CaptureState {
  capturing: boolean;
  screenshots: boolean;
  next_cycle_secs: number;
}

/** wp-agent-contract config.rs — TrackingConfig */
export interface TrackingConfig {
  version: number;
  cadence: Cadence;
  /** 0 = none … higher = stronger blur applied on-device before upload. */
  blur_level: number;
  retention_days: number;
  /** Silent monitoring (still consent-gated; no tray capture indicator). */
  silent: boolean;
}

/** panel.rs — PauseGrantDto */
export interface PauseGrant {
  granted: boolean;
  granted_secs: number;
  remaining_budget_secs: number;
}

/**
 * panel.rs — PauseStateDto. The authoritative pause window, read from the core each poll.
 *
 * The panel must not count this down locally: the core owns the budget, and a webview reload used
 * to lose the countdown while capture stayed suspended.
 */
export interface PauseState {
  paused: boolean;
  remaining_secs: number;
  remaining_budget_secs: number;
}

/**
 * panel.rs — TimerStateDto.
 *
 * `project_id` and `description` are what the backend folds time entries by, so they are part of
 * the state, not just start arguments — the panel reads them back to keep its inputs pointed at
 * whatever the core is actually timing.
 */
export interface TimerState {
  running: boolean;
  task_id: string | null;
  /** The subtask being worked on, when one was picked. `task_id` stays the parent task. */
  subtask_id: string | null;
  project_id: string | null;
  description: string;
  elapsed_secs: number;
}

/** api/projects.rs — ProjectDto (`GET /v1/projects`). */
export interface Project {
  id: string;
  name: string;
  billable: boolean;
  status: string;
}

/**
 * api/tasks.rs — TaskDto (`GET /v1/me/tasks`) **joined onto its project**.
 *
 * The wire row carries only `{id, title, project_id}`. `project_name` and `billable` are filled in
 * by `agent.ts` from the projects list; a task whose project isn't in that list falls back to
 * "Unassigned" rather than rendering a raw id.
 */
/**
 * One subtask — a single level of breakdown under a task.
 *
 * Real work, not a checklist tick: the timer runs against it, and this app is the only place they
 * are created or ticked off (the web shows them read-only).
 */
export interface Subtask {
  id: string;
  task_id: string;
  title: string;
  /** `todo` | `in_progress` | `in_review` | `done` | `blocked`. */
  status: string;
  /** Both terminal states count — a subtask signed off elsewhere is finished, not outstanding. */
  done: boolean;
}

export interface Task {
  id: string;
  title: string;
  project_id: string;
  project_name: string;
  billable: boolean;
  /** This task's breakdown. Empty is the normal case and means the timer targets the task itself. */
  subtasks: Subtask[];
  /**
   * Nobody has taken this task — it is offered to every member of its project.
   *
   * Starting a timer on one does **not** claim it: the time is recorded against the task and it
   * stays available to everyone, which is why the picker labels it rather than hiding the fact.
   */
  unassigned: boolean;
}

/**
 * Recent input-activity buckets, oldest → newest, for the activity bars.
 *
 * PROPOSED — no command returns this. The agent does collect keystroke/click *counts*
 * (INGESTION.md §1: "keystroke + click counts", never values), so showing a person their
 * own recorded activity is consistent with the transparency view (PRIVACY.md §5) — but it
 * needs a `recent_activity()` command before it can show anything real.
 */
export type ActivitySeries = number[];

/**
 * api/timesheet.rs — SessionDto (`GET /v1/me/timesheet/today`).
 *
 * The server folds this agent's own TimerStarted/Stopped events into time entries and aggregates
 * them per **(project, description)** — not per task. A session therefore has no `task_id`, and
 * the panel must not try to look one up.
 */
export interface Session {
  project_id: string;
  description: string;
  secs: number;
}

/**
 * privacy_log.rs — `LocalEvent`, served by the `privacy_log` command, newest first.
 *
 * The employee's own record of things done to this machine that they could not otherwise observe —
 * today, admin-triggered on-demand captures (taken **and** refused). PRIVACY.md §5: no silent
 * access. `detail` is a ready-to-render sentence; the panel never composes one from `kind`.
 */
export interface LocalEvent {
  /** Epoch ms. */
  ts: number;
  /** `admin_capture` | `admin_capture_refused` — more kinds may appear; render `detail`. */
  kind: string;
  detail: string;
}

/** panel.rs — IdentityDto. Name/email come from the ID-token claims, never the `sub` UUID. */
export interface Identity {
  name: string;
  email: string;
  avatar_url: string;
}

/**
 * commands/mod.rs — AuthStatus. **camelCase** (`#[serde(rename_all = "camelCase")]`), unlike the
 * panel DTOs above; optional fields are omitted entirely rather than sent as null.
 */
export interface AuthStatus {
  signedIn: boolean;
  tenantId?: string;
  username?: string;
  /** Present when Cognito demands a new password (admin-created first login). */
  newPasswordSession?: string;
  /** Present when Cognito demands a second factor — answer with `completeMfa`. */
  mfaSession?: string;
  /** `SOFTWARE_TOKEN_MFA` (authenticator app) or `SMS_MFA` (text message). */
  mfaChallenge?: string;
}

/** What `start_timer` needs. `task_id` is optional core-side — a project alone is a valid session. */
export interface TimerSelection {
  taskId: string | null;
  /**
   * The subtask to run against, or null for the task itself.
   *
   * `taskId` above is **always the parent task**, even when this is set — that is the server
   * contract, and it is what keeps project hours, timesheets and the project KPI attributing this
   * session correctly.
   */
  subtaskId: string | null;
  projectId: string | null;
  description: string;
}

/** Everything the panel renders, polled as one snapshot. */
export interface AgentSnapshot {
  auth: AuthStatus;
  /** Null when signed out — the panel hides the avatar rather than inventing a person. */
  identity: Identity | null;
  consent: ConsentState;
  capture: CaptureState;
  config: TrackingConfig;
  timer: TimerState;
  pause: PauseState;
  projects: Project[];
  tasks: Task[];
  /** Still empty — no `recent_activity` command exists (see ActivitySeries above). */
  activity: ActivitySeries;
  sessions: Session[];
  /** Local transparency log, newest first. Empty for a machine nothing has been done to. */
  privacyLog: LocalEvent[];
}

const CADENCE_PRESET_LABEL: Record<Exclude<Cadence, object>, string> = {
  off: "Off",
  min3: "Every 3 min",
  min5: "Every 5 min",
  min10: "Every 10 min",
};

export function cadenceLabel(c: Cadence): string {
  return typeof c === "object" ? `Every ${c.custom} min` : CADENCE_PRESET_LABEL[c];
}

export const BLUR_LABEL = ["None", "Light", "Moderate", "Heavy"] as const;
