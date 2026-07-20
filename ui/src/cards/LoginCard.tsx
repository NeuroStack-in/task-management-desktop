import { Eye, EyeOff } from "lucide-react";
import { useState, type FormEvent } from "react";

import { LogoMark } from "@/components/shared/logo";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/**
 * The sign-in gate — the panel's first screen on a fresh install, ahead of consent.
 *
 * Design-time only for now: this is the "Login" preview scenario, so submitting validates
 * locally and fakes a round-trip rather than touching Cognito. The real flow lands on
 * `auth_login` (USER_PASSWORD_AUTH — src-tauri/src/auth/cognito.rs), which would replace
 * `onSubmit` and surface `auth:*` errors in the same banner this already renders.
 */
export function LoginCard() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [reveal, setReveal] = useState(false);
  const [remember, setRemember] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Preview-only: local validation, then a fake round-trip so the pending and error states
  // are both reachable from the chip without a live pool.
  function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (pending) return;

    if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) {
      setError("Enter the work email your organization issued you.");
      return;
    }
    if (password.length < 8) {
      setError(
        "That password looks too short — it should be at least 8 characters.",
      );
      return;
    }

    setError(null);
    setPending(true);
    setTimeout(() => {
      setPending(false);
      setError("We couldn't verify those details. Check them and try again.");
    }, 1100);
  }

  return (
    // Fills the panel top-to-bottom: brand pinned at the top, footer at the bottom, and the form
    // centred in whatever is left. Splitting the slack above and below the form avoids the single
    // dead band a lone `mt-auto` on the footer produced.
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* The panel is 620 wide; full-bleed inputs at that width read as a web form rather than a
          desktop panel. 440 keeps a single column with a little breathing room either side. */}
      <div className="mx-auto flex w-full max-w-[440px] flex-1 flex-col">
        {/* Brand lockup — the panel header is hidden behind this gate, so sign-in carries it. */}
        <div className="flex items-center gap-2.5">
          <LogoMark className="size-9 shrink-0 rounded-[11px] shadow-sm" />
          <div className="leading-tight">
            <h1 className="font-heading text-[16px] font-semibold tracking-[0.2px]">
              WorkPulse
            </h1>
            <p className="text-[12px] text-muted-foreground/70">
              Time &amp; activity agent
            </p>
          </div>
        </div>

        <div className="flex flex-1 flex-col justify-center py-5">
          <div className="mb-4">
            <h2 className="font-heading text-[19px] font-semibold tracking-[0.2px]">
              Sign in
            </h2>
            <p className="mt-1 text-[13px] text-muted-foreground">
              Use the work account your organization set up for you.
            </p>
          </div>

          {error && (
            <div
              role="alert"
              className="mb-3 flex gap-2.5 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5"
            >
              <span
                aria-hidden
                className="mt-[6px] size-[5px] shrink-0 rounded-full bg-destructive"
              />
              <span className="leading-[1.5] text-destructive">{error}</span>
            </div>
          )}

          <form onSubmit={onSubmit} className="space-y-4">
            <div className="space-y-1.5">
              <label
                htmlFor="wp-email"
                className="block text-[13px] font-medium"
              >
                Work email
              </label>
              <Input
                id="wp-email"
                type="email"
                autoComplete="username"
                placeholder="you@company.com"
                className="h-11 px-3 text-[13px]"
                value={email}
                onValueChange={(v) => {
                  setEmail(v);
                  setError(null);
                }}
              />
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="wp-password"
                className="block text-[13px] font-medium"
              >
                Password
              </label>
              <div className="relative">
                <Input
                  id="wp-password"
                  type={reveal ? "text" : "password"}
                  autoComplete="current-password"
                  placeholder="••••••••"
                  className="h-11 px-3 pr-10 text-[13px]"
                  value={password}
                  onValueChange={(v) => {
                    setPassword(v);
                    setError(null);
                  }}
                />
                <button
                  type="button"
                  onClick={() => setReveal((r) => !r)}
                  aria-label={reveal ? "Hide password" : "Show password"}
                  aria-pressed={reveal}
                  className="absolute inset-y-0 right-0 flex w-11 cursor-pointer items-center justify-center rounded-r-md text-muted-foreground/70 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                >
                  {reveal ? (
                    <EyeOff className="size-4" />
                  ) : (
                    <Eye className="size-4" />
                  )}
                </button>
              </div>
            </div>

            {/* Governs refresh-token persistence in the OS keyring, not the session itself. */}
            <button
              type="button"
              onClick={() => setRemember((r) => !r)}
              aria-pressed={remember}
              className="flex w-full cursor-pointer items-center gap-2.5 rounded-lg border border-border bg-card px-3 py-2.5 text-left transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
            >
              <span
                aria-hidden
                className={
                  remember
                    ? "flex size-[18px] shrink-0 items-center justify-center rounded-[6px] border border-primary bg-primary"
                    : "flex size-[18px] shrink-0 items-center justify-center rounded-[6px] border border-border"
                }
              >
                {remember && (
                  <span className="size-[7px] rounded-[2px] bg-primary-foreground" />
                )}
              </span>
              <span className="leading-[1.4]">
                <span className="block text-[13px] font-medium">
                  Keep me signed in
                </span>
                <span className="block text-[11px] text-muted-foreground/70">
                  Stores a refresh token in the Windows Credential Manager.
                </span>
              </span>
            </button>

            <Button
              type="submit"
              size="lg"
              className="h-11 w-full text-[14px]"
              disabled={pending}
            >
              {pending ? "Signing in…" : "Sign in"}
            </Button>
          </form>
        </div>

        <div>
          <div className="mb-2.5 flex items-center gap-2">
            <span aria-hidden className="h-px flex-1 bg-border" />
            <span className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/50">
              Managed device
            </span>
            <span aria-hidden className="h-px flex-1 bg-border" />
          </div>
          <p className="text-center text-[12px] leading-[1.5] text-muted-foreground/70">
            Nothing is recorded until you sign in and accept the monitoring
            notice. Forgotten your password? Your workspace admin can reset it.
          </p>
        </div>
      </div>
    </div>
  );
}
