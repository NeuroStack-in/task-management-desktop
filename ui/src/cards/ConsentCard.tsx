import { Button } from "@/components/ui/button";
import type { ConsentState } from "@/lib/types";

/**
 * The consent gate. PRIVACY.md §1.2: no capture without an active consent policy — so the
 * panel shows this and nothing else until it's acknowledged. Presenting a timer next to an
 * un-acknowledged disclosure would imply the agent is usable before consent.
 *
 * Silent mode changes the *copy* only (the user is still told the device is managed); the
 * acknowledgement is still required and recorded (PRIVACY.md §2, wireframe §12.7).
 */
export function ConsentCard({
  consent,
  silent,
  onGrant,
}: {
  consent: ConsentState;
  silent: boolean;
  onGrant: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mb-4">
        <h1 className="font-heading text-[15px] font-semibold tracking-[0.2px]">
          {silent ? "This device is managed" : "Monitoring notice"}
        </h1>
        <p className="mt-1 text-muted-foreground">
          {silent
            ? "Your organization manages this device. Activity is recorded in the background."
            : "While you're on shift, WorkPulse records the following:"}
        </p>
      </div>

      <ul className="space-y-2">
        {consent.captured.map((item) => (
          <li key={item} className="flex gap-2.5 rounded-lg border border-border bg-card p-2.5">
            <span aria-hidden className="mt-[6px] h-[5px] w-[5px] shrink-0 rounded-full bg-primary" />
            <span className="leading-[1.5] text-muted-foreground">{item}</span>
          </li>
        ))}
      </ul>

      <div className="mt-auto pt-4">
        <p className="mb-3 text-[12px] text-muted-foreground/70">
          Policy version {consent.policy_version}. You'll be asked again if this changes.
        </p>
        <Button className="w-full" onClick={onGrant}>
          I understand
        </Button>
      </div>
    </div>
  );
}
