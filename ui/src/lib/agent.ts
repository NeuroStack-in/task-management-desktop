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

export async function readSnapshot(): Promise<AgentSnapshot> {
  if (USE_MOCK) return mock.read();

  // Independent reads against the real single-process core.
  const [consent, capture, config, timer, identity] = await Promise.all([
    invoke<AgentSnapshot["consent"]>("get_consent_state"),
    invoke<AgentSnapshot["capture"]>("capture_state"),
    invoke<AgentSnapshot["config"]>("effective_config"),
    invoke<AgentSnapshot["timer"]>("timer_state"),
    invoke<AgentSnapshot["identity"]>("identity"),
  ]);
  // `tasks`, `activity` and `sessions` have no command yet (see types.ts) — empty, so the UI
  // degrades to an empty sessions list rather than inventing numbers. `identity` comes from the
  // Cognito claims.
  return {
    identity,
    consent,
    capture,
    config,
    timer,
    tasks: [],
    activity: [],
    sessions: [],
  };
}

export async function grantConsent(policyVersion: number): Promise<void> {
  if (USE_MOCK) return mock.grantConsent(policyVersion);
  await invoke<ConsentState>("grant_consent", { policyVersion });
}

export async function startTimer(taskId: string | null): Promise<void> {
  if (USE_MOCK) return mock.startTimer(taskId);
  await invoke<TimerState>("start_timer", { taskId });
}

/**
 * Re-attribute the timer to another task. There is no `switch_task` command, so against the
 * real core this is stop + start — which is also what the docs describe the core doing
 * (timer_ui.rs: "start/stop/switch task"), just not atomically.
 */
export async function setTask(taskId: string, running: boolean): Promise<void> {
  if (USE_MOCK) return mock.setTask(taskId);
  if (running) {
    await invoke<TimerState>("stop_timer");
    await invoke<TimerState>("start_timer", { taskId });
  }
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
