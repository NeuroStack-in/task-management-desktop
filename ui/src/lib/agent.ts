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

  const base = { auth, consent, capture, config, timer, pause, activity: [] as number[] };

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

export function authStatus(): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_status");
}

/** May resolve with `newPasswordSession` set instead of `signedIn` — see `completeNewPassword`. */
export function login(username: string, password: string): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_login", { username, password });
}

/** Second leg of the admin-created-account first login, using the session from `login`. */
export function completeNewPassword(
  username: string,
  newPassword: string,
  session: string,
): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_complete_new_password", { username, newPassword, session });
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
    projectId: sel.projectId,
    description: sel.description,
  });
}

export async function stopTimer(): Promise<void> {
  await invoke<TimerState>("stop_timer");
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
