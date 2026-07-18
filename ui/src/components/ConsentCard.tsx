import { ipc } from "../lib/ipc";

// Monitoring consent — **the gate that turns capture on**. Capture (activity counts + screenshots)
// is off by default and never runs until the user grants this (PRIVACY.md, fails closed). Without
// this control there is no way to start monitoring from the app.
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
    <div class="rounded-lg border border-slate-800 bg-slate-900 p-4">
      {granted ? (
        <>
          <div class="flex items-center gap-2 text-sm font-medium text-teal-400">
            <span class="h-2 w-2 rounded-full bg-teal-400" /> Monitoring is on
          </div>
          <p class="mt-1 text-xs text-slate-400">
            Activity counts and screenshots are captured <b>only while the timer runs</b>.
          </p>
          <button
            onClick={toggle}
            class="mt-3 rounded-md bg-slate-800 px-3 py-1.5 text-xs font-medium hover:bg-slate-700"
          >
            Turn off monitoring
          </button>
        </>
      ) : (
        <>
          <div class="text-sm font-medium">Monitoring is off</div>
          <p class="mt-1 text-xs text-slate-400">
            WorkPulse captures activity <b>metrics</b> (keystroke/mouse <i>counts</i> — never content)
            and screenshots while your timer runs. Nothing is captured until you turn this on.
          </p>
          <button
            onClick={toggle}
            class="mt-3 rounded-md bg-teal-600 px-3 py-1.5 text-xs font-medium hover:bg-teal-500"
          >
            Turn on monitoring
          </button>
        </>
      )}
    </div>
  );
}
