import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

import { core, type AuthStatus } from "@/lib/core";
import { USE_MOCK } from "@/lib/agent";

/**
 * The auth gate that sits in front of the whole panel.
 *
 * The tray panel was designed for a core that had already enrolled the device, so it had no
 * sign-in at all. This core signs in with Cognito at runtime (auth/cognito.rs) and every other
 * command is useless until it has — so the gate is ported in rather than dropped.
 *
 * Under the mock there is no Tauri bridge to ask, so we report signed-in and let the designer
 * see the panel itself; `LoginCard` is reachable in dev via `VITE_REAL=1`.
 */
export interface Auth {
  status: AuthStatus | null;
  /** False until the first `auth_status` settles — avoids flashing the login card on reload. */
  ready: boolean;
  /** Core-pushed notices (session expiry, idle prompt, screenshot permission). */
  notice: string | null;
  dismissNotice: () => void;
  setStatus: (s: AuthStatus) => void;
  signOut: () => void;
}

const MOCK_STATUS: AuthStatus = { signedIn: true, username: "owner@acme.test" };

export function useAuth(): Auth {
  const [status, setStatus] = useState<AuthStatus | null>(USE_MOCK ? MOCK_STATUS : null);
  const [ready, setReady] = useState(USE_MOCK);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (USE_MOCK) return;
    core
      .authStatus()
      .then(setStatus)
      .catch(() => setStatus({ signedIn: false }))
      .finally(() => setReady(true));
  }, []);

  // Core → UI events (src-tauri/src/events.rs). Carried over from the previous UI verbatim:
  // these are the only way the user learns the core stopped tracking without being asked.
  useEffect(() => {
    if (USE_MOCK) return;
    const subs = [
      listen("auth:expired", () => {
        setStatus({ signedIn: false });
        setNotice("Your session expired — please sign in again.");
      }),
      listen<number>("monitor:idle-prompt", (e) =>
        setNotice(
          `You've been idle ~${Math.round((e.payload ?? 0) / 60)} min. Tracking auto-stops at 15 min.`,
        ),
      ),
      listen("monitor:screenshot-unavailable", () =>
        setNotice("Screenshots unavailable — grant Screen Recording permission (macOS) and retry."),
      ),
    ];
    return () => {
      subs.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, []);

  const signOut = useCallback(() => {
    void core.authLogout().catch(() => {});
    setStatus({ signedIn: false });
  }, []);

  return {
    status,
    ready,
    notice,
    dismissNotice: () => setNotice(null),
    setStatus,
    signOut,
  };
}
