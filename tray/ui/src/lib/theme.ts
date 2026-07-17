/**
 * Panel theme. Light is the default — the web app ships Meridian light as its default too.
 *
 * Deliberately not `next-themes` (that's Next-only) and deliberately not following the OS:
 * the choice is explicit and sticky, so a monitored user who picked light doesn't get a dark
 * panel because their laptop flipped at sunset.
 */

export type Theme = "light" | "dark";

const STORAGE_KEY = "wp-tray-theme";

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
