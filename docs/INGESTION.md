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

## 2. Upload protocol

### 2.1 Activity batches — `POST /activity/ingest`  *(proposed; perm `monitoring:submit`)*
- Body: `{ samples: ActivitySample[] }` (batched from the spool, typically 1–N minutes' worth).
- Header **`Idempotency-Key: <uuid>`** per batch. The backend stores the key
  (`PK = ORG#<o>#IDEMP#<key>`, TTL ~24 h, `attribute_not_exists` write — CONCURRENCY §1); a retried
  batch returns the stored result instead of re-applying. Each sample also carries a stable
  `clientSampleId` so partial-batch dedup is possible.
- Response: per-sample accept/reject so the agent can drop accepted rows from the spool.

### 2.2 Screenshots — presigned PUT (two steps)
1. **`POST /screenshots/upload-url`** *(proposed; perm `monitoring:submit`)* →
   `{ uploadUrl, objectKey, screenshotId }`. The server pre-computes the key
   (`org/<orgId>/user/<userId>/<date>/<screenshotId>.jpg`) and writes the pending `screenshot`
   meta item.
2. **`PUT <uploadUrl>`** — agent uploads the JPEG bytes directly to S3 (no API Gateway payload limits).
   The S3 PUT is naturally idempotent (same key = overwrite). The S3 `ObjectCreated` event triggers
   the thumbnail + scoring worker, which finalizes the meta item (BACKEND §6). A redelivered S3 event
   is safe — the worker conditions on not-already-done (CONCURRENCY §4).

Posting the screenshot **metadata** is folded into step 1 (server writes the pending item), so there
is no separate metadata POST and no window where an S3 object exists without a DynamoDB row.

### 2.3 Heartbeat — `POST /agents/heartbeat`  *(proposed; perm `monitoring:submit`)*
- Sent every ~60 s with the §1c payload. Updates the `agent` item; server recomputes `status`.
- Doubles as the **config-pull trigger**: the response may include a `policyVersion`; if it differs
  from the cached one, the agent fetches fresh policy (see [CONFIG.md](CONFIG.md)). Polling-only,
  matching the backend's no-WebSocket stance (BACKEND §1).

---

## 3. Offline-first spool

A laptop offline must lose nothing and double-count nothing.

- **Embedded SQLite** WAL queue holds pending activity samples and screenshot jobs (blob + metadata).
  Survives restart, sleep, crash. Encrypted at rest (see [UPDATES-SECURITY.md](UPDATES-SECURITY.md)).
- Each item has a stable client id and a generated `Idempotency-Key`, assigned **at capture time**,
  so a retry after an ambiguous network failure reuses the same key → exactly-once server-side.
- **Flush loop:** drain oldest-first; on success delete from spool; on failure keep and back off.
- **Backoff:** exponential with jitter, capped (e.g. 1s → … → 5 min). Distinguish retryable
  (5xx, network, throttling) from terminal (4xx auth/validation → surface, don't infinitely retry).
- **Bounded spool:** cap total on-disk size / age; when exceeded, drop **oldest activity samples
  first** (screenshots and health preserved longer) and **log the drop** so it's never a silent gap —
  it should surface as a missing-capture window, consistent with [CAPTURE.md](CAPTURE.md) §3.
- **Ordering:** activity samples are independent (each is a self-contained window), so strict order
  isn't required; the server keys by `capturedAt`. Screenshots likewise.

---

## 4. Auth & tenancy on every call

- All requests carry `Authorization: Bearer <token>` from the device credential
  ([ENROLLMENT.md](ENROLLMENT.md)). `orgId`/`userId` are taken from the **verified token claims**,
  never from the body — same rule as the backend (AUTH-RBAC §13). A body that disagrees is rejected.
- TLS only; certificate validation on; optional pinning for the API host.
- The proposed `monitoring:submit` permission lets a device write activity/screenshots/heartbeat
  **without** inheriting a human's full role — least privilege for an unattended credential.

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
