import { Check, ChevronDown, FolderKanban, PencilLine, Play } from "lucide-react";
import { useState } from "react";

import { CardLabel } from "@/components/panel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { Project } from "@/lib/types";
import { cn } from "@/lib/utils";

const INPUT =
  "h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none " +
  "placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40";

/**
 * Switch-task footer (TaskFlow layout, Meeting button removed per request): a "what are you working
 * on?" description, a real **project** picker (`GET /v1/projects`), and Start. When a session is
 * already running, Start re-attributes to the picked project without stopping; otherwise it begins
 * one. Teal `--primary` throughout — no TaskFlow purple.
 */
export function SwitchTaskCard({
  projects,
  running,
  onStart,
}: {
  projects: Project[];
  running: boolean;
  onStart: (projectId: string, description: string) => void;
}) {
  const [projectId, setProjectId] = useState<string | null>(null);
  const [desc, setDesc] = useState("");

  const selected = projects.find((p) => p.id === projectId) ?? null;
  const canStart = !!selected && desc.trim().length > 0;

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
          onKeyDown={(e) => {
            if (e.key === "Enter" && canStart) onStart(selected!.id, desc.trim());
          }}
        />
      </div>

      <DropdownMenu>
        <DropdownMenuTrigger
          disabled={projects.length === 0}
          render={
            <button
              type="button"
              className="group flex w-full items-center gap-2 rounded-md border border-input bg-background px-3 py-2 text-sm outline-none transition-colors hover:bg-muted focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40 disabled:opacity-60"
            />
          }
        >
          <FolderKanban className="size-4 shrink-0 text-muted-foreground" />
          <span className={cn("truncate", !selected && "text-muted-foreground")}>
            {selected ? selected.name : projects.length === 0 ? "No projects" : "Select project"}
          </span>
          <ChevronDown className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-data-[popup-open]:rotate-180" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" sideOffset={6} className="max-h-72 w-[19rem] p-1.5">
          {projects.map((p) => (
            <DropdownMenuItem
              key={p.id}
              onClick={() => setProjectId(p.id)}
              className={cn("gap-2.5 rounded-lg px-1.5 py-1.5", p.id === projectId && "bg-accent/60")}
            >
              <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                <FolderKanban className="size-3.5" />
              </span>
              <span className="min-w-0 flex-1 truncate font-medium">{p.name}</span>
              {p.billable && (
                <Badge variant="secondary" className="shrink-0 px-1.5 py-0 text-[0.65rem] font-normal">
                  Billable
                </Badge>
              )}
              {p.id === projectId && <Check className="size-4 shrink-0 text-primary" />}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>

      <Button
        size="lg"
        disabled={!canStart}
        onClick={() => selected && onStart(selected.id, desc.trim())}
        className="w-full gap-2"
      >
        <Play className="fill-current" />
        {running ? "Switch task" : "Start"}
      </Button>
    </div>
  );
}
