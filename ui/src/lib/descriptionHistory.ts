/**
 * Recent "what are you working on?" descriptions — a localStorage MRU ring, so the field can
 * suggest what the person typed before (the server folds time entries per (project, description),
 * so re-typing the same label slightly differently would split their timesheet rows).
 *
 * Same mechanism as TaskFlow's descriptionHistory: record on every start, suggest by substring,
 * MRU-first, case-insensitive dedupe. Nothing here auto-fills — history only surfaces as
 * suggestions the user picks.
 */

const KEY = "workpulse.descriptionHistory.v1";
const MAX = 25;

export function loadHistory(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((s): s is string => typeof s === "string") : [];
  } catch {
    return [];
  }
}

/** Remember a just-started session's description (no-op for blank). */
export function recordHistory(description: string): void {
  const trimmed = description.trim();
  if (!trimmed) return;
  const lower = trimmed.toLowerCase();
  const next = [trimmed, ...loadHistory().filter((s) => s.toLowerCase() !== lower)].slice(0, MAX);
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // Storage full/blocked — suggestions are a nicety, never worth an error.
  }
}

/** Up to `limit` recent descriptions matching `query` (all of them for an empty query), MRU-first. */
export function suggestHistory(query: string, limit = 5): string[] {
  const q = query.trim().toLowerCase();
  const all = loadHistory();
  const hits = q ? all.filter((s) => s.toLowerCase().includes(q)) : all;
  // An exact match suggests nothing new — offering the text already in the box is noise.
  return hits.filter((s) => s.toLowerCase() !== q).slice(0, limit);
}

/**
 * Wipe the description MRU. Called on sign-out so one user's "what are you working on?" entries
 * never surface as suggestions to the next person on a shared device — the key is device-global,
 * not per-user, so it must be cleared at the account boundary.
 */
export function clearHistory(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Non-fatal — worst case the suggestions linger until the next successful clear.
  }
}
