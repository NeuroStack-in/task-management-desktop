import { useState } from "preact/hooks";
import { cn } from "../lib/cn";
import { friendlyError } from "../lib/errors";
import { ipc } from "../lib/ipc";
import type { AuthStatus } from "../lib/types";
import { useTheme } from "../lib/useTheme";
import { WorkPulseLogo } from "./Logo";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";
import { Label } from "./ui/Label";

// Ported from TaskFlow Desktop's LoginForm (the reference agent) — the polished sign-in surface:
// asymmetric brand bar, textured backdrop, staggered mount, theme toggle, and the Cognito
// NEW_PASSWORD_REQUIRED challenge. Wired to WorkPulse's Tauri auth commands (USER_PASSWORD_AUTH).
export function LoginForm({ onSuccess }: { onSuccess: (s: AuthStatus) => void }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  // The Cognito challenge session — held on the frontend to complete the new-password flow.
  const [session, setSession] = useState<string | null>(null);
  const { isDark, toggle } = useTheme();

  async function handleLogin(e: Event) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const status = await ipc.authLogin(email, password);
      setPassword(""); // drop the plaintext as soon as the call returns
      if (status.newPasswordSession) {
        setSession(status.newPasswordSession);
        setLoading(false);
        return;
      }
      if (status.signedIn) {
        onSuccess(status);
      } else {
        setError("Sign-in failed.");
      }
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setLoading(false);
    }
  }

  async function handleNewPassword(e: Event) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const status = await ipc.authCompleteNewPassword(email, newPassword, session ?? "");
      setPassword("");
      setNewPassword("");
      if (status.signedIn) onSuccess(status);
      else setError("Could not set the password.");
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setLoading(false);
    }
  }

  const challengePending = session !== null;

  return (
    <div class="relative flex h-full flex-col overflow-hidden bg-background">
      {/* Background atmosphere: pinstripe texture + a primary halo. */}
      <div
        class="pointer-events-none absolute inset-0 z-0 opacity-[0.55]"
        aria-hidden="true"
        style={{
          backgroundImage:
            "repeating-linear-gradient(135deg, hsl(var(--foreground) / 0.02) 0 1px, transparent 1px 9px)",
        }}
      />
      <div
        class="pointer-events-none absolute -top-32 left-1/2 h-[420px] w-[520px] -translate-x-1/2 rounded-full bg-primary/[0.07] blur-3xl"
        aria-hidden="true"
      />

      {/* Top bar: brand on the left, theme toggle on the right. */}
      <header
        class="login-anim relative z-10 flex items-center justify-between gap-3 px-4 py-3"
        style={{ animationDelay: "0ms" }}
      >
        <div class="flex items-center gap-2.5">
          <WorkPulseLogo size={26} />
          <h1 class="text-[14px] font-extrabold leading-none tracking-[-0.015em] text-foreground">
            Work<span class="text-primary">Pulse</span>
          </h1>
        </div>
        <Button
          variant="ghost"
          size="icon"
          class="h-7 w-7 text-muted-foreground hover:bg-accent/60 hover:text-foreground"
          onClick={toggle}
          aria-label={isDark ? "Switch to light mode" : "Switch to dark mode"}
          title={isDark ? "Light mode" : "Dark mode"}
        >
          {isDark ? <SunIcon /> : <MoonIcon />}
        </Button>
      </header>

      <div class="relative z-10 flex flex-1 flex-col items-center justify-center px-6 pb-3">
        <div class="w-full max-w-[360px]">
          <div class="login-anim mb-5" style={{ animationDelay: "80ms" }}>
            <h2 class="text-[22px] font-extrabold leading-[1.05] tracking-[-0.02em] text-foreground">
              {challengePending ? "Set a new password" : "Sign in to WorkPulse"}
            </h2>
            <p class="mt-1.5 text-[12px] leading-snug text-muted-foreground">
              {challengePending
                ? "First-time login — choose a password to continue."
                : "Track focused time, surface daily progress, ship more."}
            </p>
          </div>

          <div class="login-anim" style={{ animationDelay: "200ms" }}>
            {challengePending ? (
              <form onSubmit={handleNewPassword} class="space-y-3.5">
                <FieldRow delay={260}>
                  <Label
                    htmlFor="newpw"
                    class="text-[10.5px] font-semibold uppercase tracking-[0.1em] text-muted-foreground"
                  >
                    New password
                  </Label>
                  <FocusInput
                    id="newpw"
                    type="password"
                    placeholder="At least 8 characters"
                    value={newPassword}
                    onInput={(e: Event) => setNewPassword((e.target as HTMLInputElement).value)}
                    required
                    minLength={8}
                    autoFocus
                  />
                </FieldRow>
                {error && <ErrorBox msg={error} />}
                <SubmitButton loading={loading} label="Set password & continue" delay={340} />
              </form>
            ) : (
              <form onSubmit={handleLogin} class="space-y-3.5">
                <FieldRow delay={260}>
                  <Label
                    htmlFor="identifier"
                    class="text-[10.5px] font-semibold uppercase tracking-[0.1em] text-muted-foreground"
                  >
                    Email
                  </Label>
                  <FocusInput
                    id="identifier"
                    type="email"
                    placeholder="you@company.com"
                    value={email}
                    onInput={(e: Event) => setEmail((e.target as HTMLInputElement).value)}
                    required
                    autoFocus
                  />
                </FieldRow>
                <FieldRow delay={320}>
                  <Label
                    htmlFor="password"
                    class="text-[10.5px] font-semibold uppercase tracking-[0.1em] text-muted-foreground"
                  >
                    Password
                  </Label>
                  <FocusInput
                    id="password"
                    type="password"
                    placeholder="Enter your password"
                    value={password}
                    onInput={(e: Event) => setPassword((e.target as HTMLInputElement).value)}
                    required
                  />
                </FieldRow>
                {error && <ErrorBox msg={error} />}
                <SubmitButton loading={loading} label="Sign in" delay={380} />
              </form>
            )}
          </div>

          <div
            class="login-anim mt-5 flex items-center gap-2 border-t border-border/50 pt-3 text-[10px] text-muted-foreground/85"
            style={{ animationDelay: "440ms" }}
          >
            <svg
              class="h-3 w-3 text-primary/70"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2.2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <rect x="3" y="11" width="18" height="11" rx="2" />
              <path d="M7 11V7a5 5 0 0 1 10 0v4" />
            </svg>
            <span>Cognito authentication · TLS 1.3</span>
          </div>
        </div>
      </div>

      <footer
        class="login-anim relative z-10 flex items-center justify-between px-4 py-2.5 text-[9.5px] font-medium uppercase tracking-[0.06em] text-muted-foreground/85"
        style={{ animationDelay: "500ms" }}
      >
        <span class="font-mono tabular-nums">NeuroStack · MMXXVI</span>
        <span class="font-mono tabular-nums text-muted-foreground/65">WorkPulse Agent</span>
      </footer>

      <style>{`
        @keyframes login-rise {
          from { opacity: 0; transform: translateY(6px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        .login-anim { opacity: 0; animation: login-rise 0.5s cubic-bezier(.2,.7,.2,1) forwards; }
        .focus-input { position: relative; }
        .focus-input::after {
          content: ""; position: absolute; left: 0; bottom: 0; height: 2px; width: 0;
          background: hsl(var(--primary));
          transition: width 0.28s cubic-bezier(.2,.7,.2,1);
          pointer-events: none;
          border-bottom-left-radius: var(--radius); border-bottom-right-radius: var(--radius);
        }
        .focus-input:focus-within::after { width: 100%; }
      `}</style>
    </div>
  );
}

