// Shapes the UI reads. `AuthStatus` mirrors the Rust `auth::AuthStatus` (camelCase over the IPC
// boundary — BUILD-PLAN §3). Project/Task are the selector's catalog; today they come from a local
// placeholder, later from `GET /v1/agent/tasks` (BUILD-PLAN M3a).

export interface AuthStatus {
  signedIn: boolean;
  tenantId?: string;
  username?: string;
  /** Set when Cognito requires a new password (first admin-created login). */
  newPasswordSession?: string;
}

export interface Project {
  id: string;
  name: string;
}

export interface Task {
  id: string;
  projectId: string;
  name: string;
}
