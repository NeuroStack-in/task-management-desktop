import { useCallback, useEffect, useRef, useState } from "react";

import * as agent from "@/lib/agent";
import type { AgentSnapshot, Project, Session, Task } from "@/lib/types";

/**
 * Local state is **not** polled every second — the timer runs locally (the recording numerals tick
 * from an anchor), and a start/stop reads immediately. This slow interval only exists to notice a
 * *core-side* change the UI didn't initiate: an idle auto-stop, or the capture indicator flipping.
 */
const LOCAL_POLL_MS = 15_000;
/** Today's sessions come from the backend, so they refresh slowly (+ eagerly after start/stop). */
const SESSIONS_POLL_MS = 20_000;

export interface Agent {
  snapshot: AgentSnapshot | null;
  error: string | null;
  /** Seconds left on the active pause, 0 when not paused. */
  pauseSecs: number;
  /** Set when the core refused the last pause request (budget spent, or admin-disabled). */
  pauseRefused: boolean;
  grantConsent: () => void;
  /** Start a session against a project (+ optional task) + description — or switch if one is running. */
  start: (projectId: string, description: string, taskId?: string) => void;
  stop: () => void;
  /** Re-fetch projects, tasks and today's sessions (the backend-fed lists). */
  refresh: () => void;
  requestPause: (secs: number) => void;
}

/**
 * Owns the panel's view state, split by how often each part changes and how expensive it is to read:
 *
 * - **Local state** (timer, consent, capture, config, identity) — in-process Rust reads, polled every
 *   second so the timer ticks smoothly. No network on this path, which is what keeps the clock from
 *   stuttering/skipping.
 * - **Projects + tasks** — backend reads, fetched once on mount and on an explicit refresh.
 * - **Today's sessions** — backend read, refreshed every 20 s and eagerly right after a start/stop.
 *
 * The old design fetched all three backend lists on the 1 s timer tick, so every second waited on
 * three AWS round-trips — the poll landed irregularly and the timer jumped. This separation fixes it.
 */
export function useAgent(): Agent {
  const [local, setLocal] = useState<agent.LocalState | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [pauseSecs, setPauseSecs] = useState(() => agent.initialPauseSecs());
  const [pauseRefused, setPauseRefused] = useState(false);

  // Avoids a slow local read landing after a newer one and rewinding the UI.
  const seq = useRef(0);

  // ── fast local poll (1 s, no network) ──
  const pollLocal = useCallback(async () => {
    const id = ++seq.current;
    try {
      const next = await agent.readLocal();
      if (id !== seq.current) return;
      setLocal(next);
      setError(null);
    } catch (e) {
      if (id !== seq.current) return;
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void pollLocal();
    const t = setInterval(() => void pollLocal(), LOCAL_POLL_MS);
    return () => clearInterval(t);
  }, [pollLocal]);

  // ── projects + tasks (on mount + refresh) ──
  const reloadCatalog = useCallback(() => {
    void agent.fetchProjects().then(setProjects);
    void agent.fetchTasks().then(setTasks);
  }, []);
  useEffect(() => reloadCatalog(), [reloadCatalog]);

  // ── today's sessions (slow poll + after start/stop) ──
  const reloadSessions = useCallback(() => {
    void agent.fetchSessions().then(setSessions);
  }, []);
  useEffect(() => {
    reloadSessions();
    const t = setInterval(reloadSessions, SESSIONS_POLL_MS);
    return () => clearInterval(t);
  }, [reloadSessions]);

  // Pause countdown is tracked here because the core exposes no pause-state read command.
  useEffect(() => {
    if (pauseSecs <= 0) return;
    const t = setInterval(() => setPauseSecs((s) => Math.max(0, s - 1)), 1000);
    return () => clearInterval(t);
  }, [pauseSecs > 0]); // eslint-disable-line react-hooks/exhaustive-deps

  const snapshot: AgentSnapshot | null = local
    ? {
        ...local,
        projects,
        tasks,
        activity: [],
        // Fold the live segment in so the running session's row ticks between server refreshes.
        sessions: agent.withLiveSession(sessions, local.timer),
      }
    : null;

  const grantConsent = useCallback(() => {
    if (!local) return;
    void agent
      .grantConsent(local.consent.policy_version)
      .then(pollLocal)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [local, pollLocal]);

  const start = useCallback(
    (projectId: string, description: string, taskId?: string) => {
      if (!local) return;
      // Start when idle, switch when running. Refresh the timer immediately + reload sessions so the
      // new row shows without waiting for the 20 s cycle.
      void agent.switchSession(projectId, description, local.timer.running, taskId).then(() => {
        void pollLocal();
        reloadSessions();
      });
    },
    [local, pollLocal, reloadSessions],
  );

  const stop = useCallback(() => {
    if (!local) return;
    void agent.stopTimer().then(() => {
      void pollLocal();
      // A stopped session becomes a completed entry once the batch folds — reload shortly after.
      reloadSessions();
      window.setTimeout(reloadSessions, 3000);
    });
  }, [local, pollLocal, reloadSessions]);

  const refresh = useCallback(() => {
    reloadCatalog();
    reloadSessions();
  }, [reloadCatalog, reloadSessions]);

  const requestPause = useCallback(
    (secs: number) => {
      void agent.requestPause(secs).then((grant) => {
        setPauseRefused(!grant.granted);
        if (grant.granted) setPauseSecs(grant.granted_secs);
        void pollLocal();
      });
    },
    [pollLocal],
  );

  return {
    snapshot,
    error,
    pauseSecs,
    pauseRefused,
    grantConsent,
    start,
    stop,
    refresh,
    requestPause,
  };
}