// ─── composition helpers ──────────────────────────────────────
function FieldRow({ delay, children }: { delay: number; children: any }) {
  return (
    <div class="login-anim space-y-1" style={{ animationDelay: `${delay}ms` }}>
      {children}
    </div>
  );
}

function FocusInput(props: any) {
  return (
    <span class="focus-input block">
      <Input {...props} class="bg-background/60 backdrop-blur-[1px]" />
    </span>
  );
}

function SubmitButton({ loading, label, delay }: { loading: boolean; label: string; delay: number }) {
  return (
    <div class="login-anim pt-1" style={{ animationDelay: `${delay}ms` }}>
      <Button
        type="submit"
        class={cn("group h-10 w-full gap-2 font-semibold tracking-[-0.005em] shadow-sm hover:shadow")}
        disabled={loading}
      >
        {loading ? (
          <span class="opacity-90">Working…</span>
        ) : (
          <>
            <span>{label}</span>
            <svg
              class="h-3 w-3 transition-transform group-hover:translate-x-0.5"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2.4"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M5 12h14M13 5l7 7-7 7" />
            </svg>
          </>
        )}
      </Button>
    </div>
  );
}

function ErrorBox({ msg }: { msg: string }) {
  return (
    <div
      role="alert"
      class="flex items-start gap-2 rounded-md border border-destructive/25 bg-destructive/[0.07] px-2.5 py-2 text-[11.5px] leading-snug text-destructive"
    >
      <svg
        class="mt-px h-3.5 w-3.5 flex-shrink-0"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="10" />
        <path d="M12 8v4M12 16h.01" />
      </svg>
      <span class="font-medium">{msg}</span>
    </div>
  );
}

function SunIcon() {
  return (
    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <circle cx="12" cy="12" r="5" />
      <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z" />
    </svg>
  );
}
