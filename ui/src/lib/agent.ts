/**
 * The single seam between the panel UI and the core.
 *
 * The panel was designed against the tray crate's command set (`get_consent_state`,
 * `capture_state`, `effective_config`, `timer_state`, `request_pause`). This build runs on the
 * agent core in src-tauri/, which registers a *different*, smaller set — auth, a bool consent
 * flag, and a four-argument timer. This module is the adapter between the two: it composes an
 * `AgentSnapshot` out of the commands that do exist, and is honest about the ones that don't.
 *
 * What is live:  consent (grant + read), timer start/stop/running, identity (from the signed-in
 *                user), and a derived capture state.
 * What is not:   effective config, pause, tasks, activity bars, session roll-up. Each is marked
 *                UNBACKED below with the command that would light it up. The UI degrades — it
 *                does not invent values.
 *
 * Dev (`npm run dev`) still talks to the fake core in mock.ts so the panel can be designed
 * without a running backend. Use `VITE_REAL=1 npm run dev` to hit the real commands.
 */

import { core } from "./core";
import * as mock from "./mock";
import { CADENCE_SECS, DEFAULT_CONFIG, DISCLOSURE, POLICY_VERSION } from "./policy";
import type { AgentSnapshot, Identity, PauseGrant } from "./types";

export const USE_MOCK = import.meta.env.DEV && import.meta.env.VITE_REAL !== "1";

/**
 * The core's `timer_status` returns a bare `bool` — no task, no elapsed. Both are tracked here
 * so the clock can tick, seeded from our own `timer_start` call.
 *
 * Known limitation: a session the core restored from before this window mounted (or one started
 * from the tray menu) is adopted on first sighting with `startedAtMs = now`, so the panel's
 * elapsed reads 0 and counts up rather than showing the true age. It understates, never
 * overstates. A `timer_state()` command returning `{running, task_id, elapsed_secs}` removes
 * this whole block.
 */
let localTimer: { taskId: string | null; startedAtMs: number | null } = {
  taskId: null,
  startedAtMs: null,
};

/**
 * The core's `timer_start` requires session_id/task_id/project_id/description, but the panel's
 * task picker is UNBACKED (no command lists tasks), so there is nothing to supply a project or
 * description from. These are the placeholders that go on the wire until `tasks()` exists.
 */
const UNKNOWN_PROJECT = "";
const UI_DESCRIPTION = "";

function newSessionId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `sess-${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
}

function elapsedSecs(running: boolean): number {
  if (!running || localTimer.startedAtMs === null) return 0;
  return Math.floor((Date.now() - localTimer.startedAtMs) / 1000);
}

/** Identity comes from the auth session — the core exposes no separate `identity()` command. */
function identityFrom(username: string | undefined): Identity | null {
  if (!username) return null;
  // The core knows the Cognito username (an email) and nothing else — no display name, no
  // avatar. Showing the local-part is better than a blank chip, and the gradient monogram is
  // the intended fallback for an empty avatar_url.
  const name = username.includes("@") ? username.split("@")[0] : username;
  return { name, email: username, avatar_url: "" };
}

export async function readSnapshot(): Promise<AgentSnapshot> {
  if (USE_MOCK) return mock.read();

  const [consentGranted, running, auth] = await Promise.all([
    core.consentStatus(),
    core.timerStatus(),
    core.authStatus().catch(() => ({ signedIn: false }) as { signedIn: boolean; username?: string }),
  ]);

  // Adopt / release the locally-tracked session (see `localTimer`).
  if (running && localTimer.startedAtMs === null) localTimer.startedAtMs = Date.now();
  if (!running) localTimer = { taskId: null, startedAtMs: null };

  const config = DEFAULT_CONFIG; // UNBACKED — needs `effective_config()`.

  // Derived, not read: the core gates capture on consent and runs it while a session is live
  // (monitor/mod.rs), so this mirrors that rule rather than guessing.
  const capturing = consentGranted && running && config.cadence !== "off";
  const interval = CADENCE_SECS[config.cadence];
  const nextCycle =
    capturing && interval > 0 ? interval - (Math.floor(Date.now() / 1000) % interval) : 0;

  return {
    identity: identityFrom(auth.username),
    consent: {
      granted: consentGranted,
      policy_version: POLICY_VERSION,
      captured: DISCLOSURE,
    },
    capture: {
      capturing,
      screenshots: capturing,
      next_cycle_secs: nextCycle,
    },
    config,
    timer: {
      running,
      task_id: localTimer.taskId,
      elapsed_secs: elapsedSecs(running),
    },
    // UNBACKED — `tasks()`, `recent_activity()`, `sessions_today()`. The cards render their
    // empty states rather than showing invented work.
    tasks: [],
    activity: [],
    sessions: [],
  };
}

export async function grantConsent(policyVersion: number): Promise<void> {
  if (USE_MOCK) return mock.grantConsent(policyVersion);
  // `set_consent` takes only a bool — the version the user agreed to is not recorded core-side.
  await core.setConsent(true);
}

export async function startTimer(taskId: string | null): Promise<void> {
  if (USE_MOCK) return mock.startTimer(taskId);
  await core.timerStart(newSessionId(), taskId ?? "", UNKNOWN_PROJECT, UI_DESCRIPTION);
  localTimer = { taskId, startedAtMs: Date.now() };
}

export async function stopTimer(): Promise<void> {
  if (USE_MOCK) return mock.stopTimer();
  await core.timerStop();
  localTimer = { taskId: null, startedAtMs: null };
}

/**
 * Re-attribute the timer to another task. There is no `switch_task` command, so this is
 * stop + start — the same thing the tray build did, and equally non-atomic.
 */
export async function setTask(taskId: string, running: boolean): Promise<void> {
  if (USE_MOCK) return mock.setTask(taskId);
  if (running) {
    await stopTimer();
    await startTimer(taskId);
  } else {
    localTimer.taskId = taskId;
  }
}

/**
 * UNBACKED — the core registers no `request_pause`. Refusing is the honest answer: granting it
 * locally would show "paused" while capture kept running, which is the one lie a privacy
 * indicator must never tell. PauseCard renders the refusal copy.
 */
export async function requestPause(requestedSecs: number): Promise<PauseGrant> {
  if (USE_MOCK) return mock.requestPause(requestedSecs);
  return { granted: false, granted_secs: 0, remaining_budget_secs: 0 };
}

/** No pause-state read command outside mock; nothing can be in flight at mount. */
export function initialPauseSecs(): number {
  return USE_MOCK ? mock.pauseRemainingSecs() : 0;
}
