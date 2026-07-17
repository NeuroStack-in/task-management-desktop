/**
 * Dev-only window fitting.
 *
 * The panel window is sized in tauri.conf.json to fit the shipped content exactly — no
 * scroll, no dead space. The preview-state chips are dev chrome that production never
 * renders, so they'd otherwise have to either push the last card out or force a scrollbar.
 *
 * Instead the dev window grows by exactly the chips' height, once, at startup. Production
 * builds never call this (see USE_MOCK), so the shipped window keeps its exact fit.
 *
 * No-ops in a plain browser, where the viewport is whatever you make it.
 */

const GUARD = "wp-dev-window-grown";

interface TauriWindowApi {
  getCurrentWindow(): {
    innerSize(): Promise<{ width: number; height: number }>;
    scaleFactor(): Promise<number>;
    setSize(size: unknown): Promise<void>;
  };
  LogicalSize: new (width: number, height: number) => unknown;
}

export async function growWindowForDevChrome(extraPx: number) {
  const api = (window as unknown as { __TAURI__?: { window?: TauriWindowApi } }).__TAURI__?.window;
  if (!api) return;

  // A full reload re-runs this; sessionStorage stops the window compounding each time.
  try {
    if (sessionStorage.getItem(GUARD)) return;
    sessionStorage.setItem(GUARD, "1");
  } catch {
    return; // No storage → can't guard → don't risk growing without bound.
  }

  try {
    const win = api.getCurrentWindow();
    // innerSize is physical; convert so the maths matches the logical px in tauri.conf.json.
    const [physical, factor] = await Promise.all([win.innerSize(), win.scaleFactor()]);
    const width = Math.round(physical.width / factor);
    const height = Math.round(physical.height / factor);
    await win.setSize(new api.LogicalSize(width, height + extraPx));
  } catch (e) {
    // Non-fatal: worst case the dev panel scrolls a little.
    console.warn("dev: could not grow window for the preview chips", e);
  }
}
