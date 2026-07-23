import { useEffect, useRef } from "react";
import { LogOut } from "lucide-react";

import { ConsentCard } from "@/cards/ConsentCard";
import { LoginCard } from "@/cards/LoginCard";
import { PauseCard } from "@/cards/PauseCard";
import { SessionsCard, type ResumeSelection } from "@/cards/SessionsCard";
import { TimerCard } from "@/cards/TimerCard";
import { AutostartToggle, IdentityChip, LiveDot, StatusBadge, ThemeToggle } from "@/components/panel";
import { Button } from "@/components/ui/button";
import { startTimer, takePendingResume } from "@/lib/agent";
import { recordHistory } from "@/lib/descriptionHistory";
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
    restrictedHit,
    actionError,
    grantConsent,
    toggleTimer,
    switchTo,
    requestPause,
    signOut,
    dismissIdle,
    dismissRestricted,
    dismissActionError,
    refresh,
  } = useAgent();
  const { theme, toggle: toggleTheme } = useTheme();
  useResumeLastTask(snapshot?.auth.signedIn === true, snapshot?.timer.running === true, refresh);

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

  // Resume from a session row: start a new session on that row's (project, description) — the
  // grain the server folds on, so the time lands in the same timesheet row. While a timer runs,
  // resuming another row re-attributes to it (same semantics as the hero's pickers).
  const resumeSession = (sel: ResumeSelection) => {
    const full = { taskId: null, projectId: sel.projectId, description: sel.description };
    recordHistory(sel.description);
    if (timer.running) switchTo(full);
    else toggleTimer(full);
  };

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
        <AutostartToggle />
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

      {/* The window is a fixed 420×560 companion widget (tauri.conf.json), sized so this content
          fits without scrolling — that remains the target, and any card added here should still earn
          its height back from another rather than assuming it can grow.

          But the container **scrolls as a fallback** instead of clipping. It used to be
          `overflow-hidden`, on the reasoning that a hard clip keeps the panel from degrading into a
          scrolling document. The trade stopped being worth it once the panel narrowed to the
          sign-in width: at 420 px the session rows, task names and project labels wrap onto more
          lines than they did at 620, so the height the content *needs* went up at the same moment
          the window got shorter. Under a hard clip that combination silently swallows the bottom of
          the panel — the stop button, or the last session of the day — with nothing on screen to say
          anything is missing. A scrollbar that appears only when something genuinely overflows is a
          visible, recoverable failure instead of an invisible, lossy one.

          `min-h-0` still matters: without it the flex child refuses to shrink at all. */}
      <div className="wp-enter flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto overflow-x-hidden">
        {/* A user action (start/stop/switch/consent/pause) failed. Without this the button simply
            does nothing and the user has no idea why — the single highest-value beta feedback fix. */}
        {actionError && (
          <div
            role="alert"
            className="flex shrink-0 items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-2.5 py-1.5"
          >
            <span className="min-w-0 flex-1 text-[11px]">
              <span className="font-semibold">That didn&apos;t work.</span> {actionError}
            </span>
            <Button
              size="sm"
              variant="outline"
              className="h-6 px-2 text-[11px]"
              onClick={dismissActionError}
            >
              Dismiss
            </Button>
          </div>
        )}

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

        {/* A restricted app/site was focused while tracking. The violation is already on its way
            to the server — this banner is the "you were seen" half (Option 2 semantics: warn +
            flag, never block). Dismissible; the same identifier re-warns after the cooldown. */}
        {restrictedHit && (
          <div
            role="alert"
            className="flex shrink-0 items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-2.5 py-1.5"
          >
            <span className="min-w-0 flex-1 text-[11px]">
              <span className="font-semibold">{restrictedHit}</span> is restricted by your
              organization's policy during work sessions. This has been noted.
            </span>
            <Button
              size="sm"
              variant="outline"
              className="h-6 px-2 text-[11px]"
              onClick={dismissRestricted}
            >
              Dismiss
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
        <SessionsCard
          sessions={sessions}
          projects={projects}
          timer={timer}
          onResume={resumeSession}
        />

        <PauseCard pause={pause} refused={pauseRefused} onRequest={requestPause} />
      </div>
    </>
  );
}

/** True when two instants fall on the same day in the **user's** local calendar. */
function sameLocalDay(a: number, b: number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return (
    x.getFullYear() === y.getFullYear() &&
    x.getMonth() === y.getMonth() &&
    x.getDate() === y.getDate()
  );
}

/**
 * Pick the timer back up on the task the agent was running when it last closed.
 *
 * The core hands the task over once (`take_pending_resume` clears as it reads) and takes no view on
 * whether to use it — it has no local timezone. This decides, and only resumes when the agent closed
 * **earlier the same local day**: reopening on Wednesday must never silently restart, and bill,
 * Monday evening's task.
 *
 * The offline period itself is never credited. The core already closed the old session on quit, so
 * this starts a *fresh* one from now; today's total comes from the folded entries the server
 * returns. That is the difference between "resume the work" and "bill the hours the laptop was
 * shut" — only the first is something the agent can evidence.
 *
 * Runs once per launch (`claimed`), and only once signed in: resuming needs a session to attribute
 * the new timer to.
 */
function useResumeLastTask(signedIn: boolean, running: boolean, refresh: () => void) {
  const claimed = useRef(false);
  useEffect(() => {
    if (!signedIn || running || claimed.current) return;
    claimed.current = true;
    void (async () => {
      try {
        const pending = await takePendingResume();
        if (!pending || !sameLocalDay(pending.stoppedAtMs, Date.now())) return;
        await startTimer({
          taskId: pending.taskId || null,
          projectId: pending.projectId || null,
          description: pending.description,
        });
        refresh();
      } catch {
        // A failed resume must never block the panel: the user can always press Start.
      }
    })();
  }, [signedIn, running, refresh]);
}
