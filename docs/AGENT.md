# AGENT.md — WorkPulse Desktop Agent Architecture (Tauri)

> Companion to the frontend canon in [`../../frontend/Docs/SPEC.md`](../../frontend/Docs/SPEC.md)
> and the backend canon in [`../../backend/docs/BACKEND.md`](../../backend/docs/BACKEND.md).
> This document is the **single source of truth for the desktop agent** — the third
> leg of WorkPulse. The frontend already *advertises* an agent (enrollment token,
> per-OS installers, the `/agents` management UI); the backend already *assumes*
> one ("future desktop agent → presigned PUT → S3"). This is the design of the
> agent itself: how it captures, what it sends, and the contracts it must honour.

- **Deliverable status:** **Design only** — this doc + the companions below. No
  implementation, no build sequencing committed. Mirrors how `backend/docs/` is handled today.
- **Framework:** **Tauri 2.x** — a small **Rust core** (capture, queue, scheduler, tray,
  updater) plus a minimal **React/TypeScript** window for consent/settings/status.
- **Platforms:** **Windows 10/11, macOS 12+, Linux (Ubuntu/Debian)** — cross-platform from day one.
- **Positioning:** "calm signal, not surveillance." Counts and metadata, never content
  (no keylogging, no clipboard, no audio). Consent-gated by construction.

### Resolved design decisions
| Decision | Choice |
|----------|--------|
| Framework | **Tauri 2.x** (Rust core + TS/React UI) — small footprint matching the mock's 19–24 MB installers; native OS capture in Rust |
| Capture content | **Counts + metadata only** — app/URL *names*, keystroke/click *counts*, idle/active. Never keystroke content, clipboard, or audio (SPEC §2.4) |
| Backend write path | Agent **never writes DynamoDB directly** — presigned S3 PUT for screenshots, idempotent batch POST for activity, server-side workers persist (BACKEND §6) |
| Offline behaviour | **Offline-first** — embedded SQLite spool, retry with backoff, exactly-once via `Idempotency-Key` (CONCURRENCY §1) |
| Identity | **Per-device credential** derived from an admin-issued enrollment token — *proposed*, requires backend sign-off (see [ENROLLMENT.md](ENROLLMENT.md)) |
| Real-time | **Polling over REST** — no WebSocket, matching the backend (BACKEND §1) |
| Deliverable | **Design only** — no code, no CI, no installers built yet |

### Companion design docs
- [CAPTURE.md](CAPTURE.md) — the capture engine: screenshots, active app/window, foreground URL, idle, keyboard/mouse counts, sampling cadence, cross-platform specifics.
- [INGESTION.md](INGESTION.md) — data contract + upload protocol; the captured-data → frontend-shape → backend-item mapping; offline spool, retry, exactly-once.
- [ENROLLMENT.md](ENROLLMENT.md) — device enrollment & identity; the **device-credential proposal** that fills the backend's user-only-JWT gap.
- [PRIVACY.md](PRIVACY.md) — consent, silent mode, tray indicator, quiet hours, blur/redaction, exceptions, retention, data minimization.
- [CONFIG.md](CONFIG.md) — local settings + remote policy sync; app/URL allow-block + categorization enforcement.
- [UPDATES-SECURITY.md](UPDATES-SECURITY.md) — auto-update, channels, version/rollout/rollback, code signing, data-at-rest, tamper resistance.
- [AGENT-IMPLEMENTATION-ARCHITECTURE.md](AGENT-IMPLEMENTATION-ARCHITECTURE.md) — the union of all the above in implementation order.

---

## 1. Goals & principles

1. **Feed the existing contracts, don't invent new ones.** Everything the agent emits
   must land in a field the frontend already renders ([`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts),
   [`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts)) and an item the backend
   already models ([`DATA-MODEL.md`](../../backend/docs/DATA-MODEL.md)). The round-trip
   mapping is the spec; see [INGESTION.md](INGESTION.md).
2. **Consent first, surveillance never.** The product's stance (SPEC §2.4, wireframe
   [04-monitoring.md](../../frontend/Docs/wireframes/04-monitoring.md)) is non-negotiable in
   the agent: silent mode *requires* an enabled consent policy, a tray indicator is on by
   default, quiet hours suspend capture, and the agent captures **counts and metadata, not content**.
