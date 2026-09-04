/**
 * The work box under the pickers: the chosen task's status, and its subtasks — with exactly two
 * things a person does to a subtask, each its own clearly-labelled control:
 *
 * - **Select it** — tap the row. A "Select" chip turns into "✓ Selected" and the row highlights.
 *   This is what you are working on; the timer files against it.
 * - **Mark it done** — the checkbox on the left, and nothing else. Square box, universal meaning.
 *
 * There is no timing button and no clock control: "select" and "done" are the whole vocabulary, so
 * there is nothing to mistake one for the other. Task status is a separate three-step stepper above.
 *
 * **This app is the only place subtasks are written.** The web shows them read-only. **Only subtasks
 * carry a done box** — finishing the whole task is a reviewer's sign-off that belongs on the board,
 * and the backend gates it, so an assignee ticking it here would only 403. The task can still be
 * *selected* (to work on it as a whole) and shows struck through when finished elsewhere.
 */
import { useState } from "react";
import { Check, Loader2, Plus, Square, SquareCheckBig } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { Subtask, Task } from "@/lib/types";

/**
 * The statuses an **assignee** may set from their own panel: `todo → in_progress → in_review`.
 * `done` / `closed` are a reviewer's sign-off and `blocked` a lead's escalation — an assignee picking
 * one just 403s — so a task already in one of those shows as a read-only pill, with the three
 * settable steps still offered to move it back.
 */
const SETTABLE: { value: string; label: string }[] = [
  { value: "todo", label: "To do" },
  { value: "in_progress", label: "In progress" },
  { value: "in_review", label: "In review" },
];

/** Human labels for every status the header might show, including the un-settable ones. */
const LABEL: Record<string, string> = {
  todo: "To do",
  in_progress: "In progress",
  in_review: "In review",
  done: "Done",
  blocked: "Blocked",
  closed: "Closed (reviewed)",
};

interface TaskWorkspaceProps {
  task: Task | null;
  projectName: string;
  /** The subtask selected (or will be, on Start). Null = the task itself. */
  selectedId: string | null;
  /** What the core is actually running on right now. */
  activeId: string | null;
  running: boolean;
  onPick: (subtaskId: string | null) => void;
  onAdd: (title: string) => Promise<Subtask | null>;
  onSetSubtaskDone: (subtask: Subtask, done: boolean) => Promise<boolean>;
  /** Change the task's status. Backend-gated (assignee, or a project Lead/Manager). */
  onSetStatus: (status: string) => void;
}

