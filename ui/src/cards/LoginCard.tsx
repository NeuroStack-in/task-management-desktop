import { Eye, EyeOff, ShieldCheck } from "lucide-react";
import { useState, type FormEvent } from "react";

import { LogoMark } from "@/components/shared/logo";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import * as agent from "@/lib/agent";

/**
 * The sign-in gate — the panel's first screen on a fresh install, ahead of consent.
 *
 * Wired to `auth_login` (Cognito USER_PASSWORD_AUTH — src-tauri/src/auth/cognito.rs). Tokens never
 * cross back into the webview: the core keeps them in the OS keyring and this screen only learns
 * whether it worked. An admin-created account comes back with `newPasswordSession` instead of a
 * session, which switches this card into its set-a-password second leg.
 */
export function LoginCard({ onSignedIn }: { onSignedIn: () => void }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [reveal, setReveal] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** Set when Cognito demands a new password; carries the session that leg must echo back. */
  const [challenge, setChallenge] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (pending) return;

    if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email)) {
      setError("Enter the work email your organization issued you.");
      return;
    }
    if (!password) {
      setError("Enter your password.");
      return;
    }

    setError(null);
    setPending(true);
    try {
      const status = await agent.login(email, password);
      if (status.newPasswordSession) {
        // Not an error: the account is real, it has just never had a password set.
        setChallenge(status.newPasswordSession);
        return;
      }
      if (status.signedIn) {
        onSignedIn();
        return;
      }
      setError("Sign-in didn't complete. Try again.");
    } catch (e) {
      setError(explain(e));
    } finally {
      setPending(false);
    }
  }

  async function onSetPassword(e: FormEvent) {
    e.preventDefault();
    if (pending || !challenge) return;

    if (newPassword.length < 12) {
      // Matches the Cognito pool policy (infra auth_stack.py) — failing here beats a round-trip
      // that comes back as an opaque InvalidPasswordException.
      setError("Choose a password of at least 12 characters.");
      return;
    }
    if (newPassword !== confirm) {
      setError("Those two passwords don't match.");
      return;
    }

    setError(null);
    setPending(true);
    try {
      const status = await agent.completeNewPassword(email, newPassword, challenge);
      if (status.signedIn) {
        onSignedIn();
        return;
      }
      setError("Password set, but sign-in didn't complete. Try signing in again.");
      setChallenge(null);
    } catch (e) {
      setError(explain(e));
      // A rejected password (policy) keeps the session usable; an expired one does not, and the
      // only recovery is a fresh sign-in. Drop back rather than looping on a dead session.
      if (String(e).includes("expired")) setChallenge(null);
    } finally {
      setPending(false);
    }
  }

  return (
    // One centred column rather than brand-top / form-middle / footer-bottom. The panel is a fixed
    // 620×590 and the form is short, so spreading those three apart left two dead bands and read as
    // an unfinished page; grouping them lets the surrounding space frame the card instead.
    <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden">
      {/* 380 keeps the inputs comfortably thumb-sized without stretching into web-form territory
          at this window width. */}
      <div className="w-full max-w-[380px]">
        {/* Brand lockup — the panel header is hidden behind this gate, so sign-in carries it. */}
        <div className="mb-5 flex flex-col items-center gap-2">
          <LogoMark className="size-11 shrink-0 rounded-[14px] shadow-sm" />
          <div className="text-center leading-tight">
            <h1 className="font-heading text-[15px] font-semibold tracking-[0.2px]">WorkPulse</h1>
            <p className="text-[11px] text-muted-foreground/70">Time &amp; activity agent</p>
          </div>
        </div>

        {/* The card gives the form an edge to sit against — without it the inputs float on the
            panel background and the whole screen reads as unstyled. */}
        <div className="rounded-2xl border border-border bg-card p-5 shadow-sm">
          <h2 className="font-heading text-[17px] font-semibold tracking-[0.2px]">
            {challenge ? "Choose a password" : "Sign in"}
          </h2>
          <p className="mt-0.5 text-[12px] leading-[1.45] text-muted-foreground">
            {challenge
              ? "Your account was created by an admin. Set a password to finish signing in."
              : "Use the work account your organization set up for you."}
          </p>

          {error && (
            <div
              role="alert"
              className="mt-3 flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-[12px]"
            >
              <span aria-hidden className="mt-[5px] size-[5px] shrink-0 rounded-full bg-destructive" />
              <span className="leading-[1.45] text-destructive">{error}</span>
            </div>
          )}

          {challenge ? (
            <form onSubmit={onSetPassword} className="mt-4 space-y-3">
              <Field
                id="wp-new-password"
                label="New password"
                type={reveal ? "text" : "password"}
                autoComplete="new-password"
                placeholder="At least 12 characters"
                value={newPassword}
                onChange={(v) => {
                  setNewPassword(v);
                  setError(null);
                }}
                reveal={reveal}
                onToggleReveal={() => setReveal((r) => !r)}
              />
              <Field
                id="wp-confirm-password"
                label="Confirm password"
                type={reveal ? "text" : "password"}
                autoComplete="new-password"
                value={confirm}
                onChange={(v) => {
                  setConfirm(v);
                  setError(null);
                }}
              />
              <Button type="submit" className="h-10 w-full text-[13px]" disabled={pending}>
                {pending ? "Setting password…" : "Set password and sign in"}
              </Button>
            </form>
          ) : (
            <form onSubmit={onSubmit} className="mt-4 space-y-3">
              <Field
                id="wp-email"
                label="Work email"
                type="email"
                autoComplete="username"
                placeholder="you@company.com"
                value={email}
                onChange={(v) => {
                  setEmail(v);
                  setError(null);
                }}
              />
              <Field
                id="wp-password"
                label="Password"
                type={reveal ? "text" : "password"}
                autoComplete="current-password"
                placeholder="••••••••"
                value={password}
                onChange={(v) => {
                  setPassword(v);
                  setError(null);
                }}
                reveal={reveal}
                onToggleReveal={() => setReveal((r) => !r)}
              />
              <Button type="submit" className="h-10 w-full text-[13px]" disabled={pending}>
                {pending ? "Signing in…" : "Sign in"}
              </Button>
            </form>
          )}
        </div>

        {/* Compact footer: one line of assurance, not a full-width rule plus two lines of prose.
            The shield carries the "managed device" meaning the divider label used to. */}
        <p className="mt-3.5 flex items-start justify-center gap-1.5 px-2 text-center text-[11px] leading-[1.45] text-muted-foreground/70">
          <ShieldCheck aria-hidden className="mt-[1px] size-3.5 shrink-0" />
          <span>
            Nothing is recorded until you sign in and accept the monitoring notice. Forgotten your
            password? Your workspace admin can reset it.
          </span>
        </p>
      </div>
    </div>
  );
}

