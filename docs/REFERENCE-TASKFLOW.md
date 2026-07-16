# REFERENCE-TASKFLOW.md — everything worth keeping from the sample

> **Why this file exists:** `desktop-rust/` (TaskFlow Desktop) was the working reference our agent is
> modelled on. **The local folder is being deleted.** This is the extract — the knowledge that was
> expensive to derive, plus the provenance to get the code back.
>
> **The code is NOT lost.** It is fully pushed and clean:
>
> ```
> repo: https://github.com/Giridharan0624/taskflow-desktop-rust.git
> ref:  021af7f8195a08e5b8e3594c5551502c78385460   (tag v0.1.4, branch main)
> state at extraction: 0 unpushed commits, 0 uncommitted files
> ```
>
> **[`BUILD-PLAN.md`](BUILD-PLAN.md) and [`FRONTEND-PLAN.md`](FRONTEND-PLAN.md) say "port
> `TimerView.tsx`", "port `LoginForm.tsx`" — ~5,150 lines of TS.** Those instructions are still
> executable: `git clone` the repo above at that ref. Do that *before* starting the frontend work.
>
> What it was: a complete, shipping Tauri v2 + Preact activity-monitoring agent — the same product
> category as ours. ~3,160 L Rust · ~5,150 L TS · CI-released · **zero tests**.

---

## 1. Corrections this extract forces on our plans

I planned from a summary. Reading the source docs corrected four things. **These are the highest-value
lines in this file.**

### 🔴 Auth is NOT SRP, and needs no `aws-sdk`

Our plans said *"Cognito SRP via `aws-sdk-cognitoidentityprovider`"*. **Wrong.** TaskFlow (TDD §6)
hand-rolls **three unauthenticated JSON POSTs over `reqwest`** — *"No SRP, no aws-sdk."*

- `InitiateAuth` (**USER_PASSWORD_AUTH**)
- `RespondToAuthChallenge` (**NEW_PASSWORD_REQUIRED**)
- `InitiateAuth` (**REFRESH_TOKEN_AUTH**)

Simpler, and it drops a heavy dependency. Take it.

### 🔴 Wayland — I had it exactly backwards

`BUILD-PLAN.md` risk #4 said *"Linux may honestly ship activity-only."* **The inverse is true:**

| On Wayland | Reality |
|---|---|
| `user-idle` → idle secs | ✗ **returns 0** |
| `device_query` → input counts | ✗ **returns 0** |
| `x-win` → active app | ⚠ partial |
| `xcap` → screenshots | ⚠ **works** (portal prompt) |

Their words: *"no global input API and restricted window access for **any** library. Input counters
and per-app tracking **legitimately can't work**"* — an **OS limit, not a crate gap**. So
Linux/Wayland ships **screenshots-only**.

**And this is why the session DTO bug matters.** `session_info()` reports the degradation via
`canTrackWindows: false` *"so the UI can degrade honestly"* — and that is **the exact DTO whose
consumer is broken** (§4). The one mechanism that tells a Wayland user "we can't track your activity"
is silently dead.

### 🟠 macOS needs a permissions UX that does not exist

**Two separate runtime grants**, and no crate avoids them:

- **Screen Recording (TCC)** → screenshots
- **Accessibility** → input counts **and** window titles

TaskFlow's own **open item #1**: *"detect Accessibility / Screen-Recording denial, gate
`monitor::start`, and surface a 'grant permission' hint in the UI."* **Unbuilt.** Without it, a denied
Mac silently collects nothing. **We inherit this gap** — neither of our plans mentions it.

### 🟠 Screenshots are **primary display only**

Our `CAPTURE.md` lineage said "captures all displays". TaskFlow captures the **primary display only**
(TDD §14). Decide deliberately; don't inherit it by accident.

---

## 2. The monitor — the design worth copying

**Started only while the timer runs**; idempotent start/stop bound to session state.

