/**
 * The single seam between the panel UI and the core.
 *
 * Every read and write below invokes a real Tauri command. There is no mock layer: the panel
 * shows what the core knows, or it shows nothing. Where the core has no command yet (activity
 * series, pause-state readback) the gap is explicit and commented, never papered over with a
 * plausible-looking constant.
 *
 * The webview never talks to the backend directly — the production CSP (`default-src 'self'`,
 * tauri.conf.json) has no `connect-src` for the API by design. All network egress is Rust-side.
 * Keep it that way.
 */

import type {
  AgentSnapshot,
  AuthStatus,
  ConsentState,
  Identity,
  PauseGrant,
  PauseState,
  Project,
  Session,
  Subtask,
  Task,
  TimerSelection,
  TimerState,
} from "./types";

type UnlistenFn = () => void;

type TauriWindow = Window & {
  __TAURI__?: {
    core?: { invoke<T>(cmd: string, args?: unknown): Promise<T> };
    event?: {
      listen<T>(ev: string, cb: (e: { payload: T }) => void): Promise<UnlistenFn>;
    };
  };
};

function invoke<T>(cmd: string, args?: unknown): Promise<T> {
  const fn = (window as TauriWindow).__TAURI__?.core?.invoke;
  if (!fn) {
    return Promise.reject(
      new Error(`Tauri bridge unavailable — cannot invoke "${cmd}" outside the app shell.`),
    );
  }
  return fn<T>(cmd, args);
}

/**
 * Event names the core emits (src-tauri/src/events.rs). Keep this in sync with that file — it is
 * the `ui/src/lib/ipc.ts` its doc-comment refers to, which never existed until now.
 */
export const EVENTS = {
  authExpired: "auth:expired",
  idlePrompt: "monitor:idle-prompt",
  trackingChanged: "monitor:tracking-changed",
  screenshotUnavailable: "monitor:screenshot-unavailable",
  /** A restricted app/site was focused while tracking; payload = the offending identifier. */
  policyBlocked: "monitor:policy-blocked",
  /**
   * IT released this device: the core stopped the timer, flushed what it owed and signed out. The
   * panel is about to drop to the sign-in screen, so this is the only chance to say why — an
   * unexplained bounce to login reads as a crash rather than something an admin deliberately did.
   */
  deviceReleased: "device:released",
} as const;

/**
 * Subscribe to a core event. Resolves to an unlisten function; if the bridge is missing (browser
 * dev shell) it resolves to a no-op rather than throwing, so a caller's cleanup path is uniform.
 */
export function listen<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  const fn = (window as TauriWindow).__TAURI__?.event?.listen;
  if (!fn) return Promise.resolve(() => {});
  return fn<T>(event, (e) => cb(e.payload));
}

/** api/tasks.rs `TaskDto` — the raw wire row, before the project join. */
interface TaskRow {
  id: string;
  title: string;
  project_id: string;
  unassigned?: boolean;
  status?: string;
  subtasks?: SubtaskRow[];
}

/** api/tasks.rs `SubtaskDto`. */
interface SubtaskRow {
  id: string;
  task_id: string;
  title: string;
  status: string;
}

/**
 * The backend-fed reads return `Result<_, String>`, which Tauri surfaces as a rejection — most
 * often `"auth:expired"` on a 401. One failing list must not blank the whole panel, and it must
 * not be mistaken for "the core is unreachable": the core is fine, the token isn't. Rust
 * auto-logs-out on a failed refresh, so the next poll's `auth_status` flips to signed-out and
 * the sign-in screen takes over on its own.
 */
async function soft<T>(p: Promise<T>, fallback: T): Promise<T> {
  try {
    return await p;
  } catch {
    return fallback;
  }
}

/**
 * The client's **local** `YYYY-MM-DD`, which is what `list_sessions` expects — the Lambda runs in
 * UTC and cannot know the user's day. `toISOString()` would be wrong here: it renders the UTC
 * date, so anyone east or west of UTC gets yesterday's or tomorrow's sessions near midnight.
 */
