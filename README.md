# WorkPulse Desktop Agent

The third leg of WorkPulse (alongside `frontend/` and `backend/`). It runs on employee machines,
captures activity **counts and metadata only** (never keystrokes, clipboard, or audio — the privacy
invariant is structural), classifies at the edge, and ships one batch per cycle to the backend.

Built in **Rust** with a **Tauri 2** tray. The single source of truth is the LLD
([../backend/WorkPulse-LLD.md](../backend/WorkPulse-LLD.md), Appendix A) and the design docs under
[docs/](docs/) (`AGENT.md`, `CAPTURE.md`, `INGESTION.md`, `CONFIG.md`, `ENROLLMENT.md`,
`PRIVACY.md`, `UPDATES-SECURITY.md`).

## Three processes, one agent

| Process | Crate | Role |
|---|---|---|
| **`agentd`** | [`crates/agentd`](crates/agentd) | Headless core service. Owns the outbox, config cache, timer state machine, and the **only** network egress (`uploader`). Supervises helpers; funnels everything through the outbox. |
| **`capture-helper`** | [`crates/capture-helper`](crates/capture-helper) | Per-user, in-session. The only process that touches pixels/input. Produces per-minute rollups + screenshot metadata; hands them to the core over IPC. **No HTTP capability.** |
| **`tray`** | [`tray/`](tray) | The user-facing Tauri 2 app: consent, live capture indicator, timer, bounded pause. Thin remote control — no capture, no egress. |
| *shared* | [`crates/agent-shared`](crates/agent-shared) | Internal IPC message shapes + a re-export of the wire contract. |

Data flow: `capture-helper` → (IPC) → `agentd` outbox → `POST /v1/agent/batch` (one call/cycle) →
backend. Screenshots go S3-direct via presigned URLs the batch ack returns. Config and category
rules flow the other way, applied live without a restart.

## The wire contract lives in the backend repo

The agent↔backend envelope is the crate **`wp-agent-contract`**, owned by the **backend** repo
(`backend/crates/wp-agent-contract`) — the API authority. The backend `ingest` binary depends on it
by path (workspace member); the desktop agent compiles the *same* crate, so the wire format can't
drift. Locally it's a **path dependency** to the side-by-side backend checkout; cross-repo (CI /
release) it's a **tag-pinned git dependency into the backend repo** (with a local `[patch]` for dev).
See the note in [Cargo.toml](Cargo.toml).

```
parent-dir/
├─ desktop/   (this repo)
└─ backend/   (its own repo; owns crates/wp-agent-contract — the shared envelope)
```

Clone `backend/` beside `desktop/` before building.

## Build & test

Uses [`just`](https://github.com/casey/just) (`cargo install just`), but every recipe is a plain
cargo command.

```sh
just check      # cargo check --workspace --all-targets
just test       # cargo test --workspace
just ci         # fmt + clippy (-D warnings) + test — what CI runs on the core
just run-core   # run agentd locally
```

The **tray** builds separately (it needs the Tauri CLI) and is intentionally not a workspace member:

```sh
cargo install tauri-cli --version '^2'
just tray-dev   # or: cd tray/src-tauri && cargo tauri dev
```

## Status

Scaffold, all three processes **compile**. The core workspace (`agent-shared`, `agentd`,
`capture-helper`) passes its unit tests, and the `tray` builds with `cargo check` (clippy + fmt
clean). The slices are structured skeletons — OS capture (`xcap`, `active-win-pos-rs`, `user-idle`,
`rdev`), the SQLCipher outbox, real HTTP upload, device enrollment, and the tray↔core IPC channel
are marked with `TODO(...)` and are the implementation work ahead. Capture/upload logic is stubbed;
the privacy shape (counts-not-keys) is already enforced by the types.

## Two-developer split

The backend split (see [../backend/README.md](../backend/README.md)) has the agent as a **third
track** that both backend developers depend on through the shared `wp-agent-contract`:

- **Core track** — `agentd` (outbox, uploader, config sync, timer, supervision) + `capture-helper`
  (screenshot, input counts, focus, idle, classification). Pairs with backend **Dev A**
  (ingest / fleet / device auth), which consumes the same batch envelope.
- **UX track** — the `tray` app (consent, indicator, timer UI, pause) + enrollment/onboarding flow.

Both tracks share `agent-shared` + `wp-agent-contract`, jointly owned via PR review — the same rule
as the backend's shared foundation.
