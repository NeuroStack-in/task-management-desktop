import { useEffect, useState } from "preact/hooks";
import { getSettings, setSettings, useSettings, type ThemeChoice } from "./settings";

type AppliedTheme = "light" | "dark";

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function resolveTheme(choice: ThemeChoice): AppliedTheme {
  if (choice === "system") return systemPrefersDark() ? "dark" : "light";
  return choice;
}

/**
 * Theme hook. Three behaviors:
 *
 *   - "light" / "dark" — explicit pin
 *   - "system" — follow OS preference, re-render when OS flips
 *
 * The selection lives in the unified settings store (settings.ts);
 * this hook is a thin reactor that:
 *   1. Reads the choice from settings,
 *   2. Resolves it to a concrete light/dark value,
 *   3. Applies the .dark class on <html>,
 *   4. Subscribes to OS theme changes when in "system" mode.
 *
 * `toggle()` cycles through light → dark → system for the avatar-menu
 * single-button affordance. The settings drawer uses the explicit
 * setChoice for finer control.
 */
export function useTheme() {
  const [settings] = useSettings();
  const choice = settings.theme;

  // Tracks the applied light/dark value. Re-resolves whenever choice
  // changes, AND whenever the OS preference flips (only relevant in
  // "system" mode).
  const [applied, setApplied] = useState<AppliedTheme>(() => resolveTheme(choice));

  useEffect(() => {
    setApplied(resolveTheme(choice));
  }, [choice]);

  useEffect(() => {
    if (choice !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    function onChange() {
      setApplied(systemPrefersDark() ? "dark" : "light");
    }
    // Modern browsers expose addEventListener; legacy WebKit /
    // Wails's older webview may need addListener as a fallback.
    if (mq.addEventListener) {
      mq.addEventListener("change", onChange);
    } else if ((mq as any).addListener) {
      (mq as any).addListener(onChange);
    }
    return () => {
      if (mq.removeEventListener) {
        mq.removeEventListener("change", onChange);
      } else if ((mq as any).removeListener) {
        (mq as any).removeListener(onChange);
      }
    };
  }, [choice]);

  useEffect(() => {
    const root = document.documentElement;
    if (applied === "dark") {
      root.classList.add("dark");
    } else {
      root.classList.remove("dark");
    }
  }, [applied]);

  function toggle() {
    // Cycle: light → dark → system → light. The avatar menu shows
    // light↔dark only as labelled actions, but we keep the cycle so
    // power users hitting the toggle quickly can land on system.
    const next: ThemeChoice =
      choice === "light" ? "dark" : choice === "dark" ? "system" : "light";
    setSettings({ theme: next });
  }

  function setChoice(c: ThemeChoice) {
    setSettings({ theme: c });
  }

  return {
    /** The currently RESOLVED applied theme — always concrete. */
    theme: applied,
    /** The user's preference: light | dark | system. */
    choice,
    /** Cycle handler used by the avatar-menu single-button affordance. */
    toggle,
    /** Explicit setter used by the settings drawer's three-way control. */
    setChoice,
    isDark: applied === "dark",
  };
}

// Imperative read for one-off bootstrap code (e.g. main.tsx applies
// the theme class before any React tree mounts so the user doesn't
// see a flash of the wrong palette during hydration).
export function applyThemeFromSettings(): void {
  const c = getSettings().theme;
  const a = resolveTheme(c);
  if (a === "dark") document.documentElement.classList.add("dark");
  else document.documentElement.classList.remove("dark");
}
