import { invoke } from "@tauri-apps/api/core";
import type { AuthStatus } from "./types";

// Typed wrappers over the Rust `#[command]`s — the webview's only entry into the core. Invoke arg
// keys match the Rust parameter names. `description` is forwarded on timer_start already (the core
// ignores the extra field today; it starts riding the event after the §6 contract PR lands).
export const ipc = {
  // auth (M1)
  authStatus: () => invoke<AuthStatus>("auth_status"),
  authLogin: (username: string, password: string) =>
    invoke<AuthStatus>("auth_login", { username, password }),
  authCompleteNewPassword: (username: string, newPassword: string, session: string) =>
    invoke<AuthStatus>("auth_complete_new_password", {
      username,
      new_password: newPassword,
      session,
    }),
  authLogout: () => invoke<void>("auth_logout"),

  // consent (M5 / PRIVACY) — capture is gated on this and defaults OFF
  consentStatus: () => invoke<boolean>("consent_status"),
  setConsent: (granted: boolean) => invoke<void>("set_consent", { granted }),

  // timer (M0/M3)
  timerStatus: () => invoke<boolean>("timer_status"),
  agentId: () => invoke<string>("agent_id"),
  timerStop: () => invoke<void>("timer_stop"),
  timerStart: (sessionId: string, taskId: string, projectId: string, description: string) =>
    invoke<void>("timer_start", {
      session_id: sessionId,
      task_id: taskId,
      project_id: projectId,
      description,
    }),
};
