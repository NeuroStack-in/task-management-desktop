import { ListChecks, Play } from "lucide-react";

import { CardTitleRow, PanelCard } from "@/components/panel";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatElapsed } from "@/lib/format";
import type { Project, Session, Task, TimerState } from "@/lib/types";

/** What a row hands back to resume: the fold grain, (project, description). No task id exists here. */
export interface ResumeSelection {
  projectId: string | null;
  description: string;
}

/**
 * Today's totals, as the server folded them: one row per **(project, description)**, not per task.
 * That's the grain `GET /v1/me/timesheet/today` aggregates on (api/timesheet.rs) — a task id never
 * reaches this card, so don't try to resolve one.
 *
 * The server deliberately omits the *running* session (its entry has no `duration_secs` yet), so
 * the live segment is folded in here from the local timer. That's the no-double-count contract in
 * timesheet.rs: exactly one side counts the in-flight time, and it's this one.
 *
 * Every stopped row is a resume affordance (TaskFlow's pattern: the whole row is the button, with
 * a play chip as the signifier). Clicking starts a **new** session on that row's (project,
 * description) — the old entry is closed on the server and there is no un-close, so "resume"
 * means "continue this work from now", never "reopen the hours in between".
 */
export function SessionsCard({
  sessions,
  projects,
  tasks,
  timer,
  onResume,
  capped = true,
}: {
  sessions: Session[];
  projects: Project[];
  /**
   * The picker's task list, used only to turn the ids on a session into names.
   *
   * Resolved here rather than server-side: the panel already holds this list, so naming a row costs
   * nothing, and a task that has since been deleted or reassigned simply falls back to its
   * description instead of making the row fail to load.
   */
  tasks: Task[];
  timer: TimerState;
  onResume: (sel: ResumeSelection) => void;
  /**
   * Scroll a long day **inside this list** (the default), or let it grow and leave scrolling to the
   * panel.
   *
   * The caller passes `false` exactly when a banner is on screen. Only one of the two may scroll at
   * a time: two nested scrollers means a wheel over the list is swallowed by the list, so the panel
   * behind it cannot be moved and whatever the banner pushed below the fold is unreachable. With no
   * banner the panel fits and this list absorbs the overflow; with a banner the panel takes over.
   */
  capped?: boolean;
}) {
  const rows = foldLiveSegment(sessions, timer);
  const total = rows.reduce((s, x) => s + x.secs, 0);
  const projectName = new Map(projects.map((p) => [p.id, p.name]));
  const byTaskId = new Map(tasks.map((t) => [t.id, t]));

  /**
   * What to call a session row: `Task · Subtask`, or the task alone, or — when the ids resolve to
   * nothing we can name — the description that was always there.
   *
   * The fallback is not a formality. Sessions predating subtasks carry no ids at all, and a task
   * that has since been finished and filtered out of the picker is no longer in `tasks`; in both
   * cases the description is the only honest label left, and it beats an id or a blank line.
   */
  const titleOf = (s: Session): string => {
    const task = s.task_id ? byTaskId.get(s.task_id) : undefined;
    if (!task) return s.description || "No description";
    const sub = s.subtask_id ? task.subtasks.find((x) => x.id === s.subtask_id) : undefined;
    return sub ? `${task.title} · ${sub.title}` : task.title;
  };

  return (
    // No `flex-1`. This card sits inside the panel's single `overflow-y-auto` column (App.tsx), and
    // `flex-1` made it fight that column for height: when a banner appeared the card was squeezed
    // rather than the column scrolling, so rows went under the fold and could not be reached.
    // Natural height + one scroll container is what makes the panel scroll predictably.
    <PanelCard>
      <CardHeader>
        <CardTitleRow
          icon={<ListChecks />}
          label="Today's sessions"
          action={
            rows.length > 0 ? (
              <span className="text-[12px] text-muted-foreground">
                {rows.length} {rows.length === 1 ? "session" : "sessions"}
              </span>
            ) : undefined
          }
        />
      </CardHeader>
      <CardContent>
        {rows.length === 0 ? (
          <p className="py-6 text-center text-[12px] text-muted-foreground">
            Nothing tracked yet today. Start a timer to log time against a project.
          </p>
        ) : (
          <>
            {/* **Exactly one scroller at a time** — this list, or the panel, never both.

                Capped (no banner): the list scrolls inside 128px and the panel fits, which is the
                intended resting state for a fixed-size companion window.
                Uncapped (banner up): the list grows and the panel scrolls instead.

                `scrollbar-gutter: stable` is applied **only when capped**, because the property has
                no effect on a non-scroll container — reserving a gutter that isn't there would just
                shift this list's right edge away from TOTAL's below. */}
            <ul
              className={
                capped
                  ? "max-h-[128px] space-y-2 overflow-y-auto pr-1 [scrollbar-gutter:stable]"
                  : "space-y-2 pr-1"
              }
            >
              {rows.map((s) => {
                const running = isCurrent(s, timer);
                return (
                  // `items-start` + a shared 20px line box on the badge, title and duration: the
                  // left block is two lines and the duration is one, so centring floated the
                  // duration between the title and the project name instead of reading as its value.
                  <li key={keyOf(s)}>
                    <button
                      type="button"
                      disabled={running}
                      onClick={() =>
                        onResume({ projectId: s.project_id || null, description: s.description })
                      }
                      title={running ? undefined : "Resume — start a new session on this work"}
                      aria-label={
                        running
                          ? undefined
                          : `Resume ${s.description || "session"} (starts a new session)`
                      }
                      className="group -mx-1 flex w-[calc(100%+0.5rem)] cursor-pointer items-start gap-2 rounded-lg px-1 py-0.5 text-left transition-colors hover:bg-accent/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-default disabled:hover:bg-transparent"
                    >
                      {/* **The play icon is always visible on a resumable row.**
                          It used to be a dot at rest that swapped to a play triangle only on hover
                          or focus — elegant, but it meant nothing on the row said "you can resume
                          this" until you happened to point at it, so the affordance was invisible
                          to anyone who didn't already know it was there. Discoverability wins over
                          restraint for the one action this list offers.

                          Hover/focus now deepens the same control rather than revealing it, and the
                          running row keeps its pulsing dot (it is a status, not a button). */}
                      <span
                        aria-hidden
                        className={
                          running
                            ? "flex size-6 shrink-0 items-center justify-center rounded-md bg-success/15"
                            : "flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary transition-colors group-hover:bg-primary group-hover:text-primary-foreground group-focus-visible:bg-primary group-focus-visible:text-primary-foreground"
                        }
                      >
                        {running ? (
                          <span className="size-1.5 animate-pulse rounded-full bg-success" />
                        ) : (
                          <Play className="size-3 fill-current" />
                        )}
                      </span>
                      {/* **The bold line is the work, not the note about it.**
                          It used to be `description` — the sentence someone typed into "what are
                          you working on?" — so a day read as four rows of free text with the actual
                          task nowhere on screen. The task (and the subtask, when one was being
                          timed) is what identifies the work; the description is the detail, and it
                          moves to the third line where it can be read after the name. */}
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13.5px] font-medium leading-5">
                          {titleOf(s)}
                        </span>
                        <span className="block truncate text-[11.5px] text-muted-foreground">
                          {projectName.get(s.project_id) ?? s.project_id ?? "No project"}
                        </span>
                        {/* Only when it says something the name above does not. */}
                        {s.description && s.description !== titleOf(s) ? (
                          <span className="block truncate text-[11px] text-muted-foreground/70">
                            {s.description}
                          </span>
                        ) : null}
                      </span>
                      <span
                        className={
                          running
                            ? "tabular shrink-0 text-[13px] font-medium leading-5 text-success"
                            : "tabular shrink-0 text-[13px] leading-5 text-muted-foreground"
                        }
                      >
                        {formatElapsed(s.secs)}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>

            {/* TOTAL's value must land on the same right edge as the durations above it — so this
                row reserves a scrollbar gutter **exactly when the list has one**, and by the same
                mechanism rather than a hardcoded width.

                Why not `pr-[14px]`: index.css sets `scrollbar-width: thin` on `*`, and Chromium
                (WebView2) then ignores the `::-webkit-scrollbar` sizing — so the real gutter is the
                platform's "thin" width, not the 10px that rule implies, and a hardcoded inset drifts
                by a few pixels per platform and DPI. Mirroring the mechanism keeps the two edges
                equal whatever that width turns out to be.

                The row is one line and never actually scrolls; `overflow-y-auto` here is purely what
                makes `scrollbar-gutter` apply. */}
            <div
              className={
                capped
                  ? "mt-2 flex items-baseline justify-between overflow-y-auto border-t border-border/60 pr-1 pt-2 [scrollbar-gutter:stable]"
                  : "mt-2 flex items-baseline justify-between border-t border-border/60 pr-1 pt-2"
              }
            >
              <span className="text-[12px] font-semibold uppercase tracking-wide text-muted-foreground">
                Total
              </span>
              <span className="tabular text-[15px] font-semibold">{formatElapsed(total)}</span>
            </div>
          </>
        )}
      </CardContent>
    </PanelCard>
  );
}

/**
 * Composite key for a `(project, description)` row.
 *
 * The separator is `\0` because it is the one character that cannot appear in either half, so
 * `("a", "b|c")` and `("a|b", "c")` can never collide. It is written as an **escape**: it used to be
 * a raw NUL byte in the source, which made git and grep classify this file as binary and rendered as
 * an innocent-looking space in most editors.
 */
const keyOf = (s: { project_id: string; description: string }) =>
  `${s.project_id}\0${s.description}`;

function isCurrent(s: Session, timer: TimerState): boolean {
  return (
    timer.running &&
    s.project_id === (timer.project_id ?? "") &&
    s.task_id === (timer.task_id ?? "") &&
    s.subtask_id === (timer.subtask_id ?? "") &&
    s.description === timer.description
  );
}

/**
 * Add the in-flight segment to its matching row, or introduce a row for it when today's first
 * session on that (project, description) is still running — otherwise starting a timer leaves the
 * list looking empty next to a ticking clock.
 */
function foldLiveSegment(sessions: Session[], timer: TimerState): Session[] {
  const out = sessions.map((s) => ({ ...s }));
  if (timer.running) {
    const live: Session = {
      project_id: timer.project_id ?? "",
      task_id: timer.task_id ?? "",
      subtask_id: timer.subtask_id ?? "",
      description: timer.description,
      secs: timer.elapsed_secs,
    };
    const existing = out.find((s) => keyOf(s) === keyOf(live));
    if (existing) existing.secs += live.secs;
    else out.push(live);
  }
  // Running first, then longest — the active row should never be buried.
  return out.sort((a, b) => {
    const ra = isCurrent(a, timer);
    const rb = isCurrent(b, timer);
    if (ra !== rb) return ra ? -1 : 1;
    return b.secs - a.secs;
  });
}
