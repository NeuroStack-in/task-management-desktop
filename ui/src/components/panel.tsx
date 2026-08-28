import { Power } from "lucide-react";
import { useEffect, useState, type ComponentProps, type ReactNode } from "react";

import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { getAutoStart, setAutoStart } from "@/lib/agent";
import { initials } from "@/lib/format";
import type { Identity } from "@/lib/types";
import { cn } from "@/lib/utils";

/**
 * Panel-local primitives — the bits shadcn has no component for, styled to the web app's
 * Meridian language. Anything shadcn *does* cover (Button, Card, Badge, Select) comes from
 * components/ui/, copied verbatim so the two surfaces share one design system (AGENT.md §3).
 *
 * Meridian's load-bearing rule (globals.css:1506): content uses a hairline border, never a
 * shadow. Shadows are reserved for true overlays — which here means only the Select popup.
 */

/**
 * shadcn's Card at the panel's spacing.
 *
 * `shrink-0` is load-bearing. These are flex children of a scrolling column, so without it
 * flexbox shrinks them to fit and Card's own `overflow-hidden` silently clips the last row
 * of every card rather than letting the column scroll.
 */
export function PanelCard({ className, ...props }: ComponentProps<typeof Card>) {
  return <Card size="sm" className={cn("shrink-0", className)} {...props} />;
}

/** A label/value line inside a card. */
export function Row({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2 py-[3px]">
      <span className="text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

/**
 * Status chip. Follows the web app's dominant Badge pattern rather than its variants:
 * a `/12`–`/15` tinted fill, solid semantic text, `font-normal`, transparent border
 * (e.g. agents-manager.tsx:296, security-center.tsx:577). Base Badge is already
 * `rounded-sm h-5 text-xs` — Meridian chips are square-ish, not full pills.
 */
export function StatusBadge({
  tone = "neutral",
  className,
  children,
}: {
  tone?: "neutral" | "on" | "warn";
  className?: string;
  children: ReactNode;
}) {
  return (
    <Badge
      className={cn(
        "border-transparent font-normal",
        tone === "neutral" && "bg-muted text-muted-foreground",
        tone === "on" && "bg-success/12 text-success",
        tone === "warn" && "bg-warning/15 text-warning",
        className,
      )}
    >
      {children}
    </Badge>
  );
}

/**
 * The signature icon chip — `bg-feature-tint text-primary`, the shape the web app puts
 * beside every card title (widgets.tsx:89, 289).
 */
export function IconChip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "flex size-6 shrink-0 items-center justify-center rounded-full bg-feature-tint text-primary [&>svg]:size-3.5",
        className,
      )}
    >
      {children}
    </span>
  );
}

/**
 * The live capture indicator (PRIVACY.md §5: "Persistent indicator … so monitoring is never
 * hidden by default"). It pulses only while the core reports capture actually running.
 */
export function LiveDot({ live }: { live: boolean }) {
  return (
    <span className="relative flex size-2">
      {live && (
        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-60" />
      )}
      <span
        className={cn(
          "relative inline-flex size-2 rounded-full",
          live ? "bg-success" : "bg-muted-foreground/40",
        )}
      />
    </span>
  );
}

/**
 * Uppercase micro-label — the web app's section-label register
 * (widgets.tsx:162: `text-[11px] font-semibold uppercase tracking-wide text-muted-foreground`).
 */
export function CardLabel({ children }: { children: ReactNode }) {
  return (
    <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
      {children}
    </span>
  );
}

/**
 * Segmented control — the web app's range switcher (dashboard-controls.tsx:45-60):
 * a pill track holding pill buttons, active one filled with --primary.
 *
 * Two differences from the web app's, both because this one sits *on* a card rather than on
 * the page: the track is `bg-muted` so it reads against `bg-card`, and it fills its width
 * with equal segments instead of hugging its labels — uneven halves look accidental when the
 * control spans a card header.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="flex w-full items-center gap-0.5 rounded-full border border-border bg-muted p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          aria-pressed={o.value === value}
          className={cn(
            "flex-1 cursor-pointer truncate rounded-full px-2 py-1 text-[11px] font-medium transition-colors",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40",
            o.value === value
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Card title row: icon chip + micro-label, with optional right-hand slot. */
export function CardTitleRow({
  icon,
  label,
  action,
}: {
  icon: ReactNode;
  label: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <IconChip>{icon}</IconChip>
      <CardLabel>{label}</CardLabel>
      {action && <span className="ml-auto">{action}</span>}
    </div>
  );
}

/**
 * Inline meter — widgets.tsx:314. Track `bg-muted`, fill `bg-primary`, both `rounded-full`.
 */
export function Meter({ value, className }: { value: number; className?: string }) {
  return (
    <div className="h-2 overflow-hidden rounded-full bg-muted">
      <div
        className={cn("h-full rounded-full bg-primary transition-[width] ease-standard", className)}
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  );
}

/**
 * Who this device reports as (ENROLLMENT.md §2: bound to exactly one user).
 *
 * The avatar falls back to the web app's "Gradient Monogram" — a deterministic two-tone fill
 * seeded from the name — because seeded users have no `avatarUrl`. Same seed, same gradient,
 * so a person looks identical in both surfaces.
 */
export function IdentityChip({ identity }: { identity: Identity }) {
  return (
    <span
      className="flex min-w-0 items-center gap-1.5"
      title={`${identity.name} · ${identity.email}`}
    >
      <span className="min-w-0 truncate text-[12px] font-medium">{identity.name}</span>
      <Avatar size="sm">
        {identity.avatar_url && <AvatarImage src={identity.avatar_url} alt="" />}
        <AvatarFallback>{initials(identity.name)}</AvatarFallback>
      </Avatar>
    </span>
  );
}

/**
 * Launch-at-login toggle. Reads the real OS state on mount and flips it on click. Renders **nothing**
 * when the state can't be read (browser dev shell, or a platform where the query fails) rather than
 * showing a control that lies. On Windows the installer already asked; this lets the user change it
 * afterward, and it's the only control on macOS/Linux (no install wizard).
 */
export function AutostartToggle() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    getAutoStart().then(
      (v) => live && setEnabled(v),
      () => live && setEnabled(null),
    );
    return () => {
      live = false;
    };
  }, []);

  if (enabled === null) return null;

  const toggle = async () => {
    if (busy) return;
    setBusy(true);
    const next = !enabled;
    try {
      await setAutoStart(next);
      setEnabled(next);
    } catch {
      // OS write failed — leave the toggle where it was rather than showing a false state.
    } finally {
      setBusy(false);
    }
  };

  return (
    <Button
      variant="ghost"
      size="icon-sm"
      onClick={toggle}
      disabled={busy}
      aria-pressed={enabled}
      aria-label={enabled ? "Disable launch at startup" : "Enable launch at startup"}
      title={enabled ? "Launch at startup: on" : "Launch at startup: off"}
      className={enabled ? "text-primary" : "text-muted-foreground/50"}
    >
      <Power />
    </Button>
  );
}

