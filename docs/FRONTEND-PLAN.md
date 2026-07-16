# FRONTEND-PLAN.md — the desktop agent's UI

> **Status:** Written 2026-07-16.
> **Scope:** the agent's **frontend only** — `ui/` (Preact webview). The Rust core, the wire, and the
> milestones live in [`BUILD-PLAN.md`](BUILD-PLAN.md); the architecture in [`AGENT.md`](AGENT.md).
> **Model:** [`REFERENCE-TASKFLOW.md`](REFERENCE-TASKFLOW.md) — TaskFlow Desktop's UI (~5,150 L
> TS/TSX), shipping. We port what works and add what it lacks.
> **Binding:** [`PRIVACY.md`](PRIVACY.md) is not advisory here — it dictates three surfaces.

## The shape of it

A **companion widget, not a dashboard.** 500×600, max 550×650. Two top-level screens:

```
LoginForm  ──(authenticated)──▶  TimerView
                                   ├─ ProjectTaskSelector  (NEW — project → task, + description)
                                   ├─ Timer                (ticks SERVER time)
                                   ├─ Today's sessions     (grouped by task) → SessionInspect
                                   ├─ IdlePrompt           ("still working?")
                                   ├─ SettingsDrawer · AvatarMenu · ShortcutHelp
                                   ├─ ConsentGate          (NEW — blocks everything until acknowledged)
                                   ├─ CaptureIndicator     (NEW — "you are being recorded")
                                   └─ TransparencyView     (NEW — "what is being collected")
```

**Stack:** Preact ^10.22 · Vite ^5.3 · Tailwind ^3.4 · TypeScript. **No state library** — `useState`
plus a `useSyncExternalStore` store over `localStorage` (`lib/settings.ts`,
port as-is). No router: two screens and overlays.

*(The sample's README says "React". It is **Preact**. Match the sample — the compat surface is fine
and the bundle is smaller.)*

---

## 1. The three surfaces TaskFlow does **not** have

This is the plan's headline. TaskFlow's privacy model is *"monitoring only while the timer runs"* —
which we adopt — but **our [`PRIVACY.md`](PRIVACY.md) requires more**, and none of it exists in the
sample. Verified: no consent, indicator, or pause component in `desktop-rust/src/components/`.

| Surface | What PRIVACY.md demands | Status |
|---|---|---|
| **ConsentGate** | *"No capture without an active consent policy… consent text shown at first run and from the tray."* Silent mode *"suppresses active prompts but **not** the consent requirement."* | **Build new** |
| **CaptureIndicator** | *"Notify employees monitoring is active (default on)"* · *"Visible tray indicator reflecting active/idle/paused"* | **Build new** |
| **TransparencyView** | *"Even silent, the transparency view remains accessible"* — a "what is being collected" panel | **Build new** |

**Salvage the disclosure text.** The old `tray/` is being deleted, but its one piece of real content
survives — `tray/src-tauri/src/features/consent.rs::disclosure()`:

```
Activity counts (keystroke & mouse totals — never the keys themselves)
Foreground app / website category
Periodic screenshots (blurred per your organization's policy)
Attendance and timer events
```

Carry it verbatim into `ConsentGate` + `TransparencyView`. It is accurate, plain, and already written.

**`silent` inverts the indicator, nothing else.** `TrackingConfig.silent` (default `false`) suppresses
the CaptureIndicator — that's an **admin policy on a privacy surface**, so get it exactly right:
silent hides the indicator; it never disables consent, and never hides TransparencyView.

**Pause is subsumed — don't build it.** The old tray had a `pause/` slice because capture ran on a
cadence independent of the user. With **timer-gated capture, stopping the timer *is* the pause** —
there is nothing to pause. One fewer surface, and a stronger story: the stop button is the privacy
control. (The indicator still needs an idle state; that comes from the core, not a pause button.)

> ⚠️ **Open question for the backend — flag it, don't invent it.** `TrackingConfig` is
> `{version, cadence, blur_level, retention_days, silent}` — **there is no consent field**, yet
> `silent`'s own doc comment says *"still consent-gated"* and PRIVACY.md hard-gates capture on an org
> consent policy. So the policy the gate is supposed to read **has no home in the contract**. Three
> ways out: (a) add `consent_required: bool` to `TrackingConfig`; (b) treat starting the timer as
> consent-in-action and reduce the gate to a first-run disclosure the user acknowledges locally; or
> (c) decide consent is an org-onboarding concern and the agent only *displays* it. **This needs a
> decision before ConsentGate is built** — it's the difference between a blocking gate and a notice.

