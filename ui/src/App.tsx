import { useState, type ReactNode } from "react";

import { ConsentCard } from "@/cards/ConsentCard";
import { LoginCard } from "@/cards/LoginCard";
import { PauseCard } from "@/cards/PauseCard";
import { SessionsCard } from "@/cards/SessionsCard";
import { TimerCard } from "@/cards/TimerCard";
import { DevBar } from "@/components/DevBar";
import { IdentityChip, LiveDot, StatusBadge, ThemeToggle } from "@/components/panel";
import { useAgent } from "@/hooks/useAgent";
import { useTheme } from "@/hooks/useTheme";
import { USE_MOCK } from "@/lib/agent";
import { getScenario } from "@/lib/mock";

export default function App() {
  // Remounting Panel on scenario change resets useAgent, so preview state can't drift out of
  // sync with the fake core. Dev-only: in production `key` never changes.
  const [scenarioKey, setScenarioKey] = useState(0);

  // The dev chips are passed *into* Panel rather than rendered above it, so they sit under the
  // real header. The header carries the identity chip, which must stay in the top-right corner
  // of the shipped panel — and production has no dev row to push it down.
  return (
    <div className="flex h-full flex-col p-3.5">
      <Panel
        key={scenarioKey}
        devBar={USE_MOCK ? <DevBar onChange={() => setScenarioKey((k) => k + 1)} /> : null}
      />
    </div>
  );
}

function Panel({ devBar }: { devBar: ReactNode }) {
  const {
    snapshot,
    error,
    pauseSecs,
    pauseRefused,
    grantConsent,
    toggleTimer,
    setTask,
    requestPause,
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

  const { consent, capture, config, timer } = snapshot;
  const paused = pauseSecs > 0;

  // Sign-in gates everything, ahead of consent — there's no user to attribute consent to yet.
  // Preview-only: guarded by USE_MOCK so mock.ts is never consulted in a shipped build. The real
  // gate will read auth state from the core instead of the scenario.
  if (USE_MOCK && getScenario() === "login") {
    return (
      <>
        {devBar}
        <LoginCard />
      </>
    );
  }

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

  return (
    <>
      <header className="mb-2.5 flex shrink-0 items-center gap-2">
        <LiveDot live={showLive} />
        <h1 className="font-heading text-[14px] font-semibold tracking-[0.2px]">WorkPulse</h1>
        <span className="ml-auto">
          {paused ? (
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
      </header>

      {devBar}

      {/* The window is sized to this content exactly (tauri.conf.json) and the panel must never
          scroll — `overflow-hidden` on request. That makes overflow a *clipping* failure rather
          than a scrolling one, so any card added here has to earn its height back from another.
          `min-h-0` still matters: without it the flex child refuses to shrink at all. */}
      <div className="wp-enter flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
        <TimerCard
          timer={timer}
          tasks={snapshot.tasks}
          onToggle={toggleTimer}
          onSelectTask={setTask}
          onRefresh={refresh}
        />

        {/* Sessions used to share this row with a Status card. That card was removed, so this
            runs the full panel width and lines up with the timer above and the pause card
            below, rather than sitting in a half-width column with dead space beside it. */}
        <SessionsCard sessions={snapshot.sessions} tasks={snapshot.tasks} timer={timer} />

        <PauseCard pauseSecs={pauseSecs} refused={pauseRefused} onRequest={requestPause} />
      </div>
    </>
  );
}
