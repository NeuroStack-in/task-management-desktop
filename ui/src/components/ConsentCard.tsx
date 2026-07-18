import { ipc } from "../lib/ipc";
import { Button } from "./ui/Button";
import { Card } from "./ui/Card";

// Monitoring consent — the gate that turns capture on. Off by default; capture (activity counts +
// screenshots) never runs until granted (PRIVACY.md, fails closed).
export function ConsentCard({
  granted,
  onChange,
}: {
  granted: boolean;
  onChange: (g: boolean) => void;
}) {
  async function toggle() {
    const next = !granted;
    await ipc.setConsent(next).catch(() => {});
    onChange(next);
  }

  return (
    <Card class="p-4">
      {granted ? (
        <>
          <div class="flex items-center gap-2 text-[13px] font-semibold text-emerald-500">
            <span class="h-2 w-2 animate-pulse rounded-full bg-emerald-500" /> Monitoring is on
          </div>
          <p class="mt-1 text-[11.5px] leading-snug text-muted-foreground">
            Activity counts and screenshots are captured <b>only while the timer runs</b>.
          </p>
          <Button variant="secondary" size="sm" class="mt-3" onClick={toggle}>
            Turn off monitoring
          </Button>
        </>
      ) : (
        <>
          <div class="text-[13px] font-semibold text-foreground">Monitoring is off</div>
          <p class="mt-1 text-[11.5px] leading-snug text-muted-foreground">
            WorkPulse captures activity <b>metrics</b> (keystroke/mouse <i>counts</i> — never content)
            and screenshots while your timer runs. Nothing is captured until you turn this on.
          </p>
          <Button size="sm" class="mt-3" onClick={toggle}>
            Turn on monitoring
          </Button>
        </>
      )}
    </Card>
  );
}