## 2. Port map — what comes from the sample

| Component | Sample | Plan |
|---|---|---|
| `LoginForm.tsx` | 439 L, incl. the **NEW_PASSWORD_REQUIRED** challenge | **Port.** Rewrite calls to `invoke`. The challenge screen is real Cognito behaviour — keep it. |
| `TimerView.tsx` | **1348 L** | **Port + decompose.** 1348 L in one file is not acceptable. Split: shell · today's-sessions · the hooks. This is the biggest single piece of work. |
| `TaskSelector.tsx` | 725 L, **task-only** | **Rewrite as `ProjectTaskSelector`** (§3). Port the mandatory-description field + history autocomplete wholesale — that part is good. |
| `Timer.tsx`, `SessionInspect.tsx` (182 L), `IdlePrompt.tsx`, `ShortcutHelp.tsx`, `Logo.tsx` | | **Port.** |
| `SettingsDrawer.tsx` (428 L), `AvatarMenu.tsx` (355 L) | | **Port**, then add the TransparencyView entry point. |
| `ui/{Badge,Button,Card,Input,Label}.tsx` | | **Port.** Small, unopinionated. |
| `lib/settings.ts` | `useSyncExternalStore` over localStorage | **Port as-is.** |
| `lib/serverClock.ts` | | **Port — load-bearing** (§4). |
| `lib/{useTheme,useShortcuts,notify,errors,cn,projectColor,taskHistory,descriptionHistory}.ts` | | **Port.** |
| **`lib/tauri-bridge.ts`** | Wails-compat shim | **🚫 Delete — do not port** (§5). |

## 3. `ProjectTaskSelector` — the one genuinely new screen

The sample's picker is **task-only**; we need **project → task**.

**Good news, verified:** the data supports it already. The sample's `Task` carries `project_id` +
`project_name`, and **our `TimerStarted` already carries both `task_id` and `project_id`.** So this is
a UI tier, not a data-model change.

**Bad news, verified:** the backend can't serve titles yet. `my_tasks` returns
`{id, project_id, status, due}` — **no title** (GSI1 is KEYS_ONLY-adjacent). A picker rendering
`"p1 / k1"` is unusable. This is **blocked on `GET /v1/agent/tasks`**, planned in
[`../../backend/docs/AGENT-SUPPORT-PLAN.md`](../../backend/docs/AGENT-SUPPORT-PLAN.md) §2.

