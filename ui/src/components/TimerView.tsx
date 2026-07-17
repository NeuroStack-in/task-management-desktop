import { useEffect, useState } from "preact/hooks";
import { ipc } from "../lib/ipc";
import { PROJECTS, TASKS } from "../lib/mock";
import { ProjectTaskSelector, type Selection } from "./ProjectTaskSelector";

interface Running {
  selection: Selection;
  startedAt: number;
}

function label(id: string, list: { id: string; name: string }[]): string {
  return list.find((x) => x.id === id)?.name ?? id;
}

function elapsed(fromMs: number): string {
  const s = Math.max(0, Math.floor((Date.now() - fromMs) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(sec)}`;
}

// The timer surface: a project→task selector that gates start, then a running card with an elapsed
// clock. Elapsed ticks off the local clock for now; serverClock (the timer's authoritative source)
// lands with the sender's server-offset clock (BUILD-PLAN risk #9).
export function TimerView() {
  const [selecting, setSelecting] = useState(false);
  const [running, setRunning] = useState<Running | null>(null);
  const [, setTick] = useState(0);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [running]);

  async function start(selection: Selection) {
    setError("");
    try {
      await ipc.timerStart(
        crypto.randomUUID(),
        selection.taskId,
        selection.projectId,
        selection.description,
      );
      setRunning({ selection, startedAt: Date.now() });
      setSelecting(false);
    } catch (err) {
      setError(String(err));
    }
  }

  async function stop() {
    setError("");
    try {
      await ipc.timerStop();
      setRunning(null);
    } catch (err) {
      setError(String(err));
    }
  }

  if (running) {
    const { selection } = running;
    return (
      <div class="flex flex-col gap-2 rounded-lg bg-slate-900 p-4">
        <div class="font-mono text-3xl tabular-nums">{elapsed(running.startedAt)}</div>
        <div class="text-sm text-slate-300">{label(selection.taskId, TASKS)}</div>
        <div class="text-xs text-slate-500">
          {label(selection.projectId, PROJECTS)} · {selection.description}
        </div>
        {error && <p class="text-xs text-rose-400">{error}</p>}
        <button
          onClick={stop}
          class="mt-2 rounded-md bg-rose-600 px-4 py-2 text-sm font-medium hover:bg-rose-500"
        >
          Stop timer
        </button>
      </div>
    );
  }

  if (selecting) {
    return <ProjectTaskSelector onStart={start} onCancel={() => setSelecting(false)} />;
  }

  return (
    <div class="flex flex-col gap-2">
      {error && <p class="text-xs text-rose-400">{error}</p>}
      <button
        onClick={() => setSelecting(true)}
        class="rounded-md bg-teal-600 px-4 py-2 text-sm font-medium hover:bg-teal-500"
      >
        Start timer
      </button>
      <p class="text-xs text-slate-500">
        Activity &amp; screenshots are captured only while the timer runs.
      </p>
    </div>
  );
}
