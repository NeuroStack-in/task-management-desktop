import { useCallback, useEffect, useRef, useState } from "react";

import * as agent from "@/lib/agent";
import type { AgentSnapshot } from "@/lib/types";

const POLL_MS = 1000;

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
  requestPause: (secs: number) => void;
}

/**
 * Polls the core once a second and owns the panel's whole view state.
 *
 * Polling (rather than subscribing) matches what the core can do today; when `agentd` pushes
 * state over IPC this becomes an event listener and the interval goes away.
 */
export function useAgent(): Agent {
  const [snapshot, setSnapshot] = useState<AgentSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pauseSecs, setPauseSecs] = useState(() => agent.initialPauseSecs());
  const [pauseRefused, setPauseRefused] = useState(false);

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

  // Pause countdown is tracked here because the core exposes no pause-state read command.
  useEffect(() => {
    if (pauseSecs <= 0) return;
    const t = setInterval(() => setPauseSecs((s) => Math.max(0, s - 1)), 1000);
    return () => clearInterval(t);
  }, [pauseSecs > 0]); // eslint-disable-line react-hooks/exhaustive-deps

  const grantConsent = useCallback(() => {
    if (!snapshot) return;
    void agent.grantConsent(snapshot.consent.policy_version).then(refresh).catch(setErrorMessage);
    function setErrorMessage(e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [snapshot, refresh]);

  const start = useCallback(
    (projectId: string, description: string, taskId?: string) => {
      if (!snapshot) return;
      // Start when idle, switch when running — one call either way.
      void agent
        .switchSession(projectId, description, snapshot.timer.running, taskId)
        .then(refresh);
    },
    [snapshot, refresh],
  );

  const stop = useCallback(() => {
    if (!snapshot) return;
    void agent.stopTimer().then(refresh);
  }, [snapshot, refresh]);

  const requestPause = useCallback(
    (secs: number) => {
      void agent.requestPause(secs).then((grant) => {
        setPauseRefused(!grant.granted);
        if (grant.granted) setPauseSecs(grant.granted_secs);
        void refresh();
      });
    },
    [refresh],
  );

  return {
    snapshot,
    error,
    pauseSecs,
    pauseRefused,
    grantConsent,
    start,
    stop,
    requestPause,
  };
}