export function TaskWorkspace({
  task,
  projectName,
  selectedId,
  activeId,
  running,
  onPick,
  onAdd,
  onSetSubtaskDone,
  onSetStatus,
}: TaskWorkspaceProps) {
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  /** Which row is mid-write, so only that checkbox spins rather than the whole box. */
  const [pending, setPending] = useState<string | null>(null);

  if (!task) return null;

  const subs = task.subtasks;
  const done = subs.filter((s) => s.done).length;
  const taskDone = task.status === "done" || task.status === "closed";
  /** What is selected — the running core's truth while a timer runs, otherwise the pending pick. */
  const selected = running ? activeId : selectedId;

  const submit = async () => {
    const t = title.trim();
    if (!t || busy) return;
    setBusy(true);
    const created = await onAdd(t);
    setBusy(false);
    if (!created) return; // already surfaced on the panel's error banner
    setTitle("");
    setAdding(false);
    // Deliberately NOT auto-selected: adding a subtask and choosing what to work on are two separate
    // steps, so the new row simply appears — the person selects it if and when they want it.
  };

  const tickSub = async (s: Subtask) => {
    if (pending) return;
    const markingDone = !s.done;
    setPending(s.id);
    const ok = await onSetSubtaskDone(s, markingDone);
    setPending(null);
    // A finished subtask can't be worked on, so it can't stay selected: finishing the one you had
    // selected drops the selection back to the whole task (which, while a timer runs, re-attributes
    // the clock to the task from here on).
    if (ok && markingDone && selected === s.id) onPick(null);
  };

  return (
    <div className="flex flex-col gap-2.5 rounded-xl bg-white/10 p-2.5 ring-1 ring-inset ring-white/15">
      {/* ── The task: project, then title. No control beside it — a box here read as "this is what
          I'm on" and got clicked, marking the task done. ── */}
      <div className="min-w-0">
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

      {/* ── Move the task along: three visible steps, one tap — not a dropdown that hides them. ── */}
      <StatusStepper status={task.status} onChange={onSetStatus} />

      {/* ── Subtasks: add one, select what you're working on, check it off when done. ── */}
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="text-[10.5px] font-medium uppercase tracking-wider text-white/60">
            Subtasks
          </span>
          {subs.length > 0 && (
            <span className="tabular-nums text-[11px] text-white/70">
              {done}/{subs.length}
            </span>
          )}
          {!adding && (
            <button
              type="button"
              onClick={() => setAdding(true)}
              className="ml-auto flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium text-white/80 transition-colors hover:bg-white/15 hover:text-white"
            >
              <Plus className="size-3" />
              Add subtask
            </button>
          )}
        </div>

        <ul className="flex flex-col gap-0.5">
          {/* The whole task, as a selectable row. No checkbox: it can't be completed here. It is how
              you get back to working on the task as a whole once a subtask has been selected. */}
          <Row
            label="The whole task"
            italic
            struck={taskDone}
            selected={selected === null}
            onSelect={() => onPick(null)}
          />
          {subs.map((s) => (
            <Row
              key={s.id}
              label={s.title}
              struck={s.done}
              selected={selected === s.id}
              onSelect={() => onPick(s.id)}
              done={s.done}
              spinning={pending === s.id}
              disabled={pending !== null}
              onToggleDone={() => void tickSub(s)}
            />
          ))}
        </ul>

        {subs.length === 0 && !adding && (
          <p className="px-0.5 text-[11px] leading-snug text-white/55">
            No subtasks yet — add one to break this task into pieces you can check off.
          </p>
        )}

        {adding && (
          <div className="flex items-center gap-1.5">
            <Input
              autoFocus
              aria-label="New subtask title"
              placeholder="Name a subtask…"
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
      </div>

      {/* Said once, in words — the two actions named so neither is a guess. */}
      <p className="text-[10.5px] leading-snug text-white/55">
        Tap a subtask to select it · check the box to mark it done
      </p>
    </div>
  );
}

/**
 * The three-segment task-status stepper. The current status fills its segment; the other two are one
 * tap away. A status the assignee can't set (`done` / `blocked` / `closed`) shows as a leading pill,
 * with the three settable steps still offered so the task can be moved back.
 */
function StatusStepper({ status, onChange }: { status: string; onChange: (s: string) => void }) {
  const settable = SETTABLE.some((s) => s.value === status);
  return (
    <div className="flex items-center gap-1.5" role="group" aria-label="Task status">
      {!settable && (
        <span className="shrink-0 rounded-md bg-white/20 px-1.5 py-1 text-[10.5px] font-semibold text-white ring-1 ring-inset ring-white/25">
          {LABEL[status] ?? status}
        </span>
      )}
      <div className="flex flex-1 items-center gap-0.5 rounded-lg bg-black/10 p-0.5 ring-1 ring-inset ring-white/10">
        {SETTABLE.map((s) => {
          const active = s.value === status;
          return (
            <button
              key={s.value}
              type="button"
              aria-pressed={active}
              onClick={() => {
                if (s.value !== status) onChange(s.value);
              }}
              className={cn(
                "flex-1 rounded-md px-1.5 py-1 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/70",
                active
                  ? "bg-white text-slate-900 shadow-sm"
                  : "text-white/70 hover:bg-white/10 hover:text-white",
              )}
            >
              {s.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/**
 * One row. The checkbox (left) marks it done and is the ONLY done control; tapping anywhere else on
 * the row selects it. `onToggleDone` absent = a row that can't be completed (the whole-task row),
 * which then shows a spacer where the checkbox would be so every title still lines up.
 */
function Row({
  label,
  italic,
  struck,
  selected,
  done,
  spinning,
  disabled,
  onSelect,
  onToggleDone,
}: {
  label: string;
  italic?: boolean;
  struck?: boolean;
  selected: boolean;
  done?: boolean;
  spinning?: boolean;
  disabled?: boolean;
  onSelect: () => void;
  onToggleDone?: () => void;
}) {
  // A finished subtask can't be worked on, so it can't be selected — only reopened (the checkbox).
  const selectable = !done;
  return (
    <li
      className={cn(
        "flex items-center gap-1 rounded-lg transition-colors",
        selected && selectable ? "bg-white/20" : "hover:bg-white/5",
      )}
    >
      {/* Mark done — the only done control. Kept off the row-select button so the two never share a
          tap. Absent for the whole-task row (a spacer holds the column). */}
      {onToggleDone ? (
        <button
          type="button"
          onClick={onToggleDone}
          disabled={disabled}
          aria-label={done ? `Reopen ${label}` : `Mark ${label} done`}
          title={done ? "Reopen" : "Mark done"}
          className="ml-1 shrink-0 rounded p-0.5 text-white/75 transition-colors hover:bg-white/20 hover:text-white disabled:opacity-50"
        >
          {spinning ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : done ? (
            <SquareCheckBig className="size-3.5" />
          ) : (
            <Square className="size-3.5" />
          )}
        </button>
      ) : (
        <span aria-hidden className="ml-1 size-3.5 shrink-0 p-0.5" />
      )}

      {/* Select — tapping the title (or its chip) chooses what you're working on. A done subtask is
          not selectable: the button is disabled and shows "Done" where "Select" would be, so the
          only thing left to do to it is reopen it via the checkbox. */}
      <button
        type="button"
        onClick={onSelect}
        disabled={!selectable}
        aria-pressed={selected && selectable}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-2 rounded-lg py-1 pl-1 pr-1.5 text-left",
          !selectable && "cursor-default",
        )}
        title={selectable ? label : `${label} — done`}
      >
        <span
          className={cn(
            "min-w-0 flex-1 truncate text-[12.5px]",
            struck ? "text-white/45 line-through" : "text-white/90",
            italic && !struck && "italic text-white/70",
          )}
        >
          {label}
        </span>
        {selectable ? (
          <span
            className={cn(
              "flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] font-medium transition-colors",
              selected
                ? "bg-white/25 text-white ring-1 ring-inset ring-white/30"
                : "text-white/55",
            )}
          >
            {selected && <Check className="size-3" />}
            {selected ? "Selected" : "Select"}
          </span>
        ) : (
          <span className="shrink-0 text-[10.5px] font-medium text-white/40">Done</span>
        )}
      </button>
    </li>
  );
}
