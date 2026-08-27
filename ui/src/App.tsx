import { useCallback, useEffect, useRef, useState } from "react";
import { LogOut } from "lucide-react";

import {
  updateInstall,
  updateStatus,
  type UpdateStatus,
} from "@/lib/agent";

import { LoginCard } from "@/cards/LoginCard";
import { PauseCard } from "@/cards/PauseCard";
import { PrivacyLogCard } from "@/cards/PrivacyLogCard";
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
    adminCapture,
    actionError,
    grantConsent,
    toggleTimer,
    switchTo,
    createTask,
    requestPause,
    signOut,
    dismissIdle,
    dismissRestricted,
    dismissAdminCapture,
    dismissActionError,
    refresh,
  } = useAgent();
  const { theme, toggle: toggleTheme } = useTheme();
  useResumeLastTask(snapshot?.auth.signedIn === true, snapshot?.timer.running === true, refresh);

  // The monitoring notice has been removed from the panel, but the core still gates activity and
  // screenshot capture on consent (monitor/mod.rs — fails closed). Record it silently once the user
  // is signed in so capture keeps running; it's persisted, so this is a one-time write per policy
  // version, not a call on every launch.
  const signedIn = snapshot?.auth.signedIn === true;
  const consentGranted = snapshot?.consent.granted === true;
  useEffect(() => {
    if (signedIn && !consentGranted) grantConsent();
  }, [signedIn, consentGranted, grantConsent]);

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

  const { auth, capture, config, timer, pause, projects, tasks, sessions, privacyLog } = snapshot;

  // Sign-in gates everything. `refresh` rather than a local flag: the core owns auth state, so
  // re-reading it is what proves the sign-in actually took. (There is no separate consent gate any
  // more — the monitoring notice was removed; consent is recorded silently above.)
  if (!auth.signedIn) {
    return <LoginCard onSignedIn={() => void refresh()} />;
  }

  // Silent mode suppresses the capture indicator by policy — the disclosure already covered it.
  const showLive = capture.capturing && !config.silent;

  /**
   * Is any banner occupying the top of the panel right now?
   *
   * This decides **which element scrolls**. The window is a fixed size, so there is only ever room
   * for one scroller: with no banner the panel fits and Today's sessions scrolls inside its own cap;
   * a banner takes that room away, so the cap is lifted and the panel scrolls as a whole instead.
   *
   * Keep this list in step with the banners rendered below — a banner missing from it takes height
   * without releasing the cap, which is precisely the state where content became unreachable.
   */
  const hasBanner =
    Boolean(actionError) ||
    (idleSecs !== null && timer.running) ||
    Boolean(restrictedHit) ||
    Boolean(adminCapture) ||
    Boolean(screenshotBlocked);

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

      {/* Version + update status, pinned directly under the header — the first thing on screen, not
          the last. Sits outside the scroller so it never scrolls away; renders nothing until the
          core can report a status (see the component). */}
      <UpdateStrip />

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

          `min-h-0` still matters: without it the flex child refuses to shrink at all.

          **Which element scrolls depends on whether a banner is up** — see `hasBanner` above. With
          no banner the panel is meant to fit, and Today's sessions absorbs a long day inside its own
          capped box. Once a banner takes height at the top there is no longer room to fit, so the
          cap comes off and this container becomes the single scroller. The two are never scrollable
          at the same time, which is what stops a wheel over the session list from being swallowed
          instead of moving the panel. */}
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

        {/* An administrator asked for an on-demand screenshot of this machine — taken or refused.
            This is the one capture the employee has no other way to notice, so it is announced
            rather than merely logged (PRIVACY.md §5: no silent access). Dismissing hides the
            banner; the entry stays in the privacy log below. */}
        {adminCapture && (
          <div
            role="alert"
            className="flex shrink-0 items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-2.5 py-1.5"
          >
            <span className="min-w-0 flex-1 text-[11px]">{adminCapture}</span>
            <Button
              size="sm"
              variant="outline"
              className="h-6 px-2 text-[11px]"
              onClick={dismissAdminCapture}
            >
              Dismiss
            </Button>
          </div>
        )}

        {/* Capture produced nothing. Silence here would read as "screenshots are off by policy",
            which is a very different thing from "capture is failing" (monitor risk #5).

            The text comes from the core (`events::capture_failure_hint`) rather than being written
            here, because only macOS has a screen-recording permission to grant — this banner used
            to tell Windows and Linux users to grant one anyway. */}
        {screenshotBlocked && (
          <div
            role="alert"
            className="shrink-0 rounded-lg border border-destructive/30 bg-destructive/10 px-2.5 py-1.5 text-[11px]"
          >
            {screenshotBlocked}
          </div>
        )}

        <TimerCard
          timer={timer}
          projects={projects}
          tasks={tasks}
          onToggle={toggleTimer}
          onSwitch={switchTo}
          onCreateTask={createTask}
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
          capped={!hasBanner}
        />

        <PauseCard pause={pause} refused={pauseRefused} onRequest={requestPause} />

        {/* Renders only when something has actually been done to this machine — see the card. */}
        <PrivacyLogCard events={privacyLog} />
      </div>
    </>
  );
}

