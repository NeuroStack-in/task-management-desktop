// WorkPulse mark — a pulse/activity waveform on an indigo→teal gradient tile. Inline SVG so there is
// no image asset to bundle (and the CSP stays tight).
export function WorkPulseLogo({ size = 36, class: className }: { size?: number; class?: string }) {
  return (
    <div
      class={className}
      style={{
        width: size,
        height: size,
        borderRadius: "24%",
        background: "linear-gradient(135deg, hsl(var(--primary)) 0%, #14b8a6 100%)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        boxShadow: "0 2px 8px -2px hsl(var(--primary) / 0.5)",
        flexShrink: 0,
      }}
    >
      <svg
        width={size * 0.6}
        height={size * 0.6}
        viewBox="0 0 24 24"
        fill="none"
        stroke="white"
        stroke-width="2.4"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M2 12h4l2.5 7 4-14 2.5 7H22" />
      </svg>
    </div>
  );
}
