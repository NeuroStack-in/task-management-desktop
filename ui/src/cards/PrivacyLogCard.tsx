import { ShieldAlert } from "lucide-react";

import { CardTitleRow, PanelCard } from "@/components/panel";
import { CardContent, CardHeader } from "@/components/ui/card";
import type { LocalEvent } from "@/lib/types";

/**
 * The employee's own transparency log (`privacy_log.rs`), newest first.
 *
 * It exists for one reason: the admin-triggered on-demand capture is the only thing the agent does
 * that the person in front of the screen could not otherwise observe — no cadence they were told
 * about, no indicator that distinguishes it. PRIVACY.md §5 rules out silent access, so every such
 * request lands here, **including the ones that were refused**: "someone asked and the agent said
 * no" is exactly as much their business as "someone asked and it happened".
 *
 * The card renders **only when there is something to say** — an always-present empty "nothing has
 * been done to you" panel would spend the panel's fixed height on a non-event, and this list is
 * empty on almost every machine on almost every day.
 *
 * `detail` is rendered as the core wrote it. The panel must not re-compose a sentence from `kind`:
 * the core owns the wording, and a kind it doesn't recognise must still read correctly.
 */
export function PrivacyLogCard({ events }: { events: LocalEvent[] }) {
  if (events.length === 0) return null;

  return (
    <PanelCard>
      <CardHeader>
        <CardTitleRow icon={<ShieldAlert />} label="Privacy log" />
      </CardHeader>
      <CardContent>
        {/* Not a nested scroller — same fix as SessionsCard. A `max-h` + `overflow-y-auto` here
            swallowed wheel events instead of chaining out to the panel's single scroll column
            (App.tsx), which is what made the panel feel stuck once a banner pushed content down.
            The gutter reservation went with it: `scrollbar-gutter` only applies to scroll
            containers, so it would be inert now. */}
        <ul className="space-y-1.5 pr-1">
          {events.map((e) => (
            <li key={`${e.ts}-${e.kind}`} className="flex items-start gap-2">
              <span
                aria-hidden
                className={
                  e.kind === "admin_capture"
                    ? "mt-1.5 size-1.5 shrink-0 rounded-full bg-warning"
                    : "mt-1.5 size-1.5 shrink-0 rounded-full bg-muted-foreground/50"
                }
              />
              <span className="min-w-0 flex-1 text-[11.5px] leading-4">{e.detail}</span>
              <time
                dateTime={new Date(e.ts).toISOString()}
                className="tabular shrink-0 text-[11px] leading-4 text-muted-foreground"
              >
                {new Date(e.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
              </time>
            </li>
          ))}
        </ul>
      </CardContent>
    </PanelCard>
  );
}
