import { useEffect, useState } from "preact/hooks";
import { ipc } from "./lib/ipc";

// M0 scaffold: proves the webview↔core seam (timer commands + identity). The real surface — login
// (M1), project→task selector with mandatory description + meeting mode, sessions, idle prompt,
// settings, tray reflection — ports in M3+ (BUILD-PLAN §3).
export function App() {
  const [running, setRunning] = useState(false);
  const [agentId, setAgentId] = useState("");

  useEffect(() => {
    ipc.timerStatus().then(setRunning).catch(() => {});
    ipc.agentId().then(setAgentId).catch(() => {});
  }, []);

  async function toggle() {
    if (running) {
      await ipc.timerStop();
      setRunning(false);
    } else {
      await ipc.timerStart({
        sessionId: crypto.randomUUID(),
        taskId: "demo-task",
        projectId: "demo-project",
      });
      setRunning(true);
    }
  }

  return (
    <main class="flex min-h-screen flex-col gap-4 bg-slate-950 p-6 text-slate-100">
      <h1 class="text-lg font-semibold">WorkPulse</h1>
      <p class="text-xs text-slate-400">agent {agentId || "…"}</p>
      <button
        onClick={toggle}
        class="rounded-md bg-teal-600 px-4 py-2 text-sm font-medium hover:bg-teal-500"
      >
        {running ? "Stop timer" : "Start timer"}
      </button>
      <p class="text-xs text-slate-500">
        M0 scaffold — activity/screenshots capture only while the timer runs. Selector, sessions, and
        status land in M3+.
      </p>
    </main>
  );
}
