import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "preact/hooks";
import { ConsentCard } from "./components/ConsentCard";
import { LoginForm } from "./components/LoginForm";
import { TimerView } from "./components/TimerView";
import { ipc } from "./lib/ipc";
import type { AuthStatus } from "./lib/types";

// Root: gate on auth, then on consent. Signed out → LoginForm. Signed in → the consent control +
// timer. Core events (idle prompt, screenshot-permission, session-expired) surface as notices.
export function App() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [agentId, setAgentId] = useState("");
  const [consent, setConsent] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    ipc
      .authStatus()
      .then(setAuth)
      .catch(() => setAuth({ signedIn: false }))
      .finally(() => setReady(true));
    ipc.agentId().then(setAgentId).catch(() => {});
    ipc.consentStatus().then(setConsent).catch(() => {});
  }, []);

  // Core → UI events (see Rust `events.rs`).
  useEffect(() => {
    const subs = [
      listen("auth:expired", () => {
        setAuth({ signedIn: false });
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

  async function signOut() {
    await ipc.authLogout().catch(() => {});
    setAuth({ signedIn: false });
  }

  return (
    <main class="flex min-h-screen flex-col gap-4 bg-slate-950 p-6 text-slate-100">
      <header class="flex items-center justify-between">
        <h1 class="text-base font-semibold">WorkPulse</h1>
        {auth?.signedIn && (
          <button onClick={signOut} class="text-xs text-slate-400 hover:text-slate-200">
            Sign out{auth.username ? ` (${auth.username})` : ""}
          </button>
        )}
      </header>

      {notice && (
        <div class="flex items-start justify-between gap-3 rounded-md bg-amber-500/10 p-3 text-xs text-amber-300">
          <span>{notice}</span>
          <button onClick={() => setNotice(null)} class="text-amber-400/70 hover:text-amber-300">
            ✕
          </button>
        </div>
      )}

      {!ready ? (
        <p class="text-xs text-slate-500">Loading…</p>
      ) : auth?.signedIn ? (
        <>
          <ConsentCard granted={consent} onChange={setConsent} />
          <TimerView />
        </>
      ) : (
        <LoginForm onSignedIn={setAuth} />
      )}

      <footer class="mt-auto text-[10px] text-slate-600">agent {agentId || "…"}</footer>
    </main>
  );
}
