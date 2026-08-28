/**
 * Panel theme — **the account's**, not the panel's own.
 *
 * The agent used to keep a private `localStorage` theme with its own toggle, so someone who set
 * WorkPulse to dark on the web still got a light panel on their desktop and had to set it a second
 * time in a second place. The theme now comes from `GET /v1/me/appearance` — the same stored
 * preference the web app adopts on every sign-in — read through the Rust core, which holds the
 * token.
 *
 * `localStorage` survives as a **cache, not a source**: it makes the first paint after launch
 * correct instead of flashing light before the fetch lands, and it is what the panel falls back to
 * when the agent is signed out or offline.
 */

export type Theme = "light" | "dark";
/** What the account can store. `system` is an explicit choice to follow the OS, not an absence. */
export type ThemePref = Theme | "system";

const STORAGE_KEY = "wp-tray-theme";

/**
 * Resolve a stored preference to something paintable.
 *
 * Only `system` consults the OS, and only because the server cannot: it defaults accounts to
 * `light` rather than `system` precisely so that "follow my OS" stays distinguishable from "never
 * chose". Honouring it here is what makes that distinction mean anything.
 */
export function resolveTheme(pref: ThemePref): Theme {
  if (pref !== "system") return pref;
  try {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  } catch {
    return "light";
  }
}

/** The cached theme from the last run — the first paint, before the account's is fetched. */
export function readTheme(): Theme {
  try {
    return localStorage.getItem(STORAGE_KEY) === "dark" ? "dark" : "light";
  } catch {
    // Storage can throw in a locked-down webview; light is the default either way.
    return "light";
  }
}

export function applyTheme(theme: Theme) {
  // `.dark` goes on <html> so the `dark:` variant (`&:is(.dark *)`) covers the whole tree.
  document.documentElement.classList.toggle("dark", theme === "dark");
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Non-fatal: the theme still applies for this session.
  }
}
