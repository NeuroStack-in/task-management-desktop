import { useMemo, useState } from "preact/hooks";
import { PROJECTS, TASKS } from "../lib/mock";
import { Button } from "./ui/Button";
import { Card } from "./ui/Card";
import { Input } from "./ui/Input";
import { Label } from "./ui/Label";

export interface Selection {
  projectId: string;
  taskId: string;
  description: string;
}

// Project → task, with a mandatory description (a binding product decision, BUILD-PLAN §3).
export function ProjectTaskSelector({
  onStart,
  onCancel,
}: {
  onStart: (s: Selection) => void;
  onCancel: () => void;
}) {
  const [projectId, setProjectId] = useState(PROJECTS[0]?.id ?? "");
  const tasks = useMemo(() => TASKS.filter((t) => t.projectId === projectId), [projectId]);
  const [taskId, setTaskId] = useState(tasks[0]?.id ?? "");
  const [description, setDescription] = useState("");

  function onProjectChange(id: string) {
    setProjectId(id);
    setTaskId(TASKS.find((t) => t.projectId === id)?.id ?? "");
  }

  const canStart = projectId !== "" && taskId !== "" && description.trim().length > 0;
  const selectCls =
    "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background";

  return (
    <Card class="space-y-3 p-4">
      <h2 class="text-[13px] font-semibold text-foreground">Start a timer</h2>

      <div class="space-y-1">
        <Label class="uppercase tracking-[0.08em] text-muted-foreground">Project</Label>
        <select
          value={projectId}
          onChange={(e) => onProjectChange((e.target as HTMLSelectElement).value)}
          class={selectCls}
        >
          {PROJECTS.map((p) => (
            <option value={p.id}>{p.name}</option>
          ))}
        </select>
      </div>

      <div class="space-y-1">
        <Label class="uppercase tracking-[0.08em] text-muted-foreground">Task</Label>
        <select
          value={taskId}
          onChange={(e) => setTaskId((e.target as HTMLSelectElement).value)}
          class={selectCls}
        >
          {tasks.map((t) => (
            <option value={t.id}>{t.name}</option>
          ))}
        </select>
      </div>

      <div class="space-y-1">
        <Label class="uppercase tracking-[0.08em] text-muted-foreground">Description (required)</Label>
        <Input
          type="text"
          placeholder="What are you working on?"
          value={description}
          onInput={(e) => setDescription((e.target as HTMLInputElement).value)}
        />
      </div>

      <div class="flex gap-2 pt-1">
        <Button
          class="flex-1"
          disabled={!canStart}
          onClick={() => onStart({ projectId, taskId, description: description.trim() })}
        >
          Start
        </Button>
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </Card>
  );
}
