import { useEffect, useState } from "preact/hooks";
import { LoginForm } from "./components/LoginForm";
import { TimerView } from "./components/TimerView";
import { ipc } from "./lib/ipc";
import type { AuthStatus } from "./lib/types";

// Root: gate on auth. Signed out → LoginForm; signed in → the timer surface. Auth state is polled
// once on mount (the core restores a keyring session at startup) and updated on login/logout.
export function App() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [agentId, setAgentId] = useState("");
  const [ready, setReady] = useState(false);

  useEffect(() => {
    ipc
      .authStatus()
      .then(setAuth)
      .catch(() => setAuth({ signedIn: false }))
      .finally(() => setReady(true));
    ipc.agentId().then(setAgentId).catch(() => {});
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

      {!ready ? (
        <p class="text-xs text-slate-500">Loading…</p>
      ) : auth?.signedIn ? (
        <TimerView />
      ) : (
        <LoginForm onSignedIn={setAuth} />
      )}

      <footer class="mt-auto text-[10px] text-slate-600">agent {agentId || "…"}</footer>
    </main>
  );
}
