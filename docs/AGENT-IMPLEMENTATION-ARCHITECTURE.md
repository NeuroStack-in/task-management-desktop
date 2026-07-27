# Desktop Agent — Implementation Architecture — 🗂️ INDEX ONLY

> **This file no longer contains a design.** It was a consolidation that unioned the docs in
> `desktop/docs/` into one end-to-end read. On **2026-07-16** those sources were rewritten for the
> **1-process, timer-gated** agent modelled on [`REFERENCE-TASKFLOW.md`](REFERENCE-TASKFLOW.md) — which left
> this file a frozen snapshot of an architecture that no longer exists.
>
> It described: a 3-process split · an SQLite/SQLCipher spool · an `Idempotency-Key` header ·
> `POST /activity/ingest` + a two-step screenshot presign · a per-device credential · `orgId` keys ·
> "design only — no implementation". **Every one of those is now wrong.**
>
> Rather than maintain a second copy that silently re-drifts from its sources — the exact failure that
> put the whole repo here — it is reduced to an index. *(The backend did the same with its own
> [`BACKEND-IMPLEMENTATION-ARCHITECTURE.md`](../../backend/docs/BACKEND-IMPLEMENTATION-ARCHITECTURE.md).)*

## Read these instead

| # | Topic | Doc |
|---|---|---|
| — | **Start here.** Architecture, the 1-process model, the LLD Appendix A amendment + what it costs | [`AGENT.md`](AGENT.md) |
| — | **Build sequence (M0–M8)** + the current state (M0–M8 implemented; compiles + builds installers) | [`BUILD-PLAN.md`](BUILD-PLAN.md) |
| 1 | Capture — the timer gate, the 1 s sampler, per-minute rollups, screenshots | [`CAPTURE.md`](CAPTURE.md) |
| 2 | Ingestion — `POST /v1/agent/batch`, `(agent_id, batch_seq)`, the jsonl outbox | [`INGESTION.md`](INGESTION.md) |
| 3 | Config & policy — the ETag rail, the redefined `Cadence` | [`CONFIG.md`](CONFIG.md) |
| 4 | Privacy & ethics — counts-not-content, the timer gate, exceptions | [`PRIVACY.md`](PRIVACY.md) |
| 5 | Identity — the user's Cognito ID token (batch auth); the per-install X.509 device credential now issued for the **MQTT rail** (device-JWT batch auth still **deferred**) | [`ENROLLMENT.md`](ENROLLMENT.md) |
| 6 | Updates & endpoint security — fail-closed updater, host pinning | [`UPDATES-SECURITY.md`](UPDATES-SECURITY.md) |

**Authority:** [`WorkPulse-LLD.md`](../../backend/WorkPulse-LLD.md) +
[`WorkPulse-HLD.md`](../../backend/WorkPulse-HLD.md) win on any conflict — **except LLD Appendix A**,
which [`AGENT.md`](AGENT.md) §0 amends (3 processes → 1).

**Reference implementation:** [`REFERENCE-TASKFLOW.md`](REFERENCE-TASKFLOW.md) — TaskFlow Desktop v0.1.4, a
complete shipping agent in the same product category. We take its architecture, libraries and UX; we
speak our own wire contract.
