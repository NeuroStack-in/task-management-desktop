/**
 * Local user-preferences store.
 *
 * Lives in localStorage — these are personal, per-machine preferences.
 * Anything that needs cross-device sync (display name, avatar) belongs
 * on the user object served by the backend, not here.
 *
 * Versioned via a single key so future schema changes can either
 * migrate or wipe with one branch. Reading a key that doesn't exist
 * always returns the default — no migration boilerplate.
 */

import { useSyncExternalStore } from "preact/compat";

const KEY = "workpulse.settings.v1";

export type ThemeChoice = "light" | "dark" | "system";
export type NotifPolicy = "all" | "errors-only" | "off";

export interface Settings {
  /** "light" | "dark" — explicit override
   *  "system" — follow `prefers-color-scheme`, re-evaluate on change */
  theme: ThemeChoice;
  /** Hours target for the avatar progress ring + day's-end celebration. */
  dailyGoalHours: number;
  /** Seconds of no input before the "Still working?" prompt fires. */
  idlePromptSeconds: number;
  /** Seconds of no input before the timer auto-stops. Must be > idle prompt. */
  idleAutoStopSeconds: number;
  /** Notifications policy:
   *   - all: every transient (sign-out failure, update available, etc.)
   *   - errors-only: only failures the user should act on
   *   - off: nothing visible (errors still log) */
  notifications: NotifPolicy;
  /** Launch the app at OS login. Mirrored to OS-level config via the
   *  Go SetAutoStart binding; this flag is the local source of truth so
   *  the UI stays consistent across re-launches. */
  autoStartOnLogin: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  dailyGoalHours: 8,
  idlePromptSeconds: 5 * 60,
  idleAutoStopSeconds: 15 * 60,
  notifications: "errors-only",
  autoStartOnLogin: false,
};

// CRITICAL: useSyncExternalStore requires getSnapshot to return a
// reference-stable value when the underlying store hasn't changed.
// If we returned a fresh `{ ...DEFAULT_SETTINGS, ...parsed }` on
// every call, Preact would see a new object every render, decide
// the store had changed, schedule another render, and loop forever
// — the app would freeze on the loading spinner.
//
// `cached` holds the last-computed Settings object. We invalidate
// it from `write()` (and `clearAllLocalSettings()`) so the next
// `read()` recomputes; otherwise every `read()` returns the same
// reference.
let cached: Settings | null = null;

function read(): Settings {
  if (cached) return cached;
  cached = computeFromStorage();
  return cached;
}

function computeFromStorage(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw);
    return {
      ...DEFAULT_SETTINGS,
      ...parsed,
      theme: validateTheme(parsed.theme),
      notifications: validateNotif(parsed.notifications),
      dailyGoalHours: clampNum(parsed.dailyGoalHours, 1, 24, DEFAULT_SETTINGS.dailyGoalHours),
      idlePromptSeconds: clampNum(parsed.idlePromptSeconds, 60, 60 * 60, DEFAULT_SETTINGS.idlePromptSeconds),
      idleAutoStopSeconds: clampNum(parsed.idleAutoStopSeconds, 60, 4 * 60 * 60, DEFAULT_SETTINGS.idleAutoStopSeconds),
      autoStartOnLogin: !!parsed.autoStartOnLogin,
    };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function write(s: Settings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Quota / disabled — ignore.
  }
  // Invalidate the cache BEFORE notifying so the next read() picks
  // up the new value.
  cached = null;
  notify();
}

const listeners = new Set<() => void>();
function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}
function notify() {
  for (const cb of listeners) cb();
}

/** React/Preact hook — returns the current settings + a setter that
 *  writes through localStorage and re-renders all consumers. */
export function useSettings(): [Settings, (patch: Partial<Settings>) => void] {
  // preact/compat's useSyncExternalStore only accepts the 2-arg
  // (subscribe, getSnapshot) form — the React 18 third-arg
  // getServerSnapshot is not part of the compat surface, and Wails
  // is client-only anyway so SSR semantics are irrelevant here.
  const settings = useSyncExternalStore(subscribe, read);
  function update(patch: Partial<Settings>) {
    write({ ...settings, ...patch });
  }
  return [settings, update];
}

/** Imperative read for non-React call sites (clear-cache helper, etc.) */
export function getSettings(): Settings {
  return read();
}

/** Imperative write — same notify path as the hook setter. */
export function setSettings(patch: Partial<Settings>): void {
  write({ ...read(), ...patch });
}

// Wipes settings + ALL localStorage workpulse.* keys. Used by the
// "Clear local cache" action in the settings drawer.
export function clearAllLocalSettings(): void {
  try {
    Object.keys(localStorage).forEach((k) => {
      if (k.startsWith("workpulse.") || k === "theme" || k === "sessionBannerDismissed") {
        localStorage.removeItem(k);
      }
    });
  } catch {
    // ignore
  }
  // Invalidate the snapshot cache or subsequent reads will return
  // the pre-clear settings forever (until first explicit write).
  cached = null;
  notify();
}

// ─── helpers ────────────────────────────────────────────────────
function validateTheme(v: any): ThemeChoice {
  return v === "light" || v === "dark" || v === "system" ? v : "system";
}
function validateNotif(v: any): NotifPolicy {
  return v === "all" || v === "errors-only" || v === "off" ? v : "errors-only";
}
function clampNum(v: any, lo: number, hi: number, fallback: number): number {
  if (typeof v !== "number" || Number.isNaN(v)) return fallback;
  return Math.max(lo, Math.min(hi, v));
}