**Thread model (TDD §3) — the reason it's a thread, not a task:**
> *"Win32/X11/CG handles are blocking and often `!Send`, so they stay off the async reactor… `!Send`
> platform handles never cross an `await`; only `Send` snapshots (counts, JPEG bytes) move between the
> sampler and the async loops via a shared `Arc<Mutex<Bucket>>`."*

- **Sampling thread — 1 s, dedicated OS thread.** idle (`user-idle`); input deltas (`device_query`,
  rising-edge + cursor-movement, **uint32 wrap + spike cap**); every 5th tick the active app (`x-win`).
- **Heartbeat task — 5 min, tokio.** Drains the offline backlog **first**, then snapshot+reset the
  bucket and POST. Gated by `activity_monitoring` (**fail-open**).
- **Screenshot task — jittered, tokio.** Gated by `screenshots` (**fail-closed**); skips when locked;
  `xcap` on a **blocking task**; backlog on failure.
- **Network status:** one shared flag drives a single `network:error`/`network:restored` pair across
  both loops (no duplicate notifications).

**Constants:**

| Constant | Value |
|---|---|
| `WINDOW_SAMPLE_EVERY` | `5` (ticks → 5 s) |
| `MAX_APPS_PER_BUCKET` | `30` |
| `HEARTBEAT_SECS` | `300` |
| `SCREENSHOT_BASE_SECS` / `_JITTER_SECS` | `540` / `60` (anti-evasion) |
| `IDLE_THRESHOLD_SECS` | `2` |
| `SPIKE_CAP` | `1000` |

## 3. Auth — the details that bite

- **Keyring storage is base64 + *chunked*** (`key.0..N` + a count) **to clear the Windows Credential
  Manager blob-size limit.** A single-blob write fails at runtime, not compile time. (TDD §6)
- **Singleflight refresh for free:** tokens + the challenge session live behind **one
  `tokio::sync::Mutex`**; `valid_id_token()` refreshes on expiry and the lock coalesces concurrent
  callers. Refresh failure → clear session → `Unauthorized` → `auth:expired`.
- **The challenge session and refresh token never cross the IPC boundary.**
- Presigned PUT is **retried once on a 403** (expired presign).
- Two `reqwest` clients: 30 s API, 180 s uploads.
- `Attendance` derives `Default`, so a `null` "no record today" maps to an empty SIGNED_OUT state
  rather than an error.

## 4. The bug that must not be copied

`monitor/session.rs` declares `can_track_windows` / `display_server` / `limitation` under
`#[serde(rename_all = "camelCase")]` → serializes **`canTrackWindows` / `displayServer` /
`limitation`**. `TimerView.tsx:269-270` reads **`limitationMessage` / `sessionType`** (the old Go
shape). Both `undefined` → early return → **the Wayland banner can never fire.**

**You cannot grep for this.** `canTrackWindows` appears **nowhere in the source** — `rename_all`
generates it at compile time. The search looks identical whether the code is right or wrong.

**Guard:** a Rust `#[test]` asserting the **serialized field names** of every DTO the UI reads.

Second defect: `updater/mod.rs` `GITHUB_REPO = "Giridharan0624/taskflow-desktop"` — the **legacy Go
repo**, not `-rust`. Their updater polls the wrong releases.

## 5. Security patterns to carry

1. **Upload-host pinning** — validate `https` + `amazonaws.com` **before any pixel data is PUT**, so a
   compromised backend can't redirect a frame of the user's screen.
2. **Fail-closed updates** — Ed25519 over `SHA256SUMS`; release builds **refuse unsigned** and won't
   even *advertise* an update without a pubkey.
3. **Dashboard-URL sanitization** — http(s) only, no userinfo, before it reaches a shell open.
4. **Screenshots fail-closed** on the tenant flag; activity fails open.

