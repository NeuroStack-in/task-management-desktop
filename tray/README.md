# WorkPulse Tray

The employee-facing surface of the desktop agent — a small [Tauri 2](https://tauri.app) app.

It is deliberately **thin**: it holds no capture capability and no network egress. It shows the
monitoring/consent state and the live capture indicator, drives the personal timer, and lets the
user request a bounded pause. Every action is a `TrayCommand` sent to `agentd` over local IPC
(`agent_shared::ipc`); every value it displays is state the core owns. See the three-process model
in [../README.md](../README.md) and LLD Appendix A.

## Why it's not a cargo workspace member

Building it needs the Tauri CLI + a webview toolchain, which the headless core (`agentd`,
`capture-helper`) does not. Keeping it out of `desktop/Cargo.toml` means `cargo check`/`cargo test`
on the core stays fast and toolchain-free. This app is built on its own.

## Layout

```
tray/
├─ ui/                      Vite + React + TS panel (AGENT.md §3)
│  ├─ src/cards/            one component per surface, mirroring src-tauri/src/features/
│  ├─ src/components/ui/    shadcn components, copied verbatim from the web app
│  ├─ src/lib/agent.ts      the only seam to the core (invoke, or the dev mock)
│  └─ src/index.css         Tailwind v4 tokens, copied from the web app's `.dark` scheme
└─ src-tauri/
   ├─ Cargo.toml            depends on agent-shared (→ wp-agent-contract) by path
   ├─ tauri.conf.json       windowless tray; hidden 620×590 "panel" window (fixed size)
   ├─ capabilities/         Tauri 2 permissions for the panel window
   ├─ icons/                generate with `cargo tauri icon` (see icons/README.md)
   └─ src/
      ├─ main.rs            thin shim → lib::run()
      ├─ lib.rs             Tauri builder, tray icon/menu, command registration
      └─ features/          one slice per surface, backing the #[command]s:
         consent · capture_indicator · settings_view · pause · timer_ui
```

## Prerequisites

- Rust (see `../rust-toolchain.toml`) and the platform webview deps Tauri lists for your OS.
- Node + npm, for the panel UI (`ui/`).
- To run or bundle the app: `cargo install tauri-cli --version '^2'` (provides `cargo tauri`).
- Icons are already committed under [src-tauri/icons/](src-tauri/icons); regenerate with
  `cargo tauri icon <logo.png>` only if the brand mark changes.

## Build

`cargo check` / `cargo build` still work with just the Rust toolchain — they don't touch `ui/`.
The full app uses the Tauri CLI, which starts Vite itself via `beforeDevCommand`:

```sh
cd tray/src-tauri
cargo check          # Rust only; no Node needed
cargo tauri dev      # starts Vite on :1420, then the tray + panel
cargo tauri build    # runs `npm run build`, then bundles ui/dist
```

## Designing the panel

The panel is a normal web page, so the fast loop is a browser — no Rust rebuild, full HMR:

```sh
cd tray/ui && npm install && npm run dev   # http://localhost:1420
```

Because every `#[command]` is still a `TODO(ipc)` stub returning a frozen constant, the panel
would be undesignable against the real core (a timer that never ticks, consent that can never
be dismissed). So in dev it runs against a **fake core** — `src/lib/mock.ts`, switchable
between states from the bar at the bottom of the panel. Production builds always invoke the
real commands; `VITE_REAL=1 npm run dev` opts into them early. Delete `mock.ts` and the
`USE_MOCK` branch in `src/lib/agent.ts` once the IPC rail lands.

> The command handlers are currently stubs that return shaped state; wiring them to the live IPC
> channel to `agentd` is the next step (search `TODO(ipc)`).
