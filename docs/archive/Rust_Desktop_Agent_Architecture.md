# Cloud-Native Desktop Agent — Rust Implementation Architecture

> # ⛔ SUPERSEDED — HISTORICAL ESSAY, NOT A PLAN
>
> **This document is not authoritative and describes a system we are not building.** It predates the
> real design. `docs/BUILD-PLAN.md` §1 marks it for archival. Kept only as a record of the thinking.
>
> **Do not implement anything from this file.** Specifically, it is wrong about:
>
> - **§13 "Communication Layer (REST + WebSocket)" and `tokio-tungstenite`.** WebSocket is **deferred**
>   product-wide ([`../backend/WorkPulse-HLD.md`](../backend/WorkPulse-HLD.md) §3 *Freshness*), and even
>   that migration is **server→browser only** — **the agent is not part of it.** **Do not add
>   `tokio-tungstenite`**: there is no server endpoint to connect to, and none is planned for the agent.
>   The agent's whole server conversation is `POST /v1/agent/batch` every ~300 s plus an ETag config
>   pull ([`docs/INGESTION.md`](docs/INGESTION.md), [`docs/CONFIG.md`](docs/CONFIG.md)).
> - **Live config push, remote commands, force-sync, restart, WebSocket ping liveness.** None exist.
>   Config propagates on the **`BatchAck`**; liveness is the **heartbeat** (LLD §18: online =
>   heartbeat within ~10 min).
> - **The 3-process split and the crate tree** (`agent-transport/` etc.) —
>   [`docs/AGENT.md`](docs/AGENT.md) §0 builds **one Tauri process**;
>   [`docs/BUILD-PLAN.md`](docs/BUILD-PLAN.md) §1 has the real module layout.
>
> **Where the truth lives:** [`docs/AGENT.md`](docs/AGENT.md) (agent SSOT) ·
> [`docs/BUILD-PLAN.md`](docs/BUILD-PLAN.md) (what to build, in order).

## Enterprise Activity Tracking & Task Management Platform

**Version:** 1.0
**Last Updated:** July 2026
**Status:** ⛔ **SUPERSEDED 2026-07-17**
**Language:** Rust (2021 edition)
**UI Shell:** Tauri 2.x
**Targets:** Windows · macOS · Linux
**Scope:** Concrete Rust build architecture for the desktop endpoint agent

---

## Table of Contents