3. **Offline-first, exactly-once.** Laptops sleep, travel, and lose VPN. The agent buffers
   locally and reconciles on reconnect with idempotency keys, so nothing is lost and nothing
   is double-counted (CONCURRENCY §1).
4. **Least privilege on the endpoint.** The agent requests only the OS permissions it needs
   (Screen Recording / Accessibility on macOS, etc.), stores its credential in the OS keychain,
   and runs as the logged-in user — not elevated.
5. **Drop in behind the seam.** The agent talks to the same REST API the frontend's module
   services target. Where the backend lacks a route the agent needs, this design **names it as
   a proposal** rather than assuming it exists (see §6).
6. **Tenant-scoped by token.** `orgId`/`userId` come from the agent's verified credential, never
   from local config or the request body — same rule as the backend (AUTH-RBAC §13).

---

## 2. High-level architecture

```
        Employee machine (Windows / macOS / Linux)
   ┌────────────────────────────────────────────────────────┐
   │  WorkPulse Agent (Tauri)                                │
   │                                                        │
   │   ┌───────────────┐      ┌────────────────────────┐    │
   │   │  React/TS UI  │◀────▶│      Rust core         │    │
   │   │ consent ·     │ IPC  │                        │    │
   │   │ settings ·    │ cmds │  ┌──────────────────┐  │    │
   │   │ status · tray │      │  │ Capture engine   │  │    │
   │   └───────────────┘      │  │ (screens, window,│  │    │
   │                          │  │  idle, in counts)│  │    │
   │                          │  └────────┬─────────┘  │    │
   │                          │           ▼            │    │
   │                          │  ┌──────────────────┐  │    │
   │                          │  │ Spool (SQLite)   │  │    │
   │                          │  │ + scheduler      │  │    │
   │                          │  └────────┬─────────┘  │    │
   │                          │           ▼            │    │
   │                          │  ┌──────────────────┐  │    │
   │                          │  │ Uploader (HTTPS) │  │    │
   │                          │  └────────┬─────────┘  │    │
   │                          └───────────┼────────────┘    │
   └──────────────────────────────────────┼─────────────────┘
                                           │ TLS, Bearer <device/user token>
                                           ▼
                        ┌──────────────────────────────────┐
                        │  Amazon API Gateway (REST)        │
                        │  + Cognito authorizer             │
                        └───────┬───────────────┬───────────┘
                                ▼               ▼
                   presigned PUT to S3    POST activity / heartbeat
                   wp-screenshots-<env>   → DynamoDB single-table
                                          → EventBridge workers
                                            (thumbnail, score, anomaly)
```

The agent owns capture, buffering, and upload. The backend owns persistence, scoring,
thumbnailing, and anomaly detection (off the request path via EventBridge — BACKEND §6).
The frontend reads the results by polling. The agent and frontend **never talk directly**.

---

## 3. Process & module model (Rust core)

| Module | Responsibility |
|--------|----------------|
| `capture` | Screenshot grab, active app + window title, foreground URL, idle detection, keyboard/mouse counters. Per-OS backends behind one trait. See [CAPTURE.md](CAPTURE.md). |
| `policy` | Holds the effective config (capture interval, jitter, idle threshold, allow/block lists, quiet hours, silent mode). Pulled from the server, cached locally. See [CONFIG.md](CONFIG.md). |
| `spool` | Embedded SQLite write-ahead queue of pending samples + screenshot blobs; survives restart. See [INGESTION.md](INGESTION.md). |
| `scheduler` | Jittered capture timer; flush timer; heartbeat timer; quiet-hours / timer-running gates. |
| `uploader` | Requests presigned URLs, PUTs screenshots to S3, POSTs activity batches with `Idempotency-Key`, honours retry/backoff. |
| `identity` | Enrollment, credential storage (OS keychain), token refresh. See [ENROLLMENT.md](ENROLLMENT.md). |
| `updater` | Tauri updater: channel check, signature verify, staged apply. See [UPDATES-SECURITY.md](UPDATES-SECURITY.md). |
| `tray` | Tray icon + monitoring indicator, pause/resume, "what's collected" transparency view. |

