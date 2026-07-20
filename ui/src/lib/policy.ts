/**
 * Panel-side constants for the parts of the snapshot the core has no command for.
 *
 * These were previously literals inside mock.ts. They live here because the *real* adapter
 * (agent.ts) needs them too: `consent_status` returns a bare `bool`, so the disclosure list and
 * the policy version have to come from somewhere, and no command exposes the effective config.
 *
 * Each is a stand-in for something the core owns and should eventually serve:
 *  - DISCLOSURE / POLICY_VERSION  →  a `consent_state()` command (PRIVACY.md §2)
 *  - DEFAULT_CONFIG               →  an `effective_config()` command. The core already holds
 *    the pulled TrackingConfig (state.config, fed by GET /v1/agent/config); it just isn't
 *    reachable from the webview.
 */

import type { TrackingConfig } from "./types";

/** What the agent records, as shown on the consent gate. Mirrors PRIVACY.md §2 / INGESTION.md §1. */
export const DISCLOSURE = [
  "Activity counts (keystroke & mouse totals — never the keys themselves)",
  "Foreground app / website category",
  "Periodic screenshots (blurred per your organization's policy)",
  "Attendance and timer events",
];

/**
 * Bumping this re-prompts everyone, so it must only move when DISCLOSURE materially changes.
 * The core stores consent as a single bool today and cannot tell one version from another —
 * until it can, re-prompting on a bump is not actually possible.
 */
export const POLICY_VERSION = 1;

/**
 * Conservative defaults, used until the effective config is readable from the webview.
 *
 * Deliberately *not* the mock's demo values: `silent: false` keeps the capture indicator
 * visible, which is the safe direction to be wrong in — hiding it on a guess would be the
 * one failure PRIVACY.md §5 explicitly rules out.
 */
export const DEFAULT_CONFIG: TrackingConfig = {
  version: 0,
  cadence: "min5",
  blur_level: 1,
  retention_days: 90,
  silent: false,
};

export const CADENCE_SECS: Record<TrackingConfig["cadence"], number> = {
  off: 0,
  min3: 180,
  min5: 300,
  min10: 600,
};
