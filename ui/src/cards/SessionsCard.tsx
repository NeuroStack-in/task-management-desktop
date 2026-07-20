import { ListChecks } from "lucide-react";

import { CardTitleRow, PanelCard } from "@/components/panel";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatElapsed } from "@/lib/format";
import type { Session, Task, TimerState } from "@/lib/types";

/**
 * Today's per-task totals. The running task's live segment is folded in by the core-facing
 * layer, so its row ticks alongside the hero clock rather than looking frozen.
 *
 * PROPOSED data — see `Session` in types.ts. Degrades to an empty state against the real
 * core rather than inventing rows.
 */
export function SessionsCard({
  sessions,
  tasks,
  timer,
}: {
  sessions: Session[];
  tasks: Task[];
  timer: TimerState;
}) {
  const total = sessions.reduce((s, x) => s + x.secs, 0);

  return (
    <PanelCard className="flex-1">
      <CardHeader>
        <CardTitleRow
          icon={<ListChecks />}
          label="Today's sessions"
          action={
            sessions.length > 0 ? (
              <span className="text-[11px] text-muted-foreground">
                {sessions.length} {sessions.length === 1 ? "task" : "tasks"}
              </span>
            ) : undefined
          }
        />
      </CardHeader>
      <CardContent>
        {sessions.length === 0 ? (
          <p className="py-6 text-center text-[11px] text-muted-foreground">
            Nothing tracked yet today. Start a timer to log time against a task.
          </p>
        ) : (
          <>
            {/* Capped, not unbounded: the panel is a fixed height and a real day can hold many
                tasks. The list scrolls inside its own box so the card never pushes the panel.

                `scrollbar-gutter: stable` reserves the 10px track (index.css) whether or not the
                list actually overflows, so the durations keep one right edge across scenarios —
                and TOTAL below can match it with a fixed inset instead of guessing. */}
            <ul className="max-h-[104px] space-y-1.5 overflow-y-auto pr-1 [scrollbar-gutter:stable]">
              {sessions.map((s) => {
                const task = tasks.find((t) => t.id === s.task_id);
                const running = timer.running && timer.task_id === s.task_id;
                return (
                  <li key={s.task_id} className="flex items-center gap-2">
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
                        {task?.title ?? s.task_id}
                      </span>
                      <span className="block truncate text-[10px] text-muted-foreground">
                        {task ? `${task.project_name} · ${task.id}` : "Unknown task"}
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

            {/* pr = the list's 10px scrollbar gutter + its own pr-1, so TOTAL's value lands on
                the same right edge as the per-task durations above it. */}
            <div className="mt-2 flex items-baseline justify-between border-t border-border/60 pr-[14px] pt-2">
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
