import { type FormEvent, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { core, type AuthStatus } from "@/lib/core";

/**
 * The sign-in gate. Cognito `USER_PASSWORD_AUTH`, including the NEW_PASSWORD_REQUIRED challenge
 * that admin-created accounts hit on first login (auth/cognito.rs).
 *
 * Ported from the previous UI's LoginForm — same two-step flow and the same reliance on the
 * core's stable `domain:reason` error strings, restyled into the panel's language. It sits at
 * the same altitude as ConsentCard: full-bleed, no card chrome, because it is the only thing
 * on screen when it shows.
 */
export function LoginCard({ onSignedIn }: { onSignedIn: (s: AuthStatus) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [session, setSession] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const s = session
        ? await core.authCompleteNewPassword(username, newPassword, session)
        : await core.authLogin(username, password);
      if (s.newPasswordSession) {
        setSession(s.newPasswordSession);
        setPassword("");
      } else if (s.signedIn) {
        onSignedIn(s);
      } else {
        setError("Sign-in failed.");
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col justify-center">
      <div className="mb-4">
        <h1 className="font-heading text-[15px] font-semibold tracking-[0.2px]">
          {session ? "Set a new password" : "Sign in to WorkPulse"}
        </h1>
        <p className="mt-1 text-muted-foreground">
          {session
            ? "Your account was created by an administrator — choose a password to continue."
            : "Use the account your organization set up for this device."}
        </p>
      </div>

      <form onSubmit={submit} className="flex flex-col gap-2.5">
        <Input
          type="email"
          autoComplete="username"
          placeholder="you@company.com"
          value={username}
          onChange={(e) => setUsername(e.currentTarget.value)}
          // The challenge is bound to the username already submitted; editing it would send
          // the new password against a session that doesn't match.
          readOnly={session !== null}
          required
        />
        {session ? (
          <Input
            type="password"
            autoComplete="new-password"
            placeholder="New password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.currentTarget.value)}
            required
          />
        ) : (
          <Input
            type="password"
            autoComplete="current-password"
            placeholder="Password"
            value={password}
            onChange={(e) => setPassword(e.currentTarget.value)}
            required
          />
        )}

        {error && (
          <p role="alert" className="text-[11px] text-destructive">
            {error}
          </p>
        )}

        <Button type="submit" className="mt-1 w-full" disabled={busy}>
          {busy ? "Signing in…" : session ? "Set password & continue" : "Sign in"}
        </Button>
      </form>
    </div>
  );
}
