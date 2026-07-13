# ENROLLMENT.md — Device Enrollment & Identity

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. Defines how an agent goes from
> a fresh install to a trusted, tenant-scoped, revocable identity — and **closes the gap
> that the backend today issues only user JWTs, with no device identity model.**
>
> **Status: proposed — requires backend sign-off.** [AUTH-RBAC.md](../../backend/docs/AUTH-RBAC.md)
> has Cognito user pools, `custom:orgId`/`custom:roleId` claims, and groups, but **no
> machine/device credential**. This document specifies the missing piece for the backend to ratify.

---

## 1. The gap

- The backend authenticates **humans**: Cognito issues a user JWT (`sub`, `custom:orgId`,
  `custom:roleId`); the API authorizer validates it (BACKEND §4).
- An agent is **unattended**. It cannot interactively sign in, complete MFA, or hold a short-lived
  SPA access token the way the web app does. It needs a **long-lived, revocable, per-device
  credential** that is scoped to one user in one org and can write only monitoring data.
- The frontend already implies this flow: [`mock-agents.ts`](../../frontend/src/lib/mock-agents.ts)
  ships an `AGENT_ENROLLMENT_TOKEN` (`"wp_agent_…"`) and per-OS installers; the `/agents` UI is the
  place an admin would issue/revoke. The design just makes that real.

---

## 2. Proposed identity model

A **device** is a first-class, org-scoped entity bound to exactly one user.

- **DynamoDB item** (fits the single-table model, mirrors the existing `agent` item —
  [DATA-MODEL.md](../../backend/docs/DATA-MODEL.md)):
  ```
  PK = ORG#<orgId>
  SK = DEVICE#<deviceId>
  GSI1 = ORG#<orgId>#AGSTATUS / <status>#<deviceId>   (reuses the agent status index)
  data = { deviceId, userId, hostname, os, osVersion, agentVersion,
           enrolledAt, lastSeenAt, status, revoked, credentialRef }
  ```
  The `agent` management item (`SK=AGENT#<aid>`) and this `DEVICE#` item describe the same physical
  agent; keep `deviceId == agentId` so `/agents` management and the device credential line up.
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
- Tenancy is always from the verified token, never asserted by the agent — a device can only ever
  write data for its bound `orgId`/`userId`.

---

## 6. Sign-off checklist for the backend team

- [ ] Choose credential mechanism (Cognito device identity vs custom device-token authorizer).
- [ ] Add `DEVICE#<deviceId>` to the single-table model + reuse `AGSTATUS` GSI.
- [ ] Add `monitoring:submit` to the permission catalog as a wildcard-excluded, device-scoped permission.
- [ ] Implement `POST /agents/enroll`, `/agents/token` (if option 2), and wire the authorizer for device tokens.
- [ ] Define enrollment-token issuance/rotation under `agents:manage` in the `/agents` admin API.