function localDate(d = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * `GET /v1/me/tasks` carries only `{id, title, project_id}` — the title/billable a picker needs
 * live on the project. Joined here rather than Rust-side so the two lists stay independently
 * cacheable and a project with no tasks still reaches the project picker.
 */
function joinTasks(rows: TaskRow[], projects: Project[]): Task[] {
  const byId = new Map(projects.map((p) => [p.id, p]));
  return rows.map((t) => {
    const p = byId.get(t.project_id);
    return {
      id: t.id,
      title: t.title,
      project_id: t.project_id,
      // A task whose project the user can't see still renders — labelled, not blank, and never
      // as a raw id.
      project_name: p?.name ?? "Unassigned",
      billable: p?.billable ?? false,
      // Absent means the backend predates the flag; read that as "assigned" so an old core doesn't
      // make the whole picker look like a free-for-all.
      unassigned: t.unassigned ?? false,
      // Absent reads as open. A task shown as finished when the server never said so would invite
      // someone to "reopen" work that was never closed.
      status: t.status || "todo",
      // Absent means no breakdown, which is every task today — the picker then offers the task.
      subtasks: (t.subtasks ?? []).map((s) => ({
        id: s.id,
        task_id: s.task_id,
        title: s.title,
        status: s.status,
        // `closed` cannot be set from here but can arrive on a row; counting only `done` would
        // show signed-off work as still outstanding.
        done: s.status === "done" || s.status === "closed",
      })),
    };
  });
}

export async function readSnapshot(): Promise<AgentSnapshot> {
  // Auth first: signed out, the backend-fed lists are all empty anyway (the commands short-circuit
  // on a missing token), so skipping them saves four IPC round-trips per poll.
  const auth = await invoke<AuthStatus>("auth_status");

  const [consent, capture, config, timer, pause] = await Promise.all([
    invoke<AgentSnapshot["consent"]>("get_consent_state"),
    invoke<AgentSnapshot["capture"]>("capture_state"),
    invoke<AgentSnapshot["config"]>("effective_config"),
    invoke<TimerState>("timer_state"),
    invoke<PauseState>("pause_state"),
  ]);

  const base = {
    auth,
    consent,
    capture,
    config,
    timer,
    pause,
    activity: [] as number[],
  };

  if (!auth.signedIn) {
    return { ...base, identity: null, projects: [], tasks: [], sessions: [] };
  }

  const [identity, rows, projects, sessions] = await Promise.all([
    soft(invoke<Identity | null>("identity"), null),
    soft(invoke<TaskRow[]>("list_tasks"), []),
    soft(invoke<Project[]>("list_projects"), []),
    soft(invoke<Session[]>("list_sessions", { date: localDate() }), []),
  ]);

  return { ...base, identity, projects, tasks: joinTasks(rows, projects), sessions };
}

// ── auth ─────────────────────────────────────────────────────────────────────

/** The account's stored appearance (`GET /v1/me/appearance`). */
export interface Appearance {
  /** `"light"`, `"dark"`, or `"system"` — the last an explicit choice, not an absence. */
  theme: string;
  /** One of the web app's palette ids (`fireopal`, `meridian`, …). May be empty. */
  palette: string;
}

/**
 * The appearance stored on the account, or `null` when signed out or unreachable — the panel then
 * keeps what it is showing rather than flipping to a default.
 */
export function getAppearance(): Promise<Appearance | null> {
  return invoke<Appearance | null>("appearance");
}

export function authStatus(): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_status");
}

/** May resolve with `newPasswordSession` set instead of `signedIn` — see `completeNewPassword`. */
export function login(username: string, password: string): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_login", { username, password });
}

/**
 * Sign in with Google. Opens the system browser to the Cognito Hosted UI (native OAuth + PKCE),
 * returns to the app via the `workpulse://callback` deep link, and resolves to the same `AuthStatus`
 * as a password login — so the caller handles `signedIn` identically. Rejects if the browser flow is
 * cancelled or times out.
 */
export function loginWithGoogle(): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_login_google");
}

