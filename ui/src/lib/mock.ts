/**
 * Design-time fake core.
 *
 * The real tray commands are stubs today (every `features/*.rs` command returns a hardcoded
 * literal — see the TODO(ipc) markers), so styling against them means styling a frozen panel:
 * the timer never ticks, capture is never on, consent can never be dismissed.
 *
 * This module fakes the core well enough to *see* the UI: it ticks, it holds state, and it can
 * be flipped between scenarios. It exists purely so the panel can be designed before IPC lands.
 * When `agentd` is really wired up, delete this file and the `USE_MOCK` branch in agent.ts.
 */

import { CADENCE_SECS, DISCLOSURE } from "./policy";
import type {
  ActivitySeries,
  AgentSnapshot,
  PauseGrant,
  Identity,
  Session,
  Task,
  TrackingConfig,
} from "./types";

/**
 * The bound user. Same person as the web app's seeded demo owner (data/users.json:
 * "Alex Morgan" / owner@acme.test, avatarUrl "") so both surfaces demo one identity � and the
 * empty avatarUrl exercises the gradient-monogram fallback, which is the intended default.
 */
export const IDENTITY: Identity = {
  name: "Alex Morgan",
  email: "owner@acme.test",
  avatar_url: "",
};

/** Project names mirror the web app's TASK_OPTIONS (mock-time.ts) so the two demo the same org. */
export const TASKS: Task[] = [
  { id: "WP-482", title: "Tray panel redesign", project_id: "p-platform", project_name: "Platform", billable: true },
  { id: "WP-119", title: "Ingest batch retry", project_id: "p-platform", project_name: "Platform", billable: true },
  { id: "WP-274", title: "Screenshot blur tuning", project_id: "p-platform", project_name: "Platform", billable: false },
  { id: "WP-503", title: "Consent copy review", project_id: "p-internal", project_name: "Internal", billable: false },
  { id: "WP-511", title: "Agent enrollment docs", project_id: "p-internal", project_name: "Internal", billable: false },
  { id: "WP-338", title: "Checkout flow — payment step", project_id: "p-acme", project_name: "Acme Storefront", billable: true },
  { id: "WP-341", title: "Storefront perf audit", project_id: "p-acme", project_name: "Acme Storefront", billable: true },
  { id: "WP-207", title: "Onboarding call follow-ups", project_id: "p-cs", project_name: "Customer Success", billable: false },
];

export const SCENARIOS = [
  "onboarding",
  "idle",
  "monitoring",
  "paused",
  "silent",
] as const;

export type Scenario = (typeof SCENARIOS)[number];

export const SCENARIO_LABEL: Record<Scenario, string> = {
  onboarding: "First run",
  idle: "Off shift",
  monitoring: "Working",
  paused: "Paused",
  silent: "Silent",
};

// Shared with the real adapter (agent.ts) — the consent gate must show the same disclosure
// whichever core is behind it, or the mock would be styling copy that never ships.

/**
 * Deterministic per scenario — a fresh random series on every 1s poll would make the
 * bars twitch. Paused deliberately trails off to zero, so the card tells its own story.
 */
function activityFor(scenario: Scenario): ActivitySeries {
  const n = 24;
  const out: number[] = [];
  let x = scenario.length * 7919 + 13; // seeded LCG; stable for a given scenario
  for (let i = 0; i < n; i++) {
    x = (x * 1103515245 + 12345) % 2147483648;
    const noise = x / 2147483648;
    if (scenario === "idle") out.push(Math.round(noise * 6));
    else if (scenario === "paused") out.push(i > n - 6 ? 0 : Math.round(20 + noise * 60));
    else out.push(Math.round(25 + noise * 70));
  }
  return out;
}

/**
 * Seconds already logged today per task, excluding the live segment — the same split the web
 * app's timer store uses (banked vs live), so the running task's row ticks without the banked
 * total drifting.
 */
function bankFor(scenario: Scenario): Record<string, number> {
  if (scenario === "onboarding" || scenario === "idle") return {};
  return { "WP-482": 5_820, "WP-119": 2_460, "WP-207": 480 };
}

interface MockState {
  scenario: Scenario;
  activity: ActivitySeries;
  /** taskId -> banked seconds today. */
  sessionSecs: Record<string, number>;
  consentGranted: boolean;
  policyVersion: number;
  timerRunning: boolean;
  taskId: string | null;
  /** Wall-clock ms when the timer started; null when stopped. */
  timerStartedAt: number | null;
  pausedUntil: number | null;
  pauseBudgetSecs: number;
  config: TrackingConfig;
}

function configFor(scenario: Scenario): TrackingConfig {
  const silent = scenario === "silent";
  return {
    version: 7,
    // `idle` models off-shift: the admin cadence is off, so the core captures nothing.
    cadence: scenario === "idle" ? "off" : silent ? "min10" : "min5",
    blur_level: silent ? 2 : 1,
    retention_days: 90,
    silent,
  };
}

