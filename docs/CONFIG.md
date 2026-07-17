# CONFIG.md — Settings & Remote Policy Sync

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. Defines what's configurable, where
> the truth lives (server, not the endpoint), how the agent pulls and caches policy, and how it
> enforces app/URL rules locally. Settings shapes reuse
> [`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts) and the wireframe config screens.

---

## 1. Principle: server is authoritative

The agent's behaviour is **remote policy**, not local preference. A monitored user editing files on
their machine must not be able to widen their own monitoring, disable capture, or escape a block list.
The agent treats server policy as the source of truth, caches it for offline use, and a local override
exists **only** for the few user-facing affordances the product intentionally grants (e.g. an auditable
pause — [PRIVACY.md](PRIVACY.md) §5).

---

## 2. Policy sources (server → agent)

The agent assembles its effective policy from existing admin-configured areas:

| Source (existing) | Provides |
|-------------------|----------|
| `GET /agents/settings` (`AgentSettings` — [`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts)) | `autoUpdate`, `updateChannel` (stable/beta), `offlineAlertMins`, `screenshotUpload` |
| **`GET /v1/agent/config`** (**live**) | `AgentConfig { version, tracking, rules }` — the one config endpoint. ETag-conditional (304 on match). |
| `GET /agents/policies` (wireframe §28.6) | named policies applied to teams/roles/device-groups |
| `/settings/monitoring` (wireframe §12) | idle thresholds, screenshot thresholds, productivity thresholds, work-hour rules, alert thresholds, monitoring policies, silent settings |
| `/settings/tracking-rules` (wireframe §13) | app/URL tracking, allow/block lists, categories, scoring rules, exceptions |

These are **admin** routes (perm `agents:view`/`agents:manage`, plus settings perms) already in
[API.md](../../backend/docs/API.md) §28 / settings. The agent reads a **resolved** view of them; how the
agent *fetches* its own effective policy (vs. an admin reading the raw settings) is the one proposed
addition — a device-scoped `GET /agents/policy` returning the merged, device-applicable policy +
a `policyVersion`. Marked **proposed** (needs backend sign-off), consistent with [AGENT.md](AGENT.md) §6.

---

## 3. Sync model (polling)

- The **batch ack** (`BatchAck.config_version`) carries the current version ([INGESTION.md](INGESTION.md) §2.1).
- If it differs from the cached one, the agent does an **ETag-conditional `GET /v1/agent/config`** and
  swaps the config in atomically — **live, no restart**.
- The token is a **sum** of the tracking + rules versions, deliberately: a `max()` would miss a
  rules-only bump while tracking is ahead, and using `tracking.version` alone — the original bug —
  means an admin's app/URL rules **never reach any agent**.
- **No WebSocket — and that is now the whole product, not a local choice.** WebSocket is **deferred**
  project-wide (2026-07-17, `backend/WorkPulse-HLD.md` §3 *Freshness*); the browser polls too. Nothing
  pushes to anything **today**, and a future push channel would be **server→browser first** — the
  agent's config-on-`BatchAck` rail is not what the migration is for.
- Effective propagation delay ≈ **one batch cycle (~300 s / 5 min)**, since config rides the
  `BatchAck`. ⚠️ *Corrected 2026-07-17: this line read "~60 s", which never matched
  [`AGENT.md`](AGENT.md) §4's 300 s cycle.* If "live permission reactivity" is read as **seconds**,
  the agent does not deliver it and never did — an admin's rule change lands **within ~5 minutes**.
  Set that expectation in the UI rather than implying instant.
- The agent **caches the last-known policy** (encrypted, in the spool DB) so it keeps enforcing correct
  rules while offline. A brand-new agent with no cache and no network captures nothing until it can pull
  policy + confirm consent (fail-closed).

---

## 4. Effective settings the agent holds

> ⚠️ **`Cadence` is redefined (2026-07-16).** It no longer means "how often we capture" — **the timer
> gate decides that**. `Cadence` is now **screenshot cadence while the timer runs, and nothing else**.
> It does not drive the batch cycle either (that is a fixed 300 s). `Off` = zero screenshots.

