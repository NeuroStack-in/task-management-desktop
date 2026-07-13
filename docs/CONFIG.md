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
| `GET /agents/config` (wireframe §28.5) | capture intervals, idle threshold, upload cadence, proxy, update channel |
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

- The **heartbeat response** carries the current `policyVersion` ([INGESTION.md](INGESTION.md) §2.3).
- If it differs from the cached version, the agent fetches the merged policy and atomically swaps it in.
- No WebSocket (matches BACKEND §1). Effective propagation delay ≈ one heartbeat interval (~60 s) —
  fast enough for "live permission reactivity" the way the frontend expects.
- The agent **caches the last-known policy** (encrypted, in the spool DB) so it keeps enforcing correct
  rules while offline. A brand-new agent with no cache and no network captures nothing until it can pull
  policy + confirm consent (fail-closed).

---

## 4. Effective settings the agent holds

| Setting | Default | Source | Used by |
|---------|---------|--------|---------|
| Screenshot frequency | 10 min | §11.8 / `/agents/config` | [CAPTURE.md](CAPTURE.md) scheduler |
| Capture jitter | ±3 min | §11.8 | scheduler (randomized threshold) |
| Capture only while timer running | off | §11.8 | scheduler gate |
| Blur sensitive content | per org | §11.8 / §12.2 | [CAPTURE.md](CAPTURE.md)/[PRIVACY.md](PRIVACY.md) redaction |
| Idle threshold | 5 min | §12.1 | active/idle flip, timer auto-pause |
| Quiet hours | per org | §12.7 | scheduler gate |
| Silent mode | off | §12.7 | indicator/prompt suppression (consent-gated) |
| `screenshotUpload` | true | `AgentSettings` | enables/disables the screenshot stream |
| `offlineAlertMins` | 30 | `AgentSettings` | server status derivation (agent reports heartbeats) |
| `autoUpdate` / `updateChannel` | true / stable | `AgentSettings` | [UPDATES-SECURITY.md](UPDATES-SECURITY.md) |
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
