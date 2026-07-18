import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "preact/hooks";
import { ConsentCard } from "./components/ConsentCard";
import { LoginForm } from "./components/LoginForm";
import { WorkPulseLogo } from "./components/Logo";
import { TimerView } from "./components/TimerView";
import { Button } from "./components/ui/Button";
import { ipc } from "./lib/ipc";
import type { AuthStatus } from "./lib/types";

// Root: gate on auth, then on consent. Signed out → LoginForm (TaskFlow-styled). Signed in → the
// consent control + timer. Core events (idle prompt, screenshot-permission, session-expired) surface
// as notices.
export function App() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [consent, setConsent] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    ipc
      .authStatus()
      .then(setAuth)
      .catch(() => setAuth({ signedIn: false }))
      .finally(() => setReady(true));
    ipc.consentStatus().then(setConsent).catch(() => {});
  }, []);

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

  if (!ready) {
    return (
      <div class="flex h-full items-center justify-center bg-background">
        <div class="h-6 w-6 animate-spin rounded-full border-2 border-muted border-t-primary" />
      </div>
    );
  }

  if (!auth?.signedIn) {
    return <LoginForm onSuccess={setAuth} />;
  }

  return (
    <div class="flex h-full flex-col bg-background text-foreground">
      <header class="flex items-center justify-between border-b border-border px-4 py-2.5">
        <div class="flex items-center gap-2">
          <WorkPulseLogo size={22} />
          <span class="text-[13px] font-bold tracking-[-0.01em]">
            Work<span class="text-primary">Pulse</span>
          </span>
        </div>
        <Button variant="ghost" size="sm" class="h-7 text-muted-foreground" onClick={signOut}>
          Sign out{auth.username ? ` · ${auth.username}` : ""}
        </Button>
      </header>

      {notice && (
        <div class="mx-4 mt-3 flex items-start justify-between gap-3 rounded-md border border-amber-500/25 bg-amber-500/10 px-3 py-2 text-[11.5px] text-amber-500">
          <span>{notice}</span>
          <button onClick={() => setNotice(null)} class="text-amber-500/70 hover:text-amber-500">
            ✕
          </button>
        </div>
      )}

      <div class="flex-1 space-y-3 overflow-y-auto p-4">
        <ConsentCard granted={consent} onChange={setConsent} />
        <TimerView />
      </div>
    </div>
  );
}
