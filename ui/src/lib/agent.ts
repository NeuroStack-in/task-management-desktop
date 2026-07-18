/**
 * The single seam between the panel UI and the core.
 *
 * Dev (`npm run dev`, and `just tray-dev`) talks to the fake core in mock.ts, because the real
 * Tauri commands are still `TODO(ipc)` stubs that return frozen constants — you cannot design
 * against a timer that never ticks. Production builds always invoke the real commands.
 * Override with `VITE_REAL=1 npm run dev` to see the actual (currently frozen) stub output.
 */

import * as mock from "./mock";
import type {
  AgentSnapshot,
  ConsentState,
  PauseGrant,
  Project,
  Session,
  Task,
  TimerState,
} from "./types";

/**
 * The fast, **local** slice of the snapshot — read from the core every second to drive the timer.
 * All five are in-process Rust reads (no network), so the 1 s poll stays instant and the timer ticks
 * smoothly. Projects/tasks/sessions (which DO hit the backend) are fetched separately, on a slower
 * cadence — see `fetchProjects`/`fetchTasks`/`fetchSessions`.
 */
export type LocalState = Pick<
  AgentSnapshot,
  "identity" | "consent" | "capture" | "config" | "timer"
>;

export const USE_MOCK = import.meta.env.DEV && import.meta.env.VITE_REAL !== "1";

type TauriWindow = Window & {
  __TAURI__?: { core?: { invoke<T>(cmd: string, args?: unknown): Promise<T> } };
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

/** The client's own local date (YYYY-MM-DD) — "today" is the user's calendar, not the server's UTC. */
function localDate(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** The fast local slice — polled every second. All in-process reads; never touches the network. */
export async function readLocal(): Promise<LocalState> {
  if (USE_MOCK) {
    const s = mock.read();
    return { identity: s.identity, consent: s.consent, capture: s.capture, config: s.config, timer: s.timer };
  }
  const [consent, capture, config, timer, identity] = await Promise.all([
    invoke<LocalState["consent"]>("get_consent_state"),
    invoke<LocalState["capture"]>("capture_state"),
    invoke<LocalState["config"]>("effective_config"),
    invoke<LocalState["timer"]>("timer_state"),
    invoke<LocalState["identity"]>("identity"),
  ]);
  return { identity, consent, capture, config, timer };
}

/** The user's projects (`GET /v1/projects`). Fetched on mount + refresh — not every second. */
export async function fetchProjects(): Promise<Project[]> {
  if (USE_MOCK) return mock.read().projects;
  return invoke<Project[]>("list_projects").catch(() => []);
}

/** The user's tasks (`GET /v1/me/tasks`). Fetched on mount + refresh — not every second. */
export async function fetchTasks(): Promise<Task[]> {
  if (USE_MOCK) return mock.read().tasks;
  return invoke<Task[]>("list_tasks").catch(() => []);
}

/** Today's folded sessions (`GET /v1/me/timesheet/today`). Fetched periodically + after start/stop. */
export async function fetchSessions(): Promise<Session[]> {
  if (USE_MOCK) return mock.read().sessions;
  return invoke<Session[]>("list_sessions", { date: localDate() }).catch(() => []);
}

/**
 * Fold the running session's live segment into its (project, description) row so it ticks between
 * server refreshes. The server's completed totals exclude the still-open session (no duration yet),
 * so this never double-counts — once it stops and folds, `timer.running` is false and the sum wins.
 */
export function withLiveSession(sessions: Session[], timer: TimerState): Session[] {
  if (!timer.running || !timer.project_id) return sessions;
  const desc = timer.description.trim();
  const out = sessions.map((s) => ({ ...s }));
  const row = out.find((s) => s.project_id === timer.project_id && s.description === desc);
  if (row) row.secs += timer.elapsed_secs;
  else out.push({ project_id: timer.project_id, description: desc, secs: timer.elapsed_secs });
  return out;
}

/** @deprecated kept for any legacy caller — prefer `readLocal` + the `fetch*` helpers. */
export async function readSnapshot(): Promise<AgentSnapshot> {
  if (USE_MOCK) return mock.read();
  const [local, projects, tasks, sessions] = await Promise.all([
    readLocal(),
    fetchProjects(),
    fetchTasks(),
    fetchSessions(),
  ]);
  return {
    ...local,
    projects,
    tasks,
    activity: [],
    sessions: withLiveSession(sessions, local.timer),
  };
}

export async function grantConsent(policyVersion: number): Promise<void> {
  if (USE_MOCK) return mock.grantConsent(policyVersion);
  await invoke<ConsentState>("grant_consent", { policyVersion });
}

/** Start a session against a project + optional task, with the user's free-text description. */
export async function startSession(
  projectId: string,
  description: string,
  taskId?: string,
): Promise<void> {
  if (USE_MOCK) return mock.startSession(projectId, description, taskId);
  await invoke<TimerState>("start_timer", { projectId, description, taskId });
}

/**
 * Re-attribute the running session to another project/task/description. There is no atomic `switch`
 * command, so against the real core this is stop + start (as the docs describe the core doing,
 * just not atomically). When nothing is running it's a plain start.
 */
export async function switchSession(
  projectId: string,
  description: string,
  running: boolean,
  taskId?: string,
): Promise<void> {
  if (!running) return startSession(projectId, description, taskId);
  if (USE_MOCK) return mock.switchSession(projectId, description, taskId);
  await invoke<TimerState>("stop_timer");
  await invoke<TimerState>("start_timer", { projectId, description, taskId });
}

export async function stopTimer(): Promise<void> {
  if (USE_MOCK) return mock.stopTimer();
  await invoke<TimerState>("stop_timer");
}

export async function requestPause(requestedSecs: number): Promise<PauseGrant> {
  if (USE_MOCK) return mock.requestPause(requestedSecs);
  return invoke<PauseGrant>("request_pause", { requestedSecs });
}

/**
 * Seconds already remaining on a pause at mount. The core exposes no pause-state read command
 * yet (only `request_pause`), so outside mock we start from zero and track locally.
 */
export function initialPauseSecs(): number {
  return USE_MOCK ? mock.pauseRemainingSecs() : 0;
}
