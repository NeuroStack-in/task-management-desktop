import { ListChecks } from "lucide-react";

import { CardTitleRow, PanelCard } from "@/components/panel";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatElapsed } from "@/lib/format";
import type { Project, Session, TimerState } from "@/lib/types";

/**
 * Today's sessions — one row per (project + description) worked, with the running one's live segment
 * folded in so it ticks alongside the hero clock.
 *
 * Real data has no read-back command yet (see `Session` in types.ts), so against the core this
 * degrades to an empty state rather than inventing rows.
 */
export function SessionsCard({
  sessions,
  projects,
  timer,
}: {
  sessions: Session[];
  projects: Project[];
  timer: TimerState;
}) {
  const total = sessions.reduce((s, x) => s + x.secs, 0);
  const nameOf = (id: string) => projects.find((p) => p.id === id)?.name ?? "Project";

  return (
    <PanelCard className="flex-1">
      <CardHeader>
        <CardTitleRow
          icon={<ListChecks />}
          label="Today's sessions"
          action={
            sessions.length > 0 ? (
              <span className="text-[11px] text-muted-foreground">
                {sessions.length} {sessions.length === 1 ? "session" : "sessions"}
              </span>
            ) : undefined
          }
        />
      </CardHeader>
      <CardContent>
        {sessions.length === 0 ? (
          <p className="py-6 text-center text-[11px] text-muted-foreground">
            Nothing tracked yet today. Start a timer against a project to log time.
          </p>
        ) : (
          <>
            <ul className="max-h-[104px] space-y-1.5 overflow-y-auto pr-1">
              {sessions.map((s, i) => {
                const running =
                  timer.running &&
                  timer.project_id === s.project_id &&
                  timer.description.trim() === s.description;
                return (
                  <li
                    key={`${s.project_id}:${s.description}:${i}`}
                    className="flex items-center gap-2"
                  >
                    <span
                      aria-hidden
                      className={
                        running
                          ? "flex size-5 shrink-0 items-center justify-center rounded-md bg-success/15"
                          : "flex size-5 shrink-0 items-center justify-center rounded-md bg-primary/10"
                      }
                    >
                      <span
                        className={
                          running
                            ? "size-1.5 animate-pulse rounded-full bg-success"
                            : "size-1.5 rounded-full bg-primary/60"
                        }
                      />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[12px] font-medium">
                        {s.description || "Untitled session"}
                      </span>
                      <span className="block truncate text-[10px] text-muted-foreground">
                        {nameOf(s.project_id)}
                      </span>
                    </span>
                    <span
                      className={
                        running
                          ? "tabular shrink-0 text-[11px] font-medium text-success"
                          : "tabular shrink-0 text-[11px] text-muted-foreground"
                      }
                    >
                      {formatElapsed(s.secs)}
                    </span>
                  </li>
                );
              })}
            </ul>

            <div className="mt-2 flex items-baseline justify-between border-t border-border/60 pt-2">
              <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                Total
              </span>
              <span className="tabular text-[13px] font-semibold">{formatElapsed(total)}</span>
            </div>
          </>
        )}
      </CardContent>
    </PanelCard>
  );
}
