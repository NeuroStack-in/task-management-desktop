import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { formatElapsed, formatWorked } from "@/lib/format";
import type { Project, Session, Task, TimerState } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * The live recording card — a faithful port of TaskFlow's TimerView recording hero, recoloured to
 * the panel's present theme: TaskFlow's emerald accent maps to the `--success` token, its `--primary`
 * stays the panel's teal. Structure, spacing and copy are TaskFlow's.
 *
 * Task switching lives in the Switch-Task strip below (App.tsx), exactly like the reference.
 */
export function RecordingCard({
  timer,
  projects,
  tasks,
  sessions,
  onStop,
  loading,
}: {
  timer: TimerState;
  projects: Project[];
  tasks: Task[];
  sessions: Session[];
  onStop: () => void;
  loading?: boolean;
}) {
  const desc = timer.description.trim();
  const taskTitle = tasks.find((t) => t.id === timer.task_id)?.title ?? "";
  // Title = the task if attributed, else the free-text description. Meta = project · description
  // (only when the description adds something beyond the title).
  const title = taskTitle || desc || "Working";
  const projectName = projects.find((p) => p.id === timer.project_id)?.name ?? "";
  const meta = [projectName, desc && desc !== title ? `· ${desc}` : ""].filter(Boolean).join(" ");
  const total = sessions.reduce((s, x) => s + x.secs, 0);

  return (
    <div className="mx-3 mt-3 overflow-hidden rounded-xl border border-success/30 bg-gradient-to-b from-success/[0.08] to-transparent shadow-sm">
      <div className="px-4 pb-3.5 pt-3.5 text-center">
        {/* Recording badge — pill with pulsing dot */}
        <div className="mb-3 inline-flex items-center gap-1.5 rounded-full border border-success/25 bg-success/12 px-2 py-0.5">
          <span className="relative flex h-1.5 w-1.5" aria-hidden>
            <span className="absolute h-full w-full animate-ping rounded-full bg-success opacity-70" />
            <span className="relative h-1.5 w-1.5 rounded-full bg-success" />
          </span>
          <span className="text-[9px] font-semibold uppercase tracking-[0.14em] text-success">
            Recording
          </span>
        </div>

        {/* Timer numerals — the hero */}
        <Numerals timer={timer} />

        <p
          className="mt-3 truncate px-2 text-[13px] font-semibold leading-tight tracking-[-0.005em] text-foreground"
          title={title}
        >
          {title}
        </p>
        {meta && (
          <p className="mt-0.5 truncate px-2 text-[10.5px] leading-snug text-muted-foreground" title={meta}>
            {meta}
          </p>
        )}

        {/* Stop button — outline on the tinted field so the accent stays dominant. */}
        <Button
          className={cn(
            "mt-3.5 h-9 w-full gap-2 text-[12.5px] font-semibold",
            "border border-destructive/25 bg-card text-destructive hover:border-destructive/40 hover:bg-card",
            "shadow-sm hover:shadow active:scale-[.985]",
          )}
          onClick={onStop}
          disabled={loading}
        >
          {loading ? (
            <span className="opacity-80">Stopping…</span>
          ) : (
            <>
              <StopIcon />
              Stop Timer
            </>
          )}
        </Button>
      </div>

      {/* Stats strip */}
      <div className="flex items-center justify-between border-t border-success/20 bg-success/[0.05] px-4 py-2">
        <span className="text-[10px] font-medium tracking-[0.005em] text-muted-foreground">
          {sessions.length} session{sessions.length !== 1 ? "s" : ""} today
        </span>
        <span className="tabular-nums font-mono text-[11px] font-bold text-foreground/85">
          {formatWorked(total)}
        </span>
      </div>
    </div>
  );
}

/**
 * HH:MM:SS numerals — **runs purely on the local wall-clock**, never on the poll.
 *
 * The session's start instant is anchored **once** (`startMs = now − core_elapsed`) and re-anchored
 * only when the session identity changes (a new task / a switch), NOT on every poll. Elapsed is then
 * `floor((now − startMs) / 1000)`, recomputed every 250 ms. Because the core's clock and the UI's are
 * the same wall clock, they agree — so the display can't run fast or skip, and it survives an app
 * relaunch mid-session (the core's elapsed seeds the anchor). This is the fix for the fast/jumping
 * clock: nothing about the display depends on when a network poll happens to land.
 */
function Numerals({ timer }: { timer: TimerState }) {
  // Session identity — changes on start/switch (which resets elapsed to ~0), so we re-anchor then.
  const key = `${timer.project_id ?? ""}|${timer.task_id ?? ""}|${timer.description}`;
  const anchor = useRef({ key, startMs: Date.now() - timer.elapsed_secs * 1000 });
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    anchor.current = { key, startMs: Date.now() - timer.elapsed_secs * 1000 };
    setNow(Date.now());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(id);
  }, []);

  const secs = Math.max(0, Math.floor((now - anchor.current.startMs) / 1000));
  const [hh, mm, ss] = formatElapsed(secs).split(":");
  return (
    <span
      className="block font-mono text-[38px] font-bold leading-none tracking-[-0.02em] tabular-nums text-success"
      aria-label={`${hh}:${mm}:${ss} elapsed`}
    >
      {hh}
      <span className="opacity-60">:</span>
      {mm}
      <span className="opacity-60">:</span>
      {ss}
    </span>
  );
}

function StopIcon() {
  return (
    <svg className="h-3.5 w-3.5 fill-current" viewBox="0 0 24 24" aria-hidden>
      <rect x="6" y="6" width="12" height="12" rx="1.5" />
    </svg>
  );
}
