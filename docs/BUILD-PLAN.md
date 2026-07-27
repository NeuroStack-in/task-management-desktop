# BUILD-PLAN.md — Desktop Agent Build-Out

> **Status:** Rewritten 2026-07-16; **status narrative refreshed 2026-07-27** (M0–M8 are now
> implemented — see "Where we actually are"). Supersedes the earlier 3-process roadmap.
> **Model:** [`REFERENCE-TASKFLOW.md`](REFERENCE-TASKFLOW.md) — **TaskFlow Desktop v0.1.4**, a complete,
> shipping Tauri v2 + Preact activity-monitoring agent (~3,160 L Rust, ~5,150 L TS, CI-released).
> It is the same product category as this agent, already working. We model on it and speak **our**
> wire contract.
>
> **Authority:** [`WorkPulse-LLD.md`](../../backend/WorkPulse-LLD.md) +
> [`WorkPulse-HLD.md`](../../backend/WorkPulse-HLD.md) — **except LLD Appendix A, which §0 amends.**

## Product decisions (binding)

- Timer with a **project + task selector**, and a **mandatory description**.
- **Activity tracking happens only while the timer is on.**
- **Screenshots are taken only while the timer is on.**
- **Multiplatform**, via cross-platform libraries.
- **Auth as TaskFlow does it** — AWS Cognito + the OS keyring.

## Where we actually are

> **Updated 2026-07-27.** The paragraphs below describe the *starting* state this plan was written
> against (pre-M0). **That state no longer holds — M0–M8 are implemented.** Kept for the history;
> read the box first.

