import {
  Check,
  ChevronDown,
  FolderKanban,
  Hand,
  ListChecks,
  Play,
  RefreshCw,
  Square,
} from "lucide-react";
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
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { recordHistory, suggestHistory } from "@/lib/descriptionHistory";
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

  // Deliberately NOT defaulted to `projects[0]`: the server folds time entries by
  // (project, description), so a silently pre-picked project produces a real, wrongly-attributed
  // timesheet row the moment someone hits Start without looking. Empty until chosen.
  const [projectId, setProjectId] = useState<string>(() => timer.project_id ?? "");
  const [taskId, setTaskId] = useState<string | null>(() => timer.task_id);
  const [description, setDescription] = useState(timer.description);
  // Recent-description suggestions render only while the field is focused; selection happens on
  // `onMouseDown` so it wins the race against the input's blur closing the list.
  const [descFocused, setDescFocused] = useState(false);
  const suggestions = descFocused && !timer.running ? suggestHistory(description) : [];

  // Keep the controls pointed at whatever the core is actually timing — after a switch, or on a
  // remount while a session is live. Only while running: outside that, these are the user's
  // in-progress choices and the poll must not stomp them every second.
  useEffect(() => {
    if (!timer.running) return;
    setProjectId(timer.project_id ?? "");
    setTaskId(timer.task_id);
    setDescription(timer.description);
  }, [timer.running, timer.project_id, timer.task_id, timer.description]);

  const project = projects.find((p) => p.id === projectId) ?? null;
  const inProject = tasks.filter((t) => t.project_id === projectId);
  // Yours first, then what nobody has taken. Two groups rather than one mixed list: the picker is
  // scanned, not read, and an unclaimed task sitting between two of your own reads as yours.
  const assigned = inProject.filter((t) => !t.unassigned);
  const unclaimed = inProject.filter((t) => t.unassigned);
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
    // Autofill the label with the task's title — but only into an EMPTY field, never over what
    // the person typed (the description is the timesheet row's label, so their words win).
    if (!description.trim()) setDescription(t.title);
    if (timer.running) onSwitch(selection({ taskId: t.id }));
  };

  const toggle = () => {
    // Remember the description of every session actually started, so the field can suggest it
    // next time (fold grain is (project, description) — retyping variants splits timesheet rows).
    if (!timer.running) recordHistory(description);
    onToggle(selection());
  };

  /**
   * One row of the task menu. Lifted out of the map so the assigned and unclaimed groups render
   * identically — a difference between the two would read as a difference in the task.
   */
  const renderTask = (t: Task) => {
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
            isActive
              ? "bg-success/15 text-success"
              : t.unassigned
                ? "bg-muted text-muted-foreground"
                : "bg-primary/10 text-primary",
          )}
        >
          {isActive ? (
            <span className="size-2 animate-pulse rounded-full bg-success" />
          ) : t.unassigned ? (
            <Hand className="size-4" />
          ) : (
            <ListChecks className="size-4" />
          )}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-2">
            <span className="truncate font-medium">{t.title}</span>
            {t.unassigned && (
              <Badge
                variant="secondary"
                className="shrink-0 px-1.5 py-0 text-[0.65rem] font-normal"
                // Stated rather than implied by the grouping alone: tracking time against an
                // unclaimed task does not claim it, so a colleague may be on it too.
                title="Nobody is assigned — tracking time won't claim it"
              >
                Unassigned
              </Badge>
            )}
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
  };

  const canStart = Boolean(projectId);
  const status = timer.running ? "Recording" : canStart ? "Ready" : "Select a project";

  return (
    <PanelCard
      className={cn(
        "border-transparent bg-feature text-feature-foreground shadow-none",
        // One-shot ignite when a session starts (the class appearing replays the keyframe).
        timer.running && "wp-recording-ignite",
      )}
    >
      <CardContent className="flex flex-col gap-2.5">
        {/* The clock is the hero's whole point — centred and given the room. */}
        <div className="flex flex-col items-center gap-2 text-center">
          <span className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-feature-foreground/80">
            {timer.running ? (
              // Recording: a ping ring radiating off a solid dot (TaskFlow's recording badge).
              <span aria-hidden className="relative flex size-1.5">
                <span className="absolute size-full animate-ping rounded-full bg-white opacity-70" />
                <span className="relative size-1.5 rounded-full bg-white" />
              </span>
            ) : (
              <span
                aria-hidden
                className={cn("size-1.5 rounded-full", canStart ? "bg-white/60" : "bg-white/30")}
              />
            )}
            {status}
          </span>
          <TickingClock secs={timer.elapsed_secs} />
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
          <div className="relative">
            <Input
              aria-label="What are you working on?"
              placeholder="What are you working on?"
              value={description}
              onValueChange={setDescription}
              onFocus={() => setDescFocused(true)}
              onBlur={() => setDescFocused(false)}
              className="h-9 w-full border-transparent bg-white/15 px-3 text-[13px] text-feature-foreground ring-1 ring-inset ring-white/15 placeholder:text-feature-foreground/50 focus-visible:ring-2 focus-visible:ring-white/70"
            />
            {suggestions.length > 0 && (
              <ul
                role="listbox"
                aria-label="Recent descriptions"
                className="absolute inset-x-0 top-full z-10 mt-1 overflow-hidden rounded-lg border border-border bg-popover p-1 text-popover-foreground shadow-md"
              >
                <li className="px-2 pb-0.5 pt-1 text-[10.5px] font-medium uppercase tracking-wide text-muted-foreground">
                  Recent
                </li>
                {suggestions.map((s) => (
                  <li key={s}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={false}
                      // onMouseDown, not onClick: it fires before the input's blur unmounts the list.
                      onMouseDown={(e) => {
                        e.preventDefault();
                        setDescription(s);
                        setDescFocused(false);
                      }}
                      className="w-full truncate rounded-md px-2 py-1.5 text-left text-[12.5px] hover:bg-accent"
                    >
                      {s}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        <div className="flex items-center gap-2">
          <HeroPicker
            icon={FolderKanban}
            label={project?.name ?? "Select project"}
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
            label={task?.title ?? "Select task (optional)"}
            disabled={inProject.length === 0}
            // Matches the project picker. It was `w-[19rem]` (304px): anchored `align="start"` from
            // this trigger — which sits in the right half of the row — a 304px popup overran the
            // 392px content area, so the positioner shifted it ~46px left to keep it on screen. The
            // collision handling was right; the width was wrong, and the result read as a menu
            // detached from its button. 224px fits under the trigger, so nothing has to move.
            width="w-56"
          >
            <p className="flex items-center gap-1.5 px-1.5 pb-1 pt-0.5 text-xs font-medium text-muted-foreground">
              Tasks
              <span className="font-normal text-muted-foreground/70">· {project?.name ?? "—"}</span>
            </p>
            {assigned.map((t) => renderTask(t))}

            {unclaimed.length > 0 && (
              <>
                <DropdownMenuSeparator />
                <p className="flex items-center gap-1.5 px-1.5 pb-1 pt-1 text-xs font-medium text-muted-foreground">
                  Up for grabs
                  <span className="font-normal text-muted-foreground/70">
                    · nobody assigned
                  </span>
                </p>
                {unclaimed.map((t) => renderTask(t))}
              </>
            )}
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
            onClick={toggle}
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
 * The hh:mm:ss readout with per-digit tick animation. Each digit's inner span is keyed by its
 * character, so when a digit changes Preact remounts it and replays `wp-digit-tick` — the digit
 * slides up into place. Unchanged digits (and the colons) keep their key and never re-animate.
 */
function TickingClock({ secs }: { secs: number }) {
  return (
    <span className="tabular font-heading text-[44px] font-semibold leading-none tracking-tight">
      {formatElapsed(secs)
        .split("")
        .map((ch, i) =>
          ch === ":" ? (
            <span key={`c${i}`} className="inline-block">
              :
            </span>
          ) : (
            <span key={`d${i}`} className="inline-block overflow-hidden align-baseline">
              <span key={ch} className="wp-digit-tick inline-block">
                {ch}
              </span>
            </span>
          ),
        )}
    </span>
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
      {/* `max-w` is a floor under every caller: the panel is a fixed 420px, so a popup wider than
          the content area can only be placed by shifting it away from its trigger. Capping it at the
          viewport minus the panel's own padding (`p-3.5` a side) means a future width — or a future
          narrower panel — degrades to a snug menu rather than a detached one. */}
      <DropdownMenuContent
        align="start"
        sideOffset={8}
        className={cn("max-h-72 max-w-[calc(100vw-1.75rem)] p-1.5", width)}
      >
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
