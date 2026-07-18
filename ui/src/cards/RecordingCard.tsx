import { Square } from "lucide-react";
import type { ReactNode } from "react";

import { PanelCard } from "@/components/panel";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { formatCountdown, formatElapsed } from "@/lib/format";
import type { Project, Session, TimerState } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * The recording hero (TaskFlow layout): a RECORDING pill, the big segmented clock, the current
 * task + its project, and a full-width Stop button — with today's session count + total on a
 * footer bar. Task *switching* lives in the separate Switch-Task card below, exactly like the
 * reference, so this card is only ever about the session in progress.
 *
 * Uses the panel's existing Slate & Teal tokens: the clock and Stop button are `--primary`
 * (teal), the live pill is `--success` (green) — no TaskFlow purple.
 */
export function RecordingCard({
  timer,
  projects,
  sessions,
  onStop,
}: {
  timer: TimerState;
  projects: Project[];
  sessions: Session[];
  onStop: () => void;
}) {
  const running = timer.running;
  const [hh, mm, ss] = formatElapsed(timer.elapsed_secs).split(":");
  const total = sessions.reduce((s, x) => s + x.secs, 0);
  const count = sessions.length;
  const projectName = projects.find((p) => p.id === timer.project_id)?.name ?? null;
  const description = timer.description.trim();

  return (
    <PanelCard className="overflow-hidden p-0">
      <CardContent className="flex flex-col items-center gap-3 px-5 py-5 text-center">
        <span
          className={cn(
            "inline-flex items-center gap-2 rounded-full px-3 py-1 text-[11px] font-semibold uppercase tracking-wider",
            running ? "bg-success/12 text-success" : "bg-muted text-muted-foreground",
          )}
        >
          <span
            className={cn(
              "size-1.5 rounded-full",
              running ? "animate-pulse bg-success" : "bg-muted-foreground/50",
            )}
          />
          {running ? "Recording" : "Not tracking"}
        </span>

        <div className="flex items-center font-heading tabular text-primary">
          <Digit>{hh}</Digit>
          <Colon />
          <Digit>{mm}</Digit>
          <Colon />
          <Digit>{ss}</Digit>
        </div>

        {running ? (
          <div className="min-w-0">
            <p className="truncate text-[15px] font-semibold">
              {description || "Untitled session"}
            </p>
            {projectName && (
              <p className="truncate text-[11px] text-muted-foreground">{projectName}</p>
            )}
          </div>
        ) : (
          <p className="text-[12px] text-muted-foreground">
            Pick a project below to start tracking.
          </p>
        )}

        {running && (
          <Button size="lg" onClick={onStop} className="mt-1 w-full gap-2">
            <Square className="fill-current" />
            Stop Timer
          </Button>
        )}
      </CardContent>

      <div className="flex items-center justify-between border-t border-border/60 bg-muted/40 px-4 py-2.5">
        <span className="text-[11px] text-muted-foreground">
          {count} {count === 1 ? "session" : "sessions"} today
        </span>
        <span className="tabular text-[12px] font-semibold">{formatCountdown(total)}</span>
      </div>
    </PanelCard>
  );
}

function Digit({ children }: { children: ReactNode }) {
  return (
    <span className="text-[46px] font-semibold leading-none tracking-tight">{children}</span>
  );
}

function Colon() {
  return (
    <span className="px-1.5 text-[38px] font-semibold leading-none text-primary/40">:</span>
  );
}