> ✅ **Current state (2026-07-27).** The single-process refactor is **done** and the tree **compiles**:
> `cargo check --workspace` is green, and a local `cargo tauri build` produces a signed
> `workpulse-agent.exe` plus NSIS (`WorkPulse_0.1.0_x64-setup.exe`) and MSI installers under
> `target/release/`. The former `crates/agentd` + `crates/capture-helper` + `crates/agent-shared` +
> `tray/` split is **gone**; the whole Rust core is ~6.3k lines across 46 files under `src-tauri/src/`
> (`auth api monitor timer outbox rules config commands mqtt updater` + `lib.rs`), with the Preact
> webview (~3.3k lines) under `ui/`. `wp-agent-contract` is a path-dep to the sibling `backend/`
> checkout (pin it for CI/release per §1 — still non-negotiable).
>
> **All milestones M0–M8 are implemented:** Cognito `USER_PASSWORD_AUTH` login + chunked keyring
> tokens (M1); heartbeat/batch cycle + jsonl outbox (M2); timer + project/task picker with mandatory
> description + meeting mode (M3); the 1 s monitor thread — idle/input-counts/foreground-app, minute
> buckets, classifier (M4); screenshots — xcap all-display → WebP + blur + pHash → host-pinned S3 PUT
> (M5); shell/tray polish, autostart, single-instance, per-feature entitlement gating (M6); the
> GitHub-Releases signed updater with a baked-in pubkey + periodic check (M7); and tests + CI (M8 —
> `.github/workflows/ci.yml` + `release.yml`, ~74 `#[test]`s including monitor jitter, screenshot
> host-pin, and batch-ack parsing).
>
> **Beyond the original M0–M8 plan:** per-install **device enrollment** (`POST /v1/agent/enroll` →
> X.509 credential in the keyring, `api/enroll.rs`) and the **MQTT downlink** push rail (AWS IoT
> Core mutual-TLS: `config_changed`/`capture_now` commands + presence, `src-tauri/src/mqtt/`) are now
> built — the agent-side counterpart to backend MQTT-MIGRATION Phase 3. **Screenshot cadence gained a
> `Custom` variant** (`Cadence::Custom` in the shared contract, mirrored by the panel's `cadence` type).

**Pre-M0 starting state (historical):** `cargo check --workspace` **failed** —

```
error[E0063]: missing fields `idle` and `outbox_mb` in initializer of `Heartbeat`
error[E0063]: missing field `started_at` in initializer of `AgentEvent`
```

The backend had moved `wp-agent-contract`; this repo never followed, because it's an **unpinned
path-dep to a sibling checkout**. Nobody noticed. That mechanism — not the two errors — was the real
bug, and M0 closed it. At that point only **4 slices were real** (`outbox` in-memory, `timer_engine`,
`input_counts`, `rule_classifier`); everything else was a stub and **zero** OS/IO deps were declared.

---

## 0. The amendment — stated, not buried

**This amends LLD Appendix A.** It specifies **3 processes** (`agentd` service + per-user
`capture-helper` + tray). We build **1 Tauri process**, like the sample.

**Why it's sound:** the split exists to capture with **no user present** and survive logout — Windows
Session-0 isolation, macOS TCC ownership. **Timer-gated capture means the user is always present.**
The split buys nothing here and costs a service installer, an IPC surface, and per-OS session plumbing.

**What is lost — on the record:**

- **Tamper-resistance.** The user can kill the process and tracking stops. No privileged service restarts it.
- **Survive-logout / no-user capture.** Impossible by construction. Attendance-while-logged-out is off the table.
- **Session-0 capture.** Gone.

**Design consequence:** keep `monitor/` free of Tauri types, so splitting a daemon out later is not a
rewrite.

## 1. Target architecture

> ✅ **Done (2026-07-27).** The delete happened; the layout below is what shipped, with a couple of
> pragmatic differences called out inline (`commands/` is `{mod,panel}.rs`; the tray lives in `lib.rs`,
> not a `tray/` dir; `mqtt/` was added for the push rail). `wp-agent-contract` is still a path-dep —
> **pin it for CI/release** (the one remaining item here).

**Deleted (M0):** `crates/agentd`, `crates/capture-helper`, `crates/agent-shared` (a re-export +
`ipc.rs` — one process means no IPC; depend on `wp-agent-contract` **directly and pinned**), and
`tray/` (186 L of hardcoded-value HTML, superseded by `ui/`). The three root `*_Architecture.md`
essays were archived.

```
desktop/
  Cargo.toml                      # workspace, members = ["src-tauri"]
  ui/                             # Preact + Vite + Tailwind + TS
  src-tauri/
    tauri.conf.json               # a companion widget, not a dashboard
    src/
      main.rs  lib.rs             # lib split (the sample is a plain bin) so tests can link;
                                  #   lib.rs also builds the tray (no separate tray/ dir)
      clock.rs error.rs events.rs lifecycle.rs window_size.rs
      state.rs session_state.rs privacy_log.rs cycle.rs heartbeat.rs location.rs
      config/                     # AgentConfig cache, ETag pull on version mismatch
      auth/{mod,cognito,config,token,token_store}.rs
      api/{mod,client,batch,config,tasks,projects,timesheet,enroll}.rs
      commands/{mod,panel}.rs     # the webview's #[command] surface (kishore's panel UI)
      timer/{mod,engine}.rs       <- agentd/features/timer_engine.rs
      outbox/{mod,store}.rs       # queue/batches.jsonl (+ seq/watermark prune)
      monitor/{mod,idle,input,active_window,screenshot,session,bucket}.rs
      rules/{mod,classifier}.rs   <- capture-helper/features/rule_classifier.rs
      mqtt/{mod,capture}.rs       # NEW: AWS IoT downlink (config_changed / capture_now / presence)
      updater/mod.rs
```

**Pin `wp-agent-contract`** — git dep at a rev, or path-dep plus a CI job that fails on drift.
Non-negotiable; this failure mode recurs otherwise. **(Still a path-dep as of 2026-07-27.)**

## 2. What survives — all 4 real slices, with their tests

- **`outbox` → `outbox/mod.rs`.** Keeps `next_seq`/`prune_to` + `seqs_increment_and_prune_by_watermark`.
  `enqueue_cycle` takes the full cycle (activity/events/screenshots), not just a heartbeat.
  `VecDeque` → append-only `queue/batches.jsonl` (**not SQLCipher** — the docs are wrong).
  `captured_at: 0` → real server-offset epoch-ms.
  **`agent_id()` must change in M0:** it defaults to `"dev-agent"`, so **every dev machine would
  collide on `(agent_id, batch_seq)` against the LIVE dev table** — corrupted watermarks, machines
  pruning each other's batches. Per-install UUID in the keyring; env override for dev only.
- **`timer_engine` → `timer/engine.rs`.** Keeps `only_one_session_at_a_time`. `Running` gains
  `started_at` + `description`; `task_id`/`project_id` become `Option` (meeting mode); crash recovery
  persists the running session and closes it at last-known activity with `StopReason::Shutdown`.
- **`input_counts` → `monitor/input.rs`.** **Keep `on_keystroke()` taking no argument** — the
  structural privacy invariant, and the best thing in the skeleton. Its doc says "production registers
  rdev callbacks" — **wrong now**: we use **`device_query` polling with cumulative deltas** (exactly
  what the sample chose, to dodge rdev's blocking / macOS-main-thread trap). `take_minute()` is already
  the right shape for `ActivityRollup`.
- **`rule_classifier` → `rules/classifier.rs`.** Unchanged. Now wired to three consumers: sets
  `AppSpan.category`; `exceptions` suppresses the screenshot **and** the span; `blocked` emits
  `PolicyViolation` — **only while the timer runs** (off-timer: no enforcement, ever).

Everything else in `agentd/features/` and `capture-helper/features/` is a stub and dies.

## 3. Frontend — `ui/`

Preact ^10.22 + Vite ^5.3 + Tailwind ^3.4 + TS. **No state library** — `useState` plus a
`useSyncExternalStore` store over localStorage (`lib/settings.ts`, port as-is).

**Drop the sample's `lib/tauri-bridge.ts`.** It fakes `window.go.main.App` / `window.runtime` and does
snake↔camel transforms — a fossil of its Go/Wails origin. Replace with `lib/ipc.ts`: typed
`invoke<T>()` / `listen()` wrappers, and give Rust DTOs `#[serde(rename_all = "camelCase")]` so no
transform layer exists at all.

| Component | Source |
|---|---|
| `LoginForm.tsx` | **Port** (incl. the NEW_PASSWORD_REQUIRED challenge); rewrite calls to `invoke` |
| `TimerView.tsx` | **Port + decompose** — 1348 L is not acceptable; split shell / sessions / hooks |
| **`ProjectTaskSelector.tsx`** | **NEW.** The sample is task-only; we need **project → task**. Its `Task` already carries `project_id`/`project_name`, so this is a UI tier, not a data change. Port the **mandatory description** + history autocomplete from `TaskSelector.tsx` |
| `Timer`, `SessionInspect`, `IdlePrompt`, `SettingsDrawer`, `AvatarMenu`, `ShortcutHelp`, `ui/*` | **Port** |
| `lib/serverClock.ts` | **Port** — the timer ticks **server** time, not the local clock |

**Fix the sample's defect at the boundary.** Its `monitor/session.rs` serializes
`canTrackWindows`/`displayServer`/`limitation`, but `TimerView.tsx` reads
`limitationMessage`/`sessionType` (the old Go shape) — so **its Wayland banner can never fire**. Guard:
one Rust DTO + a `#[test]` asserting its serialized field names, mirrored by a hand-checked TS type.
Every UI-read DTO gets this test. **Both compilers are blind to this bug class.**

## 4. Backend — the monitor, timer-gated

`monitor::reflect(state)` — idempotent start/stop, bound to **`TimerEngine::is_running()`**.

**Thread A — a dedicated OS thread, 1 s tick. Timer-gated.** (Blocking / `!Send` handles stay off the
async reactor — this is why the sample uses a thread; copy it.)
- `user-idle` → idle seconds
- `device_query` → cumulative kb/mouse counters, **uint32-wrap handling + `SPIKE_CAP: 1000`**
- every 5th tick (`WINDOW_SAMPLE_EVERY: 5`): `x-win` → app/title/url → classifier → `AppSpan`
- `windows-sys` `OpenInputDesktop` → screen-lock detection

**`monitor/bucket.rs` — the real divergence from the sample.** It keeps one cumulative `Bucket` per
300 s heartbeat; **our contract wants per-minute `ActivityRollup`**. So:
- `MinuteBucket` keyed `epoch_ms / 60_000`
- each tick: `idle_secs > IDLE_THRESHOLD_SECS (2)` → `idle_sec += 1`, else `active_sec += 1`.
  **Invariant: `active_sec + idle_sec ≤ 60`** — test it.
- seal on minute boundary, on timer-stop, and on screenshot early-flush
- `top_apps` capped at `MAX_APPS_PER_BUCKET: 30`, top-N by seconds

**Thread B — tokio, `CYCLE_SECS: 300`. NOT timer-gated — auth-gated.** *Deliberate:*
heartbeat→AgentDevice is the **only live fold**, so the fleet table must stay honest about an agent
that is online but not tracking. While signed in we send every 300 s regardless, with `activity: []`
and `events: []` when the timer is off.

**Thread C — tokio, jittered. Timer-gated.** Screenshot at `Cadence::interval_secs()` ±
`SCREENSHOT_JITTER_SECS: 60` (anti-evasion). `Cadence::Off` → no thread.

**Sender.** Drain the outbox oldest-first → `POST /v1/agent/batch` with the Cognito **ID token**
(the claims ride the ID token, not the access token) → on `BatchAck`: `prune_to(watermark_seq)`,
compare `config_version` → ETag pull → PUT bytes to `upload_urls`. Two reqwest clients: 30 s API,
180 s upload.

## 5. The wire mapping

Sample `POST /activity/heartbeat` @300 s → our `POST /v1/agent/batch` @300 s. **Same cadence, our envelope.**

| Sample | Ours |
|---|---|
| one cumulative `Bucket` per 5 min | **5 × `ActivityRollup`**, one per minute |
| `appsUsed`, cap 30 | `top_apps: Vec<AppSpan>`, cap 30 **per minute** |
| `screenshot_url` early-flush link | **`ScreenshotMeta.bucket_minute`** — invert it: the shot points at its minute. Still early-flush the open bucket at capture, so the shot maps to its exact window and isn't double-counted |
| its own `/screenshots/presign` | **`BatchAck.upload_urls[]`** — dropped; we already have the rail |
| no sequencing | `(agent_id, batch_seq)` + `watermark_seq` prune |

**One cycle:** `captured_at` (server-offset clock) → `heartbeat` (`sysinfo` cpu/mem/os_version,
`agent_version` = `CARGO_PKG_VERSION`, `ip`, `outbox_mb` = jsonl size, `idle` = last tick) → drain 0–5
rollups → drain timer/attendance/policy events → screenshot metas (bytes already on disk) →
`enqueue_cycle` → POST → ack → prune → PUT each `upload_urls` entry (**host-pinned**: reject unless
https + `amazonaws.com`) → delete the local file. A PUT failure keeps the file and re-declares the meta
next cycle (the client-generated `id` makes that idempotent) — **with an attempt cap**.

**`Cadence` is redefined.** It now means **screenshot cadence while the timer runs** — nothing else. It
no longer drives the batch cycle (fixed 300 s), and it no longer means "how often we capture" globally,
because the timer gate does that. `Off` = zero screenshots. The `screenshots` feature flag **fails
closed**; `activity_monitoring` fails open.

## 6. Contract PR — one batch, before M3

The envelope is **"frozen for Phase 2"** ([`DEV-A.md`](../../backend/docs/DEV-A.md)). This architecture
forces a batched unfreeze. The freeze protects the *fold* contract, and **only heartbeat folds today** —
so these cost zero now and are breaking the day Phase 2/3 folds ship.

**Deploy `ingest` first, the agent second — never the reverse.**

1. **`TimerStarted.description: String`** — required. The UI collects it, `TimeEntry` stores it, the
   contract drops it. **This alone blocks the selector.**
2. **`TimerStarted.task_id` / `project_id` → `Option`** — meeting mode (timer with no task). No sentinels.
3. **`ScreenshotMeta.bucket_minute: i64`** — the activity↔screenshot link.
4. **`TrackingConfig.auto_update: bool`** — the updater policy has no home today.
5. **`BatchAck.tasks_version: u64`** + `GET /v1/agent/tasks` (ETag pull) — the task-cache surface.
   **Don't** put the list in the ack: it's a hot path and the list is big. Mirrors how `config_version`
   already works.

No change to `Heartbeat` or `TimerStopped` — `started_at` is already correct, and its doc comment
explains why (a midnight-crossing session orphans into two half-rows without it).

### Screenshots: adopt the LLD — **768px WebP + blur + pHash**

The sample uses JPEG q85, no blur, no pHash. But **`phash` and `blur_level` are already fields in the
deployed contract** — shipping JPEG means either another contract change *or* sending `phash: ""`, a
lie that the server's dedup accounting reads. At 1 shot per 3–10 min the cost is negligible. This is
the one place we take the LLD over the sample.

*Implementation note:* `image 0.25`'s WebP encoder is **lossless-only** — use the `webp` crate for
lossy encode; blur via `image::imageops::blur` (`blur_level → sigma` map); pHash via `image_hasher`.

## 7. Milestones

> ✅ **All of M0–M8 are implemented as of 2026-07-27** (plus device enrollment + the MQTT downlink,
> which post-date this table). The "Verify" column is the acceptance bar each was built against; what
> still can't be checked from a dev box without AWS access is the live-backend confirmation in
> [`RUNBOOK.md`](RUNBOOK.md). The narrative below is the original forward-looking plan.

**Only heartbeat folds today — a 200 proves nothing.** Every proof is an observable AgentDevice row, a
local artifact, or a recorded request body. Live: `https://oqlla6l5oc.execute-api.ap-south-1.amazonaws.com`,
pool `ap-south-1_0ep998OVt`, `--profile company`.

| # | Milestone | Verify |
|---|---|---|
| **M0** | Compiles, contract pinned. Delete 3 crates + `tray/`; new `src-tauri`; pin the contract; fix both E0063s; **fix `agent_id`** | `cargo check --workspace` green, 4 tests pass, the app is **in CI** (the old tray never was) |
| **M1** | Auth — Cognito **USER_PASSWORD_AUTH**: three hand-rolled JSON POSTs over `reqwest` (`InitiateAuth` · `RespondToAuthChallenge` for NEW_PASSWORD_REQUIRED · `REFRESH_TOKEN_AUTH`) — **no SRP, no `aws-sdk`**. Keyring storage **base64 + chunked** (`key.0..N` + count) to clear the **Windows Credential Manager blob limit**. Singleflight refresh via one `tokio::sync::Mutex`. 401 → `auth:expired` | Real login as `owner@acme.test`; decode the ID token, assert the **string** `tenant_id`/`perm` claims |
| **M2** | Heartbeat rail — cycle + jsonl outbox + POST. *The only milestone the backend can truly confirm today* | GSI6 → AgentDevice row with our hostname/version/`idle`/`outbox_mb`, `last_seen` advancing every 300 s. **Kill the network 10 min** → resumes, `batch_seq` gapless, **no duplicate rows** (proves watermark + idempotency) |
| **M3a** | Contract PR (§6) + `ingest` deploy | Blocks M3 |
| **M3** | Timer + ProjectTaskSelector — mandatory description, meeting mode, switch-while-running, serverClock, 5 s undo | Capture the request body; assert `timer_started` carries `description`; SQS lands it; the processor logs the unhandled-event seam — the honest ceiling until the Phase-2 fold exists |
| **M4** | Monitor — 1 s thread, device_query/user-idle/x-win, minute buckets, classifier, idle prompt @5 min + **hard auto-stop @15 min** | `--dump-cycle` → 5 rollups, each `active_sec + idle_sec ≤ 60`; golden fold test |
| **M5** | Screenshots — xcap → 768 WebP + blur + pHash → host-pinned PUT; early-flush link. **+ the macOS permissions UX** (below) | Object in the bucket (`aws s3 ls --profile company`), format/size asserted; `screenshots` flag off → **zero** shots (fails closed); **denied macOS grant → a visible "grant permission" state, not silence** |
| **M6** | Shell — tray (tooltip reflects tracking/idle), minimize-to-tray, **auto-sign-out on quit** (idempotent, 5 s-bounded, tray-Quit/Ctrl-C/SIGTERM), notifications + policy, autostart, window-size (400 ms debounce), single-instance, Wayland probe **+ the DTO field-name test**, dashboard-URL sanitization | |
| **M7** | Updater — GitHub Releases + SHA-256 + Ed25519; release builds refuse unsigned; **point `GITHUB_REPO` at our repo** (the sample's points at the legacy Go repo) | |
| **M8** | Tests + CI — the *sample* (TaskFlow) shipped with **zero**; **this repo does not** — ~74 `#[test]`s exist (monitor jitter, screenshot host-pin, batch-ack parsing, bucket rotation, outbox prune/replay, classifier precedence, DTO field-name goldens), run by `.github/workflows/ci.yml` under `clippy -D warnings` | Done: `just test` green; CI on push to `main` |

## 8. Risks

1. **The contract PR is the critical path.** M3–M5 block on it, and `ingest` is deployed. Adding
   required fields to a deployed enum variant breaks the running processor's deserialize →
   `#[serde(default)]`-tolerant, and **ingest deploys first, always.**
2. **Only heartbeat folds.** M3–M5 build a large surface whose output SQS accepts and drops. We fly on
   golden tests, not integration. Accept it; don't pretend a 200 is proof.
3. **`device_query` is a 1 s poller, not a hook.** It misses fast bursts, and `SPIKE_CAP` silently
   truncates. The number is an **estimate** — never let a UI or a report call it "keystroke count".
4. **Wayland — the inverse of what I first wrote.** `user-idle` and `device_query` **return 0**; `x-win`
   is partial; **only `xcap` works** (portal prompt). Per the sample's own doc: *"no global input API…
   input counters and per-app tracking **legitimately can't work**"* — an **OS limit, not a crate gap**.
   So Linux/Wayland ships **screenshots-only**, not activity-only. `session_info()` reports this via
   `canTrackWindows: false` — **and that is the exact DTO whose consumer is broken in the sample**, so
   the honest-degradation path is silently dead. The DTO field-name test is the only guard.
5. **macOS needs TWO runtime grants — and the UX does not exist.** **Screen Recording (TCC)** for
   capture, **Accessibility** for input counts *and* window titles. No crate avoids either. The
   sample's own open item #1 is *"detect denial, gate `monitor::start`, surface a 'grant permission'
   hint"* — **unbuilt, so we inherit the gap.** A denied Mac silently collects nothing. Build it at M5.
6. **Screenshots are primary-display only** in the sample. Decide deliberately; don't inherit it.
   *(Resolved: the agent captures **all displays** via `xcap::Monitor::all()`; `capture_primary` is
   kept for the on-demand `capture_now` path.)*
7. **Zero tamper-resistance** — the accepted cost of §0. Keep `monitor/` Tauri-free so it's reversible.
6. **`agent_id` collision** — fix in M0, not "later". It corrupts the live dev table.
7. **Screenshot re-presign loop** — a permanently-failing PUT re-declares its meta forever. Attempt cap,
   then drop + delete.
8. **Porting ≠ copying.** `TimerView.tsx` is 1348 L with Wails-shim assumptions baked in;
   `TaskSelector.tsx` is 725 L and task-only. The invoke rewrite + the project tier is real work.
9. **Two clocks.** `serverClock` ticks server time; buckets stamp locally. A skewed device files
   activity in the wrong minute and mis-keys a `TimeEntry`. Derive `captured_at` **and** bucket minutes
   from **one** monotonic-anchored server-offset clock — never raw `SystemTime::now()`.
10. **Linux keyring** needs a live secret-service. Headless CI and minimal WMs have none — it fails at
    **runtime**, not compile time.
