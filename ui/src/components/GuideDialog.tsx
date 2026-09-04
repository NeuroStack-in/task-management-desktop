import { useState, type ReactNode } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { FolderKanban, Info, ListChecks, Play, ShieldOff, TimerReset, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * The panel's "how do I use this?" guide — an info button that opens a **four-page tour**, one topic
 * per page, sliding left-to-right, ending on "Got it". It answers the four things a first-time user
 * asks: picking what to track, working with subtasks, taking a break and resuming it, and Privacy
 * Pause.
 *
 * A paged tour rather than one long list because each topic gets a clean, centred page the reader
 * moves through at their own pace — it reads once and is dismissed. Built on the same Base UI Dialog
 * and theme tokens as the rest of the panel, so it matches in light and dark with no bespoke styling.
 *
 * `triggerClassName` styles the info button for wherever it sits — on the teal timer card it takes
 * the card's white-on-fill treatment; the default suits a neutral surface.
 */
export function GuideDialog({ triggerClassName }: { triggerClassName?: string }) {
  const [page, setPage] = useState(0);
  const last = STEPS.length - 1;

  return (
    // Reset to the first page every time it opens, so it always starts the tour at page one.
    <Dialog.Root onOpenChange={(open) => open && setPage(0)}>
      <Dialog.Trigger
        render={
          <button
            type="button"
            aria-label="How to use WorkPulse"
            title="How to use WorkPulse"
            className={
              triggerClassName ??
              "flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
            }
          />
        }
      >
        <Info className="size-4" />
      </Dialog.Trigger>

      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-black/45 backdrop-blur-[1px] duration-150 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0" />
        <Dialog.Popup className="fixed left-1/2 top-1/2 z-50 flex max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.75rem)] max-w-[360px] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-xl duration-150 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95">
          {/* Header — pinned. */}
          <div className="flex shrink-0 items-center gap-2.5 border-b border-border px-4 py-3">
            <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
              <Info className="size-4" />
            </span>
            <div className="min-w-0">
              <Dialog.Title className="font-heading text-[14px] font-semibold leading-tight">
                How to use WorkPulse
              </Dialog.Title>
              <Dialog.Description className="text-[11.5px] leading-tight text-muted-foreground">
                A quick tour — {STEPS.length} steps.
              </Dialog.Description>
            </div>
            <Dialog.Close
              render={
                <button
                  type="button"
                  aria-label="Close"
                  className="ml-auto flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                />
              }
            >
              <X className="size-4" />
            </Dialog.Close>
          </div>

          {/* Carousel — a fixed-height viewport; the track slides one page per step. Fixed height so
              the dialog never resizes between pages of different lengths. */}
          <div className="h-[256px] shrink-0 overflow-hidden">
            <div
              className="flex h-full transition-transform duration-300 ease-out"
              style={{ transform: `translateX(-${page * 100}%)` }}
            >
              {STEPS.map((s) => (
                <Slide key={s.title} {...s} />
              ))}
            </div>
          </div>

          {/* Footer — Back · dots · Next/Got it. Back stays laid out (invisible) on page one so the
              dots never shift. */}
          <div className="flex shrink-0 items-center gap-2 border-t border-border px-4 py-3">
            <button
              type="button"
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              className={cn(
                "w-14 shrink-0 rounded-md py-1 text-[12px] font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40",
                page === 0 && "pointer-events-none invisible",
              )}
            >
              Back
            </button>

            <div className="flex flex-1 items-center justify-center gap-1.5" aria-hidden>
              {STEPS.map((s, i) => (
                <span
                  key={s.title}
                  className={cn(
                    "h-1.5 rounded-full transition-all duration-200",
                    i === page ? "w-4 bg-primary" : "w-1.5 bg-muted-foreground/30",
                  )}
                />
              ))}
            </div>

            {page < last ? (
              <Button size="sm" className="w-14 shrink-0" onClick={() => setPage((p) => p + 1)}>
                Next
              </Button>
            ) : (
              <Dialog.Close render={<Button size="sm" className="w-14 shrink-0">Got it</Button>} />
            )}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** One tour page — a centred icon and heading, then the topic as a short list of points. */
function Slide({ icon: Icon, title, points }: Step) {
  return (
    <div className="flex h-full w-full shrink-0 flex-col items-center justify-center px-6">
      <span className="flex size-11 items-center justify-center rounded-2xl bg-primary/10 text-primary">
        <Icon className="size-5" />
      </span>
      <p className="mt-3 text-center text-[15px] font-semibold leading-tight">{title}</p>
      <ul className="mt-3 flex w-full max-w-[280px] flex-col gap-2 text-left">
        {points.map((point, i) => (
          <li key={i} className="flex gap-2 text-[12px] leading-snug text-muted-foreground">
            <span aria-hidden className="mt-[6px] size-1 shrink-0 rounded-full bg-primary/70" />
            <span className="min-w-0">{point}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

interface Step {
  icon: LucideIcon;
  title: string;
  points: ReactNode[];
}

const STEPS: Step[] = [
  {
    icon: FolderKanban,
    title: "Pick what you're working on",
    points: [
      <>
        Choose a <B>project</B> — the only thing required.
      </>,
      <>
        A <B>task</B> is optional — pick one if you have it.
      </>,
      <>
        Add a note in <Q>What are you working on?</Q>
      </>,
      <>
        Press <B>Start</B>.
      </>,
    ],
  },
  {
    icon: ListChecks,
    title: "Break a task into subtasks",
    points: [
      <>
        Press <B>Add subtask</B> to split a task into steps.
      </>,
      <>
        <B>Tap a subtask</B> to select the one you're on.
      </>,
      <>
        <B>Tick its box</B> to mark it done — a done one can't be selected.
      </>,
      <>
        Move the task with the <B>To&nbsp;do → In&nbsp;progress → In&nbsp;review</B> stepper.
      </>,
    ],
  },
  {
    icon: TimerReset,
    title: "Take a break, then resume",
    points: [
      <>
        Press <B>Stop</B> when you step away.
      </>,
      <>
        Find the work again in <B>Today's sessions</B>.
      </>,
      <>
        Tap the play <Play className="inline size-3.5 align-[-2px] fill-primary text-primary" /> on
        that row to continue.
      </>,
      <>It starts a fresh session on the same project and task.</>,
    ],
  },
  {
    icon: ShieldOff,
    title: "Privacy Pause",
    points: [
      <>
        <B>Privacy Pause</B> suspends screenshot capture briefly.
      </>,
      <>
        You get <B>30 minutes a day</B>.
      </>,
      <>
        Your <B>timer keeps running</B> — only screenshots pause.
      </>,
      <>Capture resumes on its own when the window ends.</>,
    ],
  },
];

/** Emphasis for a control's name, in the reader's foreground colour rather than raw bold. */
function B({ children }: { children: ReactNode }) {
  return <span className="font-medium text-foreground">{children}</span>;
}

/** A literal on-screen label, quoted so it reads as text the user will see. */
function Q({ children }: { children: ReactNode }) {
  return <span className="font-medium text-foreground">&ldquo;{children}&rdquo;</span>;
}
