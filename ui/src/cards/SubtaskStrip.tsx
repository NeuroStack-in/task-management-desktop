/**
 * The work box under the pickers — what you are timing, and what is left to do.
 *
 * **This app is the only place subtasks are written.** The web shows them read-only.
 *
 * ## The confusion this shape exists to fix
 *
 * The first version put two different actions on one row with nothing to tell them apart: a circle
 * that completed a subtask, and a label that pointed the clock at it. People clicked the circle
 * meaning "work on this" and marked it done instead.
 *
 * So the two are now separated by position, by shape, and by label:
 *
 * - **Left, a square checkbox** — finishes the item. Squares are for completion.
 * - **The row body, with a radio dot on the right** — points the clock at it. Round is for choice.
 *
 * **Only subtasks can be completed here.** The parent task is selectable but carries no checkbox:
 * finishing a whole task is a decision that belongs on the board, next to the review step and the
 * rest of the project. Two earlier attempts put a completion box on the task — beside its title,
 * then on its own row — and both were clicked meaning "this is what I'm on", which marked the task
 * done. The control that cannot be reached cannot be reached by accident.
 *
 * And the box says so in words, once, under the list. A control whose meaning has to be discovered
 * by trying it is a control that will be got wrong at least once by everyone.
 *
 * The header is the actual work — `Project · Task` — not the word "Subtasks". You already know they
 * are subtasks; what you cannot see anywhere else on this card is which task you are on.
 */
import { useState } from "react";
import { Check, Loader2, Plus, Square, SquareCheckBig } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { Subtask, Task } from "@/lib/types";

interface SubtaskStripProps {
  task: Task | null;
  projectName: string;
  /** The subtask the clock is pointed at (or will be on Start). Null = the task itself. */
  selectedId: string | null;
  /** What the core is actually timing right now. */
  activeId: string | null;
  running: boolean;
  onPick: (subtaskId: string | null) => void;
  onAdd: (title: string) => Promise<Subtask | null>;
  onSetSubtaskDone: (subtask: Subtask, done: boolean) => Promise<boolean>;
}

export function SubtaskStrip({
  task,
  projectName,
  selectedId,
  activeId,
  running,
  onPick,
  onAdd,
  onSetSubtaskDone,
}: SubtaskStripProps) {
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  /** Which row is mid-write, so only that checkbox spins rather than the whole box. */
  const [pending, setPending] = useState<string | null>(null);

  if (!task) return null;

  const subs = task.subtasks;
  const done = subs.filter((s) => s.done).length;
  const taskDone = task.status === "done" || task.status === "closed";
  /** The clock is on the task itself when nothing more specific is chosen. */
  const onTask = (running ? activeId : selectedId) === null;

  const submit = async () => {
    const t = title.trim();
    if (!t || busy) return;
    setBusy(true);
    const created = await onAdd(t);
    setBusy(false);
    if (!created) return; // already surfaced on the panel's error banner
    setTitle("");
    setAdding(false);
    onPick(created.id); // you broke it out because it is what you are about to do
  };

  const tickSub = async (s: Subtask) => {
    if (pending) return;
    setPending(s.id);
    await onSetSubtaskDone(s, !s.done);
    setPending(null);
  };

  return (
    <div className="flex flex-col gap-2 rounded-xl bg-white/10 p-2.5 ring-1 ring-inset ring-white/15">
      {/* ── The work itself: project, then task ──
          No control sits beside the title. A completion box here read as decoration next to a
          heading and was clicked meaning "this is the task I'm on" — which marked it done instead.
          The task is completed from its own row below, where every line works the same way. */}
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[10.5px] font-medium uppercase tracking-wider text-white/60">
            {projectName || "No project"}
          </p>
          <p
            className={cn(
              "truncate text-[13px] font-semibold leading-tight text-white",
              taskDone && "text-white/60 line-through",
            )}
            title={task.title}
          >
            {task.title}
          </p>
        </div>
        {subs.length > 0 && (
          <span className="mt-0.5 shrink-0 tabular-nums text-[11px] text-white/70">
            {done}/{subs.length}
          </span>
        )}
        {!adding && (
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="mt-0.5 flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium text-white/80 transition-colors hover:bg-white/15 hover:text-white"
          >
            <Plus className="size-3" />
            Add
          </button>
        )}
      </div>

      {/* ── Pick what the clock points at ── */}
      <ul className="flex flex-col gap-0.5">
        {/* The parent task — selectable, and **only** selectable.

            It carries no checkbox: from this panel a task is finished by finishing its parts, and
            closing the whole task is a decision that belongs on the board, where the person making
            it can see the review and the rest of the project. It still shows struck through when
            it is done elsewhere, because that is worth knowing while you are timing it.

            Selectable is not optional: without this line there is no way back to the task once a
            subtask has been chosen, and "the whole task" is the commonest thing to be timing. */}
        <Row
          label="The task itself"
          italic
          struck={taskDone}
          selected={onTask}
          live={running && onTask}
          onSelect={() => onPick(null)}
        />
        {subs.map((s) => (
          <Row
            key={s.id}
            label={s.title}
            struck={s.done}
            selected={(running ? activeId : selectedId) === s.id}
            live={running && activeId === s.id}
            spinning={pending === s.id}
            onSelect={() => onPick(s.id)}
            onTick={() => void tickSub(s)}
            ticked={s.done}
            disabled={pending !== null}
          />
        ))}
      </ul>

      {adding && (
        <div className="flex items-center gap-1.5">
          <Input
            autoFocus
            aria-label="New subtask title"
            placeholder="Break this task into a step…"
            value={title}
            onValueChange={setTitle}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
              if (e.key === "Escape") {
                setAdding(false);
                setTitle("");
              }
            }}
            className="h-8 flex-1 border-transparent bg-white/15 px-2.5 text-[12.5px] text-white ring-1 ring-inset ring-white/15 placeholder:text-white/50 focus-visible:ring-2 focus-visible:ring-white/70"
          />
          <Button
            size="sm"
            onClick={() => void submit()}
            disabled={!title.trim() || busy}
            className="h-8 shrink-0 rounded-lg border-transparent bg-white/15 px-2.5 text-[12px] text-white ring-1 ring-inset ring-white/15 hover:bg-white/25"
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
            className="h-8 shrink-0 rounded-lg px-2 text-[12px] text-white/80 hover:bg-white/15"
          >
            Cancel
          </Button>
        </div>
      )}

      {/* Said once, in words. The shapes carry the meaning, but only after you know what they are. */}
      <p className="text-[10.5px] leading-snug text-white/55">
        Tap a line to point the clock at it · tap a box to mark it done
      </p>
    </div>
  );
}

