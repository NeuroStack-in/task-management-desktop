/**
 * The breakdown strip under the task picker — pick a subtask to time, tick one off, add one.
 *
 * **This app is the only place subtasks are written.** The web app shows them read-only, so every
 * control here has no counterpart in the browser; that is deliberate, not an omission.
 *
 * Two different jobs share one row and must not be confused:
 *
 * - **The circle on the left ticks a subtask off.** It changes the subtask's status on the server.
 * - **The label picks what the timer runs against.** It changes nothing on the server until Start.
 *
 * They are separate hit targets for that reason — a single click that both completed a subtask and
 * retargeted the clock would be impossible to undo by eye.
 */
import { useState } from "react";
import { Check, Circle, CircleCheckBig, Plus } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { Subtask, Task } from "@/lib/types";

interface SubtaskStripProps {
  /** The selected task, or null when none is chosen yet. */
  task: Task | null;
  /** The subtask the timer will target (or is targeting), null = the task itself. */
  selectedId: string | null;
  /** The subtask the core is actually timing right now, if any. */
  activeId: string | null;
  running: boolean;
  onPick: (subtaskId: string | null) => void;
  onAdd: (title: string) => Promise<Subtask | null>;
  onSetDone: (subtask: Subtask, done: boolean) => Promise<boolean>;
}

export function SubtaskStrip({
  task,
  selectedId,
  activeId,
  running,
  onPick,
  onAdd,
  onSetDone,
}: SubtaskStripProps) {
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  // Which row is mid-write, so only that circle shows as pending rather than the whole strip.
  const [ticking, setTicking] = useState<string | null>(null);

  // Nothing to break down until a task is chosen. Rendered as null rather than a disabled strip:
  // an empty control under an empty picker is two prompts for one decision.
  if (!task) return null;

  const subtasks = task.subtasks;
  const done = subtasks.filter((s) => s.done).length;

  const submit = async () => {
    const t = title.trim();
    if (!t || busy) return;
    setBusy(true);
    const created = await onAdd(t);
    setBusy(false);
    if (!created) return; // the failure is already on the panel's error banner
    setTitle("");
    setAdding(false);
    // Target the new subtask straight away — you broke the work out because that is what you are
    // about to do. Retargets a live session too, which is the same rule the task picker follows.
    onPick(created.id);
  };

  const tick = async (s: Subtask) => {
    if (ticking) return;
    setTicking(s.id);
    await onSetDone(s, !s.done);
    setTicking(null);
  };

  return (
    <div className="flex flex-col gap-1.5 rounded-xl bg-white/10 p-2 ring-1 ring-inset ring-white/15">
      <div className="flex items-center justify-between px-0.5">
        <span className="text-[10.5px] font-semibold uppercase tracking-wider text-feature-foreground/70">
          Subtasks{subtasks.length > 0 ? ` · ${done}/${subtasks.length}` : ""}
        </span>
        {!adding && (
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium text-feature-foreground/80 hover:bg-white/15 hover:text-feature-foreground"
          >
            <Plus className="size-3" />
            Add
          </button>
        )}
      </div>

      {subtasks.length === 0 && !adding ? (
        // Says what the state means, not just that it is empty — the timer works fine without a
        // breakdown, and someone opening this for the first time should not think it is broken.
        <p className="px-0.5 pb-0.5 text-[11px] text-feature-foreground/60">
          None yet — the timer will run against the task itself.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {subtasks.map((s) => {
            const isActive = running && activeId === s.id;
            const isSel = !running && selectedId === s.id;
            return (
              <li
                key={s.id}
                className={cn(
                  "flex items-center gap-2 rounded-lg px-1.5 py-1",
                  (isActive || isSel) && "bg-white/15",
                )}
              >
                <button
                  type="button"
                  onClick={() => void tick(s)}
                  disabled={ticking !== null}
                  aria-label={s.done ? `Reopen ${s.title}` : `Mark ${s.title} done`}
                  title={s.done ? "Reopen" : "Mark done"}
                  className="shrink-0 rounded-md p-0.5 text-feature-foreground/70 hover:bg-white/15 hover:text-feature-foreground disabled:opacity-50"
                >
                  {s.done ? (
                    <CircleCheckBig className="size-3.5" />
                  ) : (
                    <Circle className="size-3.5" />
                  )}
                </button>
                <button
                  type="button"
                  onClick={() => onPick(isSel ? null : s.id)}
                  className={cn(
                    "min-w-0 flex-1 truncate text-left text-[12.5px]",
                    s.done
                      ? "text-feature-foreground/50 line-through"
                      : "text-feature-foreground/90",
                  )}
                  title={s.title}
                >
                  {s.title}
                </button>
                {isActive ? (
                  <span aria-hidden className="size-1.5 shrink-0 animate-pulse rounded-full bg-white" />
                ) : isSel ? (
                  <Check className="size-3.5 shrink-0 text-feature-foreground" />
                ) : null}
              </li>
            );
          })}
        </ul>
      )}

      {adding && (
        <div className="flex items-center gap-1.5">
          <Input
            autoFocus
            aria-label="New subtask title"
            placeholder={`Break down "${task.title}"`}
            value={title}
            onValueChange={setTitle}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
              if (e.key === "Escape") {
                setAdding(false);
                setTitle("");
              }
            }}
            className="h-8 flex-1 border-transparent bg-white/15 px-2.5 text-[12.5px] text-feature-foreground ring-1 ring-inset ring-white/15 placeholder:text-feature-foreground/50 focus-visible:ring-2 focus-visible:ring-white/70"
          />
          <Button
            size="sm"
            onClick={() => void submit()}
            disabled={!title.trim() || busy}
            className="h-8 shrink-0 rounded-lg border-transparent bg-white/15 px-2.5 text-[12px] text-feature-foreground ring-1 ring-inset ring-white/15 hover:bg-white/25"
          >
            Add
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setAdding(false);
              setTitle("");
            }}
            className="h-8 shrink-0 rounded-lg px-2 text-[12px] text-feature-foreground/80 hover:bg-white/15"
          >
            Cancel
          </Button>
        </div>
      )}
    </div>
  );
}