/**
 * The core returns machine-readable `auth:*` / provider codes. Translate the ones a person can act
 * on, and pass anything else through rather than swallowing it behind a generic message — an
 * unrecognized code on screen is what makes a support ticket diagnosable.
 */
function explain(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e);
  if (raw.includes("not_configured"))
    return "This agent isn't configured for your organization yet (missing Cognito client id). Contact your admin.";
  if (raw.includes("NotAuthorized") || raw.includes("UserNotFound"))
    return "That email and password didn't match. Check them and try again.";
  if (raw.includes("UserNotConfirmed"))
    return "This account hasn't been confirmed yet. Ask your admin to finish setting it up.";
  if (raw.includes("PasswordResetRequired"))
    return "Your password needs resetting. Ask your workspace admin to reset it.";
  if (raw.includes("TooManyRequests") || raw.includes("LimitExceeded"))
    return "Too many attempts. Wait a minute and try again.";
  if (raw.includes("InvalidPassword"))
    return "That password doesn't meet your organization's policy. Try a longer one with mixed characters.";
  if (raw.includes("network") || raw.includes("timed out"))
    return "Couldn't reach the sign-in service. Check your connection and try again.";
  return raw;
}

function Field({
  id,
  label,
  type,
  value,
  onChange,
  autoComplete,
  placeholder,
  reveal,
  onToggleReveal,
}: {
  id: string;
  label: string;
  type: string;
  value: string;
  onChange: (v: string) => void;
  autoComplete?: string;
  placeholder?: string;
  reveal?: boolean;
  onToggleReveal?: () => void;
}) {
  return (
    <div className="space-y-1">
      <label htmlFor={id} className="block text-[12px] font-medium">
        {label}
      </label>
      <div className="relative">
        <Input
          id={id}
          type={type}
          autoComplete={autoComplete}
          placeholder={placeholder}
          className={onToggleReveal ? "h-10 px-3 pr-10 text-[13px]" : "h-10 px-3 text-[13px]"}
          value={value}
          onValueChange={onChange}
        />
        {onToggleReveal && (
          <button
            type="button"
            onClick={onToggleReveal}
            aria-label={reveal ? "Hide password" : "Show password"}
            aria-pressed={reveal}
            className="absolute inset-y-0 right-0 flex w-10 cursor-pointer items-center justify-center rounded-r-md text-muted-foreground/70 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            {reveal ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
          </button>
        )}
      </div>
    </div>
  );
}
