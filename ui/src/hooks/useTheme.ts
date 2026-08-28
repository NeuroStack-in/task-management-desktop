import { useCallback, useEffect, useState } from "react";

import { getAppearance } from "@/lib/agent";
import {
  applyPalette,
  applyTheme,
  readPalette,
  readTheme,
  resolveTheme,
  type Theme,
  type ThemePref,
} from "@/lib/theme";

/** Backstop re-read, for a panel left open while the preference changes elsewhere. */
const RECHECK_MS = 5 * 60_000;

/**
 * The panel's look, taken from **the signed-in user's** account: light/dark and palette.
 *
 * Both halves matter. Syncing only the theme left someone on Fire Opal with a dark web app and a
 * dark panel that were still visibly different colours — which reads as the sync being broken
 * rather than half-done.
 *
 * Per-user by construction: `GET /v1/me/appearance` is self-scoped to the token the core holds, so
 * two people on one machine cannot see each other's choice, and signing out and back in as someone
 * else re-reads (`signedIn` is a dependency, not a mount-once fetch).
 *
 * **Kept in step, not read once.** The preference is changed in the web app, on a different screen,
 * possibly while the panel is sitting open — so a single fetch at sign-in would leave the two
 * disagreeing until the agent restarted. It re-reads when the panel is shown again (the tray popup
 * is hidden far more often than it is closed) and on a slow interval as a backstop.
 *
 * Starts from the cached values so the first paint is right. There is no toggle: the web app's
 * Settings -> Appearance is the one place this is chosen, and a second control here would
 * immediately disagree with it.
 */
export function useTheme(signedIn: boolean): Theme {
  const [theme, setTheme] = useState<Theme>(readTheme);

  // The cached palette, applied before anything is fetched. Runs once: it is only about the first
  // paint, and re-applying it after the account's would undo the account's.
  useEffect(() => {
    applyPalette(readPalette());
  }, []);

  const sync = useCallback(async () => {
    if (!signedIn) return;
    try {
      const a = await getAppearance();
      // `null` = signed out or unreachable. Keep what is on screen; appearance is not worth a
      // visible failure, and flipping to a default would look like the setting was lost.
      if (!a) return;
      const next = resolveTheme(a.theme as ThemePref);
      applyTheme(next);
      setTheme(next);
      // Empty is "the server sent no palette", which is not a palette — leave the current one.
      if (a.palette) applyPalette(a.palette);
    } catch {
      // Same reasoning: silent.
    }
  }, [signedIn]);

  useEffect(() => {
    void sync();
    if (!signedIn) return;

    // Reopening the tray popup is the moment someone is most likely to have just changed this, and
    // it costs one request per open rather than one per interval.
    const onVisible = () => {
      if (document.visibilityState === "visible") void sync();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onVisible);
    const id = window.setInterval(() => void sync(), RECHECK_MS);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onVisible);
      window.clearInterval(id);
    };
  }, [signedIn, sync]);

  return theme;
}
