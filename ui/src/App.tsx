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
import { formatWorked, initials, monogramGradient } from "@/lib/format";
import { auth } from "@/lib/ipc";
import { cn } from "@/lib/utils";

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

  // Panel is the full-height Shell (TaskFlow layout): its own header/body/footer bars, no outer
  // padding — sections inset themselves with `mx-3`.
  return (
    <Panel
      key={scenarioKey}
      devBar={USE_MOCK ? <DevBar onChange={() => setScenarioKey((k) => k + 1)} /> : null}
      onSignOut={() => setAuthed(false)}
    />
  );
}

function Panel({ devBar, onSignOut }: { devBar: ReactNode; onSignOut?: () => void }) {
  const { snapshot, error, grantConsent, start, stop, refresh } = useAgent();
  const { theme, toggle: toggleTheme } = useTheme();

  if (error && !snapshot) {
    return (
      <div className="flex h-full items-center justify-center bg-background p-6 text-center">
        <p className="text-muted-foreground">
          Can't reach the WorkPulse core.
          <span className="mt-1 block text-[11px] text-muted-foreground/60">{error}</span>
        </p>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <span className="text-muted-foreground/60">Connecting…</span>
      </div>
    );
  }

  const { consent, capture, config, timer, projects, tasks, sessions, identity } = snapshot;

  // Consent gates the whole panel: nothing else is actionable until it's acknowledged.
  if (!consent.granted) {
    return (
      <div className="flex h-full flex-col bg-background p-3.5">
        {devBar}
        <ConsentCard consent={consent} silent={config.silent} onGrant={grantConsent} />
      </div>
    );
  }

  const showLive = capture.capturing && !config.silent;
  const totalSecs = sessions.reduce((s, x) => s + x.secs, 0);
  const todayHours = totalSecs / 3600;
  const name = identity?.name ?? "You";
  const running = timer.running;

  return (
    <div className="flex h-full flex-col bg-background">
      {/* Header bar — identity + today's progress, theme + sign-out. */}
      <header className="flex shrink-0 items-center justify-between gap-2 border-b border-border bg-card px-3 py-2">
        <div className="flex min-w-0 items-center gap-2.5">
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
        </div>

        <div className="flex shrink-0 items-center gap-1">
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

      {/* Scrollable body: recording hero (active) or the Today header (stopped), then sessions. */}
      <div className="flex-1 overflow-y-auto">
        {running ? (
          <RecordingCard
            timer={timer}
            projects={projects}
            tasks={tasks}
            sessions={sessions}
            onStop={stop}
          />
        ) : (
          <div className="flex items-end justify-between gap-3 border-b border-border px-4 pb-3.5 pt-4">
            <div className="min-w-0">
              <p className="text-[9.5px] font-semibold uppercase leading-none tracking-[0.12em] text-muted-foreground/80">
                Today
              </p>
              <p className="mt-1.5 text-sm font-semibold leading-tight text-foreground">Time Tracker</p>
              <p className="mt-0.5 text-[11px] leading-tight text-muted-foreground">
                {sessions.length > 0
                  ? `${sessions.length} session${sessions.length !== 1 ? "s" : ""} logged`
                  : "No sessions yet"}
              </p>
            </div>
            <span
              className={cn(
                "tabular-nums font-mono leading-none tracking-tight",
                sessions.length > 0
                  ? "text-[22px] font-bold text-foreground"
                  : "text-[20px] font-semibold text-muted-foreground/35",
              )}
            >
              {sessions.length > 0 ? formatWorked(totalSecs) : "00:00:00"}
            </span>
          </div>
        )}

        {!running && sessions.length === 0 && (
          <div className="mx-3 mt-3 flex items-start gap-3 rounded-lg border border-border bg-muted/30 px-4 py-5">
            <span className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
              <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
                <path d="M12 6v6l4 2" />
              </svg>
            </span>
            <div className="min-w-0">
              <p className="text-[12px] font-semibold leading-tight text-foreground">Ready when you are</p>
              <p className="mt-0.5 text-[10.5px] leading-snug text-muted-foreground">
                Describe what you're working on and pick a project to start tracking.
              </p>
            </div>
          </div>
        )}

        <SessionsCard
          sessions={sessions}
          projects={projects}
          timer={timer}
          onResume={start}
          goalHours={DAILY_GOAL_HOURS}
        />
      </div>

      {/* Switch-task strip — success top border while recording, plain when stopped. */}
      <div
        className={cn(
          "shrink-0 px-3 pb-3 pt-2.5",
          running
            ? "border-t border-success/30 bg-success/[0.03]"
            : "border-t border-border bg-card",
        )}
      >
        <p
          className={cn(
            "mb-2 text-[9.5px] font-semibold uppercase tracking-[0.10em]",
            running ? "text-success/85" : "text-muted-foreground/85",
          )}
        >
          Switch Task
        </p>
        <SwitchTaskCard
          projects={projects}
          tasks={tasks}
          running={running}
          onStart={start}
          onRefresh={refresh}
        />
      </div>

      {/* Footer bar. */}
      <footer className="flex shrink-0 items-center justify-between border-t border-border bg-card px-3 py-1.5">
        <div className="flex select-none items-center gap-1.5">
          <PulseMark />
          <span className="text-[10px] font-extrabold leading-none tracking-tight text-muted-foreground/80">
            Work<span className="text-primary">Pulse</span>
          </span>
        </div>
        <span className="text-[9.5px] leading-none text-muted-foreground/70">Agent</span>
      </footer>
    </div>
  );
}

function PulseMark() {
  return (
    <span
      className="flex h-3.5 w-3.5 items-center justify-center rounded-[4px]"
      style={{ background: "linear-gradient(135deg, var(--primary), #14b8a6)" }}
      aria-hidden
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round">
        <path d="M2 12h4l2.5 7 4-14 2.5 7H22" />
      </svg>
    </span>
  );
}
