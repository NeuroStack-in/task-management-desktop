# WorkPulse Desktop Agent

The third leg of WorkPulse (alongside `frontend/` and `backend/`). It runs on employee machines,
captures activity **counts and metadata only** (never keystrokes, clipboard, or audio — the privacy
invariant is structural), classifies at the edge, and ships one batch per cycle to the backend.

Built in **Rust** with a **Tauri 2** shell + a **Preact** webview. The authority is
[docs/BUILD-PLAN.md](docs/BUILD-PLAN.md) and [docs/AGENT.md](docs/AGENT.md) (which **amend LLD
Appendix A** — see below), plus the companion docs under [docs/](docs/) (`CAPTURE.md`, `INGESTION.md`,
`CONFIG.md`, `ENROLLMENT.md`, `PRIVACY.md`, `UPDATES-SECURITY.md`).

## One process (amends LLD Appendix A)

LLD Appendix A specified **three** processes (a headless `agentd` service + a per-user
`capture-helper` + a tray). **BUILD-PLAN §0 amends that to ONE Tauri process**, because capture is
**timer-gated** — the user is always present, so the split's only benefits (capture with no user
present, survive-logout, Session-0) don't apply, and it costs a service installer + an IPC surface +
per-OS session plumbing. The tradeoff on the record: **no tamper-resistance** (the user can kill the
process and tracking stops). `monitor/` is kept **free of Tauri types** so a daemon can be split back
out later without a rewrite.

```
desktop/
├─ Cargo.toml            # workspace, members = ["src-tauri"]
├─ ui/                   # Preact + Vite + Tailwind + TS webview
└─ src-tauri/
   └─ src/
      ├─ lib.rs main.rs  # lib split so integration tests can link the core
      ├─ clock.rs error.rs events.rs lifecycle.rs window_size.rs
      ├─ state.rs        # Tauri-managed AppState (timer · outbox · config)
      ├─ config/         # AgentConfig cache; ETag pull on version mismatch
      ├─ auth/           # Cognito USER_PASSWORD_AUTH + chunked OS-keyring tokens
      ├─ api/            # reqwest: POST /v1/agent/batch, config pull, S3 PUT, enroll
      ├─ commands/       # the webview's #[command] surface (mod + panel)
      ├─ timer/engine.rs # one running session max; carries started_at
      ├─ outbox/         # queue/batches.jsonl (seq + watermark prune); per-install agent_id
      ├─ monitor/        # input (counts-only) + idle/active_window/screenshot/session/bucket
      ├─ rules/          # on-device app/URL classifier
      ├─ mqtt/           # AWS IoT downlink: config_changed / capture_now / presence
      └─ updater/        # signed GitHub-Releases self-update
```

Data flow: `monitor` (timer-gated) → per-minute `ActivityRollup`s + events → `outbox` →
`POST /v1/agent/batch` (one call / 300 s cycle) → backend. Screenshots go S3-direct via presigned
URLs the batch ack returns. Config + category rules flow back on the ack's `config_version` → ETag
pull, applied live without a restart.

## The wire contract lives in the backend repo

The agent↔backend envelope is the crate **`wp-agent-contract`**, owned by the **backend** repo
(`backend/crates/wp-agent-contract`) — the API authority. The backend `ingest` binary depends on it
too, so the wire format **can't drift**. Locally it's a **path dependency** to the side-by-side
backend checkout; CI/release **pins a tag** (git dep + a local `[patch]` for dev). See the note in
[Cargo.toml](Cargo.toml) — pinning it is non-negotiable (an unpinned path-dep silently drifted once
already).

```
parent-dir/
├─ desktop/   (this repo)
└─ backend/   (its own repo; owns crates/wp-agent-contract — the shared envelope)
```

Clone `backend/` beside `desktop/` before building — the folder **must** be named `backend`.

## First-time setup

**Prerequisites:** Rust stable (see [rust-toolchain.toml](rust-toolchain.toml)), **Node + npm** (the
webview is a Vite/Preact app), and your OS's webview deps — Windows 10/11 already ship WebView2.

```sh
mkdir workpulse && cd workpulse
git clone <backend-repo-url> backend      # folder MUST be named `backend`
git clone <this-repo-url> desktop
cd desktop

cargo install tauri-cli --version '^2'    # one-off; skip if `cargo tauri --version` answers
just ui-install                           # webview deps (node_modules is not committed)
just dev                                  # cargo tauri dev
```

Getting `failed to load source for dependency wp-agent-contract`? There's no `backend/` beside this
repo (the folder name matters).

## Build & test

Uses [`just`](https://github.com/casey/just), but every recipe is a plain cargo/npm command.

```sh
just check   # cargo check --workspace --all-targets
just test    # cargo test --workspace
just ci      # fmt + clippy (-D warnings) + test — what CI runs on the Rust core
just dev     # run the app (Tauri CLI + ui/ deps required)
just build   # packaged installer
```

## Status — M0–M8 implemented (as of 2026-07-27)

The single-process workspace **compiles** (`cargo check --workspace`), its **unit tests pass**
(`just test` — ~74 `#[test]`s across auth/api/monitor/timer/outbox/rules/…), and a local
`cargo tauri build` produces `workpulse-agent.exe` plus **NSIS and MSI installers** under
`target/release/`. The privacy shape (counts-not-keys) is enforced by the types.

All the milestones from [docs/BUILD-PLAN.md](docs/BUILD-PLAN.md) are built:

- **M1** Auth — Cognito `USER_PASSWORD_AUTH` (hand-rolled over `reqwest`) + chunked OS-keyring tokens.
- **M2** Heartbeat rail — cycle + jsonl outbox + `POST /v1/agent/batch` (the one milestone the live backend confirms today).
- **M3** Timer + project→task selector (mandatory description, meeting mode).
- **M4** Monitor — 1 s thread (`device_query` / `user-idle` / `x-win`), per-minute buckets, idle prompt + hard auto-stop.
- **M5** Screenshots — `xcap` (all displays) → WebP + blur + pHash → host-pinned S3 PUT.
- **M6** Shell — tray reflection, minimize-to-tray, auto-sign-out, autostart, single-instance, per-feature entitlement gating.
- **M7** Updater — signed GitHub Releases (SHA-256 + Ed25519), pubkey baked in, periodic check.
- **M8** Tests + CI — `.github/workflows/ci.yml` + `release.yml`; goldens, bucket rotation, outbox replay, classifier precedence, `clippy -D warnings`.

**Beyond the M0–M8 plan:** per-install **device enrollment** (`POST /v1/agent/enroll` → X.509
credential in the keyring) and an **MQTT downlink** (AWS IoT Core mutual-TLS: `config_changed` /
`capture_now` + presence) — the agent side of backend MQTT-MIGRATION Phase 3. Screenshot cadence now
also supports a **`Custom`** interval alongside the `Off / 3m / 5m / 10m` presets.

What still can't be checked from a dev box is the live-backend confirmation — see
[docs/RUNBOOK.md](docs/RUNBOOK.md).

Live dev backend: `https://oqlla6l5oc.execute-api.ap-south-1.amazonaws.com` (pool `ap-south-1_0ep998OVt`).
