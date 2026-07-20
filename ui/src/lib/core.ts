/**
 * Typed wrappers over the core's Tauri `#[command]`s — the webview's only entry into Rust.
 *
 * This is the full surface registered in src-tauri/src/lib.rs (`generate_handler!`). Invoke arg
 * keys match the Rust parameter names exactly; DTOs come back camelCase (BUILD-PLAN §3).
 *
 * Ported from the previous Preact UI's lib/ipc.ts — the command contract is unchanged, only the
 * UI on top of it is new.
 */

import { invoke } from "@tauri-apps/api/core";

/** Mirrors the Rust `auth::AuthStatus`. */
export interface AuthStatus {
  signedIn: boolean;
  tenantId?: string;
  username?: string;
  /** Set when Cognito requires a new password (first admin-created login). */
  newPasswordSession?: string;
}

export const core = {
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

  // consent (M5 / PRIVACY.md) — capture is gated on this and defaults OFF
  consentStatus: () => invoke<boolean>("consent_status"),
  setConsent: (granted: boolean) => invoke<void>("set_consent", { granted }),

  // timer (M0/M3)
  timerStatus: () => invoke<boolean>("timer_status"),
  timerStop: () => invoke<void>("timer_stop"),
  timerStart: (sessionId: string, taskId: string, projectId: string, description: string) =>
    invoke<void>("timer_start", {
      session_id: sessionId,
      task_id: taskId,
      project_id: projectId,
      description,
    }),

  // diagnostics / updater
  agentId: () => invoke<string>("agent_id"),
  checkForUpdates: () => invoke<boolean>("check_for_updates"),
};
