import { useMemo, useState } from "preact/hooks";
import { PROJECTS, TASKS } from "../lib/mock";

export interface Selection {
  projectId: string;
  taskId: string;
  description: string;
}

// Project → task, with a **mandatory description** (a binding product decision, BUILD-PLAN §3). The
// description can't be sent to the server until the §6 contract PR adds `TimerStarted.description`,
// but the UI enforces it now so the requirement is real from day one.
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
    const first = TASKS.find((t) => t.projectId === id);
    setTaskId(first?.id ?? "");
  }

  const canStart = projectId !== "" && taskId !== "" && description.trim().length > 0;

  return (
    <div class="flex flex-col gap-3">
      <h2 class="text-sm font-semibold">Start a timer</h2>

      <label class="text-xs text-slate-400">Project</label>
      <select
        value={projectId}
        onChange={(e) => onProjectChange((e.target as HTMLSelectElement).value)}
        class="rounded-md bg-slate-800 px-3 py-2 text-sm outline-none"
      >
        {PROJECTS.map((p) => (
          <option value={p.id}>{p.name}</option>
        ))}
      </select>

      <label class="text-xs text-slate-400">Task</label>
      <select
        value={taskId}
        onChange={(e) => setTaskId((e.target as HTMLSelectElement).value)}
        class="rounded-md bg-slate-800 px-3 py-2 text-sm outline-none"
      >
        {tasks.map((t) => (
          <option value={t.id}>{t.name}</option>
        ))}
      </select>

      <label class="text-xs text-slate-400">Description (required)</label>
      <input
        type="text"
        placeholder="What are you working on?"
        value={description}
        onInput={(e) => setDescription((e.target as HTMLInputElement).value)}
        class="rounded-md bg-slate-800 px-3 py-2 text-sm outline-none placeholder:text-slate-500"
      />

      <div class="flex gap-2">
        <button
          disabled={!canStart}
          onClick={() => onStart({ projectId, taskId, description: description.trim() })}
          class="flex-1 rounded-md bg-teal-600 px-4 py-2 text-sm font-medium hover:bg-teal-500 disabled:opacity-50"
        >
          Start
        </button>
        <button
          onClick={onCancel}
          class="rounded-md bg-slate-800 px-4 py-2 text-sm font-medium hover:bg-slate-700"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
