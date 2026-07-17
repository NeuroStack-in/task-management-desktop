# INGESTION.md — Data Contract & Upload Protocol

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. Defines the wire shapes the
> agent emits, how they round-trip to the frontend's rendered shapes and the backend's
> DynamoDB items, and the offline-first upload protocol. **The mapping table in §1 is the
> contract** — no field exists on the agent that doesn't land somewhere the frontend renders
> or the backend stores.

The endpoints below are **proposed** — [API.md](../../backend/docs/API.md) §28 only defines the
admin `agents:view`/`agents:manage` routes today. See [AGENT.md](AGENT.md) §6.

---

## 1. Round-trip mapping (the contract)

Frontend shapes: [`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts),
[`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts). Backend items:
[`DATA-MODEL.md`](../../backend/docs/DATA-MODEL.md).

### 1a. Activity sample
| Agent field | Type | → Frontend | → Backend item |
|-------------|------|-----------|----------------|
| `userId` (from credential) | string | attribution | `PK = ORG#<o>#USER#<u>` |
| `capturedAt` | ISO ts | timeline bucket | `SK = ACT#<date>#<ts>` |
| `app` | string | `UsageItem.name` (app) | `data` |
| `urlHost` | string? | `UsageItem.name` (website) | `data` |
| `category` | `productive\|neutral\|distracting` | `UsageItem.category` | `data` (server may re-classify) |
| `active` | bool | active/inactive share | `data` |
| `idleSeconds` | number | idle gaps | `data` |
| `keystrokeCount` | number | `KEYBOARD_BY_*` buckets | `data` |
| `clickCount` / `mouseDistance` | number | `MOUSE_BY_*` buckets | `data` |
| `windowSeconds` | number | normalizes counts → rates | `data` |

The server rolls these per-minute samples up into the hourly/daily/weekly `ActivitySeries`
and the `APP_USAGE`/`URL_USAGE` minute totals the frontend renders.

### 1b. Screenshot
| Agent field | Type | → Frontend (`Screenshot`) | → Backend (`screenshot` item) |
|-------------|------|---------------------------|-------------------------------|
| `userId` | string | `user` | `PK = ORG#<o>#USER#<u>` |
| `capturedAt` | ISO ts | `date`, `time` | `SK = SHOT#<date>#<ts>`; `GSI1 = ORG#<o>#SHOT#<date> / USER#<u>#<ts>` |
| (S3 object) | blob | thumbnail/full image | `s3Key` → `wp-screenshots-<env>` |
| `app` | string | `app` | `data` |
| `windowTitle?` | string? | lightbox detail | `data` (redactable) |
| `blurred` | bool | "blurred" badge | `data` |
| `monitorIndex` | number | multi-monitor | `data` |
| *(server-derived)* `activity` | 0–100 | `activity` score badge | `data` (scoring worker) |
| *(server-derived)* `flagged` | bool | risk/flag badge; `GSI2 = ORG#<o>#FLAGGED / <date>#<ts>` | `data` |

`activity` and `flagged` are **computed server-side** from the activity stream + scoring rules
(wireframe §13.6) — the agent never sends a productivity number. This keeps scoring authoritative
and re-runnable when rules change.

### 1c. Agent health (heartbeat)
| Agent field | → Frontend (`Agent`) | → Backend (`agent` item) |
|-------------|----------------------|--------------------------|
| `agentId` | row key | `SK = AGENT#<aid>` |
| `hostname` | `hostname` | `data` |
| `os` / `osVersion` | `os` / `osVersion` | `data` |
| `version` | `version` (vs `LATEST_AGENT_VERSION` → "outdated") | `data` |
| `status` | `status` (`online\|idle\|offline`) | `GSI1 = ORG#<o>#AGSTATUS / <status>#<aid>` |
| `lastSeenAt` | `lastSeen` | `data` |
| `ip` | `ip` | `data` |
| `cpu` / `memory` | `cpu` / `memory` | `data` |
| `capabilities` / `permissionState` | health/warn badge (degraded perms) | `data` |