/**
 * One selectable line. The checkbox and the label are separate buttons on purpose — see the module
 * note. `onTick` absent means the row has nothing to complete (the "task itself" line).
 */
function Row({
  label,
  italic,
  struck,
  selected,
  live,
  spinning,
  ticked,
  disabled,
  onSelect,
  onTick,
}: {
  label: string;
  italic?: boolean;
  struck?: boolean;
  selected: boolean;
  live: boolean;
  spinning?: boolean;
  ticked?: boolean;
  disabled?: boolean;
  onSelect: () => void;
  onTick?: () => void;
}) {
  return (
    <li
      className={cn(
        "flex items-center gap-2 rounded-lg px-1.5 py-1 transition-colors",
        selected ? "bg-white/20" : "hover:bg-white/10",
      )}
    >
      {onTick ? (
        <button
          type="button"
          onClick={onTick}
          disabled={disabled}
          aria-label={ticked ? `Reopen ${label}` : `Mark ${label} done`}
          title={ticked ? "Reopen" : "Mark done"}
          className="shrink-0 rounded p-0.5 text-white/70 transition-colors hover:bg-white/20 hover:text-white disabled:opacity-50"
        >
          {spinning ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : ticked ? (
            <SquareCheckBig className="size-3.5" />
          ) : (
            <Square className="size-3.5" />
          )}
        </button>
      ) : (
        // Keeps the labels in one column whether or not the row can be completed.
        <span aria-hidden className="size-3.5 shrink-0" />
      )}

      <button
        type="button"
        onClick={onSelect}
        className={cn(
          "min-w-0 flex-1 truncate text-left text-[12.5px]",
          struck ? "text-white/45 line-through" : "text-white/90",
          italic && "italic text-white/70",
        )}
        title={label}
      >
        {label}
      </button>

      {/* Nothing is drawn for an unselected row.
          An empty outline here was a third round shape in a box that already had round dots for
          "timing" and square boxes for "done", and it invited the reading "another thing to click".
          Selection is already carried by the row's fill; the tick just confirms it. The reserved
          span keeps every label ending on the same edge so the list does not shift as you move
          between rows. */}
      {live ? (
        <span
          aria-label="Timing this now"
          title="Timing this now"
          className="size-2 shrink-0 animate-pulse rounded-full bg-white"
        />
      ) : selected ? (
        <Check aria-label="Selected" className="size-3.5 shrink-0 text-white" />
      ) : (
        <span aria-hidden className="size-3.5 shrink-0" />
      )}
    </li>
  );
}
