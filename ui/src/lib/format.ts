/** Deterministic 32-bit FNV-1a hash of a string. */
function fnv1a(str: string): number {
  let h = 2166136261;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/**
 * The WorkPulse "Gradient Monogram" avatar fill — a deterministic two-tone diagonal gradient
 * derived from a seed (a person's name or initials). Same seed → same gradient. Sits behind
 * white initials (see AvatarFallback); the default avatar for anyone without a picture.
 *
 * Copied verbatim from the web app's src/lib/format.ts so one person's monogram is identical
 * in both surfaces — a different gradient for the same name would read as a different person.
 */
export function monogramGradient(seed: string): string {
  const s = fnv1a(seed);
  const h = s % 360;
  const h2 = (h + 26) % 360;
  const angle = (s >>> 3) % 360;
  return `linear-gradient(${angle}deg, hsl(${h} 66% 56%), hsl(${h2} 70% 44%))`;
}

/** Initials for the monogram: "Alex Morgan" → "AM". */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}

/** hh:mm:ss — the timer readout. */
export function formatElapsed(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  const h = String(Math.floor(s / 3600)).padStart(2, "0");
  const m = String(Math.floor((s % 3600) / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${h}:${m}:${ss}`;
}

/** Compact duration for countdowns: "4m 12s", "48s". */
export function formatCountdown(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return rem === 0 ? `${m}m` : `${m}m ${String(rem).padStart(2, "0")}s`;
}

/** Rounded duration for budgets: "25 min", "1h 05m". */
export function formatBudget(secs: number): string {
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} min`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m === 0 ? `${h}h` : `${h}h ${String(m).padStart(2, "0")}m`;
}
