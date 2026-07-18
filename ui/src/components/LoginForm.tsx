import { useState } from "preact/hooks";
import { ipc } from "../lib/ipc";
import type { AuthStatus } from "../lib/types";

// Cognito USER_PASSWORD_AUTH, incl. the NEW_PASSWORD_REQUIRED challenge for admin-created accounts
// (BUILD-PLAN §3). Errors are the core's stable `domain:reason` strings.
export function LoginForm({ onSignedIn }: { onSignedIn: (s: AuthStatus) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [session, setSession] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: Event) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const s = session
        ? await ipc.authCompleteNewPassword(username, newPassword, session)
        : await ipc.authLogin(username, password);
      if (s.newPasswordSession) {
        setSession(s.newPasswordSession);
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
    <form onSubmit={submit} class="flex flex-col gap-3">
      <h1 class="text-lg font-semibold">Sign in to WorkPulse</h1>
      <input
        type="email"
        placeholder="you@company.com"
        value={username}
        onInput={(e) => setUsername((e.target as HTMLInputElement).value)}
        class="rounded-md bg-slate-800 px-3 py-2 text-sm outline-none placeholder:text-slate-500"
        required
      />
      {session ? (
        <input
          type="password"
          placeholder="Set a new password"
          value={newPassword}
          onInput={(e) => setNewPassword((e.target as HTMLInputElement).value)}
          class="rounded-md bg-slate-800 px-3 py-2 text-sm outline-none placeholder:text-slate-500"
          required
        />
      ) : (
        <input
          type="password"
          placeholder="Password"
          value={password}
          onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
          class="rounded-md bg-slate-800 px-3 py-2 text-sm outline-none placeholder:text-slate-500"
          required
        />
      )}
      {error && <p class="text-xs text-rose-400">{error}</p>}
      <button
        type="submit"
        disabled={busy}
        class="rounded-md bg-teal-600 px-4 py-2 text-sm font-medium hover:bg-teal-500 disabled:opacity-50"
      >
        {busy ? "…" : session ? "Set password & continue" : "Sign in"}
      </button>
    </form>
  );
}