/**
 * Abandon a Google sign-in that is still waiting on the browser.
 *
 * This makes the pending {@link loginWithGoogle} promise **reject** with the ordinary "sign-in was
 * cancelled" error, so the caller's existing `catch`/`finally` does the cleanup and no separate
 * teardown path is needed. Without it the only exit was the agent's five-minute timeout, which
 * leaves the sign-in card disabled long enough to look like a hang.
 *
 * Resolves `false` when nothing was waiting — a cancel that raced the redirect home.
 */
export function cancelGoogleLogin(): Promise<boolean> {
  return invoke<boolean>("auth_cancel_google");
}

/**
 * Open the WorkPulse web app in the user's system browser (org sign-up, invites, the full dashboard).
 * Egress stays Rust-side — the panel never navigates, so this is a core command, not an `<a href>`.
 */
export async function openWebsite(): Promise<void> {
  await invoke<void>("open_website");
}

/** Second leg of the admin-created-account first login, using the session from `login`. */
export function completeNewPassword(
  username: string,
  newPassword: string,
  session: string,
): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_complete_new_password", { username, newPassword, session });
}

/**
 * Answer an MFA challenge with the user's 6-digit code, using the session from `login`.
 *
 * `challenge` is echoed back rather than assumed: the answer field Cognito expects differs between
 * TOTP and SMS, and sending the wrong one fails as a generic parameter error.
 */
export function completeMfa(
  challenge: string,
  username: string,
  code: string,
  session: string,
): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_complete_mfa", { challenge, username, code, session });
}

export async function logout(): Promise<void> {
  await invoke<void>("auth_logout");
}

// ── consent ──────────────────────────────────────────────────────────────────

export async function grantConsent(policyVersion: number): Promise<void> {
  await invoke<ConsentState>("grant_consent", { policyVersion });
}

// ── timer ────────────────────────────────────────────────────────────────────

/**
 * `project_id` and `description` are not optional in practice even though the command accepts
 * them as such: the server folds time entries per (project, description), so a start missing
 * both lands in an unlabelled bucket the user can't tell apart from any other.
 */
export async function startTimer(sel: TimerSelection): Promise<void> {
  await invoke<TimerState>("start_timer", {
    taskId: sel.taskId,
    subtaskId: sel.subtaskId,
    projectId: sel.projectId,
    description: sel.description,
  });
}

export async function stopTimer(): Promise<void> {
  await invoke<TimerState>("stop_timer");
}

/**
 * Create a task in a project and return it.
 *
 * Only the title is sent, and the core assigns it to the caller: someone creating a task from their
 * own timer panel is telling you who is going to do it. Description, due date and priority are
 * editable in the web app, which is where task detail belongs — this exists so nobody has to open a
 * browser to have something to time against.
 *
 * Returns the task rather than an id so the picker can select it immediately, without waiting for
 * the next poll to join it into the list.
 */
export async function createTask(projectId: string, title: string): Promise<Task> {
  const row = await invoke<TaskRow>("create_task", { projectId, title });
  return {
    id: row.id,
    title: row.title,
    project_id: row.project_id,
    // The create response carries no project name or billable flag — both belong to the project,
    // not the task, and the next poll fills them in. Blank beats guessing: a wrong billable flag on
    // a row someone is about to time is a billing error, not a cosmetic one.
    project_name: "",
    billable: false,
    status: row.status ?? "todo",
    subtasks: [],
    unassigned: row.unassigned ?? false,
  };
}

/**
 * Add a subtask under a task and return it.
 *
 * Only the title is sent: the core's server defaults the status to `todo` and the assignee to the
 * signed-in user, which is what breaking down your own work means.
 */
export async function createSubtask(
  projectId: string,
  taskId: string,
  title: string,
): Promise<Subtask> {
  const row = await invoke<SubtaskRow>("create_subtask", { projectId, taskId, title });
  return {
    id: row.id,
    task_id: row.task_id,
    title: row.title,
    status: row.status,
    done: row.status === "done" || row.status === "closed",
  };
}

