# ENROLLMENT.md — Device Enrollment & Identity

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. Defines how an agent goes from
> a fresh install to a trusted, tenant-scoped, revocable identity — and **closes the gap
> that the backend today issues only user JWTs, with no device identity model.**
>
> **Status (updated 2026-07-27): device *auth* for batches is still DEFERRED; a per-install device
> *credential* now exists — but for the MQTT rail, not for batch auth.** `POST /v1/agent/batch` is
> still a plain user-JWT route (below). What changed: the agent now also calls **`POST /v1/agent/enroll`**
> once per install (first signed-in run) and gets a **per-install X.509 credential** — an IoT Thing name,
> cert + private key, broker endpoint, and topics — persisted in the OS keyring (`api/enroll.rs`). That
> credential authenticates the **MQTT downlink only** (mutual-TLS to AWS IoT Core for
> `config_changed` / `capture_now` / presence — `mqtt/`, backend MQTT-MIGRATION Phase 3). Batches are
> untouched by it. The device-JWT / `monitoring:submit` model this document proposes remains unbuilt.
>
> **What ships for batch auth:** the agent **signs in as the user** with the same Cognito login the web
> app uses, and sends the **ID token** (the claims ride the ID token, not the access token). Tokens live
> in the **OS keyring**; refresh is automatic and de-duplicated; a 401 tears the session down. So
> `POST /v1/agent/batch` is a normal user-JWT route and each batch is attributed to `auth.user_id`.
> **There is no device credential in the batch path and no `monitoring:submit` permission.**
>
> **`agent_id` is not a credential.** It is a **per-install UUIDv4**, generated once at first boot and
> persisted next to the outbox sequence. It *identifies* the install; it does not *authenticate* it.
> It must be born and die with the sequence — delete one and `batch_seq` restarts at 1, so every batch
> is silently rejected as a duplicate.
>
> ⚠️ **Server-side hardening this implies:** `agent_id` is a self-declared payload string and
> `AgentDevice` is keyed `DEVICE#<agent_id>` under the tenant — so a spoofed id **clobbers another
> device's row**. The server should bind `agent_id → user_id` on first sight and reject mismatches.

---

## 1. The gap — restated (2026-07-16)

- The backend authenticates **humans**: Cognito issues a user JWT; the API Gateway JWT authorizer
  validates it ([AUTH-RBAC.md](../../backend/docs/AUTH-RBAC.md)). The tenant claim is **`tenant_id`** —
  *not* `orgId`; `custom:orgId` is only the Cognito **attribute** name.
- **The agent is no longer unattended.** Capture is **timer-gated** — no timer, no capture — so a
  human is signed in and present whenever anything is recorded. The original premise of this document
  ("an agent cannot interactively sign in") **no longer holds**: it can, because it only ever runs
  while someone did.
- That removes the *need* for a long-lived machine credential. It remains a **future upgrade** for
  headless / independently-revocable operation — e.g. if tamper-resistance or capture-without-a-user
  is ever required (see [AGENT.md](AGENT.md) §0, which records what the 1-process model gives up).

---

## 2. Proposed identity model *(deferred — for the backend to ratify if revived)*

A **device** as a first-class, tenant-scoped entity bound to one user.

- **DynamoDB item** — keys corrected to the real single-table shape
  ([DATA-MODEL.md](../../backend/docs/DATA-MODEL.md)); the live `AgentDevice` item already uses these:
  ```
  PK    = TENANT#<tenant_id>            # NOT ORG#<orgId>
  SK    = DEVICE#<agent_id>
  GSI6  = TENANT#<tenant_id>#FLEET / HB#<last_heartbeat:020>   # the fleet/stale index
  attrs = { user_id, hostname, os, agent_version, last_heartbeat,
            idle_flag, cpu_pct, mem_pct, outbox_mb, status }
  ```
  There is one item per agent — `DEVICE#<agent_id>` **is** the fleet item (§18); there is no separate
  `AGENT#<aid>` record. A revived credential would hang off this item, not a parallel one.
- **Credential.** Two acceptable implementations, to be chosen with the backend:
  1. **Cognito device / per-device refresh token** — a confidential, machine-scoped Cognito identity
     whose pre-token-generation trigger injects `custom:orgId`, `custom:userId`, and a
     `custom:deviceId`, plus the scope `monitoring:submit`. Reuses the existing authorizer unchanged.
  2. **Custom device-token authorizer** — the device holds an opaque long-lived refresh secret
     (stored server-side hashed); a `POST /agents/token` exchanges it for a short-lived access JWT.
     A Lambda authorizer validates it. More code, fewer Cognito constraints.
  **Recommendation:** option 1 if Cognito's machine-identity ergonomics allow per-device revocation
  cleanly; otherwise option 2. Either way the **access token is short-lived and refreshed**; only the
  refresh secret is long-lived, and it lives in the OS keychain.

