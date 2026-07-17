import { PauseCircle } from "lucide-react";

import { CardTitleRow, Meter, PanelCard } from "@/components/panel";
import { Button } from "@/components/ui/button";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatCountdown } from "@/lib/format";

const PAUSE_SECS = 5 * 60;

/**
 * Bounded pause. The tray only *requests* — the core clamps to org policy and records the
 * pause as an auditable event (PRIVACY.md §5: "agency without creating blind spots").
 */
export function PauseCard({
  pauseSecs,
  refused,
  onRequest,
}: {
  pauseSecs: number;
  refused: boolean;
  onRequest: (secs: number) => void;
}) {
  const active = pauseSecs > 0;

  return (
    <PanelCard>
      <CardHeader>
        <CardTitleRow
          icon={<PauseCircle />}
          label="Privacy pause"
          action={
            active ? (
              <span className="tabular text-sm font-semibold text-warning">
                {formatCountdown(pauseSecs)}
              </span>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onClick={() => onRequest(PAUSE_SECS)}
                disabled={refused}
              >
                Pause 5 min
              </Button>
            )
          }
        />
      </CardHeader>
      <CardContent>
        {active ? (
          <>
            <Meter value={(pauseSecs / PAUSE_SECS) * 100} className="bg-warning duration-1000" />
            <p className="mt-0.5 text-[11px] text-muted-foreground">
              Capture is suspended. Your organization is notified that it was paused.
            </p>
          </>
        ) : (
          <p className="text-[11px] text-muted-foreground">
            {refused
              ? "Pausing isn't available under your organization's policy."
              : "Suspends capture for a bounded window. Logged as an auditable event."}
          </p>
        )}
      </CardContent>
    </PanelCard>
  );
}
