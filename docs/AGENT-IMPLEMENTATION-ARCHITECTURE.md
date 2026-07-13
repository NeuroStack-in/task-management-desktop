# WorkPulse Desktop Agent — Implementation Architecture

> **Single consolidated reference.** This document unions the desktop-agent design docs in
> `desktop/docs/` into one end-to-end read, in implementation order. The individual source
> documents remain authoritative; this is their union plus the cross-cutting picture.
> Same role as the backend's [`BACKEND-IMPLEMENTATION-ARCHITECTURE.md`](../../backend/docs/BACKEND-IMPLEMENTATION-ARCHITECTURE.md).
>
> **At a glance:** Tauri 2.x · Rust core (capture, spool, scheduler, uploader, updater) + thin
> React/TS UI · Windows/macOS/Linux · offline-first SQLite spool · presigned-S3 screenshots +
> idempotent activity POST · per-device credential (proposed) · consent-gated, counts-not-content.
> **Design only — no implementation.**

## Contents
1. **Overview & Architecture** — goals, topology, process model, phasing. *(from [AGENT.md](AGENT.md))*
2. **Capture** — signals, sampling, cross-platform matrix, screenshot pipeline. *(from [CAPTURE.md](CAPTURE.md))*
3. **Ingestion** — round-trip data contract, upload protocol, offline spool. *(from [INGESTION.md](INGESTION.md))*
4. **Enrollment & Identity** — device credential proposal closing the backend's user-only-JWT gap. *(from [ENROLLMENT.md](ENROLLMENT.md))*
5. **Privacy & Ethics** — consent, control mapping, redaction, minimization. *(from [PRIVACY.md](PRIVACY.md))*
6. **Config & Policy Sync** — server-authoritative policy, enforcement. *(from [CONFIG.md](CONFIG.md))*
7. **Updates & Security** — auto-update, signing, endpoint hardening. *(from [UPDATES-SECURITY.md](UPDATES-SECURITY.md))*

---

## Part 1 — Overview & Architecture
> Source: [AGENT.md](AGENT.md)

WorkPulse has three legs: the **frontend** (built, mock-only), the **backend** (`backend/docs/`,
design-only), and this **desktop agent** — the missing producer of real activity/screenshot data.
The frontend already advertises it (enrollment token, per-OS installers, `/agents` UI); the backend
already assumes it ("future desktop agent → presigned PUT → S3"). This design fills the gap, as
**documentation only**.

**Topology:** a Tauri app with a **Rust core** (capture, SQLite spool, scheduler, uploader, identity,
updater, tray) and a **thin React/TS window** (enrollment, consent, settings, status, transparency).
The core is authoritative; the UI ships no business logic and reuses the frontend's Tailwind tokens.
The agent owns capture/buffer/upload; the backend owns persistence/scoring/thumbnailing/anomalies (off
the request path via EventBridge); the frontend reads results by polling. Agent and frontend never talk
directly.

