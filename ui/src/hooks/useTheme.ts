import { useEffect, useState } from "react";

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

/**
 * The panel's look, taken from the signed-in account: light/dark **and** palette.
 *
 * Both halves matter. Syncing only the theme left someone on Fire Opal with a dark web app and a
 * dark panel that were still visibly different colours — which reads as the sync being broken
 * rather than half-done.
 *
 * Starts from the cached values so the first paint is right, then adopts the account's. There is no
 * toggle: the web app's Settings -> Appearance is the one place this is chosen, and a second
 * control here would immediately disagree with it.
 *
 * `signedIn` is a dependency rather than a one-shot mount fetch — the agent can start signed out
 * and sign in later, and the read needs a token. It also means switching accounts re-reads.
 */
export function useTheme(signedIn: boolean): Theme {
  const [theme, setTheme] = useState<Theme>(readTheme);

  // The cached palette, applied before anything is fetched. Runs once: it is only about the first
  // paint, and re-applying it after the account's would undo the account's.
  useEffect(() => {
    applyPalette(readPalette());
  }, []);

  useEffect(() => {
    if (!signedIn) return;
    let live = true;
    getAppearance()
      .then((a) => {
        // `null` = signed out or unreachable. Keep what is on screen; appearance is not worth a
        // visible failure, and flipping to a default would look like the setting was lost.
        if (!live || !a) return;
        const next = resolveTheme(a.theme as ThemePref);
        applyTheme(next);
        setTheme(next);
        // Empty is "the server sent no palette", which is not a palette — leave the current one.
        if (a.palette) applyPalette(a.palette);
      })
      .catch(() => {
        // Same reasoning: silent.
      });
    return () => {
      live = false;
    };
  }, [signedIn]);

  return theme;
}