/**
 * Tick a subtask off, or move it back to `todo`.
 *
 * Rejects with `subtask:not-yours` when it belongs to someone else and the caller is only a project
 * Member — a rule the person can act on, which the panel shows as-is.
 */
export async function setSubtaskStatus(
  projectId: string,
  taskId: string,
  subtaskId: string,
  status: string,
): Promise<void> {
  await invoke<SubtaskRow>("set_subtask_status", { projectId, taskId, subtaskId, status });
}

/**
 * Change a **task's** status (`todo` | `in_progress` | `in_review` | `done` | `blocked`).
 *
 * Rejects with `task:status-not-yours` when the task is someone else's and the caller is only a
 * project Member — a plain Member may move only a task they're assigned to; a Lead/Manager moves
 * anyone's. `closed` is not settable here (it's the reviewed state). The panel re-reads after, so no
 * return value is needed.
 */
export async function setTaskStatus(
  projectId: string,
  taskId: string,
  status: string,
): Promise<void> {
  await invoke<TaskRow>("set_task_status", { projectId, taskId, status });
}

/** The task the agent was running when it last closed. See {@link takePendingResume}. */
export interface PendingResume {
  taskId: string;
  /** The subtask that was running, so a resume picks up exactly where it left off. */
  subtaskId: string;
  projectId: string;
  description: string;
  /** Epoch ms the agent closed on this task. */
  stoppedAtMs: number;
}

/**
 * Claim the task the agent was running when it last closed, if any.
 *
 * **Claiming, not reading**: the core clears it as it hands it over, so one restart resumes at most
 * one session. A plain read would let a task the user stopped days ago restart on every launch.
 *
 * The core deliberately does not decide whether to resume — it has no local timezone (its only clock
 * is UTC ms, which is also why `listSessions` takes the date as a parameter). The caller compares
 * `stoppedAtMs` against the local calendar; see `sameLocalDay`.
 */
export function takePendingResume(): Promise<PendingResume | null> {
  return invoke<PendingResume | null>("take_pending_resume");
}

/**
 * Re-attribute a running timer. There is no atomic `switch_task` command, so this is stop + start
 * — two events, and a sub-second gap between them the backend will see. Worth a real command if
 * switching turns out to be common.
 */
export async function switchTo(sel: TimerSelection): Promise<void> {
  await invoke<TimerState>("stop_timer");
  await startTimer(sel);
}

// ── privacy pause ────────────────────────────────────────────────────────────

export function requestPause(requestedSecs: number): Promise<PauseGrant> {
  return invoke<PauseGrant>("request_pause", { requestedSecs });
}

// ── launch at login ────────────────────────────────────────────────────────────

/**
 * The real OS launch-at-login state (macOS LaunchAgent / Windows Run key / Linux .desktop entry),
 * read live from the autostart plugin — not a cached guess, so the toggle reflects reality.
 */
export function getAutoStart(): Promise<boolean> {
  return invoke<boolean>("get_auto_start");
}

/** Enable or disable launch-at-login. On Windows the installer sets the initial value; this changes it. */
export async function setAutoStart(enabled: boolean): Promise<void> {
  await invoke<void>("set_auto_start", { enabled });
}

// ── self-update ────────────────────────────────────────────────────────────────────────────────

export interface UpdateStatus {
  /** The version running right now. */
  current: string;
  /** Whether the check actually reached the manifest. `false` = offline or no signing key. */
  checked: boolean;
  /** Newer version available, or `null` when already current. */
  latest: string | null;
  /** Why the check failed, when it did. */
  error: string | null;
}

/**
 * Ask whether a newer signed build exists — **without installing it**.
 *
 * The agent already self-updates on its own (at launch, then every 6 h). This exists so the panel
 * can *say so*: before it, the only evidence an update had happened was the version silently
 * changing, and there was no way to answer "am I on the latest?" without reading a log file.
 */
export function updateStatus(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_status");
}

/** Install the pending update now. Resolves with the version installed; the app then relaunches. */
export function updateInstall(): Promise<string> {
  return invoke<string>("update_install");
}