**`TrackingConfig`** (from `CONFIG#TRACK`) and **`AppUrlRules`** (from `CONFIG#RULES`) are the two
real items; `AgentConfig` bundles them behind one version token.

| Setting | Default | Source | Used by |
|---------|---------|--------|---------|
| `cadence` — **screenshot** interval, timer-gated | `Min5` (`Off / 3m / 5m / 10m`) | `TrackingConfig` | [CAPTURE.md](CAPTURE.md) screenshot loop |
| Screenshot jitter | ±60 s (fixed, not configurable) | code | anti-evasion |
| `blur_level` | 0 | `TrackingConfig` | on-device blur before upload |
| `retention_days` | 90 | `TrackingConfig` | server-side lifecycle |
| `silent` | false | `TrackingConfig` | **suppresses the tray capture indicator** — an admin policy on a privacy surface, so get it right |
| **`auto_update`** *(contract PR)* | true | `TrackingConfig` | [UPDATES-SECURITY.md](UPDATES-SECURITY.md) — org-level only, no per-device targeting |
| Idle threshold | 5 min (prompt), **15 min hard auto-stop** | user setting | idle prompt / session end |
| App/URL categories, `blocked`, `exceptions` | per org | `AppUrlRules` | local classification + enforcement |
| **`tasks_version`** *(contract PR)* | — | `BatchAck` | change token → `GET /v1/agent/tasks` (ETag), mirroring `config_version` |

**Capture-only-while-the-timer-runs is not a setting.** It is the product decision, enforced in code —
see [CAPTURE.md](CAPTURE.md) §2 and [PRIVACY.md](PRIVACY.md).
| Productivity thresholds / scoring | per org | §12.3 / §13.6 | **server-side scoring** (agent supplies raw context only) |
| App/URL categories + allow/block | per org | §13 | local enforcement (§5) |
| Monitoring exceptions | per user/team | §13.7 | per-signal suppression ([PRIVACY.md](PRIVACY.md)) |

Productivity scoring lives server-side so re-scoring on rule changes is possible; the agent never
computes the `activity` score (see [INGESTION.md](INGESTION.md) §1b).

---

## 5. Local app/URL enforcement

The tracking-rules (§13) drive two distinct jobs:

1. **Categorization** (productive/neutral/distracting) — the agent tags each `UsageItem` from its cached
   ruleset (incl. pattern rules like `*.github.com`) so offline samples are classifiable; the server may
   re-classify authoritatively on ingest.
2. **Allow/Block enforcement** (§13.3/§13.4) — for entries with an action:
   - **Block** → the agent records the attempt and surfaces a block (and, where the product intends
     enforcement, prevents/warns) per policy. In Phase-1 framing the frontend calls this "mock enforce";
     the real agent makes it actual, but enforcement scope (warn vs hard-block) is **org policy**, not a
     unilateral agent decision.
   - **Warn** → user-facing warning, logged, not blocked.
   Pattern matching supports the same globs the UI advertises (`*.facebook.com`, `tiktok.com`, `steam://*`).

Enforcement actions are themselves auditable events (they feed the backend audit log's "tracking events"
category, wireframe §26).

---

## 6. Proxy & network config

- Honour an admin-set HTTP(S) **proxy** (wireframe §28.5) and system proxy as fallback.
- Respect corporate TLS interception only via explicitly configured trust (no blind cert acceptance —
  [UPDATES-SECURITY.md](UPDATES-SECURITY.md)).
- All endpoints are the same API host the frontend uses; no separate ingestion domain unless the backend
  chooses one.

---

## 7. Sign-off note

The only **proposed** addition here is a device-scoped **`GET /agents/policy`** (merged, device-applicable
policy + `policyVersion`). Everything else reads existing admin-configured settings. Listed in
[AGENT.md](AGENT.md) §6 for backend ratification.
