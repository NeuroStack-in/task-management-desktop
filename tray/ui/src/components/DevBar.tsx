import { useState } from "react";

import { getScenario, SCENARIO_LABEL, SCENARIOS, setScenario, type Scenario } from "@/lib/mock";
import { cn } from "@/lib/utils";

/** Height this row costs, including its bottom margin. The dev window grows by exactly
 *  this much (lib/dev-window.ts) so the panel below is the size production ships. */
export const DEV_BAR_PX = 34;

/**
 * Design-time scenario switcher. Rendered only when the fake core is in play, so it can never
 * ship — see USE_MOCK in lib/agent.ts. Lets you see every card in every state without IPC.
 */
export function DevBar({ onChange }: { onChange: () => void }) {
  const [active, setActive] = useState<Scenario>(getScenario);

  return (
    <div className="mb-2 flex shrink-0 items-center gap-1.5">
      <span
        title="Mock data — this row is dev-only and never ships"
        className="shrink-0 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/60"
      >
        Preview
      </span>
      <div className="flex flex-wrap items-center gap-1">
        {SCENARIOS.map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => {
              setScenario(s);
              setActive(s);
              onChange();
            }}
            className={cn(
              "cursor-pointer rounded-md border px-2 py-[3px] text-[11px] font-medium transition-colors",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40",
              s === active
                ? "border-primary/40 bg-accent text-accent-foreground"
                : "border-border text-muted-foreground hover:bg-muted hover:text-foreground",
            )}
          >
            {SCENARIO_LABEL[s]}
          </button>
        ))}
      </div>
    </div>
  );
}