- **New permission: `monitoring:submit`** (proposed) — grants *write* of activity/screenshots/heartbeat
  for the bound `userId` only. It is **device-scoped**, deliberately *not* part of any human role and
  *not* granted by the `"*"` wildcard (treat like the existing `CONTRIBUTOR_ONLY` exceptions in
  [AUTH-RBAC.md](../../backend/docs/AUTH-RBAC.md), so an org owner's `"*"` never implicitly turns a
  person into a data-submitting device). Admin management of devices stays under the existing
  `agents:view` / `agents:manage`.

---

## 3. Enrollment flow

```
Admin (/agents UI)                 Employee machine                 Backend
─────────────────                  ────────────────                 ───────
 issue enrollment token  ───────▶  installs agent
 (org-scoped, short TTL,            first run:
  single/limited use)              paste token + sign in once
                                   (or token carries a one-time
                                    user-bind code)
                                          │  POST /agents/enroll
                                          │  { enrollmentToken, hostname,
                                          │    os, osVersion, agentVersion }
                                          ▼
                                                          validate token (org, TTL, uses)
                                                          create DEVICE# item, bind userId
                                                          mint device credential
                                          ◀───────────────  { deviceId, refreshSecret,
                                                              accessToken, policyVersion }
                                   store refreshSecret in
                                   OS keychain; show consent
                                   screen; begin heartbeat
```

- **`POST /agents/enroll`** *(proposed; no prior auth — the enrollment token IS the auth)*:
  consumes the org-scoped `AGENT_ENROLLMENT_TOKEN`, creates the `DEVICE#` item, binds it to the user,
  returns the device credential + initial `policyVersion`.
- **Token scoping.** Enrollment tokens are **org-scoped, short-TTL, and use-limited** (rotate via the
  `/agents` UI). They identify the org and authorize *device creation only* — never data submission.
  A leaked token can at most enroll an extra device (visible in `/agents`, revocable), not read or write tenant data.
- **User binding.** The token either (a) carries a one-time bind to a specific user (admin pre-assigns
  the machine to an employee), or (b) requires one interactive user sign-in at first run to establish
  `userId`. Choice is an org policy; both are supported.

---

## 4. Credential lifecycle

| Event | Behaviour |
|-------|-----------|
| **Refresh** | Short-lived access token refreshed from the keychain-held refresh secret before expiry; transparent to capture. |
| **Revoke** | Admin clicks revoke in `/agents` → `DEVICE#` item `revoked=true`; next agent call gets a terminal `401/403`; agent stops capture, wipes local credential + spool, shows re-enroll prompt. |
| **Rotate** | Server can force a refresh-secret rotation (compromise response); agent re-exchanges on next refresh. |
| **Re-image / reinstall** | New install = new enrollment = new `deviceId`; old device goes `offline` then can be revoked/pruned. |
| **User offboarding** | Disabling the bound user cascades: device credential invalidated; agent stops. |

---

## 5. Trust boundaries

- The **refresh secret** is the only long-lived secret on the endpoint; it sits in the OS keychain,
  not in a config file, and never in the spool DB or logs.
- The agent trusts the **server's policy** (signed responses / TLS) over any local config; a user
  cannot widen their own monitoring or escape allow/block lists by editing local files
  (see [CONFIG.md](CONFIG.md), [UPDATES-SECURITY.md](UPDATES-SECURITY.md)).
- Tenancy is always from the verified token, never asserted by the agent — a device could only ever
  write data for its bound `tenant_id`/`user_id`. *(This holds today too: the agent sends the user's
  Cognito ID token and the server reads `tenant_id` from the claims, never from the body.)*

---

## 6. Sign-off checklist for the backend team

- [ ] Choose credential mechanism (Cognito device identity vs custom device-token authorizer).
- [ ] Add `DEVICE#<deviceId>` to the single-table model + reuse `AGSTATUS` GSI.
- [ ] Add `monitoring:submit` to the permission catalog as a wildcard-excluded, device-scoped permission.
- [ ] Implement `POST /agents/enroll`, `/agents/token` (if option 2), and wire the authorizer for device tokens.
- [ ] Define enrollment-token issuance/rotation under `agents:manage` in the `/agents` admin API.
