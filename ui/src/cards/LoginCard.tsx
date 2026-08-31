import { ExternalLink, Eye, EyeOff, ShieldCheck } from "lucide-react";
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
/**
 * The Cognito pool's minimum password length (`infra/stacks/auth_stack.py`).
 *
 * **8, not 12.** This file enforced 12 — a stale value the pool moved off on 2026-07-22, when the
 * frontend's `lib/password.ts` was updated and the agent was not. It rejected valid 8–11 character
 * passwords before they ever reached Cognito. Keep all three in step.
 */
const MIN_PASSWORD = 8;

export function LoginCard({
  onSignedIn,
  releasedByAdmin = false,
}: {
  onSignedIn: () => void;
  /**
   * IT released this device, so the core signed this person out mid-session.
   *
   * Without saying so, the sign-in screen is indistinguishable from a crash or an expired session,
   * and the employee's first thought is that they have lost the hours they were tracking. The two
   * facts that actually matter to them are that their work was saved and that signing back in is
   * the expected next step — so those are what this says.
   */
  releasedByAdmin?: boolean;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [reveal, setReveal] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** Set when Cognito demands a new password; carries the session that leg must echo back. */
  const [challenge, setChallenge] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");

  /** Set when Cognito demands a second factor; carries the session + which challenge to answer. */
  const [mfa, setMfa] = useState<{ session: string; challenge: string } | null>(null);
  const [code, setCode] = useState("");

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (pending) return;

    if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email.trim())) {
      setError("Enter the work email your organization issued you.");
      return;
    }
    if (!password) {
      setError("Enter your password.");
      return;
    }
    // **The invite code is not the password.** An invite email carries a 6-digit OTP
    // (`wp_platform::invites::new_otp`), and people reasonably read "the credentials in the email"
    // as meaning it. Cognito answers `NotAuthorizedException`, which used to render as "that email
    // and password didn't match" — sending them off to re-check a password they never set.
    //
    // This is decidable locally and costs no round-trip: the pool's minimum password length is 8
    // (infra/stacks/auth_stack.py), so six digits cannot be anyone's password.
    if (/^\d{6}$/.test(password)) {
      setError(
        "That looks like the 6-digit code from your invite email, which isn't your password. Open the invite link in a browser, finish setting up your account, then sign in here with the password you chose.",
      );
      return;
    }
    if (password.length < MIN_PASSWORD) {
      setError(`Passwords are at least ${MIN_PASSWORD} characters, so that one can't be right.`);
      return;
    }

    setError(null);
    setPending(true);
    try {
      // Trimmed, never case-folded. The Cognito username **is** the email exactly as the admin
      // typed it into the invite (`identity::shared::cognito` passes it verbatim to
      // `AdminCreateUser`), and the pool is case-sensitive, so lower-casing here would break a
      // mixed-case account. A stray space from a paste or an autocorrect, though, is never intended
      // and produces `UserNotFoundException` — which reads to the user as "wrong password".
      const status = await agent.login(email.trim(), password);
      if (status.newPasswordSession) {
        // Not an error: the account is real, it has just never had a password set.
        setChallenge(status.newPasswordSession);
        return;
      }
      if (status.mfaSession && status.mfaChallenge) {
        // Also not an error: the password was correct and a second factor is outstanding. Before
        // this branch existed the challenge fell through as `auth:unexpected_challenge`, so anyone
        // with TOTP enrolled was told their password was wrong.
        setMfa({ session: status.mfaSession, challenge: status.mfaChallenge });
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

  function openSite() {
    // Opens in the system browser (core command; the panel itself never navigates). Opening rarely
    // fails, but if the OS has no default browser, say where to go rather than swallow it.
    void agent.openWebsite().catch(() => {
      setError("Couldn't open your browser. Go to workpulse-ns.vercel.app to reach the WorkPulse web app.");
    });
  }

  async function onGoogle() {
    if (pending) return;
    setError(null);
    setPending(true);
    try {
      // Opens the OS browser; resolves once the `workpulse://callback` deep link returns to the app.
      // Same result shape as a password login, so `signedIn` is handled identically.
      const status = await agent.loginWithGoogle();
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

  async function onSubmitCode(e: FormEvent) {
    e.preventDefault();
    if (pending || !mfa) return;

    const trimmed = code.trim();
    if (!/^\d{6}$/.test(trimmed)) {
      setError("Enter the 6-digit code.");
      return;
    }

    setError(null);
    setPending(true);
    try {
      const status = await agent.completeMfa(mfa.challenge, email, trimmed, mfa.session);
      if (status.signedIn) {
        onSignedIn();
        return;
      }
      setError("Sign-in didn't complete. Try again.");
    } catch (e) {
      setError(explain(e));
      setCode("");
      // A Cognito MFA session is single-use and short-lived: once it is spent or expired the only
      // recovery is a fresh sign-in, so don't leave the user retyping codes against a dead session.
      if (/expired|NotAuthorized/i.test(String(e))) setMfa(null);
    } finally {
      setPending(false);
    }
  }

  async function onSetPassword(e: FormEvent) {
    e.preventDefault();
    if (pending || !challenge) return;

    if (newPassword.length < MIN_PASSWORD) {
      // Mirrors the Cognito pool policy — failing here beats a round-trip that comes back as an
      // opaque InvalidPasswordException.
      setError(`Choose a password of at least ${MIN_PASSWORD} characters.`);
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
    // Fills the panel rather than floating a narrow card in it. The window is a fixed 620×590, so a
    // 380px card left ~210px of dead width and a band of empty space top and bottom — on a surface
    // this small that reads as a page that failed to load rather than a deliberately airy one.
    //
    // The card now spans the panel and *grows* to take the leftover height (`flex-1`), with the form
    // centred inside it. So the whitespace ends up inside the card, where it looks like padding,
    // instead of around it, where it looked like a mistake.
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {releasedByAdmin && (
        <div
          role="status"
          className="mb-3 shrink-0 rounded-xl border border-primary/25 bg-primary/10 px-3 py-2.5 text-[12px] leading-snug text-foreground"
        >
          <p className="font-medium">Your administrator released this device</p>
          <p className="mt-0.5 text-muted-foreground">
            Your timer was stopped and the time you tracked has been saved. Sign in again to carry
            on — here or on your new laptop.
          </p>
        </div>
      )}
      {/* Brand lockup — the panel header is hidden behind this gate, so sign-in carries it. */}
      <div className="mb-3 flex shrink-0 items-center justify-center gap-2.5">
        <LogoMark className="size-9 shrink-0 rounded-[12px] shadow-sm" />
        <div className="leading-tight">
          <h1 className="font-heading text-[15px] font-semibold tracking-[0.2px]">WorkPulse</h1>
          <p className="text-[11px] text-muted-foreground/70">Time &amp; activity agent</p>
        </div>
      </div>

      {/* The card gives the form an edge to sit against — without it the inputs float on the
          panel background and the whole screen reads as unstyled. */}
      <div className="flex min-h-0 flex-1 flex-col justify-center rounded-2xl border border-border bg-card px-7 py-6 shadow-sm">
          <h2 className="font-heading text-[19px] font-semibold tracking-[0.2px]">
            {mfa ? "Two-step verification" : challenge ? "Choose a password" : "Sign in"}
          </h2>
          <p className="mt-1 text-[12.5px] leading-[1.45] text-muted-foreground">
            {mfa
              ? mfa.challenge === "SMS_MFA"
                ? "Enter the 6-digit code we texted you."
                : "Enter the 6-digit code from your authenticator app."
              : challenge
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

          {mfa ? (
            <form onSubmit={onSubmitCode} className="mt-5 space-y-4">
              <Field
                id="wp-mfa-code"
                label="Verification code"
                type="text"
                // `one-time-code` lets the OS offer the code from the Messages/Keychain autofill.
                autoComplete="one-time-code"
                inputMode="numeric"
                placeholder="123456"
                value={code}
                onChange={(v) => {
                  // Digits only, capped at six — stops a pasted "123 456" failing as malformed.
                  setCode(v.replace(/\D/g, "").slice(0, 6));
                  setError(null);
                }}
              />
              <Button type="submit" className="h-11 w-full text-[14px]" disabled={pending}>
                {pending ? "Verifying…" : "Verify and sign in"}
              </Button>
              <Button
                type="button"
                variant="outline"
                className="h-9 w-full text-[13px]"
                disabled={pending}
                onClick={() => {
                  setMfa(null);
                  setCode("");
                  setError(null);
                }}
              >
                Back to sign in
              </Button>
            </form>
          ) : challenge ? (
            <form onSubmit={onSetPassword} className="mt-5 space-y-4">
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
              <Button type="submit" className="h-11 w-full text-[14px]" disabled={pending}>
                {pending ? "Setting password…" : "Set password and sign in"}
              </Button>
            </form>
          ) : (
            <form onSubmit={onSubmit} className="mt-5 space-y-4">
              <Field
                id="wp-email"
                label="Work email"
                type="email"
                autoComplete="username"
                // macOS/iOS Tauri runs on **WKWebView**, which unlike WebView2 (Windows) and
                // WebKitGTK applies autocapitalize and autocorrect to text inputs by default. The
                // pool signs in case-sensitively on the email, so a capitalized first letter is a
                // failed sign-in that looks like a wrong password — and only ever on a Mac.
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
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
              <Button type="submit" className="h-11 w-full text-[14px]" disabled={pending}>
                {pending ? "Signing in…" : "Sign in"}
              </Button>

              <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
                <span className="h-px flex-1 bg-border" />
                or
                <span className="h-px flex-1 bg-border" />
              </div>

              {/* Opens the system browser for Google sign-in (native OAuth + PKCE), then returns
                  here signed in. Works for an existing account and a brand-new one. */}
              <Button
                type="button"
                variant="outline"
                className="h-11 w-full gap-2 text-[14px]"
                disabled={pending}
                onClick={onGoogle}
              >
                <GoogleGlyph />
                {pending ? "Opening browser…" : "Continue with Google"}
              </Button>
            </form>
          )}
      </div>

      {/* Compact footer: one line of assurance, plus the way out to the web app — the full dashboard
          and account settings live there, and the agent panel has no navigation of its own. */}
      <div className="mt-3 flex shrink-0 flex-col items-center gap-1.5 px-2 text-center text-[11px] leading-[1.45] text-muted-foreground/70">
        <p className="flex items-start justify-center gap-1.5">
          <ShieldCheck aria-hidden className="mt-[1px] size-3.5 shrink-0" />
          <span>Forgotten your password? Your workspace admin can reset it.</span>
        </p>
        <button
          type="button"
          onClick={openSite}
          className="inline-flex items-center gap-1 rounded underline-offset-2 transition-colors hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
        >
          Open the WorkPulse web app
          <ExternalLink aria-hidden className="size-3 shrink-0" />
        </button>
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

  // ── Google (Hosted UI) sign-in ────────────────────────────────────────────────────────────────
  // A brand-new Google account that belongs to no organization. Open self-signup admits it to the web
  // app (which routes to onboarding), but the agent has no org to track against — so say that plainly
  // rather than sign in to an empty session or show a bare `invalid_request`.
  if (raw.includes("auth:oauth:no_org"))
    return "This Google account isn't part of a WorkPulse organization yet. Create one at workpulse-ns.vercel.app, or ask your workspace admin to invite this email — then sign in here.";
  // The person closed the Google window or declined the consent prompt.
  if (raw.includes("access_denied"))
    return "Google sign-in was cancelled. Try again, and choose your work account when Google asks.";
  if (raw.includes("auth:oauth:state_mismatch"))
    return "Google sign-in couldn't be verified, so it was stopped for your safety. Please try again.";
  if (
    raw.includes("auth:oauth: sign-in was cancelled") ||
    raw.includes("auth:oauth: timed out") ||
    raw.includes("auth:oauth:no_code")
  )
    return "Google sign-in didn't finish. Try again — a browser window opens for you to pick your Google account, then brings you straight back here.";
  // Any other Hosted-UI/Cognito OAuth refusal arrives as `auth:oauth:<code>: <description>`. Show
  // Cognito's own sentence (after the code) when present — that names the real reason; the bare code
  // does not.
  const oauth = /auth:oauth:[a-z_]+(?::\s*(.+))?/i.exec(raw);
  if (oauth)
    return oauth[1]?.trim() || "Google sign-in couldn't be completed. Try again, or ask your workspace admin.";

  if (raw.includes("not_configured"))
    return "This agent isn't configured for your organization yet (missing Cognito client id). Contact your admin.";
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

  // An MFA challenge the agent can't answer — most likely `MFA_SETUP`, which needs an enrolment
  // round-trip we don't implement. Naming it beats "wrong password", which is what this used to say.
  const unsupported = /auth:unsupported_challenge:(\w+)/.exec(raw);
  if (unsupported)
    return `This account needs a sign-in step the agent doesn't support yet (${unsupported[1]}). Sign in on the web portal, or ask your admin.`;

  // `NotAuthorizedException` covers several very different situations and Cognito distinguishes them
  // ONLY in its `message`. Conflating them is what sent someone with an expired invite off to
  // re-check a password that was never going to work.
  if (raw.includes("NotAuthorized")) {
    if (/temporary password/i.test(raw))
      return "Your temporary password has expired. Ask your workspace admin to send a new invite.";
    if (/disabled/i.test(raw))
      return "This account is disabled. Contact your workspace admin.";
    return "That email and password didn't match. If you normally sign in with Google or Microsoft, your account has no password — use the web portal instead.";
  }
  // Distinct from the above: the address itself isn't in the directory.
  if (raw.includes("UserNotFound"))
    return "There is no account with this email.";

  // Second-factor answers (`complete_mfa`) surface here too — a wrong or stale 6-digit code.
  if (raw.includes("CodeMismatch"))
    return "That code didn't match. Check your authenticator app or text message and enter the current 6-digit code.";
  if (raw.includes("ExpiredCode"))
    return "That code has expired. Start sign-in again to get a fresh one.";

  // The service answered but with nothing usable (an empty result, or a body we couldn't read). Not
  // the user's doing — a plain retry is the right advice.
  if (raw.includes("auth:no_result") || raw.includes("auth:parse"))
    return "Sign-in didn't complete. Please try again in a moment.";
  // A gateway 5xx wears the `auth:cognito:<status>:…` shape — the sign-in service is briefly down.
  if (/auth:cognito:5\d\d/.test(raw))
    return "The sign-in service is temporarily unavailable. Try again in a minute.";

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
  inputMode,
  autoCapitalize,
  autoCorrect,
  spellCheck,
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
  /** Hints the on-screen keyboard — `numeric` for the MFA code. */
  inputMode?: "numeric" | "text";
  /** WKWebView (macOS) autocapitalizes by default; `none` for identifiers like an email. */
  autoCapitalize?: "none" | "sentences";
  autoCorrect?: "on" | "off";
  spellCheck?: boolean;
  reveal?: boolean;
  onToggleReveal?: () => void;
}) {
  return (
    <div className="space-y-1.5">
      <label htmlFor={id} className="block text-[12.5px] font-medium">
        {label}
      </label>
      <div className="relative">
        <Input
          id={id}
          type={type}
          autoComplete={autoComplete}
          inputMode={inputMode}
          autoCapitalize={autoCapitalize}
          autoCorrect={autoCorrect}
          spellCheck={spellCheck}
          placeholder={placeholder}
          className={onToggleReveal ? "h-11 px-3.5 pr-11 text-[13.5px]" : "h-11 px-3.5 text-[13.5px]"}
          value={value}
          onValueChange={onChange}
        />
        {onToggleReveal && (
          <button
            type="button"
            onClick={onToggleReveal}
            aria-label={reveal ? "Hide password" : "Show password"}
            aria-pressed={reveal}
            className="absolute inset-y-0 right-0 flex w-11 cursor-pointer items-center justify-center rounded-r-md text-muted-foreground/70 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            {reveal ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
          </button>
        )}
      </div>
    </div>
  );
}

/** The Google "G" mark — inline SVG, so it needs no icon dependency or bundled asset. */
function GoogleGlyph() {
  return (
    <svg viewBox="0 0 18 18" className="size-4 shrink-0" aria-hidden="true">
      <path fill="#4285F4" d="M17.64 9.2c0-.64-.06-1.25-.16-1.84H9v3.48h4.84a4.14 4.14 0 0 1-1.8 2.72v2.26h2.92c1.7-1.57 2.68-3.88 2.68-6.62z" />
      <path fill="#34A853" d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.92-2.26c-.8.54-1.84.86-3.04.86-2.34 0-4.32-1.58-5.03-3.7H.96v2.33A9 9 0 0 0 9 18z" />
      <path fill="#FBBC05" d="M3.97 10.72a5.4 5.4 0 0 1 0-3.44V4.95H.96a9 9 0 0 0 0 8.1l3.01-2.33z" />
      <path fill="#EA4335" d="M9 3.58c1.32 0 2.5.45 3.44 1.35l2.58-2.58C13.46.9 11.43 0 9 0A9 9 0 0 0 .96 4.95l3.01 2.33C4.68 5.16 6.66 3.58 9 3.58z" />
    </svg>
  );
}
