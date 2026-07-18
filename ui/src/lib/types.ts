/**
 * Mirrors the serde shapes returned by the tray's Tauri commands.
 * Source of truth: tray/src-tauri/src/features/*.rs and
 * backend/crates/wp-agent-contract/src/config.rs (TrackingConfig).
 */

export type Cadence = "off" | "min3" | "min5" | "min10";

/** tray/src-tauri/src/features/consent.rs */
export interface ConsentState {
  granted: boolean;
  policy_version: number;
  captured: string[];
}

/** tray/src-tauri/src/features/capture_indicator.rs */
export interface CaptureState {
  capturing: boolean;
  screenshots: boolean;
  next_cycle_secs: number;
}

/** backend/crates/wp-agent-contract/src/config.rs */
export interface TrackingConfig {
  version: number;
  cadence: Cadence;
  /** 0 = none … higher = stronger blur applied on-device before upload. */
  blur_level: number;
  retention_days: number;
  /** Silent monitoring (still consent-gated; no tray capture indicator). */
  silent: boolean;
}

/** tray/src-tauri/src/features/pause.rs */
export interface PauseGrant {
  granted: boolean;
  granted_secs: number;
  remaining_budget_secs: number;
}

/** tray/src-tauri/src/features/timer_ui.rs */
export interface TimerState {
  running: boolean;
  task_id: string | null;
  /** The project the running session is attributed to (null in a task-only/legacy session). */
  project_id: string | null;
  /** What the user typed they're working on ("what are you working on?"). */
  description: string;
  elapsed_secs: number;
}

/**
 * A project the timer can be attributed to — the "Select project" picker's rows.
 *
 * Real: `list_projects` fetches `GET /v1/projects` with the user's JWT (id/name/billable). Mock:
 * derived from the demo task list. `billable` is display-only on the agent side.
 */
export interface Project {
  id: string;
  name: string;
  billable: boolean;
}

/**
 * A task the timer can be attributed to, with the project it belongs to.
 *
 * Field names mirror the core's IPC contract, which already carries a project:
 * `TrayCommand::StartTimer { task_id, project_id, description }`
 * (crates/agent-shared/src/ipc.rs:37). `billable` matches the web app's TaskOption
 * (mock-time.ts:23) and has no agent-side meaning — it's display only.
 *
 * PROPOSED — no command returns these yet. `start_timer(task_id)` takes an id, so the core
 * can *consume* a task selection; it can't supply the list, and its Tauri command doesn't
 * yet expose the `project_id`/`description` the IPC command underneath already accepts.
 */
export interface Task {
  id: string;
  title: string;
  project_id: string;
  project_name: string;
  billable: boolean;
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
 * A task worked today and its total for the day, including any live segment.
 *
 * Mirrors the web app's per-task day clock (timer.store.ts: "Banked seconds **per task**
 * … so returning to a task always resumes from its day total").
 *
 * PROPOSED — no command returns this. The core does own the authoritative timer state
 * machine and emits TimerStarted/TimerStopped (timer_ui.rs), so it has the raw material;
 * it needs a `sessions_today()` command to expose the roll-up.
 */
export interface Session {
  project_id: string;
  /** What was worked on (the free-text description); the session's human label. */
  description: string;
  secs: number;
}

/**
 * The person this device reports as.
 *
 * ENROLLMENT.md §2: "A device is a first-class, org-scoped entity bound to exactly one user."
 * Showing who that is, is transparency — the monitored person should never have to guess whose
 * timesheet their activity lands on.
 *
 * PROPOSED — no command returns this. The binding is established at enrollment and the core
 * holds it; it needs an `identity()` command (or the state push) to surface it.
 * `avatar_url` mirrors the web app's User.avatarUrl, which is empty for seeded users — the
 * gradient monogram is the intended default, not a missing image.
 */
export interface Identity {
  name: string;
  email: string;
  avatar_url: string;
}

/** Everything the panel renders, polled as one snapshot. */
export interface AgentSnapshot {
  /** Null until the proposed command above exists. */
  identity: Identity | null;
  consent: ConsentState;
  capture: CaptureState;
  config: TrackingConfig;
  timer: TimerState;
  /** The user's projects for the picker — real, from `GET /v1/projects`. */
  projects: Project[];
  /** Empty until the proposed commands above exist. */
  tasks: Task[];
  activity: ActivitySeries;
  sessions: Session[];
}

export const CADENCE_LABEL: Record<Cadence, string> = {
  off: "Off",
  min3: "Every 3 min",
  min5: "Every 5 min",
  min10: "Every 10 min",
};

export const BLUR_LABEL = ["None", "Light", "Moderate", "Heavy"] as const;