> **⚠️ Code-signing ≠ update-signing.** TaskFlow's installers are **not** OS-code-signed —
> SmartScreen/Gatekeeper warn. That is *separate* from the Ed25519 update channel and remains an open
> item for them. Don't conflate the two and assume we're covered.

## 6. Build & release — a working 3-OS pipeline

- **`build.rs`** bakes `TASKFLOW_*` env vars via `cargo:rustc-env`; `config.rs` reads them with
  `env!`, falls back to a dev `config.json`, and **panics at startup if a required field is missing**.
- **`build.yml`** — on push/PR, compiles **Windows + Linux + macOS**. This is the cross-platform gate;
  a Windows dev host cannot catch platform breakage alone. No secrets needed.
- **`release.yml`** — on a `v*` tag: build per-OS installers (NSIS / AppImage+deb / dmg) → generate
  `SHA256SUMS` → **Ed25519-sign it** → publish a GitHub Release.
- **Version must match across the tag, `Cargo.toml`, and `tauri.conf.json`.**
- **Linux needs these dev libs** (CI installs them; `.deb` `depends` mirrors them):
  `libwebkit2gtk-4.1-dev`, `libx11-dev`, `libxi-dev`, `libxtst-dev`, `libxrandr-dev`, `libxss-dev`,
  `libxext-dev`, `libxcb1-dev`, `libxcb-randr0-dev`, `libdbus-1-dev`, `libpipewire-0.3-dev`.

## 7. Lifecycle

- Window **close-request** → `prevent_close()` + `hide()` (minimize to tray), unless a real quit is in
  progress.
- **Every quit path** — tray Quit, SIGTERM, Ctrl-C — routes through `auto_sign_out`: **run-once**
  (atomic guard), signs out **only if the timer is active**, **5 s-bounded so shutdown never hangs**,
  then `app.exit`.

## 8. Their honest limitations (TDD §14 / PRD §7) — our inherited risk list

1. Linux/macOS capture is **compile-verified, not runtime-verified**.
2. **Wayland**: no global input; limited window access.
3. **macOS**: TCC/Accessibility grants required — **no UX yet**.
4. Installers **not OS-code-signed** (SmartScreen/Gatekeeper warn).
5. **Single-monitor** screenshot (primary display only).
6. Windows uses `xcap` (Graphics Capture/BitBlt), not hand-rolled DXGI — *"functionally equivalent,
   different edge cases."*
7. **Zero tests** — nothing we port arrives tested.

## 9. What we deliberately do NOT take

| Theirs | Ours | Why |
|---|---|---|
| `lib/tauri-bridge.ts` (Wails-compat shim: fake `window.go.main.App`, snake↔camel transforms) | `lib/ipc.ts` — typed `invoke`/`listen`; `#[serde(rename_all="camelCase")]` on Rust DTOs | Theirs exists to port a Go/Wails UI unchanged. We have no such constraint — and the shim's transform is what hid the `sessionType` rename. |
| `POST /activity/heartbeat` + its own `/screenshots/presign` | `POST /v1/agent/batch` + `BatchAck.upload_urls` | Our contract is deployed and folding. Only the *client* is modelled on them. |
| JPEG q85, no blur, no pHash | **768px WebP + blur + pHash** | `phash`/`blur_level` are already deployed contract fields — JPEG would mean sending `phash: ""`, a lie the server's dedup reads. |
| Task-only picker | **project → task** | Their `Task` already carries `project_id`/`project_name`; ours carries both on `TimerStarted`. A UI tier, not a data change. |
| No consent / indicator / transparency UI | **All three, built new** | Our `PRIVACY.md` requires them; theirs has none. See [`FRONTEND-PLAN.md`](FRONTEND-PLAN.md) §1. |
| A `pause` surface | — | Timer-gating subsumes it: **stopping the timer *is* the pause**. |
| Zero tests | Envelope goldens, bucket rotation, DTO field-names, clippy `-D warnings` | Their §14 limitation is not one to inherit. |
