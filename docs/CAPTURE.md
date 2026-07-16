# CAPTURE.md — The Capture Engine

> Part of the [WorkPulse Desktop Agent](AGENT.md) design. Defines exactly what the
> agent samples from the endpoint, how often, and how it differs per OS. The guiding
> invariant: **counts and metadata, never content** (SPEC §2.4; wireframe
> [04-monitoring.md](../../frontend/Docs/wireframes/04-monitoring.md) — "counts only, no keylog").

---

## 1. What is captured

| Signal | What | Becomes |
|--------|------|---------|
| **Active app** | Foreground process name + window title (title is optional, redactable) | `Screenshot.app`, `UsageItem.name` (app) |
| **Foreground URL** | Domain/host of the active browser tab (host only, never full path/query) | `UsageItem.name` (website), enforced against allow/block lists |
| **Active vs idle** | Whether the user is present (input within the idle threshold) | activity `active`/`inactive` share; idle gaps |
| **Keyboard activity** | **Count of keystrokes per sample window** — a number, nothing else | keyboard buckets in [`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts) (`KEYBOARD_BY_HOUR`, …) |
| **Mouse activity** | Count of clicks + coarse movement distance per window | mouse buckets (`MOUSE_BY_HOUR`, …) |
| **Screenshot** | Periodic still of the screen(s), optionally blurred | `Screenshot` blob → S3; `activity` score + `flagged` |
| **Machine health** | OS + version, agent version, CPU %, memory %, local IP, hostname | `Agent { os, osVersion, version, cpu, memory, ip, hostname, lastSeen, status }` |

App/URL **category** (productive / neutral / distracting) is **not** decided on the endpoint
beyond what the cached ruleset says — categorization is policy-driven and authoritative
server-side (see [CONFIG.md](CONFIG.md)). The agent tags each `UsageItem` with the category its
cached ruleset yields so offline samples are still classifiable; the server may re-classify.

### What is never captured (invariant)
- **No keystroke values / keylogging.** Only a monotonic counter is read; the key identity is
  never inspected or stored. There is deliberately no API in the `capture` module that returns a key value.
- **No clipboard contents.**
- **No microphone / camera / audio.**
- **No full URLs** — host only; query strings and paths are dropped at the source.
- **No file contents or directory listings.**
- Window titles and screenshots can be **redacted/blurred** per policy ([PRIVACY.md](PRIVACY.md)).

---

## 2. Sampling model

> ⚠️ **Corrected 2026-07-16.** Capture-while-timer-running is **not** an option — it is **the
> product decision**. And `Cadence` no longer means "how often we capture": it means **screenshot
> cadence only**.

**The timer is the master gate.** No timer → **no sampling, no screenshots, no enforcement. Ever.**
This is enforced in code (`monitor::reflect()` starts/stops on `TimerEngine::is_running()`), not by
configuration, and it is the product's privacy stance made structural.

While the timer runs, three loops:

- **The 1 s tick** (a dedicated OS thread — blocking/`!Send` handles must stay off the async
  reactor). Reads idle seconds (`user-idle`) and **cumulative** keyboard/mouse counters
  (`device_query`), and every **5th** tick samples the foreground app (`x-win`).
- **Per-minute fold.** Ticks accumulate into a `MinuteBucket` keyed `epoch_ms / 60_000` → one
  **`ActivityRollup`** per minute (`keystrokes`, `mouse`, `active_sec`, `idle_sec`, `top_apps`
  capped at 30). Each tick adds a second to `active_sec` **or** `idle_sec`, so the invariant is
  **`active_sec + idle_sec ≤ 60`**. Buckets seal on the minute boundary, on timer-stop, and on a
  screenshot early-flush.
- **Screenshot** — every `Cadence::interval_secs()` (`Off / 3m / 5m / 10m`) **± up to 60 s of
  jitter**, so the timing isn't predictable or gameable. `Off` means the loop doesn't run at all.

**`device_query` is a 1 s sampler, not a low-level hook.** It polls rather than intercepting — which
is deliberate (it avoids `rdev`'s blocking listener and its macOS main-thread/run-loop requirement),
but it has a real consequence: **fast bursts are missed and `SPIKE_CAP: 1000` silently truncates**.
The counts are an **estimate**. Never present them in a UI or a report as an exact "keystroke count".

Idle threshold (default **5 min**) drives the "still working?" prompt, and a **hard auto-stop at 15
min** ends the session. Two seconds without input marks a tick idle (`IDLE_THRESHOLD_SECS: 2`).

**Exceptions carve-out:** if the focused app/site is in `exceptions`, the screenshot **and** the span
are suppressed — nothing about that window is recorded.

All cadence/threshold values are policy, pulled from the server and cached locally — see [CONFIG.md](CONFIG.md).
The agent uses a monotonic clock for scheduling (no dependence on wall-clock jumps from sleep/resume).

---

## 3. Cross-platform capability matrix

Each capture signal has a per-OS backend behind one Rust trait (`CaptureBackend`). Wayland is
called out separately because its screenshot/window model differs materially from X11.

| Capability | Windows 10/11 | macOS 12+ | Linux X11 | Linux Wayland |
|------------|---------------|-----------|-----------|---------------|
| Screenshot | ✅ DXGI/GDI | ✅ ScreenCaptureKit — **needs Screen Recording permission** | ✅ X11 grab | ⚠️ **Portal-gated** (`xdg-desktop-portal` ScreenCast); compositor-dependent, may prompt; some compositors unsupported → degrade to "no capture, flagged gap" |
| Active app / process | ✅ Win32 `GetForegroundWindow` | ✅ `NSWorkspace` — **needs Accessibility for titles** | ✅ EWMH `_NET_ACTIVE_WINDOW` | ⚠️ Limited; foreground title often unavailable → app-only, title omitted |
| Foreground URL | ✅ UIA / per-browser | ✅ AppleScript/AX per-browser — **needs Automation/Accessibility** | ✅ best-effort via window props | ⚠️ best-effort; often app-only |
| Idle detection | ✅ `GetLastInputInfo` | ✅ `CGEventSourceSecondsSinceLastEventType` | ✅ XScreenSaver ext | ✅ `ext-idle-notify` portal |
| Keyboard/mouse **counts** | ✅ low-level hook (count only) | ✅ `CGEventTap` — **needs Accessibility/Input Monitoring** (count only) | ✅ XInput2 (count only) | ⚠️ portal/`libinput`; may require extra permission |
| Autostart | ✅ Registry Run / Task Scheduler | ✅ LaunchAgent | ✅ XDG autostart / systemd-user | ✅ XDG autostart / systemd-user |
| Tray indicator | ✅ | ✅ menu-bar | ✅ (libappindicator) | ✅ (StatusNotifierItem) |
| Blur/redaction | ✅ post-capture | ✅ post-capture | ✅ post-capture | ✅ post-capture (applies to whatever was captured) |

**Permission handling.** On macOS the agent must request and verify **Screen Recording** and
**Accessibility/Input-Monitoring** grants on first run; if denied, the affected signals are
disabled and reported as a **degraded health state** (surfaced in `/agents` as a warning), never
silently dropped. On Wayland, when screenshots are portal-blocked the agent records a **missing-capture
gap** so the frontend's "Missing Screenshots" view (wireframe §11.7) and the
`"Agent offline — no captures received"` anomaly (already in [`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts))
reflect reality rather than appearing as zero activity.

