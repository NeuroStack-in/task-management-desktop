import { Check, ChevronDown, FolderKanban, ListChecks, Play, RefreshCw, Square } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { PanelCard } from "@/components/panel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { formatElapsed } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { Task, TimerState } from "@/lib/types";

/**
 * The timer hero, mirroring the web app's Time Tracking hero
 * (modules/time-tracking/components/timer-hero.tsx): a filled `bg-feature` card with
 * `shadow-none`, a Project picker and a Task picker, and the clock + transport on the right.
 *
 * The house rules for a filled surface apply throughout: nested controls go translucent
 * `bg-white/15` with an inset white ring rather than keeping their semantic colour, and
 * secondary text drops to `text-feature-foreground/70..80`.
 *
 * Choosing a project filters the Task picker to that project — same as the web app
 * (`tasksForProject`). Picking a task while the timer runs switches attribution immediately,
 * so the menu is never a dead end.
 */
export function TimerCard({
  timer,
  tasks,
  onToggle,
  onSelectTask,
  onRefresh,
}: {
  timer: TimerState;
  tasks: Task[];
  onToggle: (taskId?: string | null) => void;
  onSelectTask: (taskId: string) => void;
  onRefresh: () => Promise<void>;
}) {
  // The poll already re-reads every second; this is for when the task list is visibly stale
  // (a task added in the web app) and waiting a beat feels broken. The spin is held to a
  // minimum so a sub-100ms read still reads as "something happened".
  const [refreshing, setRefreshing] = useState(false);

  const refresh = () => {
    if (refreshing) return;
    setRefreshing(true);
    void Promise.allSettled([onRefresh(), new Promise((r) => setTimeout(r, 550))]).then(() =>
      setRefreshing(false),
    );
  };

  const projects = [...new Set(tasks.map((t) => t.project_name))];
  const active = tasks.find((t) => t.id === timer.task_id) ?? null;

  const [project, setProject] = useState(() => active?.project_name ?? projects[0] ?? "");
  const [pending, setPending] = useState<Task | null>(
    () => active ?? tasks.find((t) => t.project_name === projects[0]) ?? null,
  );

  // Keep the pickers pointed at the running task — e.g. after the core switches it, or on
  // a remount while a timer is live.
  useEffect(() => {
    if (!active) return;
    setProject(active.project_name);
    setPending(active);
  }, [active?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const inProject = tasks.filter((t) => t.project_name === project);
  const selected = active ?? pending;

  const chooseProject = (p: string) => {
    setProject(p);
    const first = tasks.find((t) => t.project_name === p) ?? null;
    setPending(first);
    // Switching project while running would silently keep timing the old task.
    if (timer.running && first) onSelectTask(first.id);
  };

  const chooseTask = (t: Task) => {
    setPending(t);
    if (timer.running) onSelectTask(t.id);
  };

  const status = timer.running ? "Recording" : selected ? "Ready" : "No task";

  return (
    <PanelCard className="border-transparent bg-feature text-feature-foreground shadow-none">
      <CardContent className="flex flex-col gap-3">
        {/* The clock is the hero's whole point — centred and given the room. */}
        <div className="flex flex-col items-center gap-2 text-center">
          <span className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-feature-foreground/80">
            <span
              className={cn(
                "size-1.5 rounded-full",
                timer.running ? "animate-pulse bg-white" : selected ? "bg-white/60" : "bg-white/30",
              )}
            />
            {status}
          </span>
          <span className="tabular font-heading text-[44px] font-semibold leading-none tracking-tight">
            {formatElapsed(timer.elapsed_secs)}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <HeroPicker
            icon={FolderKanban}
            label={project || "No project"}
            disabled={projects.length === 0}
            width="w-56"
          >
            <p className="px-1.5 pb-1 pt-0.5 text-xs font-medium text-muted-foreground">Project</p>
            {projects.map((p) => (
              <DropdownMenuItem
                key={p}
                onClick={() => chooseProject(p)}
                className={cn("gap-2.5 rounded-lg px-1.5 py-1.5", p === project && "bg-accent/60")}
              >
                <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                  <FolderKanban className="size-4" />
                </span>
                <span className="flex-1 truncate font-medium">{p}</span>
                {p === project && <Check className="size-4 shrink-0 text-primary" />}
              </DropdownMenuItem>
            ))}
          </HeroPicker>

          <HeroPicker
            icon={ListChecks}
            label={selected?.title ?? "Select a task"}
            disabled={tasks.length === 0}
            width="w-[19rem]"
          >
            <p className="flex items-center gap-1.5 px-1.5 pb-1 pt-0.5 text-xs font-medium text-muted-foreground">
              Tasks
              <span className="font-normal text-muted-foreground/70">· {project}</span>
            </p>
            {inProject.map((t) => {
              const isActive = timer.running && active?.id === t.id;
              const isSel = !timer.running && selected?.id === t.id;
              return (
                <DropdownMenuItem
                  key={t.id}
                  onClick={() => chooseTask(t)}
                  className={cn(
                    "items-start gap-2.5 rounded-lg px-1.5 py-1.5",
                    (isActive || isSel) && "bg-accent/60",
                  )}
                >
                  <span
                    className={cn(
                      "mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md",
                      isActive ? "bg-success/15 text-success" : "bg-primary/10 text-primary",
                    )}
                  >
                    {isActive ? (
                      <span className="size-2 animate-pulse rounded-full bg-success" />
                    ) : (
                      <ListChecks className="size-4" />
                    )}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-2">
                      <span className="truncate font-medium">{t.title}</span>
                      {t.billable && (
                        <Badge
                          variant="secondary"
                          className="shrink-0 px-1.5 py-0 text-[0.65rem] font-normal"
                        >
                          Billable
                        </Badge>
                      )}
                    </span>
                    <span className="tabular block truncate text-[0.7rem] text-muted-foreground">
                      {t.id}
                    </span>
                  </span>
                  {isSel && <Check className="mt-0.5 size-4 shrink-0 text-primary" />}
                </DropdownMenuItem>
              );
            })}
          </HeroPicker>

          {/* Same translucent treatment as the pickers either side — a semantic fill would
              fight the teal surface (see the card's house rules above). */}
          <Button
            size="sm"
            onClick={refresh}
            disabled={refreshing}
            aria-label="Refresh projects and tasks"
            title="Refresh projects and tasks"
            className="size-9 shrink-0 rounded-xl border-transparent bg-white/15 p-0 text-feature-foreground ring-1 ring-inset ring-white/15 hover:bg-white/25 disabled:opacity-100"
          >
            <RefreshCw className={cn("size-4", refreshing && "animate-spin")} />
          </Button>

          <Button
            size="sm"
            onClick={() => onToggle(selected?.id ?? null)}
            disabled={!selected}
            className="h-9 shrink-0 gap-1.5 rounded-xl border-transparent bg-white/15 text-feature-foreground ring-1 ring-inset ring-white/15 hover:bg-white/25"
          >
            {timer.running ? <Square className="fill-current" /> : <Play className="fill-current" />}
            {timer.running ? "Stop" : "Start"}
          </Button>
        </div>

        <p className="text-[11px] text-feature-foreground/70">
          {timer.running
            ? "Tracking — pick another task to switch without stopping."
            : selected
              ? "Ready to track. Switching project changes the task list."
              : "No tasks available from the core yet."}
        </p>
      </CardContent>
    </PanelCard>
  );
}

/**
 * The hero's dropdown trigger — copied from the web app's HeroPicker (timer-hero.tsx:384):
 * translucent white fill with an inset ring, chevron rotating on open.
 */
function HeroPicker({
  icon: Icon,
  label,
  disabled,
  width,
  children,
}: {
  icon: LucideIcon;
  label: string;
  disabled?: boolean;
  width?: string;
  children: ReactNode;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        disabled={disabled}
        render={
          <button
            type="button"
            // Focus ring is white, not the themed --ring: on the filled teal surface the
            // UA default (black) and a teal ring are both illegible.
            className="group inline-flex min-w-0 flex-1 items-center gap-2 rounded-xl bg-white/15 px-3 py-2 text-sm font-medium text-white shadow-sm ring-1 ring-inset ring-white/15 transition-colors hover:bg-white/25 hover:ring-white/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/70 disabled:opacity-70 disabled:hover:bg-white/15"
          />
        }
      >
        <Icon className="size-4 shrink-0 text-white/80" />
        <span className="truncate">{label}</span>
        {!disabled && (
          <ChevronDown className="ml-auto size-4 shrink-0 text-white/70 transition-transform duration-fast ease-standard group-data-[popup-open]:rotate-180" />
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" sideOffset={8} className={cn("max-h-72 p-1.5", width)}>
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
