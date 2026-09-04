import { useCallback, useEffect, useRef, useState } from "react";

import * as agent from "@/lib/agent";
import { EVENTS } from "@/lib/agent";
import { clearHistory } from "@/lib/descriptionHistory";
import type { AgentSnapshot, Subtask, TimerSelection } from "@/lib/types";

// The core (timer/pause/consent/capture/config/identity) is cheap local IPC — read it every second
// so the clock and any out-of-panel transition show up promptly.
const POLL_MS = 1000;
// The backend-fed lists (projects/tasks/today's sessions) are live HTTP reads that change on the
// order of minutes at most, so they get a slow poll — plus an on-focus and after-every-write refresh
// so they still feel live. Polling them at POLL_MS put projects + time-attendance at ~3 req/s, 24/7,
// which was ~96% of the API Gateway bill; this is the fix (see agent.readLists).
const LISTS_POLL_MS = 30_000;

/**
 * Turn a core error into something a person can act on.
 *
 * Tauri commands surface failures as `timer:<engine message>` — an internal code that was being
 * rendered verbatim, so the panel said **"That didn't work. timer:a session is already running"**.
 * It names the module that failed rather than what the reader should do about it.
 *
 * "Already running" is the one that matters, because it is usually not a user mistake: the panel
 * auto-resumes the last task on launch (`useResumeLastTask`), and a Start pressed while that is in
 * flight loses the race. The timer *is* running — the message should say so, and the refresh that
 * now follows every action makes the panel agree.
 */
function humanizeActionError(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e);
  if (raw.includes("already running")) {
    return "A timer is already running — the panel has been refreshed to show it.";
  }
  if (raw.includes("not_running") || raw.includes("no session")) {
    return "No timer is running.";
  }
  // A task/subtask status change the backend refused because it belongs to someone else and the
  // caller is only a project Member — `task:status-not-yours` / `subtask:not-yours`. Name the rule
  // rather than show the raw code, since it is one the person can act on.
  if (raw.includes("not-yours")) {
    return "You can only change the status of a task assigned to you. A project lead or manager can change anyone's.";
  }
  // Strip the `module:` prefix rather than inventing wording for an error we do not know.
  return raw.replace(/^[a-z_]+:/, "");
}

export interface Agent {
  snapshot: AgentSnapshot | null;
  error: string | null;
  /** Set when the core refused the last pause request (budget spent, or admin-disabled). */
  pauseRefused: boolean;
  /** Idle seconds reported by the last `monitor:idle-prompt`; null when not prompting. */
  idleSecs: number | null;
  /** The restricted app/site last focused during tracking (`monitor:policy-blocked`); null = none. */
  restrictedHit: string | null;
  /**
   * IT released this device, so the core signed the employee out (`device:released`).
   *
   * Deliberately **outlives the sign-out** — it is cleared when someone next signs in successfully,
   * not when the session drops — because the whole point is to explain the sign-in screen the
   * employee is now looking at.
   */
  deviceReleased: boolean;
  /** Set when a user action (start/stop/switch/consent/pause) failed; shown inline until dismissed. */
  actionError: string | null;
  /** True while a write action is in flight — buttons disable / ignore re-clicks. */
  busy: boolean;
  grantConsent: () => void;
  /** Starts with `sel` when stopped; stops (ignoring `sel`) when running. */
  toggleTimer: (sel: TimerSelection) => void;
  /** Re-attribute a running timer without stopping the clock. */
  switchTo: (sel: TimerSelection) => void;
  /** Change a task's status. Backend-gated (assignee, or a project Lead/Manager); refreshes after. */
  setTaskStatus: (projectId: string, taskId: string, status: string) => void;
  /** Silent best-effort status nudge (e.g. todo→in_progress on start); bypasses the busy guard. */
  advanceTaskStatus: (projectId: string, taskId: string, status: string) => void;
  /** Add a subtask under a task; resolves to it (so the strip can target it) or null on failure. */
  createSubtask: (projectId: string, taskId: string, title: string) => Promise<Subtask | null>;
  /** Tick a subtask off or reopen it; resolves true when the write landed. */
  setSubtaskDone: (
    projectId: string,
    taskId: string,
    subtaskId: string,
    done: boolean,
  ) => Promise<boolean>;
  requestPause: (secs: number) => void;
  signOut: () => void;
  /** Dismiss the idle prompt, keeping the timer running. */
  dismissIdle: () => void;
  /** Dismiss the restricted-site warning banner. */
  dismissRestricted: () => void;
  /** Dismiss the action-error banner. */
  dismissActionError: () => void;
  /** Re-read the core now instead of waiting out the poll — what the hero's refresh drives. */
  refresh: () => Promise<void>;
}

