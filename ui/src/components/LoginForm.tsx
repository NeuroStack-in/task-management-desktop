import { useState, type FormEvent } from "react";

import { Button } from "@/components/ui/button";
import { auth, type AuthStatus } from "@/lib/ipc";

function friendly(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  if (raw.includes("auth:not_configured"))
    return "This build isn't set up yet — WP_COGNITO_CLIENT_ID is missing (see docs/RUNBOOK.md).";
  if (raw.includes("USER_PASSWORD_AUTH") && raw.includes("not enabled"))
    return "Password sign-in isn't enabled on the server yet.";
  if (raw.includes("NotAuthorized") || raw.includes("Incorrect username or password"))
    return "Wrong email or password.";
  if (raw.includes("UserNotFound") || raw.includes("User does not exist"))
    return "Account not found. Check your email.";
  if (raw.includes("network")) return "Can't reach the server. Check your connection.";
  return "Sign-in failed. Please try again.";
}

const INPUT =
  "h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none " +
  "placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40";
const FIELD_LABEL = "text-[10.5px] font-semibold uppercase tracking-[0.1em] text-muted-foreground";

export function LoginForm({ onSignedIn }: { onSignedIn: (s: AuthStatus) => void }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [session, setSession] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const challenge = session !== null;

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const status = session
        ? await auth.completeNewPassword(email, newPassword, session)
        : await auth.login(email, password);
      setPassword("");
      if (status.newPasswordSession) setSession(status.newPasswordSession);
      else if (status.signedIn) onSignedIn(status);
      else setError("Sign-in failed.");
    } catch (err) {
      setError(friendly(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-background p-6 text-foreground">
      <div
        className="pointer-events-none absolute inset-0 opacity-[0.5]"
        aria-hidden
        style={{
          backgroundImage:
            "repeating-linear-gradient(135deg, color-mix(in oklch, var(--foreground) 3%, transparent) 0 1px, transparent 1px 9px)",
        }}
      />
      <div
        className="pointer-events-none absolute -top-24 left-1/2 h-80 w-96 -translate-x-1/2 rounded-full blur-3xl"
        aria-hidden
        style={{ background: "color-mix(in oklch, var(--primary) 10%, transparent)" }}
      />

      <div className="relative flex items-center gap-2.5">
        <Logo />
        <h1 className="text-[15px] font-bold tracking-tight">WorkPulse</h1>
      </div>

      <div className="relative flex flex-1 flex-col justify-center">
        <h2 className="text-[22px] font-extrabold tracking-tight">
          {challenge ? "Set a new password" : "Sign in to WorkPulse"}
        </h2>
        <p className="mt-1 text-[12px] text-muted-foreground">
          {challenge
            ? "First-time login — choose a password to continue."
            : "Track focused time, surface daily progress, ship more."}
        </p>

        <form onSubmit={submit} className="mt-5 space-y-3.5">
          <div className="space-y-1">
            <label className={FIELD_LABEL}>Email</label>
            <input
              className={INPUT}
              type="email"
              placeholder="you@company.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              autoFocus={!challenge}
            />
          </div>

          {challenge ? (
            <div className="space-y-1">
              <label className={FIELD_LABEL}>New password</label>
              <input
                className={INPUT}
                type="password"
                placeholder="At least 8 characters"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                required
                minLength={8}
                autoFocus
              />
            </div>
          ) : (
            <div className="space-y-1">
              <label className={FIELD_LABEL}>Password</label>
              <input
                className={INPUT}
                type="password"
                placeholder="Enter your password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>
          )}

          {error && (
            <div className="rounded-md border border-destructive/25 bg-destructive/10 px-3 py-2 text-[11.5px] text-destructive">
              {error}
            </div>
          )}

          <Button type="submit" disabled={busy} size="lg" className="w-full">
            {busy ? "Working…" : challenge ? "Set password & continue" : "Sign in"}
          </Button>
        </form>

        <p className="mt-4 text-[10px] text-muted-foreground/80">Cognito authentication · TLS 1.3</p>
      </div>

      <div className="relative flex items-center justify-between text-[9.5px] font-medium uppercase tracking-[0.06em] text-muted-foreground/70">
        <span>NeuroStack · MMXXVI</span>
        <span>WorkPulse Agent</span>
      </div>
    </div>
  );
}

function Logo() {
  return (
    <div
      className="flex h-7 w-7 items-center justify-center rounded-[7px]"
      style={{ background: "linear-gradient(135deg, var(--primary), #14b8a6)" }}
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="white"
        strokeWidth="2.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M2 12h4l2.5 7 4-14 2.5 7H22" />
      </svg>
    </div>
  );
}
