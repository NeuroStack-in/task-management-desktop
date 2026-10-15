import { PauseCircle, PlayCircle } from "lucide-react";

import { CardTitleRow, Meter, PanelCard } from "@/components/panel";
import { Button } from "@/components/ui/button";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatCountdown } from "@/lib/format";
import type { PauseState } from "@/lib/types";

/**
 * Privacy pause, taken in as many pieces as the person likes (owner request 2026-10-15).
 *
 * "Pause" asks the core for the whole of today's remaining allowance; the pause lasts until
 * "Resume" — which gives the unused part back — or until the allowance runs out. So 5 seconds,
 * then 50, then … all draw on one daily total. The allowance is the organization's, set by an owner
 * or admin (0 turns pausing off). It used to be a fixed "Pause 5 min" with no way to resume: five
 * minutes were spent whether the pause lasted five minutes or five seconds.
 *
 * Screenshots, activity and location pause; the timer keeps running. The copy says only that — it
 * used to claim the organization is notified, which no pause event does (QA audit WP-213).
 *
 * Every number is read back from the core each poll; nothing is counted down locally, so a reload
 * can't lose a pause that is still in effect.
 */
export function PauseCard({
  pause,
  refused,
  onRequest,
  onResume,
}: {
  pause: PauseState;
  refused: boolean;
  onRequest: (secs: number) => void;
  onResume: () => void;
}) {
  const allowance = pause.allowance_secs;
  const off = allowance === 0;
  const budgetSpent = pause.remaining_budget_secs <= 0;
  const ofAllowance = allowance ? ` of ${formatCountdown(allowance)}` : "";

  return (
    <PanelCard>
      <CardHeader>
        <CardTitleRow
          icon={<PauseCircle />}
          label="Privacy pause"
          action={
            pause.paused ? (
              <Button variant="outline" size="sm" onClick={onResume}>
                <PlayCircle className="size-4" /> Resume
              </Button>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onClick={() => onRequest(pause.remaining_budget_secs)}
                disabled={off || refused || budgetSpent}
              >
                Pause
              </Button>
            )
          }
        />
      </CardHeader>
      <CardContent>
        {pause.paused ? (
          <>
            <div className="flex items-baseline justify-between">
              <span className="text-[11px] text-muted-foreground">Pause time left today</span>
              <span className="tabular text-sm font-semibold text-warning">
                {formatCountdown(pause.remaining_secs)}
              </span>
            </div>
            <Meter
              value={allowance ? Math.min(100, (pause.remaining_secs / allowance) * 100) : 0}
              className="bg-warning duration-1000"
            />
            <p className="mt-0.5 text-[11px] text-muted-foreground">
              Screenshots, activity and location are paused; your timer keeps running. Resume any
              time — only the time you were paused is used.
            </p>
          </>
        ) : (
          <p className="text-[11px] text-muted-foreground">
            {off
              ? "Your organization has turned the privacy pause off."
              : refused
                ? "Pausing isn't available under your organization's policy."
                : budgetSpent
                  ? "You've used today's pause time. It resets tomorrow."
                  : `Pauses screenshots, activity and location; your timer keeps running. Pause as often as you need — ${formatCountdown(
                      pause.remaining_budget_secs,
                    )}${ofAllowance} left today.`}
          </p>
        )}
      </CardContent>
    </PanelCard>
  );
}
