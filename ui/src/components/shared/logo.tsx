/**
 * The WorkPulse mark — pulse-line glyph in an accent badge.
 *
 * Ported verbatim (the path data, at least) from the web app's marketing logo
 * (task-management-frontend/src/modules/marketing/logo.tsx) so the agent and the web app carry
 * the same brand, and it matches the bundled app icon in src-tauri/icons.
 *
 * The badge takes the panel's `--primary` rather than the marketing `--m-accent`: the desktop
 * palette is teal-led, and a stray indigo badge would be the one off-palette element on screen.
 */
export function LogoMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 30 30" fill="none" aria-hidden className={className}>
      <rect width="30" height="30" rx="8.5" fill="var(--primary)" />
      <path
        d="M5.5 16.2h3.6l2-6.4 3.4 10.2 2.1-5.3h4.4"
        stroke="var(--primary-foreground)"
        strokeWidth="2.1"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