**Shape:** a two-stage combobox over the cached task list, grouped by project. Keyboard-first
(the sample's shortcut layer already exists). Fed by an ETag-cached pull on open — not per-cycle.

**Mandatory description** (ported): **Start stays disabled until it's non-empty.** That's the sample's
rule and it's the right one — it's what makes a `TimeEntry` legible later. Note the contract currently
**drops** `description` (`TimerStarted` has no such field, so the fold writes `""`) — the field lands
with the contract PR. **Don't build the UI around a field the wire silently discards**: this component
is gated on that PR.

**Meeting mode** (ported): start with no task. Requires `task_id`/`project_id` → `Option` — same PR.

## 3b. macOS: a "grant permission" state — a fourth new surface

macOS requires **two separate runtime grants**: **Screen Recording (TCC)** for capture and
**Accessibility** for input counts + window titles. **No crate avoids them**, and the sample
**has no UX for denial** — it's their open item #1. Inherit the code and you inherit an agent that,
on a denied Mac, **silently collects nothing and says nothing**.

Build it: the core detects denial and reports it (same DTO discipline as §5 — with a field-name
test); the UI renders a **blocking, actionable** "grant permission" state with a deep-link to the
right System Settings pane. Two grants ⇒ **two independent states** — Accessibility can be denied
while Screen Recording is granted, and the agent then collects screenshots but no activity.

This pairs with the Wayland banner (§5): both are *honest degradation* surfaces, and both are the
difference between "gathering nothing" and "lying about gathering nothing".

## 4. Two rules worth stating plainly

**The timer ticks server time, not the local clock.** Port `lib/serverClock.ts`. A device with a
skewed clock must not invent hours. The sample also applies an optimistic timestamp covering the
click→server gap — keep that; it's why the UI feels instant without lying.

**Stop has a 5-second Undo.** Ported. It's the difference between a misclick and a lost session, and
time is **immutable with no corrections** (LLD §4) — there is no "fix it later". The Undo *is* the
correction mechanism.

## 5. IPC — delete the shim, type the boundary

**Delete `lib/tauri-bridge.ts`.** It installs fake Wails globals (`window.go.main.App`,
`window.runtime`) and deep snake↔camel transforms — a fossil of TaskFlow's Go/Wails origin, kept so
its UI could be ported unchanged. **We have no such constraint.**

Replace with **`lib/ipc.ts`**: thin typed wrappers over `invoke<T>()` / `listen()`. Give every Rust
DTO `#[serde(rename_all = "camelCase")]` so **no transform layer exists at all**.

### The bug class this prevents — and the guard that's mandatory

The sample's `monitor/session.rs` declares `can_track_windows` / `display_server` / `limitation` under
`#[serde(rename_all = "camelCase")]`, so it **serializes** `canTrackWindows` / `displayServer` /
`limitation`. But `TimerView.tsx:269-270` reads `s.limitationMessage` and `s.sessionType` — the **old
Go shape**. Both are `undefined`, so the guard returns early and **its Wayland banner can never fire.**
Silent, permanent, shipped.

**Neither compiler can see this, and neither can you.** Rust doesn't know the TS; TS doesn't know the
Rust; and the shim's snake→camel transform doesn't bridge a *rename*. Worse — **you cannot even grep
for it**: the serialized name `canTrackWindows` appears **nowhere in the source**, because
`rename_all` generates it at compile time. Searching the Rust for the name the TS expects finds
nothing, and finds nothing whether the code is right or wrong.

That is exactly why the guard must assert the **serialized output**, not the source.

**The guard, and it is not optional:** every DTO the UI reads gets a Rust `#[test]` asserting its
**serialized field names**, mirrored by a hand-checked TS type. Cheap, and it's the only thing standing
between us and the same bug.

## 6. Events the UI listens for

The sample's six, adapted: `attendance:updated`, `network:error`, `network:restored`,
`update:available`, `update:package-managed`, `auth:expired`.

Ours adds the ones the new surfaces need: **capture-state changes** (drives CaptureIndicator) and
**config changes** (a live `silent`/`cadence` swap must reach the UI without a restart — the config
rail applies live, so the UI must too).

**One design call, carried from the audit:** the sample's `index.html` polls **6 read commands every
1 second**. Don't make that six IPC round-trips per second. The core exposes **one `TrayState`**; the UI
runs a single background task refreshing a cached snapshot at 1 Hz, and every component reads the
cache. One round-trip/sec.

## 7. Sequence

Frontend work is gated on the Rust side; these map onto [`BUILD-PLAN.md`](BUILD-PLAN.md)'s milestones.

| Step | Depends on | Deliver |
|---|---|---|
| **F1** | M0 | Scaffold `ui/` — Vite + Preact + Tailwind + TS, `lib/ipc.ts`, the `ui/*` primitives. **Verify:** builds, and CI builds it (the old tray never was). |
| **F2** | M1 (auth) | `LoginForm` + the NEW_PASSWORD_REQUIRED challenge. **Verify:** real login as `owner@acme.test` against the live pool. |
| **F3** | — | **ConsentGate + TransparencyView + CaptureIndicator** (§1). *Gated on the consent-contract decision.* **Verify:** `silent: true` hides the indicator and **nothing else**; TransparencyView still reachable. |
| **F4** | **M3a contract PR** | `ProjectTaskSelector` + mandatory description + meeting mode. **Verify:** the request body carries `description` — not `""`. |
| **F5** | M4 | `TimerView` (decomposed) + serverClock + 5 s Undo + today's sessions + `SessionInspect`. **Verify:** cross-midnight session renders on the start date. |
| **F6** | M4/M5 | `IdlePrompt` (5 min) + hard auto-stop (15 min) + `SettingsDrawer` + `AvatarMenu` + `ShortcutHelp`. **Verify:** "keep tracking" suppresses the prompt but **does not** exempt the 15-min auto-stop. |

## 8. Risks

1. **Porting ≠ copying.** `TimerView.tsx` is 1348 L with shim assumptions baked in; `TaskSelector.tsx`
   is 725 L and task-only. The invoke rewrite + the project tier + the decomposition is **real work** —
   budget it as such, not as a file copy.
2. **F4 is hard-blocked** on the contract PR. Building the selector against a wire that discards
   `description` produces a UI that looks right and silently loses data — exactly the failure this
   repo keeps hitting.
3. **The DTO rename bug is invisible.** Only the field-name tests catch it. Skip them and we ship the
   sample's dead banner.
4. **`silent` is an admin policy on a privacy surface.** Getting it wrong is not a cosmetic bug.
5. **Consent has no contract home** (§1). Do not invent a policy field in the UI — resolve it first.
6. **The sample has zero tests.** Whatever we port arrives untested. The field-name tests are the floor,
   not the goal.
