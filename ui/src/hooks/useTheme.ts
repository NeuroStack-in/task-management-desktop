import { useEffect, useState } from "react";

import { getAppearance } from "@/lib/agent";
import { applyTheme, readTheme, resolveTheme, type Theme, type ThemePref } from "@/lib/theme";

/**
 * The panel's theme, taken from the signed-in account.
 *
 * Starts from the cached value so the first paint is right, then adopts whatever the account says.
 * There is no toggle: the web app's Settings -> Appearance is the one place this is chosen, and a
 * second control here would immediately disagree with it.
 *
 * `signedIn` is a dependency rather than a one-shot mount fetch — the agent can start signed out
 * and sign in later, and the fetch needs a token. It also means switching accounts re-reads.
 */
export function useTheme(signedIn: boolean): Theme {
  const [theme, setTheme] = useState<Theme>(readTheme);

  useEffect(() => {
    if (!signedIn) return;
    let live = true;
    getAppearance()
      .then((pref) => {
        // `null` = signed out or unreachable. Keep what is on screen; a theme is not worth a
        // visible failure, and flipping to a default would look like the setting was lost.
        if (!live || !pref) return;
        const next = resolveTheme(pref as ThemePref);
        applyTheme(next);
        setTheme(next);
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