**Principles:** feed existing contracts (don't invent shapes); consent first, surveillance never;
offline-first exactly-once; least privilege on the endpoint; drop in behind the REST seam; tenant-scoped
by token.

**Two backend gaps, flagged as proposals (need sign-off):** (1) device identity/auth — backend issues
only user JWTs; (2) ingestion endpoints — API.md §28 has only admin `agents:*` routes. Proposed additions:
`POST /agents/enroll`, `/agents/heartbeat`, `/screenshots/upload-url`, `/activity/ingest`, device-scoped
`GET /agents/policy`, and a wildcard-excluded `monitoring:submit` permission.

**Phasing:** A Skeleton (shell, tray, enroll+consent, heartbeat) → B Activity → C Screenshots →
D Policy & enforcement → E Updates.

---

## Part 2 — Capture
> Source: [CAPTURE.md](CAPTURE.md)

**Captured:** active app (process + optional title), foreground URL **host only**, active/idle,
keyboard **counts**, mouse **counts**/distance, periodic screenshot, machine health (os, version, cpu,
memory, ip, hostname). **Never captured (invariant):** keystroke values/keylogging, clipboard,
audio/camera, full URLs, file contents.

**Sampling:** activity every ~60 s (counts reset per window); screenshot every 10 min ± jitter
(off/15/10/5). Gated by quiet hours, capture-only-while-timer-running, idle threshold (5 min), and
consent. All values are server policy.

**Cross-platform matrix** (key caveats): macOS needs Screen Recording + Accessibility/Input-Monitoring
grants; **Wayland** screenshots are portal-gated and compositor-dependent (degrade to a recorded
missing-capture gap); denied permissions degrade to a warning health state, never silent failure.

**Screenshot pipeline (on-device):** grab → **redact/blur before spooling** (unredacted frame never
persists) → downscale/encode → metadata → spool. The `activity` score and `flagged` flag are
**server-derived**, not sent by the agent.

**Resource budget:** Rust core stays light (no resident Chromium); screenshot encoding is the heaviest
op, off-thread and rate-limited; the agent reports its own cpu/memory in the heartbeat.

---

## Part 3 — Ingestion
> Source: [INGESTION.md](INGESTION.md)

**The mapping table is the contract** — every agent field lands in a frontend-rendered field and a
backend item:
- **Activity sample** → `activity` item (`PK=ORG#<o>#USER#<u>`, `SK=ACT#<date>#<ts>`) → frontend
  `UsageItem`/`ActivitySeries` + keyboard/mouse buckets.
- **Screenshot** → S3 object in `wp-screenshots-<env>` + `screenshot` item (`SK=SHOT#<date>#<ts>`,
  `s3Key`, GSI1 by date, GSI2 when `flagged`) → `Screenshot { app, activity, flagged, date, time }`.
- **Heartbeat** → `agent` item (`SK=AGENT#<aid>`, status via `AGSTATUS` GSI) → `/agents` fleet row.

**Protocol:** `POST /activity/ingest` with per-batch `Idempotency-Key` (server stores key
`PK=ORG#<o>#IDEMP#<key>`, TTL ~24 h, `attribute_not_exists` — CONCURRENCY §1). Screenshots: `POST
/screenshots/upload-url` (server writes pending meta + key `org/<orgId>/user/<userId>/<date>/<id>.jpg`)
then direct `PUT` to S3; the `ObjectCreated` event drives thumbnail+scoring. `POST /agents/heartbeat`
every ~60 s, returns `policyVersion` for config-pull.

**Offline-first spool:** encrypted SQLite WAL queue; idempotency key assigned **at capture time** →
exactly-once on retry; oldest-first flush; exponential backoff with jitter; retryable vs terminal
errors distinguished; bounded size drops **oldest activity first and logs the drop** (never a silent
gap). Tenancy always from verified token claims, never the body.

---

## Part 4 — Enrollment & Identity
> Source: [ENROLLMENT.md](ENROLLMENT.md) — **proposed, needs backend sign-off**

The backend authenticates humans (user JWT); an unattended agent needs a **long-lived, revocable,
per-device credential** scoped to one user/org, able to write only monitoring data.

**Model:** a `DEVICE#<deviceId>` single-table item (reusing the `AGSTATUS` GSI), `deviceId == agentId`.
Credential via either Cognito per-device identity (preferred if revocation is clean) or a custom
device-token authorizer; access token short-lived, refresh secret long-lived in the OS keychain. New
**`monitoring:submit`** permission — device-scoped, **excluded from the `"*"` wildcard** like the
existing contributor-only exceptions.

**Flow:** admin issues an org-scoped, short-TTL, use-limited enrollment token (the existing
`AGENT_ENROLLMENT_TOKEN`) → agent `POST /agents/enroll` → server creates `DEVICE#`, binds `userId`,
returns credential + `policyVersion` → agent stores secret in keychain, shows consent, starts heartbeat.
A leaked enrollment token can at most create a visible, revocable device — not read/write tenant data.

**Lifecycle:** transparent refresh; admin **revoke** in `/agents` → terminal 401/403 → agent stops,
wipes credential+spool, prompts re-enroll; rotation on compromise; offboarding cascades.

---

## Part 5 — Privacy & Ethics
> Source: [PRIVACY.md](PRIVACY.md)

**Invariants:** counts/metadata never content; no capture without an active consent policy (even silent
mode requires consent); tenant isolation; transparency on demand.

**Control mapping** — every wireframe toggle has a defined agent behaviour: consent policy (hard gate),
notify/indicator (default on), silent mode (suppresses prompts, not consent), quiet hours (suspend
capture), idle prompt/auto-pause, blur, capture-only-while-timer, jitter, **monitoring exceptions**
(per-signal suppression with expiry), anonymize (drop titles, reduce fidelity), retention (server
lifecycle; agent keeps only until uploaded).

**Redaction** happens on the raw frame before spooling; heuristic regions first, full-frame blur for
denied apps/URLs, default-safe over-blur, limits stated openly. **Minimization** throughout. **Agency:**
user pause is auditable (no covert blind spots), mirroring the Remote Support governance framing.

---

## Part 6 — Config & Policy Sync
> Source: [CONFIG.md](CONFIG.md)

**Server is authoritative** — behaviour is remote policy, not local preference; a user can't widen their
own monitoring or escape block lists. Policy is assembled from existing admin areas (`/agents/settings`,
`/agents/config`, `/agents/policies`, `/settings/monitoring`, `/settings/tracking-rules`); the agent reads
a **merged, device-applicable** view via the proposed `GET /agents/policy` (+ `policyVersion`). Sync is
**polling** via the heartbeat's `policyVersion`; last policy is cached (encrypted) for offline; a
brand-new agent with no cache **fails closed** (captures nothing until policy + consent confirmed).

**Local enforcement:** categorization tags `UsageItem`s from cached rules (server may re-classify);
allow/block lists (glob patterns `*.facebook.com`, `steam://*`) drive warn/block actions per org policy,
all auditable. Honours admin proxy; no blind TLS-intercept trust.

---

## Part 7 — Updates & Security
> Source: [UPDATES-SECURITY.md](UPDATES-SECURITY.md)

**Auto-update** via the Tauri updater: channel check (`stable`/`beta` from `AgentSettings`), independent
**signature verification**, staged apply across the spool. **Version management** is server-driven —
staged rollout %, push, and rollback (incl. signed downgrade) per wireframe §28.7; version in every
heartbeat powers the adoption chart and the "outdated" badge (vs `LATEST_AGENT_VERSION`).

**Signing:** Authenticode (.msi), Developer ID + notarization (.dmg), GPG (.deb); updater key separate
from OS signing identity. **Endpoint security:** refresh secret in OS keychain; spool encrypted at rest;
content-free logs; TLS + optional pinning; runs unelevated with least-privilege OS grants; server-
authoritative policy for tamper resistance; kill/uninstall surfaces as `offline` + the existing
"Agent offline" anomaly. **Supply chain:** pinned/audited deps, signed artifacts, restricted release feed.

---

## Appendix — Proposed backend additions (consolidated)

| Item | Type | Doc |
|------|------|-----|
| `POST /agents/enroll` | endpoint (enrollment-token auth) | [ENROLLMENT.md](ENROLLMENT.md) |
| `POST /agents/token` (if custom authorizer) | endpoint | [ENROLLMENT.md](ENROLLMENT.md) |
| `POST /agents/heartbeat` | endpoint (`monitoring:submit`) | [INGESTION.md](INGESTION.md) |
| `POST /activity/ingest` | endpoint (`monitoring:submit`) | [INGESTION.md](INGESTION.md) |
| `POST /screenshots/upload-url` | endpoint (`monitoring:submit`) | [INGESTION.md](INGESTION.md) |
| `GET /agents/policy` | endpoint (device-scoped, merged policy + version) | [CONFIG.md](CONFIG.md) |
| `DEVICE#<deviceId>` item + `AGSTATUS` GSI reuse | data model | [ENROLLMENT.md](ENROLLMENT.md) |
| `monitoring:submit` permission (wildcard-excluded, device-scoped) | RBAC | [ENROLLMENT.md](ENROLLMENT.md) |

All other behaviour reads contracts that already exist in `frontend/` and `backend/docs/`.
