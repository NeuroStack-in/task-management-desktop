import { invoke } from "@tauri-apps/api/core";

// Typed wrappers over the Rust `#[command]`s — the webview's only entry into the core. The invoke
// arg keys match the Rust parameter names. M3 formalizes request/response DTOs + the project→task
// selector (BUILD-PLAN §3/§6); M1 adds the auth commands.
export const ipc = {
  timerStatus: () => invoke<boolean>("timer_status"),
  agentId: () => invoke<string>("agent_id"),
  timerStop: () => invoke<void>("timer_stop"),
  timerStart: (a: { sessionId: string; taskId: string; projectId: string }) =>
    invoke<void>("timer_start", {
      session_id: a.sessionId,
      task_id: a.taskId,
      project_id: a.projectId,
    }),
};