1. [Purpose](#1-purpose)
2. [Design Goals for the Rust Agent](#2-design-goals-for-the-rust-agent)
3. [Process Model: Service + UI](#3-process-model-service--ui)
4. [Workspace & Crate Layout](#4-workspace--crate-layout)
5. [Module Architecture](#5-module-architecture)
6. [Async Runtime & Concurrency Model](#6-async-runtime--concurrency-model)
7. [Crate Selection (Dependency Baseline)](#7-crate-selection-dependency-baseline)
8. [The Event Pipeline](#8-the-event-pipeline)
9. [Local Storage: SQLite as a Durable Queue](#9-local-storage-sqlite-as-a-durable-queue)
10. [Monitoring Subsystems](#10-monitoring-subsystems)
11. [Screenshot Subsystem](#11-screenshot-subsystem)
12. [Synchronization Engine](#12-synchronization-engine)
13. [Communication Layer (REST + WebSocket)](#13-communication-layer-rest--websocket)
14. [Configuration Management](#14-configuration-management)
15. [Remote Command Handling](#15-remote-command-handling)
16. [Auto-Update Subsystem](#16-auto-update-subsystem)
17. [Health & Observability](#17-health--observability)
18. [Security Implementation](#18-security-implementation)
19. [Error Handling Strategy](#19-error-handling-strategy)
20. [Cross-Platform Concerns](#20-cross-platform-concerns)
21. [Build, Packaging & Signing](#21-build-packaging--signing)
22. [Testing Strategy](#22-testing-strategy)
23. [Recommended Crate Reference Table](#23-recommended-crate-reference-table)
24. [Implementation Roadmap](#24-implementation-roadmap)

---

## 1. Purpose

This document specifies the **concrete Rust architecture** for the desktop endpoint agent. It assumes the platform-level decisions are already settled (cloud-native, offline-first, AWS backend, Rust for the agent) and focuses on **how the agent is actually built in Rust**: crate choices, workspace structure, module boundaries, concurrency, storage, and the code-level patterns that keep the agent lightweight, resilient, and maintainable across Windows, macOS, and Linux.

It is a companion to the platform architecture baseline and the Go-vs-Rust evaluation, not a replacement for either.

---

## 2. Design Goals for the Rust Agent

Every design decision serves these goals:

- **Small resident footprint** — target 8–20 MB RAM after an 8-hour session; no garbage collector, minimal allocation on hot paths.
- **Predictable CPU** — event-driven, target 0.2–0.8% CPU; no busy-waiting.
- **Never lose data** — every event is committed to a durable local queue before it is considered captured.
- **No single point of internal failure** — one subsystem crashing must not take down the others.
- **Offline-first** — full monitoring continues with no connectivity; the cloud is caught up later.
- **Cross-compilable** — one codebase, three OS targets, with platform code isolated behind traits.
- **Observable in the field** — structured logs, metrics, and health reporting built in from day one.

---

## 3. Process Model: Service + UI

The agent is split into **two processes** so that monitoring never depends on the UI being open.

```text
┌──────────────────────────┐        IPC         ┌──────────────────────────┐
│   Background Service      │  ◀──────────────▶  │   Desktop UI (Tauri)      │
│   (headless daemon)       │   local socket     │   Rust backend + React    │
├──────────────────────────┤                    ├──────────────────────────┤
│ Monitoring subsystems     │                    │ Status / consent view     │
│ SQLite queue              │                    │ Settings (read-mostly)    │
│ Sync engine               │                    │ Notifications             │
│ WebSocket + REST clients  │                    │ Talks ONLY to the service │
│ Update + health services  │                    │ via IPC                   │
└──────────────────────────┘                    └──────────────────────────┘
```

- **Background service** — a long-running OS service (Windows Service / launchd LaunchAgent / systemd unit). Owns all monitoring, storage, and networking. Survives UI closure and user logout where policy requires.
- **Desktop UI** — a small Tauri app (Rust backend + React/TypeScript/shadcn-ui frontend). Displays status and consent, exposes limited settings. It holds **no monitoring logic** and talks to the service over a local IPC channel.

**IPC transport:** a local domain socket (`interprocess` crate) carrying length-prefixed JSON or MessagePack frames. The UI is a client; the service is the authority.

---

## 4. Workspace & Crate Layout

Use a **Cargo workspace** with small, focused crates. This keeps compile times reasonable, enforces module boundaries at the crate level, and makes platform code swappable.

```text
agent/
├── Cargo.toml                 # [workspace]
├── crates/
│   ├── agent-core/            # orchestration, lifecycle, supervisor
│   ├── agent-config/          # config schema, load/merge/validate
│   ├── agent-storage/         # SQLite queue, migrations, retry state
│   ├── agent-monitor/         # trait-based monitoring abstractions
│   │   ├── src/keyboard.rs
│   │   ├── src/mouse.rs
│   │   ├── src/apps.rs
│   │   ├── src/browser.rs
│   │   └── src/idle.rs
│   ├── agent-capture/         # screenshot capture + encode
│   ├── agent-sync/            # batching, compression, encryption, upload
│   ├── agent-transport/       # REST client + WebSocket client
│   ├── agent-commands/        # remote command dispatch
│   ├── agent-update/          # self-update / tauri updater glue
│   ├── agent-telemetry/       # tracing, metrics, health
│   ├── agent-security/        # token store, crypto, TLS config
│   ├── agent-ipc/             # service <-> UI protocol
│   └── platform/              # OS-specific impls behind common traits
│       ├── src/windows.rs
│       ├── src/macos.rs
│       └── src/linux.rs
├── service/                   # binary: the background daemon
└── ui/                        # Tauri app (Rust + React)
```

**Rule:** higher-level crates depend on abstractions (`agent-monitor` traits), never directly on `platform`. The `platform` crate provides concrete implementations selected at compile time via `#[cfg(target_os = ...)]`.

---

## 5. Module Architecture

The `agent-core` crate runs a **supervisor** that owns every subsystem and restarts any that fail. This is the internal analogue of "no single module terminates the agent."

```text
agent-core::Supervisor
├── ConfigManager        (agent-config)
├── StorageQueue         (agent-storage)
├── MonitorSet           (agent-monitor)
│   ├── KeyboardMonitor
│   ├── MouseMonitor
│   ├── AppMonitor
│   ├── BrowserMonitor
│   └── IdleDetector
├── CaptureService       (agent-capture)
├── SyncEngine           (agent-sync + agent-transport)
├── CommandDispatcher    (agent-commands)
├── UpdateService        (agent-update)
├── HealthService        (agent-telemetry)
└── IpcServer            (agent-ipc)
```

Each subsystem is a **supervised task**: it runs as its own Tokio task, reports failures through a shared channel, and is restarted with backoff by the supervisor. A panic in one task is caught at the task boundary and does not unwind the process.

---

## 6. Async Runtime & Concurrency Model

**Runtime:** Tokio (multi-threaded scheduler, but tuned small — the agent is I/O-bound, not compute-bound).

Concurrency pattern — **channels over shared mutable state**:

- Each monitor produces `ActivityEvent`s and sends them on an `mpsc` channel.
- A single **collector task** owns the write side of SQLite (SQLite writers should be serialized), draining the channel and committing in batches.
- The **sync engine** reads committed rows, uploads, and deletes on acknowledgement.
- Control signals (config changes, remote commands, shutdown) fan out via `tokio::sync::broadcast` / `watch`.

```text
Monitors ──mpsc──▶ Collector ──▶ SQLite (single writer)
                                    │
                                    ▼
                              Sync Engine ──▶ Cloud ──ack──▶ delete rows

Config/Commands ──watch/broadcast──▶ all subsystems
Shutdown token  ──CancellationToken─▶ all subsystems
```

**Graceful shutdown:** a `tokio_util::sync::CancellationToken` is threaded into every task. On shutdown the collector flushes pending events to SQLite before exit, guaranteeing nothing in-flight is dropped.

**Backpressure:** bounded channels. If a monitor outpaces the collector, the channel applies backpressure rather than growing memory unboundedly.

---

## 7. Crate Selection (Dependency Baseline)

The dependency set is deliberately small and mainstream. Each crate is chosen for maturity and cross-platform support.

| Concern            | Crate                     | Why                                              |
| ------------------ | ------------------------- | ------------------------------------------------ |
| Async runtime      | `tokio`                   | De facto standard; rich I/O + task primitives    |
| Task utilities     | `tokio-util`              | `CancellationToken`, codecs                       |
| HTTP client        | `reqwest` (rustls)        | Ergonomic async HTTP; pairs with rustls           |
| WebSocket          | `tokio-tungstenite`       | Async WS over the same TLS stack                  |
| TLS                | `rustls`                  | Pure-Rust TLS 1.3; no OpenSSL dependency          |
| Serialization      | `serde` + `serde_json`    | Universal; MessagePack via `rmp-serde` for IPC    |
| SQLite             | `rusqlite` (bundled)      | Bundled SQLite; simple, synchronous, fast         |
| Migrations         | `refinery` / hand-rolled  | Versioned schema management                        |
| Compression        | `zstd`                    | High ratio + speed for batch payloads             |
| Encryption         | `aes-gcm`                 | Authenticated encryption for at-rest payloads      |
| Key handling       | `zeroize`                 | Wipe key material from memory                       |
| Screenshots        | `xcap`                    | Cross-platform capture (X11/Wayland/Win/macOS)     |
| Image encode       | `image`                   | Encode to WebP/PNG/JPEG                             |
| System info        | `sysinfo`                 | Processes, CPU, memory                              |
| Input events       | `rdev`                    | Cross-platform keyboard/mouse; native APIs where needed |
| File watching      | `notify`                  | Config file / path watching                        |
| Local IPC          | `interprocess`            | Cross-platform local sockets                       |
| Logging/tracing    | `tracing` + `tracing-subscriber` | Structured, leveled, span-based             |
| Telemetry export   | `opentelemetry` + OTLP    | Metrics/traces to the cloud collector              |
| Errors             | `thiserror` + `anyhow`    | Typed lib errors + ergonomic app errors            |
| Time               | `time` / `chrono`         | Timestamps, scheduling                             |
| Scheduling         | `tokio-cron-scheduler`    | If local periodic scheduling is required           |
| Config             | `config` + `serde`        | Layered config load/merge                          |
| Retry/backoff      | `backoff`                 | Exponential backoff with jitter                    |
| Auto-update        | `tauri-plugin-updater` / `self_update` | Signed update download + apply        |

> Only capture privacy-relevant **metrics** (counts, activity ratios, window titles per policy) — never raw keystroke content.

---

## 8. The Event Pipeline

Every data type — keyboard metrics, mouse metrics, app usage, browser activity, screenshots — flows through **one uniform pipeline**. This is the single most important pattern in the agent.

```text
Event
  ↓
Validate            (reject malformed, apply policy filters)
  ↓
Store Locally       (INSERT into SQLite, status = pending)
  ↓
Queue               (row is now durable)
  ↓
Compress            (zstd, per batch)
  ↓
Encrypt             (aes-gcm, per batch)
  ↓
Upload              (REST batch POST)
  ↓
Receive Ack         (server confirms)
  ↓
Delete Local Copy   (status = done → row removed)
```

**Invariant:** a row is only deleted **after** the cloud acknowledges receipt. A crash at any step leaves the row in `pending`, and the sync engine reprocesses it on restart. This is what makes the agent lossless across crashes, restarts, and outages.

---

## 9. Local Storage: SQLite as a Durable Queue

SQLite is **not the system of record** — it is a durable, crash-safe queue and cache. The cloud (DynamoDB + S3) is authoritative once data is acknowledged.

### PRAGMA configuration

```text
journal_mode = WAL        # concurrent reads while writing; crash-safe
synchronous  = NORMAL     # good durability/perf balance under WAL
busy_timeout = 5000       # tolerate brief lock contention
foreign_keys = ON
```

### Core tables (illustrative)

| Table            | Purpose                                             |
| ---------------- | --------------------------------------------------- |
| `events`         | Activity metrics; `status`, `retry_count`, `batch_id` |
| `screenshots`    | Metadata + local file path or blob; upload status   |
| `outbox_batches` | Batch grouping, attempt count, next-retry timestamp |
| `config`         | Last-applied configuration snapshot + version       |
| `device_state`   | Registration, token references, health counters     |
| `schema_version` | Migration bookkeeping                               |

### Writer discipline

- **Single writer task** owns the SQLite connection for writes; readers may use a separate read connection under WAL.
- Writes are **batched** inside transactions to minimize fsync overhead and keep CPU low.
- On startup, the sync engine scans for `pending` and `in-flight` rows and resumes — no orphaned data.

---

## 10. Monitoring Subsystems

Each monitor implements a common trait so the supervisor treats them uniformly and the `platform` crate can swap OS-specific implementations.

```text
trait Monitor {
    fn name(&self) -> &'static str;
    async fn run(self, tx: EventSender, cancel: CancellationToken) -> Result<()>;
}
```

| Monitor          | Captures                                   | Platform surface                        |
| ---------------- | ------------------------------------------ | --------------------------------------- |
| KeyboardMonitor  | keypress **counts / rates** (not content)  | `rdev` + native hooks                    |
| MouseMonitor     | movement/click **metrics**                 | `rdev` + native hooks                    |
| AppMonitor       | foreground app, window title (per policy)  | Win32 / CoreGraphics / X11-Wayland-DBus  |
| BrowserMonitor   | active tab/domain (per policy + extension) | Accessibility APIs / browser extension   |
| IdleDetector     | idle intervals via last-input time         | native idle-time APIs                    |

Monitors emit `ActivityEvent`s onto the shared channel. They **never** touch SQLite or the network directly — they only produce events. This keeps them simple, testable, and independently restartable.

---

## 11. Screenshot Subsystem

Screenshots are policy-gated and treated as large payloads on a separate path from lightweight events.

```text
Timer / Command trigger
        ↓
Policy check (enabled? interval? consent?)
        ↓
Capture (xcap)
        ↓
Downscale + encode (image → WebP, quality from config)
        ↓
Encrypt (aes-gcm)
        ↓
Persist metadata in SQLite + payload to local file / blob
        ↓
Upload via presigned URL / multipart to S3
        ↓
Ack → delete local copy
```

- **Interval and quality are remote-configurable** and can be changed live via WebSocket.
- Capture runs on a **blocking-safe** path (`tokio::task::spawn_blocking`) so image encoding never stalls the async runtime.
- Failure here is isolated: if capture fails, activity monitoring continues unaffected.

---

## 12. Synchronization Engine

The sync engine is the bridge between the local queue and the cloud, built around **batching, compression, encryption, and acknowledgement**.

```text
loop {
    wait_for(new_data OR interval_tick OR force_sync_command);
    let batch = storage.take_pending(BATCH_SIZE);
    if batch.is_empty() { continue; }
    let payload = encrypt(compress(serialize(batch)));
    match transport.upload(payload).await {
        Ok(ack)  => storage.mark_done(ack.ids),          // then rows are deleted
        Err(e)   => storage.mark_retry(batch.ids, backoff.next()), // exponential backoff + jitter
    }
}
```

Key properties:

- **Batching** reduces request count and AWS cost; **one large request** beats many small ones.
- **Exponential backoff with jitter** (`backoff` crate) prevents thundering-herd retries when the cloud or network recovers.
- **Idempotency** — each batch carries a client-generated ID so a retried upload the server already processed is deduplicated, not double-counted.
- **Ordering is not assumed** — events carry timestamps; the cloud reconstructs order, so out-of-order batch delivery is safe.

---

## 13. Communication Layer (REST + WebSocket)

A **hybrid** transport, mirroring the platform design, isolated in `agent-transport`.

### REST / HTTPS (`reqwest` + `rustls`) — bulk & batch

- Device registration and authentication
- Batched activity uploads
- Screenshot uploads (presigned S3 URLs)
- Configuration fetch
- Health reporting

### WebSocket (`tokio-tungstenite`) — real-time control

- Remote commands
- Live configuration push
- Force-sync requests
- Restart requests
- Immediate update notifications
- Heartbeat supplementing periodic health POSTs

### Connection management

- **Auto-reconnect** with backoff; while disconnected, the agent keeps monitoring and buffering (offline-first).
- **Heartbeat/ping** on the WebSocket detects dead connections quickly.
- All traffic is **TLS 1.3** with certificate validation; tokens attached as bearer credentials.

```text
Agent ──HTTPS (reqwest/rustls)──▶  auth · batch upload · screenshots · config · health
Agent ◀─WebSocket (tungstenite)─▶  commands · config push · force-sync · restart · updates
```

---

## 14. Configuration Management

Configuration is **remote-owned** with a safe local fallback.

```text
Precedence (highest → lowest):
  Remote config (cloud, live)  >  cached last-applied (SQLite)  >  bundled defaults
```

- On startup: load bundled defaults → overlay cached config → fetch remote → apply and cache.
- **Live updates:** the cloud pushes a new config over WebSocket; the agent validates it (`serde` + explicit validation), applies it atomically, and persists the snapshot.
- **Offline:** if the agent is offline when an admin changes config, the change is pending in the cloud and fetched/applied automatically on reconnect.
- **Validation before apply:** an invalid remote config is rejected and the previous known-good snapshot is retained — a bad push can never brick the agent.

Configurable surface includes: screenshot interval/quality, which monitors are enabled, batch sizes, sync intervals, and feature flags.

---

## 15. Remote Command Handling

Commands arrive over WebSocket (or are queued in the cloud and delivered on reconnect) and are dispatched by `agent-commands`.

| Command          | Effect                                              |
| ---------------- | --------------------------------------------------- |
| Start Screenshot | Enable capture subsystem                            |
| Stop Screenshot  | Disable capture subsystem                           |
| Restart Agent    | Graceful flush → service restart                    |
| Refresh Config   | Force a config fetch + apply                        |
| Force Sync       | Wake the sync engine immediately                    |
| Collect Logs     | Bundle recent structured logs and upload            |
| Upgrade          | Trigger the update subsystem                        |
| Rollback         | Revert to previous signed version                   |

Each command is:

- **Authenticated** — validated against the device's session before execution.
- **Acknowledged** — the agent reports success/failure back to the cloud.
- **Idempotent where possible** — re-delivery of a command is safe.

---

## 16. Auto-Update Subsystem

Updates are signed, verified, and support staged rollout — never trust a binary that fails signature verification.

```text
Startup / update notification
        ↓
Query Update Service (current channel + version)
        ↓
New version for this device?      (canary / % rollout / forced)
        ↓  yes
Download package (from S3 via CDN)
        ↓
Verify signature + manifest       (reject on mismatch)
        ↓
Apply update
        ↓
Restart service
```

- **Library:** `tauri-plugin-updater` for the UI shell; `self_update` (or a custom updater) for the headless service.
- **Signature verification is mandatory** — the manifest is checked and the package signature validated before anything is written.
- **Rollback protection** — the previous known-good version is retained so a `Rollback` command or a failed health check can revert.
- **Offline behavior** — if offline when a version ships, nothing happens; the update proceeds on reconnect.
- Supports **canary deployments, percentage rollouts, scheduled updates, and forced updates**.

---

## 17. Health & Observability

Built in from day one via `agent-telemetry`, using `tracing` and OpenTelemetry.

### Metrics (exported OTLP → cloud collector)

- CPU and memory of the agent
- Queue depth (pending events / screenshots)
- Upload latency and success rate
- Sync backlog age
- Reconnect counts
- Version / channel

### Logs

- **Structured** (`tracing` JSON layer), leveled, with spans across the pipeline.
- Error and crash logs retained locally and uploadable on demand (`Collect Logs`).

### Tracing

- Spans propagate across the upload path so a batch can be followed agent → API → workers via OpenTelemetry.

### Heartbeat

- Periodic health POST plus WebSocket ping so the fleet dashboard sees per-agent liveness, version distribution, and sync status.

---

## 18. Security Implementation

| Layer            | Implementation                                                        |
| ---------------- | --------------------------------------------------------------------- |
| Identity         | Device registration → JWT access token + refresh token               |
| Token storage    | OS secure store where available; encrypted at rest via `aes-gcm`; `zeroize` on drop |
| Transport        | `rustls` TLS 1.3, certificate validation, bearer tokens               |
| At-rest payloads | `aes-gcm` authenticated encryption of buffered events/screenshots     |
| Update integrity | Signed packages, manifest verification, rollback protection           |
| Key material     | Never logged; wiped from memory with `zeroize`                        |

The cloud side (Cognito, IAM, KMS, Secrets Manager, CloudTrail, WAF) is out of scope here but is the counterpart to this agent-side security surface.

---

## 19. Error Handling Strategy

Rust's type system is used deliberately to make failure explicit and non-fatal.

- **Library crates** define typed errors with `thiserror`; the **binary** uses `anyhow` for ergonomic context.
- **No `unwrap()`/`expect()` on runtime paths** — only in tests and provably-infallible startup invariants.
- **Panics are contained at task boundaries.** Each supervised task is wrapped so a panic becomes a task failure the supervisor can restart, not a process abort.
- **Fallible I/O returns `Result`**, and the pipeline's queue semantics mean a failed upload is a *retry*, not data loss.
- **`#[must_use]`** on acknowledgement types so a caller cannot silently ignore whether the cloud confirmed receipt.

This is the code-level expression of the platform rule: *no single module should terminate the entire agent.*

---

## 20. Cross-Platform Concerns

One codebase, three targets. Platform divergence is isolated behind traits in the `platform` crate.

```text
agent-monitor::traits  ← stable abstractions used everywhere
        ▲
        │ implemented by
platform::{windows,macos,linux}   ← selected via #[cfg(target_os = ...)]
```

| Surface            | Windows                | macOS                      | Linux                       |
| ------------------ | ---------------------- | -------------------------- | --------------------------- |
| Input hooks        | Win32 hooks            | Accessibility APIs         | X11 / Wayland               |
| Foreground window  | Win32                  | CoreGraphics               | X11 / Wayland / DBus        |
| Screenshot         | `xcap`                 | `xcap`                     | `xcap` (X11 + Wayland)      |
| Idle time          | Win32 last-input       | IOKit / CGEventSource      | X11 idle / DBus             |
| Service lifecycle  | Windows Service        | launchd LaunchAgent        | systemd unit                |
| Secure token store | Credential Manager     | Keychain                   | Secret Service (libsecret)  |

Wayland deserves explicit attention — screenshot and input access differ from X11 and may require portal APIs; test both display servers on Linux.

---

## 21. Build, Packaging & Signing

### Build

- **Release profile tuned for size and speed:**

```text
[profile.release]
opt-level = "z"     # or "s"; measure — "z" favors size, "3" favors speed
lto = true          # link-time optimization
codegen-units = 1   # better optimization, slower build
strip = true        # strip symbols from the shipped binary
panic = "abort"     # smaller binary; panics are contained at task boundaries anyway
```

- **Cross-compilation** via `cross` or per-OS CI runners. Rust's cross-compile story makes three-target builds straightforward.

### Packaging & signing per platform

| Target  | Package         | Signing                                  |
| ------- | --------------- | ---------------------------------------- |
| Windows | MSI / installer | Authenticode code signing                |
| macOS   | `.app` / `.pkg` | Developer ID signing + **notarization**  |
| Linux   | `.deb` / `.rpm` / AppImage | GPG-signed repositories        |

Tauri's bundler produces installers for the UI shell; the headless service is packaged alongside it. **All shipped artifacts are signed**, and the auto-update manifest references signed packages.

---

## 22. Testing Strategy

| Level             | Approach                                                            |
| ----------------- | ------------------------------------------------------------------ |
| Unit              | Pure logic in each crate (validation, batching, backoff math)      |
| Trait mocking     | Monitors and transport behind traits → inject fakes                |
| Storage           | `rusqlite` against a temp DB; assert queue invariants (no loss)    |
| Pipeline          | End-to-end: inject events → simulate outage → assert replay        |
| Property tests    | `proptest` for serialization round-trips and idempotency           |
| Fault injection   | Force upload failures, crashes mid-batch → verify no data loss     |
| Cross-platform CI | Build + smoke-test on Windows, macOS, and Linux runners            |

The **critical test** is the loss-freedom invariant: inject N events, kill the process mid-sync, restart, and assert exactly N events reach the (mocked) cloud with no duplicates and no drops.

---

## 23. Recommended Crate Reference Table

Consolidated dependency baseline for the agent.

| Area               | Crate(s)                                    |
| ------------------ | ------------------------------------------- |
| Async runtime      | `tokio`, `tokio-util`                       |
| HTTP client        | `reqwest` (rustls-tls)                       |
| WebSocket          | `tokio-tungstenite`                         |
| TLS                | `rustls`                                    |
| Serialization      | `serde`, `serde_json`, `rmp-serde`          |
| SQLite             | `rusqlite` (bundled)                        |
| Migrations         | `refinery`                                  |
| Compression        | `zstd`                                      |
| Encryption         | `aes-gcm`, `zeroize`                        |
| Screenshots        | `xcap`, `image`                             |
| System info        | `sysinfo`                                   |
| Input events       | `rdev`                                      |
| File watching      | `notify`                                    |
| IPC                | `interprocess`                             |
| Logging / tracing  | `tracing`, `tracing-subscriber`             |
| Telemetry          | `opentelemetry`, `opentelemetry-otlp`       |
| Errors             | `thiserror`, `anyhow`                       |
| Time               | `time` (or `chrono`)                        |
| Scheduling         | `tokio-cron-scheduler`                      |
| Config             | `config`, `serde`                           |
| Retry / backoff    | `backoff`                                   |
| Auto-update        | `tauri-plugin-updater`, `self_update`       |
| UI shell           | `tauri` (+ React / TypeScript / shadcn-ui)  |

---

## 24. Implementation Roadmap

A pragmatic build order that gets a lossless core working before adding surface area.

1. **Foundation** — workspace, `agent-core` supervisor, `tracing`, config load, graceful shutdown.
2. **Durable queue** — `agent-storage` with WAL SQLite, migrations, single-writer collector, queue invariants + tests.
3. **One monitor end-to-end** — `AppMonitor` → event pipeline → SQLite, proving the uniform pipeline.
4. **Sync engine + REST** — batching, zstd, aes-gcm, upload, ack, delete; backoff and idempotency; offline replay test.
5. **Remaining monitors** — keyboard/mouse metrics, browser, idle, behind the common trait.
6. **Screenshot subsystem** — capture, encode, presigned upload, policy gating.
7. **WebSocket + remote commands + live config** — real-time control channel and command dispatch.
8. **Health & telemetry** — metrics, heartbeat, log collection.
9. **Auto-update** — signed download, verify, apply, rollback; staged rollout.
10. **UI shell** — Tauri app + IPC to the service; status and consent.
11. **Cross-platform hardening** — Windows/macOS/Linux service lifecycle, secure token stores, Wayland.
12. **Packaging & signing** — installers, code signing, notarization, signed update manifests.

The ordering front-loads the **loss-freedom guarantee** (steps 2–4), because everything else depends on the queue being trustworthy.

---

*End of document.*
