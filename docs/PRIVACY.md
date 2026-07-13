# PRIVACY.md — Consent, Ethics & Privacy Controls

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. WorkPulse's positioning is
> **"calm signal, not surveillance."** This document specifies how that stance is enforced
> *on the endpoint*, and maps every privacy affordance to the control the frontend already
> designed in [04-monitoring.md](../../frontend/Docs/wireframes/04-monitoring.md). Where the
> UI shows a toggle, the agent here defines the behaviour behind it.

---

## 1. Non-negotiable invariants

These are properties of the agent, not configurable away:

1. **Counts and metadata, never content.** No keylogging, no clipboard, no audio/camera, no full
   URLs, no file contents. Enforced in [CAPTURE.md](CAPTURE.md) — there is no code path that reads a
   keystroke value. ("counts only, no keylog" — wireframe §10.4/§10.5.)
2. **No capture without an active consent policy.** If the org's consent policy is disabled, the
   agent collects nothing — including in silent mode (silent mode *requires* consent enabled,
   wireframe §12.7).
3. **Tenant isolation.** Data is scoped to the bound `orgId`/`userId` from the verified credential
   ([ENROLLMENT.md](ENROLLMENT.md)); the agent cannot address another person or tenant.
4. **Transparency on demand.** The tray exposes a "what is being collected" view so the monitored
   person can always see the agent's current behaviour (status, what's captured, when).

---

## 2. Control mapping (UI toggle → agent behaviour)

| Control (wireframe) | Agent behaviour |
|---------------------|-----------------|
| **Consent statement + policy** (§12.6) | Capture is hard-gated on the policy being active; the consent text is shown at first run and from the tray. |
| **Notify employees monitoring is active** (§12.7, default on) | First-run + persistent disclosure; default-on tray indicator. |
| **Show tray indicator** (§12.7) | Visible menu-bar/tray icon reflecting active/idle/paused; recommended on. |
| **Silent mode** (§12.7) | Suppresses *active prompts* but **not** the consent requirement; only available when consent policy is enabled. Even silent, the transparency view remains accessible. |
| **Quiet hours** (§12.7, e.g. 22:00–06:00) | No capture during the window — screenshots, activity, and input counts all suspended. |
| **Idle thresholds + "prompt user on idle" / "auto-pause timer on idle"** (§12.1) | Idle flips active→inactive after the threshold; optionally prompts and/or pauses the time-tracking timer. |
| **Blur sensitive content** (§11.8) | On-device redaction *before* the image is spooled — see §3. |
| **Capture only while timer running** (§11.8) | Screenshots (and optionally activity) only while the user's WorkPulse timer is active. |
| **Randomized threshold / jitter** (§11.8) | Capture cadence jittered ± the configured minutes so timing isn't predictable/gameable. |
| **Monitoring exceptions** (§13.7: who · what's exempt · reason · expiry) | The agent honours per-user/per-team exemptions in its cached policy: an exempt signal (screenshots / apps / urls) is **not captured at all** while the exception is active; expiry re-enables it. |
| **Anonymize toggle** (§12.6) | When set, the agent omits window titles and reduces screenshot fidelity/redacts aggressively; identity still flows for attribution but content is minimized. |
| **Data retention period** (§12.6) | Enforced server-side (S3 lifecycle, BACKEND §6); the agent keeps only what's needed to upload, then deletes from the local spool. |

---

## 3. Redaction / blur strategy

When `Blur sensitive content` (or `Anonymize`) is on, redaction happens **inside the capture module,
on the raw frame, before anything is written to the spool or uploaded** — the unredacted image never
persists or leaves the device.

- **Heuristic regions first:** blur likely-sensitive UI (password fields, known finance/banking app
  windows, OS credential dialogs) where the OS exposes enough to locate them.
- **App/URL deny → full-frame:** if the foreground app/URL is on a "do not screenshot" exception or a
  blocked category, blur the **entire** frame (or skip the shot and record a deliberate gap), rather
  than risk leaking content.
- **Default-safe:** when region detection is uncertain, prefer over-blurring. A degraded-but-safe shot
  beats a sharp leak.
- **Window titles** are redactable independently (they often contain document names, customer names);
  `Anonymize` drops them.

Redaction is best-effort by nature; the design states its limits openly rather than implying perfect
sensitive-data detection.

---

## 4. Data minimization

- Host-only URLs (never path/query), app names (never file contents), input **counts** (never values).
- Screenshots downscaled/compressed to the minimum useful fidelity ([CAPTURE.md](CAPTURE.md) §4).
- Local spool holds data only until uploaded, then deletes; it is encrypted at rest
  ([UPDATES-SECURITY.md](UPDATES-SECURITY.md)).
- The agent logs **events** (enrolled, paused, upload failed) but not **content**; logs are scrubbed of
  titles/URLs beyond host.

---

## 5. User-facing transparency & agency

- **Persistent indicator** (default on) so monitoring is never hidden by default; silent mode is an
  explicit, consent-gated org choice, not a covert default.
- **Transparency view** from the tray: current status, which signals are on/off, quiet-hours window,
  and "monitoring is active because: <org consent policy>".
- **Pause semantics:** any user-initiated pause is itself an auditable event (the org sees that capture
  was paused and when) — agency without creating blind spots that look like data loss.
- This deliberately mirrors the Remote Support Center's governance framing
  ([PLAN-remote-support.md](../../frontend/Docs/PLAN-remote-support.md)): no silent access, everything leaves a trail.

---

## 6. Compliance posture (informative)

Not legal advice, but the design is built to support common obligations: explicit consent capture,
data minimization, retention limits, the right to see what's collected (transparency view), and
revocation/erasure (device revoke + server retention). Jurisdiction-specific configuration (e.g.
works-council constraints, regional consent text) is expressed through the existing
consent-statement editor and monitoring policies (§12.6), not hard-coded.
