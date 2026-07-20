import { LogOut } from "lucide-react";

import { ConsentCard } from "@/cards/ConsentCard";
import { LoginCard } from "@/cards/LoginCard";
import { PauseCard } from "@/cards/PauseCard";
import { SessionsCard } from "@/cards/SessionsCard";
import { TimerCard } from "@/cards/TimerCard";
import { IdentityChip, LiveDot, StatusBadge, ThemeToggle } from "@/components/panel";
import { Button } from "@/components/ui/button";
import { useAgent } from "@/hooks/useAgent";
import { useTheme } from "@/hooks/useTheme";

export default function App() {
  return (
    <div className="flex h-full flex-col p-3.5">
      <Panel />
    </div>
  );
}

function Panel() {
  const {
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
  } = useAgent();
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

  const { auth, consent, capture, config, timer, pause, projects, tasks, sessions } = snapshot;

  // Sign-in gates everything, ahead of consent — there's no user to attribute consent to yet.
  // `refresh` rather than a local flag: the core owns auth state, so re-reading it is what proves
  // the sign-in actually took.
  if (!auth.signedIn) {
    return <LoginCard onSignedIn={() => void refresh()} />;
  }

  // Consent gates the rest of the panel: nothing else is actionable until it's acknowledged.
  if (!consent.granted) {
    return <ConsentCard consent={consent} silent={config.silent} onGrant={grantConsent} />;
  }

  // Silent mode suppresses the capture indicator by policy — the disclosure already covered it.
  const showLive = capture.capturing && !config.silent;

  return (
    <>
      <header className="mb-2.5 flex shrink-0 items-center gap-2">
        <LiveDot live={showLive} />
        <h1 className="font-heading text-[14px] font-semibold tracking-[0.2px]">WorkPulse</h1>
        <span className="ml-auto">
          {pause.paused ? (
            <StatusBadge tone="warn">paused</StatusBadge>
          ) : (
            <StatusBadge tone={showLive ? "on" : "neutral"}>
              {showLive ? "monitoring" : "idle"}
            </StatusBadge>
          )}
        </span>
        <ThemeToggle theme={theme} onToggle={toggleTheme} />
        {/* Hidden rather than faked when the core can't say who it's bound to. */}
        {snapshot.identity && (
          <>
            <span aria-hidden className="mx-0.5 h-4 w-px shrink-0 bg-border" />
            <IdentityChip identity={snapshot.identity} />
          </>
        )}
        <button
          type="button"
          onClick={signOut}
          aria-label="Sign out"
          title="Sign out"
          className="flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
        >
          <LogOut className="size-4" />
        </button>
      </header>

      {/* The window is sized to this content exactly (tauri.conf.json) and the panel must never
          scroll — `overflow-hidden` on request. That makes overflow a *clipping* failure rather
          than a scrolling one, so any card added here has to earn its height back from another.
          `min-h-0` still matters: without it the flex child refuses to shrink at all. */}
      <div className="wp-enter flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
        {/* Idle prompt — the core emits this at 5 min and hard-stops at 15 (events.rs:7). Offering
            keep/stop here is the whole point of the event: silently banking idle time, or silently
            discarding it, are both wrong. */}
        {idleSecs !== null && timer.running && (
          <div
            role="alert"
            className="flex shrink-0 items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-2.5 py-1.5"
          >
            <span className="min-w-0 flex-1 truncate text-[11px]">
              No activity for {Math.round(idleSecs / 60)} min — still working?
            </span>
            <Button size="sm" variant="outline" className="h-6 px-2 text-[11px]" onClick={dismissIdle}>
              Keep going
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="h-6 px-2 text-[11px]"
              onClick={() => toggleTimer({ taskId: null, projectId: null, description: "" })}
            >
              Stop
            </Button>
          </div>
        )}

        {/* Capture denied at the OS level. Silence here would read as "screenshots are off by
            policy", which is a very different thing from "the OS is refusing" (monitor risk #5). */}
        {screenshotBlocked && (
          <div
            role="alert"
            className="shrink-0 rounded-lg border border-destructive/30 bg-destructive/10 px-2.5 py-1.5 text-[11px]"
          >
            Screen capture is blocked by your operating system. Grant screen-recording permission to
            WorkPulse, then restart the agent.
          </div>
        )}

        <TimerCard
          timer={timer}
          projects={projects}
          tasks={tasks}
          onToggle={toggleTimer}
          onSwitch={switchTo}
          onRefresh={refresh}
        />

        {/* Sessions used to share this row with a Status card. That card was removed, so this
            runs the full panel width and lines up with the timer above and the pause card
            below, rather than sitting in a half-width column with dead space beside it. */}
        <SessionsCard sessions={sessions} projects={projects} timer={timer} />

        <PauseCard pause={pause} refused={pauseRefused} onRequest={requestPause} />
      </div>
    </>
  );
}
