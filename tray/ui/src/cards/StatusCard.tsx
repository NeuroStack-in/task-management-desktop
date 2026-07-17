import { useState } from "react";

import { ActivityBars } from "@/components/shared/activity-bars";
import { PanelCard, Row, Segmented, StatusBadge } from "@/components/panel";
import { CardContent, CardHeader } from "@/components/ui/card";
import { formatCountdown } from "@/lib/format";
import {
  BLUR_LABEL,
  CADENCE_LABEL,
  type ActivitySeries,
  type CaptureState,
  type ConsentState,
  type TrackingConfig,
} from "@/lib/types";

type Tab = "status" | "collected";

/**
 * What the agent is doing right now, or what it's allowed to collect.
 *
 * They share a slot because both answer "what is happening to me?" at different depths:
 * Status is the live answer, What's collected is the standing policy behind it. Status leads
 * because it changes; policy rarely does.
 *
 * PRIVACY.md §1.4 requires the transparency view be available *on demand* — one click, in
 * every state including silent mode — not that it be permanently on screen.
 */
export function StatusCard({
  capture,
  paused,
  activity,
  config,
  consent,
}: {
  capture: CaptureState;
  paused: boolean;
  activity: ActivitySeries;
  config: TrackingConfig;
  consent: ConsentState;
}) {
  const [tab, setTab] = useState<Tab>("status");

  return (
    <PanelCard className="flex-1">
      <CardHeader>
        <Segmented<Tab>
          value={tab}
          onChange={setTab}
          options={[
            { value: "status", label: "Status" },
            { value: "collected", label: "What's collected" },
          ]}
        />
      </CardHeader>
      <CardContent>
        {tab === "status" ? (
          <StatusPanel capture={capture} paused={paused} activity={activity} />
        ) : (
          <CollectedPanel
            config={config}
            consent={consent}
            capturing={capture.capturing && !paused}
          />
        )}
      </CardContent>
    </PanelCard>
  );
}

function StatusPanel({
  capture,
  paused,
  activity,
}: {
  capture: CaptureState;
  paused: boolean;
  activity: ActivitySeries;
}) {
  return (
    <>
      <Row label="Capturing">
        {paused ? (
          <StatusBadge tone="warn">paused</StatusBadge>
        ) : (
          <StatusBadge tone={capture.capturing ? "on" : "neutral"}>
            {capture.capturing ? "yes" : "no"}
          </StatusBadge>
        )}
      </Row>
      <Row label="Screenshots">
        <StatusBadge tone={capture.screenshots ? "on" : "neutral"}>
          {capture.screenshots ? "on" : "off"}
        </StatusBadge>
      </Row>
      <Row label="Next cycle">
        <span className="tabular text-foreground">
          {capture.capturing && capture.next_cycle_secs > 0
            ? formatCountdown(capture.next_cycle_secs)
            : "—"}
        </span>
      </Row>

      {/* Hidden rather than faked when the core has nothing to give (there is no activity
          command yet — see types.ts). */}
      {activity.length > 0 && (
        <div className="mt-2 border-t border-border/60 pt-2">
          <div className="mb-1.5 flex items-baseline justify-between">
            <span className="text-[11px] text-muted-foreground">Your activity</span>
            <span className="text-[11px] text-muted-foreground">last 2h</span>
          </div>
          <ActivityBars data={activity} muted={paused} />
        </div>
      )}
    </>
  );
}

/**
 * The transparency view (PRIVACY.md §1.4, §5): read-only — config is admin-owned and arrives
 * over the CONFIG rail.
 *
 * PRIVACY.md §5 also requires the quiet-hours window here. It is deliberately absent:
 * `TrackingConfig` (backend/crates/wp-agent-contract/src/config.rs) carries no quiet-hours
 * field, and inventing one would show the user a policy the agent isn't running.
 */
function CollectedPanel({
  config,
  consent,
  capturing,
}: {
  config: TrackingConfig;
  consent: ConsentState;
  capturing: boolean;
}) {
  const blur = BLUR_LABEL[config.blur_level] ?? `Level ${config.blur_level}`;

  return (
    <>
      <Row label="Screenshots">
        <span className="text-foreground">{CADENCE_LABEL[config.cadence]}</span>
      </Row>
      <Row label="Blur">
        <span className="text-foreground">{blur}</span>
      </Row>
      <Row label="Retention">
        <span className="tabular text-foreground">{config.retention_days} days</span>
      </Row>
      {config.silent && (
        <Row label="Silent mode">
          <StatusBadge tone="warn">on</StatusBadge>
        </Row>
      )}

      {/* PRIVACY.md §5: "monitoring is active because: <org consent policy>" */}
      <p className="mt-2 border-t border-border/60 pt-2 text-[11px] leading-[1.5] text-muted-foreground">
        {capturing ? "Monitoring is active because" : "Monitoring is governed by"} your
        organization's consent policy (v{consent.policy_version}), which you acknowledged. Set by
        your admin.
      </p>
    </>
  );
}