/**
 * The version line, and the only place an employee can see whether their agent is current.
 *
 * The agent already self-updates — at launch, then every 6 h — so this is deliberately **not** the
 * mechanism, just the window onto it. Before it existed the sole evidence of an update was the
 * version quietly changing, and "am I on the latest?" could only be answered by reading a log file.
 *
 * Three states, and the distinction between the last two is the point:
 *   - **up to date** — we asked, and there is nothing newer.
 *   - **update available** — offered as a button, because waiting up to 6 h for the next automatic
 *     check is a poor answer to someone who has just been told a fix exists.
 *   - **couldn't check** — offline, or a build with no signing key. Never rendered as "up to date":
 *     that is a claim we haven't earned, and it is exactly the one someone would rely on.
 */
function UpdateStrip() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [installed, setInstalled] = useState<string | null>(null);

  const check = useCallback(() => {
    setBusy(true);
    updateStatus()
      .then(setStatus)
      .catch(() => setStatus(null))
      .finally(() => setBusy(false));
  }, []);

  useEffect(check, [check]);

  const install = () => {
    setBusy(true);
    updateInstall()
      .then((v) => setInstalled(v))
      .catch((e: unknown) =>
        setStatus((s) =>
          s ? { ...s, checked: false, error: String(e) } : s,
        ),
      )
      .finally(() => setBusy(false));
  };

  // The core can't tell us anything yet — say nothing rather than guess.
  if (!status) return null;

  const available = status.latest !== null;

  return (
    <div className="mb-2.5 flex shrink-0 items-center gap-2 border-b border-border/60 px-0.5 pb-2.5 text-[11px] text-muted-foreground">
      <span className="font-mono">v{status.current}</span>
      <span aria-hidden>·</span>

      {installed ? (
        <span className="text-foreground">
          Updated to {installed} — restart to finish
        </span>
      ) : available ? (
        <>
          <span className="text-foreground">Version {status.latest} available</span>
          <Button
            size="sm"
            variant="outline"
            className="ml-auto h-6 px-2 text-[11px]"
            disabled={busy}
            onClick={install}
          >
            {busy ? "Updating…" : "Update now"}
          </Button>
        </>
      ) : status.checked ? (
        <>
          <span>Up to date</span>
          <button
            type="button"
            onClick={check}
            disabled={busy}
            className="ml-auto transition-colors hover:text-foreground disabled:opacity-50"
          >
            {busy ? "Checking…" : "Check again"}
          </button>
        </>
      ) : (
        <>
          <span title={status.error ?? undefined}>Couldn&apos;t check for updates</span>
          <button
            type="button"
            onClick={check}
            disabled={busy}
            className="ml-auto transition-colors hover:text-foreground disabled:opacity-50"
          >
            {busy ? "Checking…" : "Retry"}
          </button>
        </>
      )}
    </div>
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
