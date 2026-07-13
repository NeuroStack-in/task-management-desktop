# UPDATES-SECURITY.md — Updates, Code Signing & Endpoint Security

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. Covers how the agent updates itself
> safely, how releases are signed and rolled out per the `/agents` version-management UI
> (wireframe [07-admin-security-support.md](../../frontend/Docs/wireframes/07-admin-security-support.md) §28.7),
> and how data and the agent itself are protected on the endpoint.

---

## 1. Auto-update

- **Mechanism:** the Tauri updater. The agent periodically checks a release feed for its channel,
  verifies the update's **signature**, downloads, and applies on next safe restart (staged — capture
  is not interrupted mid-sample; the spool persists across the swap).
- **Channels:** `stable` and `beta` — exactly the `UPDATE_CHANNEL_OPTIONS` in
  [`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts). The active channel comes from
  `AgentSettings.updateChannel`; `autoUpdate` toggles auto-apply (manual otherwise).
- **Current/target version:** the agent reports `version`; the frontend compares against
  `LATEST_AGENT_VERSION` (`"2.4.1"`) to render the "outdated ⬆" state in `/agents`.

## 2. Version management & rollout (server-driven)

Maps to wireframe §28.7 (release list · channel · rollout % · push/rollback):

- **Staged rollout:** the server gates which devices may move to a new version (rollout %, by
  channel/team). The agent asks "is an update available *for me*?" and the server answers per the
  rollout plan — the agent never force-pulls a version it isn't targeted for.
- **Rollback:** the server can target a *lower* version; the agent honours a pinned/forced version,
  including downgrade, when signed and instructed (admin "rollback" in §28.7).
- **Adoption telemetry:** version is in every heartbeat ([INGESTION.md](INGESTION.md) §1c), so the
  per-version adoption chart in §28.7 is real data.

## 3. Code signing (per OS)

Releases and installers are signed both for OS trust and for the updater's own signature check:

| OS | Installer | Signing |
|----|-----------|---------|
| Windows | `.msi` (`WorkPulseAgent-<v>.msi`) | Authenticode (EV cert recommended for SmartScreen) |
| macOS | `.dmg` (`WorkPulseAgent-<v>.dmg`) | Developer ID signing **+ notarization** (Gatekeeper) |
| Linux | `.deb` (`workpulse-agent_<v>_amd64.deb`) | repo/package GPG signing |

Installer file names/sizes match `AGENT_PLATFORMS` in
[`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts). The **updater signature key is separate
from the OS signing identity**; the updater verifies its own signature on every artifact before applying,
so a compromised mirror cannot push a malicious update.

---

## 4. Endpoint security

### 4.1 Secrets & data at rest
- **Device refresh secret** lives in the OS keychain (Keychain / Windows Credential Manager / Secret
  Service), never in a config file or log ([ENROLLMENT.md](ENROLLMENT.md)).
- **Local spool (SQLite)** is **encrypted at rest**; it holds queued samples + un-uploaded screenshot
  blobs only until upload, then deletes ([INGESTION.md](INGESTION.md) §3, [PRIVACY.md](PRIVACY.md) §4).
- **Logs** record events, never captured content; titles/URLs are scrubbed to host-only.

### 4.2 Transport
- TLS only, certificate validation on, optional pinning for the API host. Honours configured proxy
  ([CONFIG.md](CONFIG.md)); no blind acceptance of intercepting certs.

### 4.3 Least privilege
- Runs as the **logged-in user**, not elevated. Requests only the OS permissions each capability needs
  (macOS Screen Recording / Accessibility / Input Monitoring); denied permissions degrade gracefully and
  report a warning health state ([CAPTURE.md](CAPTURE.md) §3), never silently fail.

### 4.4 Tamper resistance
- Server policy overrides local files; a user editing local config cannot widen monitoring or escape
  block lists ([CONFIG.md](CONFIG.md) §1). Policy responses are integrity-protected (TLS; optionally signed).
- A user can **pause** (auditable) but cannot covertly disable reporting without it being visible as a
  paused/offline state in `/agents`.
- Killing the process / uninstalling shows as `offline` in `/agents` after `offlineAlertMins`; the
  `"Agent offline — no captures received"` anomaly already exists in
  [`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts) to surface exactly this.

### 4.5 Supply chain
- Pin and audit Rust crates and JS deps; reproducible builds where feasible; sign artifacts; restrict who
  can publish to the release feed. The updater's independent signature check is the last line of defence.

---

## 5. Threats addressed (summary)

| Threat | Mitigation |
|--------|------------|
| Malicious update / mirror compromise | independent updater signature verification + signed installers |
| Stolen device credential | keychain storage, short-lived access tokens, server-side revoke ([ENROLLMENT.md](ENROLLMENT.md)) |
| Local tampering to evade monitoring | server-authoritative policy, fail-closed without policy, visible offline/paused state |
| Data exfiltration from spool | encryption at rest, minimal retention, content-free logs |
| Privilege escalation | runs unelevated, least-privilege OS grants |
| Cross-tenant access | tenancy from verified token only ([INGESTION.md](INGESTION.md) §4) |
| Covert/non-consensual capture | consent-gated, default-on indicator, transparency view ([PRIVACY.md](PRIVACY.md)) |
