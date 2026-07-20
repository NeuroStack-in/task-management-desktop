import { Check, ChevronDown, FolderKanban, ListChecks, Play, RefreshCw, Square } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { PanelCard } from "@/components/panel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { formatElapsed } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { Project, Task, TimerSelection, TimerState } from "@/lib/types";

/**
 * The timer hero, mirroring the web app's Time Tracking hero
 * (modules/time-tracking/components/timer-hero.tsx): a filled `bg-feature` card with
 * `shadow-none`, a Project picker and a Task picker, and the clock + transport on the right.
 *
 * The house rules for a filled surface apply throughout: nested controls go translucent
 * `bg-white/15` with an inset white ring rather than keeping their semantic colour, and
 * secondary text drops to `text-feature-foreground/70..80`.
 *
 * **A project is what a session needs; a task is optional.** That matches the core
 * (`start_timer` takes `task_id: Option<String>`) and the server, which folds time entries per
 * (project, description). So the project picker is fed from `list_projects` directly rather than
 * being derived from the task list — a project with no assigned tasks is still trackable.
 */
export function TimerCard({
  timer,
  projects,
  tasks,
  onToggle,
  onSwitch,
  onRefresh,
}: {
  timer: TimerState;
  projects: Project[];
  tasks: Task[];
  onToggle: (sel: TimerSelection) => void;
  onSwitch: (sel: TimerSelection) => void;
  onRefresh: () => Promise<void>;
}) {
  // The poll already re-reads every second; this is for when the lists are visibly stale
  // (a project added in the web app) and waiting a beat feels broken. The spin is held to a
  // minimum so a sub-100ms read still reads as "something happened".
  const [refreshing, setRefreshing] = useState(false);

  const refresh = () => {
    if (refreshing) return;
    setRefreshing(true);
    void Promise.allSettled([onRefresh(), new Promise((r) => setTimeout(r, 550))]).then(() =>
      setRefreshing(false),
    );
  };

  const [projectId, setProjectId] = useState<string>(() => timer.project_id ?? projects[0]?.id ?? "");
  const [taskId, setTaskId] = useState<string | null>(() => timer.task_id);
  const [description, setDescription] = useState(timer.description);

  // Keep the controls pointed at whatever the core is actually timing — after a switch, or on a
  // remount while a session is live. Only while running: outside that, these are the user's
  // in-progress choices and the poll must not stomp them every second.
  useEffect(() => {
    if (!timer.running) return;
    setProjectId(timer.project_id ?? "");
    setTaskId(timer.task_id);
    setDescription(timer.description);
  }, [timer.running, timer.project_id, timer.task_id, timer.description]);

  // Once projects arrive, adopt the first as a default rather than leaving the picker empty.
  useEffect(() => {
    if (!projectId && projects.length > 0) setProjectId(projects[0].id);
  }, [projects, projectId]);

  const project = projects.find((p) => p.id === projectId) ?? null;
  const inProject = tasks.filter((t) => t.project_id === projectId);
  const task = inProject.find((t) => t.id === taskId) ?? null;

  const selection = (over: Partial<TimerSelection> = {}): TimerSelection => ({
    taskId,
    projectId: projectId || null,
    description: description.trim(),
    ...over,
  });

  const chooseProject = (p: Project) => {
    setProjectId(p.id);
    // The old task belongs to the old project; keeping it would misattribute the session.
    setTaskId(null);
    if (timer.running) onSwitch(selection({ projectId: p.id, taskId: null }));
  };

  const chooseTask = (t: Task) => {
    setTaskId(t.id);
    if (timer.running) onSwitch(selection({ taskId: t.id }));
  };

  const canStart = Boolean(projectId);
  const status = timer.running ? "Recording" : canStart ? "Ready" : "No project";

  return (
    <PanelCard className="border-transparent bg-feature text-feature-foreground shadow-none">
      <CardContent className="flex flex-col gap-2.5">
        {/* The clock is the hero's whole point — centred and given the room. */}
        <div className="flex flex-col items-center gap-2 text-center">
          <span className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-feature-foreground/80">
            <span
              className={cn(
                "size-1.5 rounded-full",
                timer.running ? "animate-pulse bg-white" : canStart ? "bg-white/60" : "bg-white/30",
              )}
            />
            {status}
          </span>
          <span className="tabular font-heading text-[44px] font-semibold leading-none tracking-tight">
            {formatElapsed(timer.elapsed_secs)}
          </span>
        </div>

        {/* The server folds time entries per (project, description), so this text is the label the
            user will see in their own timesheet — not a nicety.

            While running it becomes read-only text rather than a disabled input: re-labelling a live
            session would mean stopping and restarting it, and a locked empty box is just a dead bar
            taking up the panel's fixed height. No description → nothing renders at all. */}
        {timer.running ? (
          description.trim() && (
            <p className="truncate text-center text-[13px] text-feature-foreground/90">
              {description}
            </p>
          )
        ) : (
          <Input
            aria-label="What are you working on?"
            placeholder="What are you working on?"
            value={description}
            onValueChange={setDescription}
            className="h-9 border-transparent bg-white/15 px-3 text-[13px] text-feature-foreground ring-1 ring-inset ring-white/15 placeholder:text-feature-foreground/50 focus-visible:ring-2 focus-visible:ring-white/70"
          />
        )}

        <div className="flex items-center gap-2">
          <HeroPicker
            icon={FolderKanban}
            label={project?.name ?? "No project"}
            disabled={projects.length === 0}
            width="w-56"
          >
            <p className="px-1.5 pb-1 pt-0.5 text-xs font-medium text-muted-foreground">Project</p>
            {projects.map((p) => (
              <DropdownMenuItem
                key={p.id}
                onClick={() => chooseProject(p)}
                className={cn("gap-2.5 rounded-lg px-1.5 py-1.5", p.id === projectId && "bg-accent/60")}
              >
                <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                  <FolderKanban className="size-4" />
                </span>
                <span className="flex-1 truncate font-medium">{p.name}</span>
                {p.id === projectId && <Check className="size-4 shrink-0 text-primary" />}
              </DropdownMenuItem>
            ))}
          </HeroPicker>

          <HeroPicker
            icon={ListChecks}
            label={task?.title ?? "No task (optional)"}
            disabled={inProject.length === 0}
            width="w-[19rem]"
          >
            <p className="flex items-center gap-1.5 px-1.5 pb-1 pt-0.5 text-xs font-medium text-muted-foreground">
              Tasks
              <span className="font-normal text-muted-foreground/70">· {project?.name ?? "—"}</span>
            </p>
            {inProject.map((t) => {
              const isActive = timer.running && timer.task_id === t.id;
              const isSel = !timer.running && taskId === t.id;
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
            onClick={() => onToggle(selection())}
            disabled={!timer.running && !canStart}
            className="h-9 shrink-0 gap-1.5 rounded-xl border-transparent bg-white/15 text-feature-foreground ring-1 ring-inset ring-white/15 hover:bg-white/25"
          >
            {timer.running ? <Square className="fill-current" /> : <Play className="fill-current" />}
            {timer.running ? "Stop" : "Start"}
          </Button>
        </div>

        <p className="text-[11px] text-feature-foreground/70">
          {timer.running
            ? "Tracking — switching project or task re-attributes without stopping the clock."
            : projects.length === 0
              ? "No projects yet. Ask your admin to add you to one, then hit refresh."
              : "Pick a project and describe what you're doing. A task is optional."}
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
