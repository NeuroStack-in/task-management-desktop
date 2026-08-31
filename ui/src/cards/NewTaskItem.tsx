import { useState } from "react";
import { Plus } from "lucide-react";

import { DropdownMenuSeparator } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { createTask } from "@/lib/agent";
import type { Task } from "@/lib/types";

/**
 * "New task…" at the foot of the task picker — a title, and nothing else.
 *
 * ## Why this is a separate file
 *
 * The composer this replaces lived inline in `TimerCard`, which is also where subtask selection and
 * the timer controls live. Keeping it out means `TimerCard` gains one line rather than sixty, so
 * work happening in that file at the same time does not collide with this.
 *
 * ## Why only a title
 *
 * The removed version asked for description, due date, priority and an assign-to-me switch: five
 * controls in a 320px panel, for a task the employee is about to start timing in the next second.
 * Everything else is editable in the web app, which is where task *detail* belongs. This exists for
 * one reason — so nobody has to open a browser to have something to time against.
 *
 * The task is assigned to the caller, because someone creating a task from their own timer panel is
 * telling you who is going to do it.
 */
export function NewTaskItem({
  projectId,
  onCreated,
}: {
  /** The project the task lands in. The picker only renders this once a project is chosen. */
  projectId: string;
  /** Hands back the created task so the picker can select it without waiting for the next poll. */
  onCreated: (task: Task) => void;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    const t = title.trim();
    if (!t || busy) return;
    setBusy(true);
    setError(null);
    try {
      const task = await createTask(projectId, t);
      setTitle("");
      setOpen(false);
      onCreated(task);
    } catch (e) {
      // Shown in place rather than as a toast: the panel is small, and a toast over a dropdown
      // covers the field the person needs to correct.
      setError(e instanceof Error ? e.message : "Couldn't create the task.");
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <>
        <DropdownMenuSeparator />
        <button
          type="button"
          // `onMouseDown` with `preventDefault`, not `onClick`: the dropdown closes on blur, and a
          // click would unmount this before the state flip lands.
          onMouseDown={(e) => {
            e.preventDefault();
            setOpen(true);
          }}
          className="hover:bg-accent flex w-full items-center gap-1.5 rounded-md px-1.5 py-1.5 text-left text-[12.5px] font-medium"
        >
          <Plus className="size-3.5 shrink-0" />
          New task…
        </button>
      </>
    );
  }

  return (
    <>
      <DropdownMenuSeparator />
      <div
        className="space-y-1.5 px-1.5 py-1.5"
        // The dropdown closes on outside interaction; typing inside it must not count as one.
        onMouseDown={(e) => e.stopPropagation()}
      >
        <Input
          autoFocus
          value={title}
          onValueChange={setTitle}
          placeholder="Task title"
          disabled={busy}
          className="h-8 text-[12.5px]"
          onKeyDown={(e) => {
            // Keep every keystroke inside this input. The composer lives inside a Base UI dropdown
            // menu, which runs its own typeahead + Enter/arrow handling on keydown — without this it
            // swallowed the letters (typeahead jumping between tasks) and stole Enter (activating the
            // highlighted task instead of submitting), so a title could never be typed or created.
            e.stopPropagation();
            if (e.key === "Enter") {
              e.preventDefault();
              void submit();
            }
            if (e.key === "Escape") {
              e.preventDefault();
              setOpen(false);
              setTitle("");
              setError(null);
            }
          }}
        />
        {error ? <p className="text-destructive text-[11px]">{error}</p> : null}
        <p className="text-muted-foreground/70 text-[11px]">
          {busy ? "Creating…" : "Enter to create · Esc to cancel"}
        </p>
      </div>
    </>
  );
}