/**
 * Polls the core once a second and owns the panel's whole view state.
 *
 * Polling covers steady state; the core's events cover the transitions a 1 s poll would show late
 * or miss entirely (a session dying, capture being denied). Both feed the same `refresh`, so an
 * event is only ever an early nudge — never a second source of truth.
 */
export function useAgent(): Agent {
  const [snapshot, setSnapshot] = useState<AgentSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pauseRefused, setPauseRefused] = useState(false);
  const [idleSecs, setIdleSecs] = useState<number | null>(null);
  const [restrictedHit, setRestrictedHit] = useState<string | null>(null);
  /** IT released this machine — survives the sign-out so the login screen can explain it. */
  const [deviceReleased, setDeviceReleased] = useState(false);
  // User-action feedback: set when an explicit action (start/stop/switch/consent/pause) fails, so the
  // panel can show it — unlike the poll's `error`, which is cleared on the next successful read.
  const [actionError, setActionError] = useState<string | null>(null);
  // In-flight flag so buttons can disable / ignore re-clicks while an action is pending.
  const [busy, setBusy] = useState(false);
  // Ref mirror of `busy`, checked synchronously so a rapid double-click is dropped before a second
  // call can fire (state updates are async and would let both clicks through).
  const busyRef = useRef(false);

  // Avoids a slow poll landing after a newer one and rewinding the UI. Core and lists refresh on
  // independent cadences, so they get independent sequence guards.
  const coreSeq = useRef(0);
  const listsSeq = useRef(0);
  // Latest known sign-in state, read synchronously by the slow lists poll + focus handler so they
  // can skip the backend reads while signed out without waiting on a render.
  const signedInRef = useRef(false);

  // The fast path: local core state only (timer/pause/consent/capture/config/identity). No backend
  // HTTP, so it is safe every second. The backend lists are left untouched — the last-good copy.
  const refreshCore = useCallback(async () => {
    const id = ++coreSeq.current;
    try {
      const core = await agent.readCore();
      if (id !== coreSeq.current) return;
      signedInRef.current = core.auth.signedIn;
      // Signing out clears the lists in the same beat, so a previous user's projects can't linger.
      setSnapshot((prev) =>
        prev
          ? { ...prev, ...core, ...(core.auth.signedIn ? {} : agent.EMPTY_LISTS) }
          : { ...core, ...agent.EMPTY_LISTS },
      );
      setError(null);
    } catch (e) {
      if (id !== coreSeq.current) return;
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  // The slow path: the backend-fed lists. Polled on LISTS_POLL_MS and refreshed on demand (focus,
  // after a write, on a core event) — never per second. Keeps the last-good lists on failure so the
  // picker never blanks.
  const refreshLists = useCallback(async () => {
    const id = ++listsSeq.current;
    try {
      const lists = await agent.readLists(signedInRef.current);
      if (id !== listsSeq.current) return;
      setSnapshot((prev) => (prev ? { ...prev, ...lists } : prev));
    } catch {
      /* keep the last-good lists */
    }
  }, []);

  // Force everything fresh now: core, then lists (sequential so the lists read sees the sign-in
  // state the core just set). Used by the hero refresh, the core events, and after every write —
  // all low-frequency, so the list read here is not the poll's concern.
  const refresh = useCallback(async () => {
    await refreshCore();
    await refreshLists();
  }, [refreshCore, refreshLists]);

  useEffect(() => {
    void refresh();
    const coreTimer = setInterval(() => void refreshCore(), POLL_MS);
    const listsTimer = setInterval(() => void refreshLists(), LISTS_POLL_MS);
    // A panel brought back to the front should show current lists at once, not up to 30 s late.
    const onFocus = () => void refreshLists();
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(coreTimer);
      clearInterval(listsTimer);
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh, refreshCore, refreshLists]);

  // Core events. Each unlisten is awaited on cleanup so a remount can't double-subscribe.
  useEffect(() => {
    const subs = [
      // The session died (401 on refresh). Re-read immediately so the login gate takes over now
      // rather than up to a second later.
      agent.listen(EVENTS.authExpired, () => void refresh()),
      // Tracking started/stopped outside the panel (idle auto-stop, tray).
      agent.listen(EVENTS.trackingChanged, () => void refresh()),
      agent.listen<number>(EVENTS.idlePrompt, (secs) => setIdleSecs(secs)),
      // A restricted app/site was focused mid-session. The core already queued the violation for
      // the server; this is the employee-facing half of the warning.
      agent.listen<string>(EVENTS.policyBlocked, (identifier) => setRestrictedHit(identifier)),
      // IT released this device. The core has already stopped, flushed and signed out by the time
      // this arrives; refreshing pulls the now-signed-out snapshot so the login gate takes over in
      // the same beat as the explanation appears.
      agent.listen(EVENTS.deviceReleased, () => {
        setDeviceReleased(true);
        void refresh();
      }),
    ];
    return () => {
      for (const s of subs) void s.then((un) => un());
    };
  }, [refresh]);

  // The core hard-stops the timer well after the prompt; once it isn't running, the prompt is moot.
  useEffect(() => {
    if (snapshot && !snapshot.timer.running) setIdleSecs(null);
  }, [snapshot?.timer.running]); // eslint-disable-line react-hooks/exhaustive-deps

  // Whenever the session drops — an explicit sign-out OR a token that expired (auth:expired) — clear
  // the ephemeral per-session banners so a policy violation, pause-refusal or idle prompt from one
  // user can never greet the next person who signs in on this device.
  // The release notice is cleared on the next successful **sign-in**, not on the sign-out that
  // follows a release — clearing it there would erase the message in the same beat it was raised.
  useEffect(() => {
    if (snapshot?.auth.signedIn) setDeviceReleased(false);
  }, [snapshot?.auth.signedIn]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (snapshot && !snapshot.auth.signedIn) {
      setRestrictedHit(null);
      setPauseRefused(false);
      setIdleSecs(null);
      setActionError(null);
    }
  }, [snapshot?.auth.signedIn]); // eslint-disable-line react-hooks/exhaustive-deps

  const reportAction = useCallback((e: unknown) => {
    setActionError(humanizeActionError(e));
  }, []);

  // Run a write action single-flight, surfacing any failure. The ref guard is checked synchronously,
  // so a rapid double-click is dropped before a second call can fire (e.g. starting two timers).
  const run = useCallback(
    (fn: () => Promise<unknown>) => {
      if (busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setActionError(null);
      void fn()
        .catch(reportAction)
        // **Refresh after a failure too, not just a success.** A refused action is exactly when the
        // panel is most likely to be out of step with the core: the timer is refused *because*
        // something already started one, so the snapshot that made the button look pressable is
        // stale by definition. Refreshing only in `.then` left the panel showing READY 00:00:00
        // beside "a session is already running" — the two halves of one contradiction on screen at
        // once, with no way to reconcile short of restarting the app.
        .then(() => refresh())
        .finally(() => {
          busyRef.current = false;
          setBusy(false);
        });
    },
    [refresh, reportAction],
  );

  const grantConsent = useCallback(() => {
    if (!snapshot) return;
    run(() => agent.grantConsent(snapshot.consent.policy_version));
  }, [snapshot, run]);

  const toggleTimer = useCallback(
    (sel: TimerSelection) => {
      if (!snapshot) return;
      setIdleSecs(null);
      run(() => (snapshot.timer.running ? agent.stopTimer() : agent.startTimer(sel)));
    },
    [snapshot, run],
  );

  const switchTo = useCallback(
    (sel: TimerSelection) => {
      if (!snapshot?.timer.running) return;
      run(() => agent.switchTo(sel));
    },
    [snapshot, run],
  );

  // Uses `run` (unlike the subtask writes): the caller doesn't need the result, and `run` gives it
  // the busy-guard, the refused-action error banner, and — crucially — the refresh, so the status
  // dropdown reflects the new value the moment the write lands. A 403 (`task:status-not-yours`)
  // surfaces through `reportAction` as the assignee-only sentence.
  const setTaskStatus = useCallback(
    (projectId: string, taskId: string, status: string) => {
      run(() => agent.setTaskStatus(projectId, taskId, status));
    },
    [run],
  );

  // The auto-advance to "In progress" when a timer starts fires *immediately after* the start
  // action, which is still holding `run`'s single-flight lock (`busyRef`) — so routing it through
  // `run` like `setTaskStatus` above meant it hit the busy guard and was silently dropped, leaving a
  // running timer sitting on a "To do" task. This is a best-effort background nudge, so it bypasses
  // `run` entirely: no busy state, no error banner (a refusal is swallowed), just the write and a
  // refresh so the new status shows on the next paint.
  const advanceTaskStatus = useCallback(
    (projectId: string, taskId: string, status: string) => {
      void agent
        .setTaskStatus(projectId, taskId, status)
        .then(() => refresh())
        .catch(() => {});
    },
    [refresh],
  );

  // Both subtask writes bypass `run` deliberately: the caller awaits the result to retarget the
  // timer, and a fire-and-forget would leave the strip a poll behind.
  const createSubtask = useCallback(
    async (projectId: string, taskId: string, title: string): Promise<Subtask | null> => {
      const t = title.trim();
      if (!projectId || !taskId || !t) return null;
      setActionError(null);
      try {
        const created = await agent.createSubtask(projectId, taskId, t);
        await refresh(); // the new row joins the picker on the next read
        return created;
      } catch (e) {
        reportAction(e);
        return null;
      }
    },
    [refresh, reportAction],
  );

  const setSubtaskDone = useCallback(
    async (
      projectId: string,
      taskId: string,
      subtaskId: string,
      done: boolean,
    ): Promise<boolean> => {
      setActionError(null);
      try {
        // Reopening returns to `todo`, not `in_progress`: the agent cannot know whether the person
        // is picking the work back up or simply undoing a mis-click, and `todo` is the claim that
        // assumes less.
        await agent.setSubtaskStatus(projectId, taskId, subtaskId, done ? "done" : "todo");
        await refresh();
        return true;
      } catch (e) {
        reportAction(e);
        return false;
      }
    },
    [refresh, reportAction],
  );

  const requestPause = useCallback(
    (secs: number) => {
      if (busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setActionError(null);
      void agent
        .requestPause(secs)
        .then((grant) => {
          setPauseRefused(!grant.granted);
          // No local countdown: the next poll reads the authoritative window from the core.
          void refresh();
        })
        .catch(reportAction)
        .finally(() => {
          busyRef.current = false;
          setBusy(false);
        });
    },
    [refresh, reportAction],
  );

  const signOut = useCallback(() => {
    // Clear the device-global description MRU + every ephemeral per-session banner, so the next
    // person who signs in on this device inherits none of the previous user's UI state.
    clearHistory();
    setActionError(null);
    setRestrictedHit(null);
    setPauseRefused(false);
    setIdleSecs(null);
    void agent.logout().then(refresh).catch(reportAction);
  }, [refresh, reportAction]);

  const dismissIdle = useCallback(() => setIdleSecs(null), []);
  const dismissRestricted = useCallback(() => setRestrictedHit(null), []);
  const dismissActionError = useCallback(() => setActionError(null), []);

  return {
    snapshot,
    error,
    pauseRefused,
    idleSecs,
    restrictedHit,
    deviceReleased,
    actionError,
    busy,
    grantConsent,
    toggleTimer,
    switchTo,
    setTaskStatus,
    advanceTaskStatus,
    createSubtask,
    setSubtaskDone,
    requestPause,
    signOut,
    dismissIdle,
    dismissRestricted,
    dismissActionError,
    refresh,
  };
}
