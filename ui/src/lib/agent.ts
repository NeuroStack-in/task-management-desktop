/**
 * The single seam between the panel UI and the core.
 *
 * Dev (`npm run dev`, and `just tray-dev`) talks to the fake core in mock.ts, because the real
 * Tauri commands are still `TODO(ipc)` stubs that return frozen constants — you cannot design
 * against a timer that never ticks. Production builds always invoke the real commands.
 * Override with `VITE_REAL=1 npm run dev` to see the actual (currently frozen) stub output.
 */

import * as mock from "./mock";
import type { AgentSnapshot, ConsentState, PauseGrant, TimerState } from "./types";

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

export async function readSnapshot(): Promise<AgentSnapshot> {
  if (USE_MOCK) return mock.read();

  // Independent reads against the real single-process core. `list_projects` and `list_sessions` hit
  // the backend (GET /v1/projects, GET /v1/me/timesheet/today); both fail soft to [] so a backend
  // outage doesn't blank the panel.
  const [consent, capture, config, timer, identity, projects, tasks, sessions] = await Promise.all([
    invoke<AgentSnapshot["consent"]>("get_consent_state"),
    invoke<AgentSnapshot["capture"]>("capture_state"),
    invoke<AgentSnapshot["config"]>("effective_config"),
    invoke<AgentSnapshot["timer"]>("timer_state"),
    invoke<AgentSnapshot["identity"]>("identity"),
    invoke<AgentSnapshot["projects"]>("list_projects").catch((): AgentSnapshot["projects"] => []),
    invoke<AgentSnapshot["tasks"]>("list_tasks").catch((): AgentSnapshot["tasks"] => []),
    invoke<AgentSnapshot["sessions"]>("list_sessions", { date: localDate() }).catch(
      (): AgentSnapshot["sessions"] => [],
    ),
  ]);

  // Fold the running session's live segment into its (project, description) row so it ticks. The
  // server's completed totals exclude the still-open session (no duration yet), so this never
  // double-counts — once it stops and folds, `timer.running` is false and the sum takes over.
  if (timer.running && timer.project_id) {
    const desc = timer.description.trim();
    const row = sessions.find((s) => s.project_id === timer.project_id && s.description === desc);
    if (row) row.secs += timer.elapsed_secs;
    else sessions.push({ project_id: timer.project_id, description: desc, secs: timer.elapsed_secs });
  }

  // `tasks` and `activity` still have no command (see types.ts) — empty. `identity` comes from the
  // Cognito claims.
  return {
    identity,
    consent,
    capture,
    config,
    timer,
    projects,
    tasks,
    activity: [],
    sessions,
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
