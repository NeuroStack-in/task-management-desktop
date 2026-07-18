import { Check, ChevronDown, FolderKanban, PencilLine, Play, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";

import { CardLabel } from "@/components/panel";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { Task } from "@/lib/types";
import { cn } from "@/lib/utils";

const INPUT =
  "h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none " +
  "placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40";

/**
 * Switch-task footer (TaskFlow layout, Meeting button removed per request): a "what are you
 * working on?" description, a project→task picker, a refresh, and Start. When the timer is
 * already running, Start re-attributes to the picked task without stopping; otherwise it begins
 * a session. Teal `--primary` throughout — no TaskFlow purple.
 *
 * The reference shows a single "Select Project" control; we pick a *task* (the core's
 * `start_timer` takes a task id), so the one dropdown lists tasks grouped by project.
 */
export function SwitchTaskCard({
  tasks,
  running,
  onStart,
}: {
  tasks: Task[];
  running: boolean;
  /** `description` is captured for when the core accepts it; today it's display-only. */
  onStart: (taskId: string, description: string) => void;
}) {
  const byProject = useMemo(() => {
    const m = new Map<string, Task[]>();
    for (const t of tasks) (m.get(t.project_name) ?? m.set(t.project_name, []).get(t.project_name)!).push(t);
    return [...m.entries()];
  }, [tasks]);

  const [taskId, setTaskId] = useState<string | null>(null);
  const [desc, setDesc] = useState("");
  const [spin, setSpin] = useState(false);

  const selected = tasks.find((t) => t.id === taskId) ?? null;

  const refresh = () => {
    setTaskId(null);
    setDesc("");
    setSpin(true);
    window.setTimeout(() => setSpin(false), 500);
  };

  return (
    <div className="space-y-2.5">
      <CardLabel>Switch task</CardLabel>

      <div className="relative">
        <PencilLine className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <input
          className={cn(INPUT, "pl-9")}
          placeholder="What are you working on?"
          value={desc}
          onChange={(e) => setDesc(e.target.value)}
        />
      </div>

      <div className="flex items-center gap-2">
        <DropdownMenu>
          <DropdownMenuTrigger
            disabled={tasks.length === 0}
            render={
              <button
                type="button"
                className="group flex min-w-0 flex-1 items-center gap-2 rounded-md border border-input bg-background px-3 py-2 text-sm outline-none transition-colors hover:bg-muted focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40 disabled:opacity-60"
              />
            }
          >
            <FolderKanban className="size-4 shrink-0 text-muted-foreground" />
            <span className={cn("truncate", !selected && "text-muted-foreground")}>
              {selected ? `${selected.project_name} · ${selected.title}` : "Select project"}
            </span>
            <ChevronDown className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-data-[popup-open]:rotate-180" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" sideOffset={6} className="max-h-72 w-[19rem] p-1.5">
            {byProject.map(([project, list]) => (
              <div key={project}>
                <p className="px-1.5 pb-1 pt-1.5 text-[11px] font-medium text-muted-foreground">
                  {project}
                </p>
                {list.map((t) => (
                  <DropdownMenuItem
                    key={t.id}
                    onClick={() => setTaskId(t.id)}
                    className={cn(
                      "gap-2.5 rounded-lg px-1.5 py-1.5",
                      t.id === taskId && "bg-accent/60",
                    )}
                  >
                    <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                      <FolderKanban className="size-3.5" />
                    </span>
                    <span className="min-w-0 flex-1 truncate font-medium">{t.title}</span>
                    {t.id === taskId && <Check className="size-4 shrink-0 text-primary" />}
                  </DropdownMenuItem>
                ))}
              </div>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        <Button
          variant="outline"
          size="icon"
          onClick={refresh}
          aria-label="Refresh projects"
          title="Refresh projects"
        >
          <RefreshCw className={cn(spin && "animate-spin")} />
        </Button>
      </div>

      <Button
        size="lg"
        disabled={!selected}
        onClick={() => selected && onStart(selected.id, desc)}
        className="w-full gap-2"
      >
        <Play className="fill-current" />
        {running ? "Switch task" : "Start"}
      </Button>
    </div>
  );
}