---

## 4. Screenshot processing pipeline (on-device)

1. **Grab** the screen(s) at native resolution.
2. **Redact/blur** if `Blur sensitive content` is enabled — see [PRIVACY.md](PRIVACY.md) for the
   redaction strategy (heuristic regions + optional full-frame blur). Redaction happens **before**
   the image leaves the capture module; the unblurred frame is never spooled or uploaded.
3. **Downscale + encode** to a capped resolution / JPEG quality to hit installer-era sizes and
   keep upload cheap. (Thumbnails are generated server-side by the EventBridge worker — BACKEND §6
   — the agent uploads one full image.)
4. **Derive metadata**: `app`, `windowTitle?` (redactable), capture `date`/`time`, multi-monitor index.
5. **Hand to spool** as a screenshot job (blob + metadata) — see [INGESTION.md](INGESTION.md).

The **`activity` productivity score** and **`flagged`** fields on `Screenshot` are computed
**server-side** from the activity stream + scoring rules (wireframe §13.6), not by the agent — the
agent supplies the raw app/URL/active context the scorer needs. (`flagged` in
[`mock-insights.ts`](../../frontend/src/lib/mock-insights.ts) is derived, e.g. distracting app or low activity.)

---

## 5. Resource budget

The agent is always-on, so it must stay light (a reason Tauri/Rust was chosen over Electron):
- Idle CPU target negligible; capture spikes bounded and brief.
- Memory footprint small (no bundled Chromium; the settings webview is created on demand and can
  be torn down — the resident core is Rust).
- Screenshot encoding is the heaviest op; it runs off the UI thread and is rate-limited by the cadence.
- The agent reports its own `cpu` / `memory` in the heartbeat so the `/agents` health column is real.
