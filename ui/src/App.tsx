import { LogOut } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { ConsentCard } from "@/cards/ConsentCard";
import { RecordingCard } from "@/cards/RecordingCard";
import { SessionsCard } from "@/cards/SessionsCard";
import { SwitchTaskCard } from "@/cards/SwitchTaskCard";
import { DevBar } from "@/components/DevBar";
import { LoginForm } from "@/components/LoginForm";
import { LiveDot, ThemeToggle } from "@/components/panel";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { useAgent } from "@/hooks/useAgent";
import { useTheme } from "@/hooks/useTheme";
import { USE_MOCK } from "@/lib/agent";
import { initials, monogramGradient } from "@/lib/format";
import { auth } from "@/lib/ipc";

/** Daily tracked-hours goal shown in the header ("Xh / 4h goal"). Config-driven later. */
const DAILY_GOAL_HOURS = 4;

export default function App() {
  // Remounting Panel on scenario change resets useAgent, so preview state can't drift out of
  // sync with the fake core. Dev-only: in production `key` never changes.
  const [scenarioKey, setScenarioKey] = useState(0);

  // Auth gate. Mock/dev never has a real Cognito session, so it goes straight to the panel
  // (the whole point of mock is to design without the core). Production starts unknown (null),
  // resolves against a resumed keyring session, then shows login-or-panel.
  const [authed, setAuthed] = useState<boolean | null>(USE_MOCK ? true : null);

  useEffect(() => {
    if (USE_MOCK) return;
    let live = true;
    auth
      .status()
      .then((s) => live && setAuthed(!!s.signedIn))
      .catch(() => live && setAuthed(false));
    return () => {
      live = false;
    };
  }, []);

  if (authed === null) {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <span className="text-muted-foreground/60">Starting WorkPulse…</span>
      </div>
    );
  }

  if (!authed) {
    return (
      <div className="h-full">
        <LoginForm onSignedIn={() => setAuthed(true)} />
      </div>
    );
  }

  // The dev chips are passed *into* Panel rather than rendered above it, so they sit under the
  // real header. The header carries the identity chip, which must stay in the top-right corner
  // of the shipped panel — and production has no dev row to push it down.
  return (
    <div className="flex h-full flex-col p-3.5">
      <Panel
        key={scenarioKey}
        devBar={USE_MOCK ? <DevBar onChange={() => setScenarioKey((k) => k + 1)} /> : null}
        onSignOut={() => setAuthed(false)}
      />
    </div>
  );
}

function Panel({ devBar, onSignOut }: { devBar: ReactNode; onSignOut?: () => void }) {
  const { snapshot, error, grantConsent, start, stop } = useAgent();
  const { theme, toggle: toggleTheme } = useTheme();

  if (error && !snapshot) {
    return (
      <div className="flex flex-1 items-center justify-center p-6 text-center">
        <p className="text-muted-foreground">
          Can't reach the WorkPulse core.
          <span className="mt-1 block text-[11px] text-muted-foreground/60">{error}</span>
        </p>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <span className="text-muted-foreground/60">Connecting…</span>
      </div>
    );
  }

  const { consent, capture, config, timer, projects, sessions, identity } = snapshot;

  // Consent gates the whole panel: nothing else is actionable until it's acknowledged.
  // The dev chips still render, or there'd be no way back out of the First run preview.
  if (!consent.granted) {
    return (
      <>
        {devBar}
        <ConsentCard consent={consent} silent={config.silent} onGrant={grantConsent} />
      </>
    );
  }

  // Silent mode suppresses the capture indicator by policy — the disclosure already covered it.
  const showLive = capture.capturing && !config.silent;
  const todayHours = sessions.reduce((s, x) => s + x.secs, 0) / 3600;
  const name = identity?.name ?? "You";

  return (
    <>
      <header className="mb-3 flex shrink-0 items-center gap-2.5">
        <Avatar size="lg">
          {identity?.avatar_url && <AvatarImage src={identity.avatar_url} alt="" />}
          <AvatarFallback
            className="text-[13px] font-semibold text-white"
            style={{ background: monogramGradient(name) }}
          >
            {initials(name)}
          </AvatarFallback>
        </Avatar>
        <div className="min-w-0">
          <p className="truncate text-[13px] font-bold uppercase tracking-wide">{name}</p>
          <p className="text-[11px] text-muted-foreground">
            <span className="font-semibold text-primary">{todayHours.toFixed(1)}h</span> /{" "}
            {DAILY_GOAL_HOURS}h goal
          </p>
        </div>

        <div className="ml-auto flex shrink-0 items-center gap-1">
          <span className="mr-1 inline-flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            <LiveDot live={showLive} />
            {showLive ? "monitoring" : "idle"}
          </span>
          <ThemeToggle theme={theme} onToggle={toggleTheme} />
          {onSignOut && !USE_MOCK && (
            <Button
              variant="ghost"
              size="icon-sm"
              title="Sign out"
              aria-label="Sign out"
              className="text-muted-foreground"
              onClick={async () => {
                try {
                  await auth.logout();
                } finally {
                  onSignOut();
                }
              }}
            >
              <LogOut />
            </Button>
          )}
        </div>
      </header>

      {devBar}

      {/* The window is sized to this content exactly (tauri.conf.json), so nothing should ever
          scroll. `overflow-y-auto` stays as a safety valve — if a future card pushes past the
          fixed height, scrolling is a better failure than Card's `overflow-hidden` silently
          clipping the last row. `min-h-0` is what lets that valve work at all: without it the
          flex child refuses to shrink and overflows the panel instead. */}
      <div className="wp-enter flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
        <RecordingCard timer={timer} projects={projects} sessions={sessions} onStop={stop} />
        <SessionsCard sessions={sessions} projects={projects} timer={timer} />
        <SwitchTaskCard projects={projects} running={timer.running} onStart={start} />
      </div>

      <footer className="mt-3 flex shrink-0 items-center justify-between text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
        <span className="font-semibold text-foreground/70">WorkPulse</span>
        <span>Agent</span>
      </footer>
    </>
  );
}
