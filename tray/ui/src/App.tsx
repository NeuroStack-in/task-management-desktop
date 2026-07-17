import { useState, type ReactNode } from "react";

import { ConsentCard } from "@/cards/ConsentCard";
import { PauseCard } from "@/cards/PauseCard";
import { SessionsCard } from "@/cards/SessionsCard";
import { StatusCard } from "@/cards/StatusCard";
import { TimerCard } from "@/cards/TimerCard";
import { DevBar } from "@/components/DevBar";
import { IdentityChip, LiveDot, StatusBadge, ThemeToggle } from "@/components/panel";
import { useAgent } from "@/hooks/useAgent";
import { useTheme } from "@/hooks/useTheme";
import { USE_MOCK } from "@/lib/agent";

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

      {/* The window is sized to this content exactly (tauri.conf.json), so nothing should ever
          scroll. `overflow-y-auto` stays as a safety valve — if a future card pushes past the
          fixed height, scrolling is a better failure than Card's `overflow-hidden` silently
          clipping the last row. `min-h-0` is what lets that valve work at all: without it the
          flex child refuses to shrink and overflows the panel instead. */}
      <div className="wp-enter flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
        <TimerCard
          timer={timer}
          tasks={snapshot.tasks}
          onToggle={toggleTimer}
          onSelectTask={setTask}
        />

        {/* Side by side: the panel is 620 wide, and stacking these left a long dead gap
            under the last card. `items-stretch` keeps both cards the same height. */}
        <div className="grid grid-cols-2 items-stretch gap-3">
          <StatusCard
            capture={capture}
            paused={paused}
            activity={snapshot.activity}
            config={config}
            consent={consent}
          />
          <SessionsCard sessions={snapshot.sessions} tasks={snapshot.tasks} timer={timer} />
        </div>

        <PauseCard pauseSecs={pauseSecs} refused={pauseRefused} onRequest={requestPause} />
      </div>
    </>
  );
}
