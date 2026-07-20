import { useCallback, useEffect, useRef, useState } from "react";

import * as agent from "@/lib/agent";
import { EVENTS } from "@/lib/agent";
import type { AgentSnapshot, TimerSelection } from "@/lib/types";

const POLL_MS = 1000;

export interface Agent {
  snapshot: AgentSnapshot | null;
  error: string | null;
  /** Set when the core refused the last pause request (budget spent, or admin-disabled). */
  pauseRefused: boolean;
  /** Idle seconds reported by the last `monitor:idle-prompt`; null when not prompting. */
  idleSecs: number | null;
  /** True after `monitor:screenshot-unavailable` — capture is denied at the OS level. */
  screenshotBlocked: boolean;
  grantConsent: () => void;
  /** Starts with `sel` when stopped; stops (ignoring `sel`) when running. */
  toggleTimer: (sel: TimerSelection) => void;
  /** Re-attribute a running timer without stopping the clock. */
  switchTo: (sel: TimerSelection) => void;
  requestPause: (secs: number) => void;
  signOut: () => void;
  /** Dismiss the idle prompt, keeping the timer running. */
  dismissIdle: () => void;
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
  const [screenshotBlocked, setScreenshotBlocked] = useState(false);

  // Avoids a slow poll landing after a newer one and rewinding the UI.
  const seq = useRef(0);

  const refresh = useCallback(async () => {
    const id = ++seq.current;
    try {
      const next = await agent.readSnapshot();
      if (id !== seq.current) return;
      setSnapshot(next);
      setError(null);
    } catch (e) {
      if (id !== seq.current) return;
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(t);
  }, [refresh]);

  // Core events. Each unlisten is awaited on cleanup so a remount can't double-subscribe.
  useEffect(() => {
    const subs = [
      // The session died (401 on refresh). Re-read immediately so the login gate takes over now
      // rather than up to a second later.
      agent.listen(EVENTS.authExpired, () => void refresh()),
      // Tracking started/stopped outside the panel (idle auto-stop, tray).
      agent.listen(EVENTS.trackingChanged, () => void refresh()),
      // Capture is denied at the OS level (e.g. macOS Screen Recording). Sticky: it stays until the
      // user acts, because the next capture attempt is a whole cadence away.
      agent.listen(EVENTS.screenshotUnavailable, () => setScreenshotBlocked(true)),
      agent.listen<number>(EVENTS.idlePrompt, (secs) => setIdleSecs(secs)),
    ];
    return () => {
      for (const s of subs) void s.then((un) => un());
    };
  }, [refresh]);

  // The core hard-stops the timer well after the prompt; once it isn't running, the prompt is moot.
  useEffect(() => {
    if (snapshot && !snapshot.timer.running) setIdleSecs(null);
  }, [snapshot?.timer.running]); // eslint-disable-line react-hooks/exhaustive-deps

  const report = useCallback((e: unknown) => {
    setError(e instanceof Error ? e.message : String(e));
  }, []);

  const grantConsent = useCallback(() => {
    if (!snapshot) return;
    void agent.grantConsent(snapshot.consent.policy_version).then(refresh).catch(report);
  }, [snapshot, refresh, report]);

  const toggleTimer = useCallback(
    (sel: TimerSelection) => {
      if (!snapshot) return;
      const action = snapshot.timer.running ? agent.stopTimer() : agent.startTimer(sel);
      void action.then(refresh).catch(report);
      setIdleSecs(null);
    },
    [snapshot, refresh, report],
  );

  const switchTo = useCallback(
    (sel: TimerSelection) => {
      if (!snapshot?.timer.running) return;
      void agent.switchTo(sel).then(refresh).catch(report);
    },
    [snapshot, refresh, report],
  );

  const requestPause = useCallback(
    (secs: number) => {
      void agent
        .requestPause(secs)
        .then((grant) => {
          setPauseRefused(!grant.granted);
          // No local countdown: the next poll reads the authoritative window from the core.
          void refresh();
        })
        .catch(report);
    },
    [refresh, report],
  );

  const signOut = useCallback(() => {
    void agent.logout().then(refresh).catch(report);
  }, [refresh, report]);

  const dismissIdle = useCallback(() => setIdleSecs(null), []);

  return {
    snapshot,
    error,
    pauseRefused,
    idleSecs,
    screenshotBlocked,
    grantConsent,
    toggleTimer,
    switchTo,
    requestPause,
    signOut,
    dismissIdle,
    refresh,
  };
}
