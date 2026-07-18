# RUNBOOK — running the agent against the live backend

How to take the desktop agent from "built" to "talking to the backend and visible in the fleet."
The code is complete (M0–M8); this is the config + verification that isn't code and can't be done
from a dev box without AWS access.

**Live dev backend:** API `https://oqlla6l5oc.execute-api.ap-south-1.amazonaws.com`, Cognito pool
`ap-south-1_0ep998OVt`, account `896823725438`, region `ap-south-1`, `--profile company`.

---

## The critical path (3 moves)

### 1. Backend — enable + deploy `USER_PASSWORD_AUTH` (one-time)
The agent logs in with client-side `USER_PASSWORD_AUTH`. The app client must allow that flow, and
the change must be **deployed**. (The code change is `infra/stacks/auth_stack.py` — committed on the
backend `feat/giri` branch; it just needs to ship.)

```bash
cd backend/infra && source .venv/Scripts/activate
cdk deploy -c env=dev --all --require-approval never --profile company
```
Verify the client now allows it:
```bash
aws cognito-idp describe-user-pool-client \
  --user-pool-id ap-south-1_0ep998OVt --client-id <APP_CLIENT_ID> \
  --profile company --query 'UserPoolClient.ExplicitAuthFlows'
# expect ALLOW_USER_PASSWORD_AUTH in the list
```

### 2. Desktop — set the App Client ID
Get the client id (a CDK stack output):
```bash
aws cloudformation describe-stacks --profile company \
  --query "Stacks[].Outputs[?OutputKey=='UserPoolClientId'].OutputValue" --output text
```
Then:
```bash
cd desktop
cp .env.example .env
# put the id in .env → WP_COGNITO_CLIENT_ID=<that value>
```
(`WP_INGEST_URL` / `WP_COGNITO_REGION` already default to the live dev stack.)

### 3. Run + sign in
```bash
just ui-install     # once per clone
just dev            # cargo tauri dev
```
Sign in as **`owner@acme.test`**. First login for an admin-created user hits
`NEW_PASSWORD_REQUIRED` — the UI collects a new password and completes the challenge automatically.
(If the owner has no permanent password yet: `just bootstrap-owner ap-south-1_0ep998OVt` on the backend.)

---

## What "working" looks like — verification checklist

| # | Milestone | How to confirm |
|---|-----------|----------------|
| M1 | **Auth** | Login succeeds; no `auth:not_configured` / `auth:cognito:*` in the agent log. The ID token decodes to string `tenant_id`/`perm` claims. |
| M2 | **Heartbeat rail** | Every ~300 s the agent POSTs `/v1/agent/batch`. The **AgentDevice** row appears (GSI6 fleet) with this host's `hostname`/`agent_version`/`idle`/`outbox_mb`; `last_seen` advances each cycle. |
| M2 | **Offline replay** | Kill the network ~10 min → on reconnect the outbox drains, `batch_seq` is gapless, **no duplicate** AgentDevice churn (watermark + idempotency). |
| M3 | **Timer** | Start a timer (project → task + description) → the `timer_started` event rides the next batch; a `TimeEntry` lands (SQS-folded), description included. |
| M4 | **Activity** | While the timer runs + consent granted, per-minute `ActivityRollup`s ride the batch (`active_sec + idle_sec ≤ 60`). `agent --dump-cycle` prints a sample offline. |
| M5 | **Screenshots** | With `Cadence ≠ Off` + consent, a WebP lands in the bucket: `aws s3 ls s3://wp-screenshots-dev/ --profile company`. `screenshots` off ⇒ **zero** shots (fails closed). |

**Consent gate:** capture (activity + screenshots) is **off by default** — call `set_consent(true)`
(the tray/UI grants it) or nothing is captured. This is intentional (PRIVACY.md, fails closed).

---

## Gotchas

- **`auth:not_configured`** → `WP_COGNITO_CLIENT_ID` is empty. Set it in `.env`.
- **`auth:cognito:400 … USER_PASSWORD_AUTH is not enabled`** → step 1 wasn't deployed.
- **`.env` not picked up** → it must sit in `desktop/` (or the cwd the binary runs from); dotenvy
  walks up from cwd. Real OS env vars override `.env`.
- **Nothing captured despite a running timer** → consent is off (default), or `Cadence::Off`, or (on
  Wayland) input/window tracking is an OS limit — screenshots-only there (CAPTURE.md risk #4).
- **Updates disabled** → `WP_UPDATER_PUBKEY` unset; that's expected until a signing key exists
  (UPDATES-SECURITY.md). The agent refuses unsigned updates by design.

---

## What still needs AWS access (can't be done from a plain dev box)

- Deploying the auth stack (step 1) and reading stack outputs — needs `--profile company`.
- Granting SES production access, and the updater signing key (`cargo tauri signer generate`) — both
  separate one-time setup, not required for agent↔backend communication.
