import { PauseCircle } from "lucide-react";

import { CardTitleRow, Meter, PanelCard } from "@/components/panel";
import { Button } from "@/components/ui/button";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatCountdown } from "@/lib/format";
import type { PauseState } from "@/lib/types";

/**
 * How much pause this button asks for. The core clamps it to whatever budget is left
 * (`PauseState::request`), so the granted window is `min(this, remaining budget)` — which is why
 * it is also a safe denominator for the meter: the live window can never exceed it.
 */
const PAUSE_REQUEST_SECS = 5 * 60;

/**
 * Bounded pause. The panel only *requests* — the core clamps it to the day's allowance (30 minutes
 * per local day, kept across restarts) and suspends screenshots, activity and location for the
 * window; the timer keeps running.
 *
 * The copy says only that. It used to claim "Your organization is notified that it was paused" —
 * no pause event exists, and nothing on the server reads the paused flag — and "only screenshots
 * pause" in the guide, when all three captures do (QA audit 2026-10-08, WP-213).
 *
 * Every number here is read back from the core each poll; nothing is counted down locally, so a
 * reload can't lose a pause that is still in effect.
 */
export function PauseCard({
  pause,
  refused,
  onRequest,
}: {
  pause: PauseState;
  refused: boolean;
  onRequest: (secs: number) => void;
}) {
  const budgetSpent = pause.remaining_budget_secs <= 0;

  return (
    <PanelCard>
      <CardHeader>
        <CardTitleRow
          icon={<PauseCircle />}
          label="Privacy pause"
          action={
            pause.paused ? (
              <span className="tabular text-sm font-semibold text-warning">
                {formatCountdown(pause.remaining_secs)}
              </span>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onClick={() => onRequest(PAUSE_REQUEST_SECS)}
                disabled={refused || budgetSpent}
              >
                Pause 5 min
              </Button>
            )
          }
        />
      </CardHeader>
      <CardContent>
        {pause.paused ? (
          <>
            <Meter
              value={Math.min(100, (pause.remaining_secs / PAUSE_REQUEST_SECS) * 100)}
              className="bg-warning duration-1000"
            />
            <p className="mt-0.5 text-[11px] text-muted-foreground">
              Screenshots, activity and location are paused. Your timer keeps running.
            </p>
          </>
        ) : (
          <p className="text-[11px] text-muted-foreground">
            {refused
              ? "Pausing isn't available under your organization's policy."
              : budgetSpent
                ? "You've used today's pause allowance. It resets tomorrow."
                : `Pauses screenshots, activity and location; your timer keeps running. ${formatCountdown(
                    pause.remaining_budget_secs,
                  )} of pause time left today.`}
          </p>
        )}
      </CardContent>
    </PanelCard>
  );
}
