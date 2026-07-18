import { useEffect, useState } from "preact/hooks";
import { ipc } from "../lib/ipc";
import { PROJECTS, TASKS } from "../lib/mock";
import { ProjectTaskSelector, type Selection } from "./ProjectTaskSelector";
import { Button } from "./ui/Button";
import { Card } from "./ui/Card";

interface Running {
  selection: Selection;
  startedAt: number;
}

function label(id: string, list: { id: string; name: string }[]): string {
  return list.find((x) => x.id === id)?.name ?? id;
}

function elapsed(fromMs: number): string {
  const s = Math.max(0, Math.floor((Date.now() - fromMs) / 1000));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
}

// The timer surface — a project→task selector that gates start, then a running card with a live
// elapsed clock. Ticks off the local clock for now (serverClock lands with the sender's server-offset
// clock, BUILD-PLAN risk #9).
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
      <Card class="recording-ignite border-emerald-500/25 p-5">
        <div class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-emerald-500">
          <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" /> Recording
        </div>
        <div class="timer-display mt-2 text-[40px] leading-none tabular-nums">
          {elapsed(running.startedAt)}
        </div>
        <div class="mt-3 text-[13px] font-medium text-foreground">{label(selection.taskId, TASKS)}</div>
        <div class="text-[11.5px] text-muted-foreground">
          {label(selection.projectId, PROJECTS)} · {selection.description}
        </div>
        {error && <p class="mt-2 text-[11.5px] text-destructive">{error}</p>}
        <Button variant="destructive" size="sm" class="mt-4 w-full" onClick={stop}>
          Stop timer
        </Button>
      </Card>
    );
  }

  if (selecting) {
    return <ProjectTaskSelector onStart={start} onCancel={() => setSelecting(false)} />;
  }

  return (
    <Card class="p-5 text-center">
      <div class="mx-auto mb-3 flex h-11 w-11 items-center justify-center rounded-full bg-primary/10 text-primary">
        <svg
          class="h-5 w-5"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <circle cx="12" cy="12" r="9" />
          <path d="M12 8v4l3 2" />
        </svg>
      </div>
      {error && <p class="mb-2 text-[11.5px] text-destructive">{error}</p>}
      <Button class="w-full" onClick={() => setSelecting(true)}>
        Start timer
      </Button>
      <p class="mt-2 text-[11px] text-muted-foreground">
        Activity &amp; screenshots are captured only while the timer runs.
      </p>
    </Card>
  );
}