`status` is derived server-side from heartbeat recency vs `AgentSettings.offlineAlertMins`
([`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts), default 30).

---

## 2. Upload protocol — **one call per cycle**

> ⚠️ **Corrected 2026-07-16.** This section previously described `POST /activity/ingest`, an
> `Idempotency-Key: <uuid>` header, a two-step `POST /screenshots/upload-url`, and a separate
> `POST /agents/heartbeat`. **None of those exist.** The real contract is a single combined endpoint,
> deployed and live. What follows is `wp-agent-contract` as it actually ships.

### 2.1 The one endpoint — `POST /v1/agent/batch`

Every **300 s** the agent sends **one** `BatchEnvelope`. Activity, events, heartbeat and screenshot
*metadata* all ride it — there are no other agent endpoints and no extra calls per cycle.

```jsonc
{ "agent_id": "<per-install UUID>",   // from the keyring — NOT env, NOT hostname
  "batch_seq": 42,                    // monotonic per agent
  "captured_at": 1721030400000,       // epoch ms, server-offset clock
  "config_version": 7,
  "heartbeat":   { "hostname": …, "os": …, "os_version": …, "agent_version": …,
                   "ip": …, "cpu_pct": …, "mem_pct": …, "outbox_mb": …, "idle": false },
  "activity":    [ /* 0–5 ActivityRollup, one per minute — EMPTY when the timer is off */ ],
  "events":      [ /* TimerStarted/Stopped, attendance, PolicyViolation */ ],
  "screenshots": [ /* ScreenshotMeta only — never bytes */ ] }
```

**Idempotency is `(agent_id, batch_seq)` in the body — there is no `Idempotency-Key` header.** The
server does a conditional put on that pair; a replayed `batch_seq` is a no-op that returns the same
ack.

**Response — `BatchAck`:**

| Field | The agent must |
|---|---|
| `watermark_seq` | **prune the outbox up to it** — the highest seq durably accepted |
| `config_version` | pull fresh config (ETag) if it differs from the local one |
| `upload_urls[]` | PUT the bytes for each pending screenshot |

### 2.2 Screenshots — S3-direct, via the ack

No presign endpoint. The agent declares `ScreenshotMeta` in the batch; the **ack returns a presigned
URL per shot**; the agent PUTs the bytes straight to S3 (bytes never transit Lambda).

- **Upload-host pinning:** refuse any URL that isn't `https` + `amazonaws.com`. A compromised backend
  must not be able to redirect a frame of the user's screen.
- A failed PUT keeps the local file and **re-declares the meta next cycle** — the client-generated
  `ScreenshotMeta.id` makes that idempotent. Cap the attempts, then drop and delete.

### 2.3 Heartbeat — a field, not an endpoint

The heartbeat is a **field of the envelope**, never a separate call. It is the **only signal that
folds today** (→ `AgentDevice` at `TENANT#<id> / DEVICE#<agent_id>`, GSI6); activity, timer and
screenshot folds are deferred seams (Phase 2/3).

The 300 s cycle is **not** timer-gated (it is auth-gated) — the fleet table must stay honest about an
agent that is online but not tracking. When the timer is off the envelope simply carries
`activity: []` and `events: []`.

Config propagates on the ack: `config_version` differs → ETag pull (see [CONFIG.md](CONFIG.md)). **No
WebSocket** — none exists anywhere in the product; it is **deferred** to a future migration
(`backend/WorkPulse-HLD.md` §3 *Freshness*). The agent polls by batching; the browser polls with
`If-None-Match`.

---

## 3. Offline-first outbox

A laptop offline must lose nothing and double-count nothing.

- **`queue/batches.jsonl`** — an append-only file; the sealed envelope is persisted **before** send.
  (**Not SQLite, not SQLCipher** — earlier drafts of this doc said otherwise.) Screenshot bytes sit
  beside it as `queue/screenshots/<id>.webp`.
- **Idempotency is the sequence**, assigned when the cycle is sealed: `(agent_id, batch_seq)`. A retry
  after an ambiguous failure re-sends the same seq → the server no-ops.
- **Drain oldest-first**, in order. On ack, `prune_to(watermark_seq)`; on failure keep and back off
  (exponential + jitter, capped). Distinguish retryable (5xx/network/throttle) from terminal
  (4xx auth/validation → surface, don't loop forever).
- **Bounded:** ≥72 h buffer, ~1 GB cap, **oldest-first eviction** beyond it, and **log the drop** — it
  must surface as a missing-capture window, never a silent gap.
- **`agent_id` and the sequence are born together.** They live in the same store: delete it and the seq
  restarts at 1, so every batch is silently rejected as a duplicate. They reset **together or not at
  all**.

---

## 4. Auth & tenancy on every call

- The agent authenticates **as the logged-in user** — `Authorization: Bearer <Cognito ID token>`. The
  claims ride the **ID token**, not the access token. There is **no device credential**: the per-device
  identity in [ENROLLMENT.md](ENROLLMENT.md) is **deferred**, and there is no `monitoring:submit`
  permission.
- **`tenant_id` comes from the verified token claims, never the body** (the claim is `tenant_id` — not
  `orgId`; `custom:orgId` is only the Cognito *attribute* name). A body that disagrees is rejected.
- `agent_id` is a **self-declared payload string** with no cryptographic backing — it identifies the
  install, it does not authenticate it. (Server-side hardening: bind `agent_id → user_id` on first
  sight and reject mismatches, or a spoofed id clobbers another device's row.)
- TLS only; certificate validation on.

---

## 5. Error & edge handling

| Situation | Behaviour |
|-----------|-----------|
| Token expired | refresh via [ENROLLMENT.md](ENROLLMENT.md); queue keeps filling meanwhile |
| Device revoked (401/403 terminal) | stop capture, clear credential, show re-enroll prompt; do not retry |
| Clock skew | `capturedAt` uses local time + monotonic deltas; server tolerates skew when bucketing |
| Partial batch accept | drop accepted `clientSampleId`s, retain + retry the rest |
| Screenshot S3 PUT fails | retain blob in spool, re-request a fresh presigned URL on retry (URLs expire) |
| Feature disabled for org | backend returns `403 feature_disabled` (API.md) → agent pauses that stream, keeps heartbeat |