The **React/TS window** is intentionally thin: first-run enrollment + consent, a settings/status
view, and the transparency panel. It reuses the frontend's Tailwind v4 design tokens for visual
consistency but ships no business logic — the Rust core is authoritative.

---

## 4. What the agent produces (contract summary)

Two output streams; full field mapping in [INGESTION.md](INGESTION.md).

- **Activity samples** → backend `activity` item (`PK=ORG#<o>#USER#<u>`, `SK=ACT#<date>#<ts>`),
  surfaced by the frontend as `UsageItem` / `ActivitySeries` and the keyboard/mouse buckets in
  [`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts). Carries: active/idle state,
  foreground app + URL name and category, keystroke + click *counts*, sampled window.
- **Screenshots** → S3 object in `wp-screenshots-<env>` + backend `screenshot` meta item
  (`SK=SHOT#<date>#<ts>`, `s3Key`, `flagged`), surfaced as `Screenshot { app, activity, flagged, date, time }`.
- **Agent health** → backend `agent` item (`SK=AGENT#<aid>`), surfaced in the `/agents`
  fleet table as `Agent { hostname, os, osVersion, version, status, lastSeen, ip, cpu, memory }`.

---

## 5. Security & privacy stance (summary)

- **Counts, not content** — invariant. Enforced in `capture`: there is no code path that
  records a keystroke value, clipboard contents, or audio. See [CAPTURE.md](CAPTURE.md) §"What is never captured".
- **Consent-gated** — capture does not start until the org's consent policy is active and (unless
  silent mode is explicitly enabled with consent) the user has acknowledged monitoring. See [PRIVACY.md](PRIVACY.md).
- **Credential at rest** in the OS keychain (Keychain / Credential Manager / Secret Service); the
  local spool is encrypted; no plaintext tokens on disk. See [UPDATES-SECURITY.md](UPDATES-SECURITY.md).
- **Tenant isolation** — every uploaded record is scoped by the credential's `orgId`/`userId`;
  the agent cannot address another tenant.

---

## 6. Backend gaps this design depends on (proposed — needs sign-off)

The backend docs cover **admin management** of agents but not the **agent's own data path**.
Two additions are required; both are specced as proposals here, not assumed:

1. **Device identity / auth.** [AUTH-RBAC.md](../../backend/docs/AUTH-RBAC.md) issues only
   *user* Cognito JWTs. The agent needs a long-lived, revocable **per-device credential**.
   Proposal + token model in [ENROLLMENT.md](ENROLLMENT.md).
2. **Ingestion endpoints.** [API.md](../../backend/docs/API.md) §28 exposes only
   `agents:view` / `agents:manage` admin routes. The agent needs (names proposed):
   `POST /agents/enroll`, `POST /agents/heartbeat`, `POST /screenshots/upload-url`,
   `POST /activity/ingest`. Full request/response shapes in [INGESTION.md](INGESTION.md) and
   [ENROLLMENT.md](ENROLLMENT.md). A new device-scoped permission (`monitoring:submit`) is proposed
   so a device credential can write activity without inheriting a human's full role.

---

## 7. Phasing (implementation order, when build is greenlit)

Design-only today. When the backend exists, build in this order so each phase is demoable
against the existing `/agents` UI:

| Phase | Delivers |
|-------|----------|
| A — Skeleton | Tauri shell, tray, enrollment + consent UI, OS-keychain credential, heartbeat → agent appears online in `/agents` with real `hostname/os/version/cpu/memory`. |
| B — Activity | Active app/window + idle + input counts → spool → `POST /activity/ingest`; populates `/insights/activity`. |
| C — Screenshots | Screenshot capture (interval + jitter, capture-only-while-timer-running, blur) → presigned PUT → `/insights/screenshots`. |
| D — Policy & enforcement | Remote config sync; app/URL allow-block + categorization; quiet hours; silent mode; exceptions. |
| E — Updates | Auto-update channels (stable/beta), staged rollout, rollback, signed releases. |

This mirrors the backend's own MVP-first phasing (BACKEND §"phasing").