function stateFor(scenario: Scenario): MockState {
  const now = Date.now();
  const base: MockState = {
    scenario,
    activity: activityFor(scenario),
    sessionSecs: bankFor(scenario),
    consentGranted: scenario !== "onboarding",
    policyVersion: 1,
    timerRunning: false,
    taskId: null,
    timerStartedAt: null,
    pausedUntil: null,
    pauseBudgetSecs: 30 * 60,
    config: configFor(scenario),
  };

  switch (scenario) {
    case "monitoring":
      return {
        ...base,
        timerRunning: true,
        taskId: "WP-482",
        timerStartedAt: now - 4_142_000, // ~1h09m in, so the clock reads convincingly
      };
    case "paused":
      return {
        ...base,
        pausedUntil: now + 214_000, // ~3m34s left
        pauseBudgetSecs: 30 * 60 - 300,
        timerRunning: true,
        taskId: "WP-482",
        timerStartedAt: now - 812_000,
      };
    case "silent":
      return { ...base, timerRunning: true, taskId: "WP-119", timerStartedAt: now - 96_000 };
    default:
      return base;
  }
}

let state = stateFor("monitoring");

export function getScenario(): Scenario {
  return state.scenario;
}

export function setScenario(scenario: Scenario) {
  state = stateFor(scenario);
}

function isPaused() {
  return state.pausedUntil !== null && state.pausedUntil > Date.now();
}

/** True when the fake core would be capturing right now. */
function isCapturing() {
  if (!state.consentGranted) return false;
  if (isPaused()) return false;
  return state.config.cadence !== "off";
}

/** The live segment only — TimerState.elapsed_secs is "elapsed seconds in the current
 *  session" (timer_ui.rs). Day totals live in `sessionSecs`, not here. */
function elapsedSecs() {
  if (!state.timerRunning || state.timerStartedAt === null) return 0;
  return Math.floor((Date.now() - state.timerStartedAt) / 1000);
}

/** Move the live segment into the current task's day total. Must run before the timer
 *  stops or switches task, or that time is silently lost from today's sessions. */
function bankLive() {
  if (!state.taskId || state.timerStartedAt === null) return;
  const live = Math.floor((Date.now() - state.timerStartedAt) / 1000);
  state.sessionSecs[state.taskId] = (state.sessionSecs[state.taskId] ?? 0) + live;
}

export function read(): AgentSnapshot {
  const capturing = isCapturing();
  const interval = CADENCE_SECS[state.config.cadence];
  // Derive a countdown from wall-clock so it visibly ticks between polls.
  const nextCycle =
    capturing && interval > 0 ? interval - (Math.floor(Date.now() / 1000) % interval) : 0;

  return {
    identity: IDENTITY,
    consent: {
      granted: state.consentGranted,
      policy_version: state.policyVersion,
      captured: DISCLOSURE,
    },
    capture: {
      capturing,
      screenshots: capturing && state.config.cadence !== "off",
      next_cycle_secs: nextCycle,
    },
    config: state.config,
    timer: {
      running: state.timerRunning,
      task_id: state.taskId,
      elapsed_secs: elapsedSecs(),
    },
    tasks: TASKS,
    activity: state.activity,
    sessions: sessionsToday(),
  };
}

/**
 * Today's per-task totals. The running task's live segment is folded in so its row ticks,
 * exactly as the web app's TimerHero does — otherwise the list would look frozen next to a
 * running clock.
 */
function sessionsToday(): Session[] {
  const out = new Map(Object.entries(state.sessionSecs));
  if (state.timerRunning && state.taskId) {
    out.set(state.taskId, (out.get(state.taskId) ?? 0) + elapsedSecs());
  }
  return (
    [...out.entries()]
      .map(([task_id, secs]) => ({ task_id, secs }))
      // Running task first, then longest — the active row should never be buried.
      .sort((a, b) => {
        if (a.task_id === state.taskId) return -1;
        if (b.task_id === state.taskId) return 1;
        return b.secs - a.secs;
      })
  );
}

/**
 * Switch the attributed task. Time already worked is banked to the *old* task first, then the
 * live segment restarts — otherwise switching would misattribute it to the new task.
 */
export function setTask(taskId: string) {
  if (state.timerRunning) {
    bankLive();
    state.timerStartedAt = Date.now();
  }
  state.taskId = taskId;
}

export function grantConsent(policyVersion: number) {
  state.consentGranted = true;
  state.policyVersion = policyVersion;
}

export function startTimer(taskId: string | null) {
  if (state.timerRunning) return;
  state.timerRunning = true;
  // No fallback task: if the caller starts without one, the panel should show that
  // honestly rather than silently attributing the time to some default.
  state.taskId = taskId;
  state.timerStartedAt = Date.now();
}

export function stopTimer() {
  if (!state.timerRunning) return;
  bankLive();
  state.timerRunning = false;
  state.timerStartedAt = null;
}

export function requestPause(requestedSecs: number): PauseGrant {
  // Mirrors the documented core behaviour: clamp to the remaining daily budget.
  const granted = Math.min(requestedSecs, state.pauseBudgetSecs);
  if (granted <= 0) {
    return { granted: false, granted_secs: 0, remaining_budget_secs: 0 };
  }
  state.pausedUntil = Date.now() + granted * 1000;
  state.pauseBudgetSecs -= granted;
  return {
    granted: true,
    granted_secs: granted,
    remaining_budget_secs: state.pauseBudgetSecs,
  };
}

/** Seconds left on the current pause, or 0. */
export function pauseRemainingSecs() {
  if (!isPaused()) return 0;
  return Math.ceil((state.pausedUntil! - Date.now()) / 1000);
}

export function resume() {
  state.pausedUntil = null;
}
