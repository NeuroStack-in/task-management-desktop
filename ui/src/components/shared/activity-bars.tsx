import { cn } from "@/lib/utils";

/**
 * Inline activity bars.
 *
 * Mirrors the web app's bar treatment (team-comparison-chart.tsx:63-73): rounded top corners,
 * the peak bar in `--chart-1`, every other bar in
 * `color-mix(in srgb, var(--primary) 28%, var(--muted))`.
 *
 * Deliberately CSS rather than recharts. The web app reaches for recharts at ~300×200 with
 * axes, grid and tooltips; this is a ~28px strip of 24 buckets with none of that, and the
 * library would cost ~100KB to draw rectangles. The fills are the same, so it reads as the
 * same chart family.
 *
 * The web app's radius is [8,8,0,0] on `maxBarSize={44}` bars — that ratio on a ~7px bar
 * would round the whole thing away, so it scales down to 2px here.
 */
export function ActivityBars({
  data,
  muted = false,
  className,
}: {
  data: number[];
  /** Drains the colour — used while capture is paused. */
  muted?: boolean;
  className?: string;
}) {
  const peak = Math.max(...data, 1);

  return (
    <div className={cn("flex h-7 items-end gap-px", className)} aria-hidden="true">
      {data.map((v, i) => {
        const isPeak = v === peak && !muted;
        return (
          <div
            key={i}
            className="min-h-px flex-1 rounded-t-[2px]"
            style={{
              height: `${Math.max(4, (v / peak) * 100)}%`,
              background: muted
                ? "var(--border)"
                : isPeak
                  ? "var(--chart-1)"
                  : "color-mix(in srgb, var(--primary) 28%, var(--muted))",
            }}
          />
        );
      })}
    </div>
  );
}
