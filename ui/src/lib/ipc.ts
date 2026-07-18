// Thin Tauri invoke wrapper (same global-bridge pattern as lib/agent.ts — withGlobalTauri is on, so
// no @tauri-apps/api dependency). Used by the auth gate.
type TauriWindow = Window & {
  __TAURI__?: { core?: { invoke<T>(cmd: string, args?: unknown): Promise<T> } };
};

export function invoke<T>(cmd: string, args?: unknown): Promise<T> {
  const fn = (window as TauriWindow).__TAURI__?.core?.invoke;
  if (!fn) {
    return Promise.reject(new Error(`Tauri bridge unavailable — cannot invoke "${cmd}".`));
  }
  return fn<T>(cmd, args);
}

export interface AuthStatus {
  signedIn: boolean;
  tenantId?: string;
  username?: string;
  newPasswordSession?: string;
}

export const auth = {
  status: () => invoke<AuthStatus>("auth_status"),
  login: (username: string, password: string) =>
    invoke<AuthStatus>("auth_login", { username, password }),
  completeNewPassword: (username: string, newPassword: string, session: string) =>
    invoke<AuthStatus>("auth_complete_new_password", { username, newPassword, session }),
  logout: () => invoke<void>("auth_logout"),
};
