# AGENT.md — WorkPulse Desktop Agent Architecture

> **Rewritten 2026-07-16; status/module details refreshed 2026-07-27.** The **single source of truth
> for the desktop agent**. The design below is current; **implementation status: M0–M8 are done**, the
> single-process tree compiles and produces installers, and device enrollment + an MQTT downlink (§6)
> now exist. See [`BUILD-PLAN.md`](BUILD-PLAN.md) "Where we actually are".
>
> **Backend canon:** [`WorkPulse-LLD.md`](../../backend/WorkPulse-LLD.md) (feature behaviour — §4
> timer, §10 screenshots, §14 rules, §18 fleet) + [`WorkPulse-HLD.md`](../../backend/WorkPulse-HLD.md)
> (infra). **They win on any conflict — except LLD Appendix A, which §0 amends.**
>
> **Modelled on** [`REFERENCE-TASKFLOW.md`](REFERENCE-TASKFLOW.md) — **TaskFlow Desktop v0.1.4**, a
> complete, shipping Tauri v2 + Preact agent in the same product category. We take its architecture,
> libraries and UX; we speak **our** wire contract.
>
> **Build sequence + current state:** [`BUILD-PLAN.md`](BUILD-PLAN.md) (M0–M8).

## Product decisions (binding)

- Timer with a **project + task selector** and a **mandatory description**.
- **Activity tracking only while the timer is on.**
- **Screenshots only while the timer is on.**
- **Multiplatform** — Windows / macOS / Linux, via cross-platform libraries.
- **Auth as TaskFlow does it** — AWS Cognito + the OS keyring.

## 0. Process model — an amendment to LLD Appendix A

**Appendix A specifies 3 processes** (`agentd` service + per-user `capture-helper` + Tauri tray).
**We build 1 Tauri process.**

**Why:** the split exists to capture with **no user present** and to survive logout — Windows
Session-0 isolation, macOS TCC ownership. **Timer-gated capture means the user is always present.**
The split buys nothing here and costs a service installer, an IPC surface, and per-OS session plumbing.

**What is lost — on the record:**

| Lost | Consequence |
|---|---|
| **Tamper-resistance** | The user can kill the process and tracking stops. No privileged service restarts it. |
| **Survive-logout / no-user capture** | Impossible by construction. Attendance-while-logged-out is off the table. |
| **Session-0 capture** | Gone. |

**Design consequence:** `monitor/` stays free of Tauri types, so splitting a daemon out later is a
move, not a rewrite.

## 1. Goals & principles

1. **Feed the existing contract; don't invent one.** Everything the agent emits lands in a
   `wp-agent-contract` field the backend already models. That crate lives in the **backend repo** and
   is compiled by both sides — it is the anti-drift mechanism, and it must be **pinned** (§6).
2. **Consent first, surveillance never.** Counts and metadata, never content — no keylogging, no
   clipboard, no audio. The timer gate makes the privacy stance structural: **no timer, no capture,
   enforced in code.**
3. **Offline-first.** Every batch is persisted before send and pruned on a watermark ack. Nothing goes
   network-direct.
4. **Least privilege.** Only the OS permissions actually needed; tokens in the OS keyring; runs as the
   logged-in user, never elevated.
5. **Tenant-scoped by token.** `tenant_id` comes from the verified Cognito **ID token** — never from
   local config or the request body.

## 2. Architecture

One Tauri process: a Preact webview for the UI, a Rust core for everything else.

```
        Employee machine (Windows / macOS / Linux)
   ┌────────────────────────────────────────────────────────┐
   │  WorkPulse Agent  (ONE Tauri process, runs as the user) │
   │                                                        │
   │   ┌───────────────┐  invoke /  ┌────────────────────┐  │
   │   │   Preact UI   │◀─ listen ─▶│      Rust core     │  │
   │   │ login · timer │            │                    │  │
   │   │ project/task  │            │  timer/   monitor/ │  │
   │   │ settings      │            │  rules/   outbox/  │  │
   │   └───────────────┘            │  auth/    api/     │  │
   │                                │  tray/  updater/   │  │
   │                                └─────────┬──────────┘  │
   └──────────────────────────────────────────┼─────────────┘
                                              │ TLS, Bearer <Cognito ID token>
                            ┌─────────────────┴──────────────────┐
                            │  API Gateway HTTP API + JWT auth   │
                            └────┬──────────────────────┬────────┘
                                 ▼                      ▼
                POST /v1/agent/batch            PUT presigned S3
                → SQS → ingest-processor        (screenshot bytes —
                → AgentDevice / folds            never through Lambda)
```

## 3. Module model (Rust core)

```
src-tauri/src/
  main.rs  lib.rs           # lib split so integration tests can link; lib.rs also builds the tray
  clock.rs error.rs events.rs lifecycle.rs window_size.rs
  state.rs session_state.rs privacy_log.rs cycle.rs heartbeat.rs location/
  config/                   # AgentConfig cache; ETag pull on version mismatch
  auth/{mod,cognito,config,token,token_store}.rs   # keyring token store (chunked)
  api/{mod,client,batch,config,tasks,projects,timesheet,enroll}.rs
  commands/{mod,panel}.rs   # the webview's #[command] surface
  timer/{mod,engine}.rs     # one running session max; crash-safe
  outbox/{mod,store}.rs     # queue/batches.jsonl; seq + watermark prune
  monitor/{mod,idle,input,active_window,screenshot,session,bucket}.rs
  rules/{mod,classifier}.rs # synced category rules, applied on-device
  mqtt/{mod,capture}.rs     # AWS IoT downlink: config_changed / capture_now / presence
  updater/mod.rs            # signed GitHub-Releases self-update
```

> The tray is built inline in `lib.rs` (there is no separate `tray/` dir), and `commands/` is
> `{mod,panel}.rs` rather than the per-domain `*_cmds.rs` sketch above's earlier drafts implied.

**The monitor is timer-gated.** `monitor::reflect()` starts/stops idempotently on
`TimerEngine::is_running()`.

- **Thread A** — dedicated OS thread, 1 s tick, **timer-gated**. Blocking / `!Send` handles stay off
  the async reactor. `user-idle` → idle secs; **`device_query`** → cumulative kb/mouse deltas
  (uint32-wrap handling, `SPIKE_CAP: 1000`); every 5th tick **`x-win`** → app/title/url → classifier →
  `AppSpan`; `OpenInputDesktop` → screen-lock detect.
- **`bucket.rs`** — folds ticks into **per-minute `ActivityRollup`s** (`epoch_ms / 60_000`).
  Invariant: `active_sec + idle_sec ≤ 60`.
- **Thread B** — tokio, **300 s cycle, NOT timer-gated (auth-gated)**. Heartbeat→`AgentDevice` is the
  only live fold, so the fleet table must stay honest about an agent that is online but idle. Sends
  `activity: []` / `events: []` when the timer is off.
- **Thread C** — tokio, jittered, **timer-gated**. Screenshots at `Cadence::interval_secs()` ± 60 s
  (anti-evasion). `Cadence::Off` → no thread.

The **Preact window** is intentionally thin — login, timer + project/task selector, today's sessions,
settings. No business logic: the Rust core is authoritative.

## 4. What the agent produces

`POST /v1/agent/batch` every 300 s — a `BatchEnvelope`:

| Field | Source |
|---|---|
| `agent_id`, `batch_seq` | per-install UUID (keyring) + monotonic seq — **the idempotency key** |
| `captured_at`, `config_version` | server-offset clock; the config the agent is applying |
| `heartbeat` | `sysinfo` cpu/mem/os, `agent_version`, `outbox_mb`, `idle` → folds to `AgentDevice` (`TENANT#<id> / DEVICE#<agent_id>`, GSI6) |
| `activity[]` | 0–5 `ActivityRollup` (one per minute) — **empty when the timer is off** |
| `events[]` | `TimerStarted` / `TimerStopped`, attendance, `PolicyViolation` |
| `screenshots[]` | `ScreenshotMeta` **only** — bytes go S3-direct via `BatchAck.upload_urls` |

`BatchAck` returns `watermark_seq` (prune to it), `config_version` (pull if it differs), and
`upload_urls[]`.

## 5. Security & privacy

- **Counts, not content.** `on_keystroke()` takes **no argument** — the key identity is dropped at the
  source and cannot enter the process. Structural, not a convention.
- **`device_query` is a 1 s sampler, not a hook.** The counts are an **estimate** (`SPIKE_CAP`
  truncates bursts). Never present them as an exact "keystroke count".
- **Timer gate.** No timer → no sampling, no screenshots, no enforcement. Ever.
- **Exceptions carve-out.** A focused app/site in `exceptions` suppresses the screenshot **and** the
  span — nothing about it is recorded.
- **Upload-host pinning.** Refuse any presigned URL that isn't `https` + `amazonaws.com`, so a
  compromised backend cannot redirect a frame of the user's screen.
- **Fail-closed updates.** Release builds refuse unsigned installs (SHA-256 + Ed25519) and won't even
  *advertise* an update without a pubkey.
- Tokens live in the **OS keyring**; `tenant_id` comes only from the verified ID token.

## 6. Dependencies on the backend

- **Pin `wp-agent-contract`.** It is still an **unpinned path-dep to a sibling checkout** (`../backend/
  crates/wp-agent-contract`). This once made the workspace stop compiling (the backend moved the
  contract; nobody noticed) — that's fixed and the tree builds today, but the pin (git dep at a rev, or
  a CI drift check) is still the standing to-do. Non-negotiable for CI/release.
- **Contract PR required before the timer ships** ([`BUILD-PLAN.md`](BUILD-PLAN.md) §6):
  `TimerStarted.description`; `task_id`/`project_id` → `Option` (meeting mode);
  `ScreenshotMeta.bucket_minute`; `TrackingConfig.auto_update`; `BatchAck.tasks_version` +
  `GET /v1/agent/tasks`.
- **`ingest` deploys before the agent, always** — adding required fields to a deployed enum breaks the
  running processor's deserialize.
- **Only heartbeat folds today.** Activity, timer and screenshot folds are deferred seams (Phase 2/3),
  so a `200` proves the wire, not the feature.

## 7. Companion docs

- [`BUILD-PLAN.md`](BUILD-PLAN.md) — **the build sequence (M0–M8)** and the current state
- [`CAPTURE.md`](CAPTURE.md) — the capture engine, per-OS specifics
- [`INGESTION.md`](INGESTION.md) — the wire contract + upload protocol
- [`CONFIG.md`](CONFIG.md) — settings + remote policy sync
- [`PRIVACY.md`](PRIVACY.md) — consent, indicator, exceptions, retention
- [`ENROLLMENT.md`](ENROLLMENT.md) — identity. Batch auth is user-JWT (device-JWT enrollment stays
  **deferred**), but a **per-install X.509 device credential is now issued** by `POST /v1/agent/enroll`
  for the MQTT downlink (`api/enroll.rs` + `mqtt/`)
- [`UPDATES-SECURITY.md`](UPDATES-SECURITY.md) — updates, signing, endpoint security
