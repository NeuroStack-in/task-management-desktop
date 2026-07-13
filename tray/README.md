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
├─ ui/                      static webview (no bundler — plain index.html)
└─ src-tauri/
   ├─ Cargo.toml            depends on agent-shared (→ wp-agent-contract) by path
   ├─ tauri.conf.json       windowless tray; hidden 380×560 "panel" window
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
- `cargo install tauri-cli --version '^2'` (provides `cargo tauri`).
- Generate icons once: `cargo tauri icon <logo.png>` (see [src-tauri/icons/README.md](src-tauri/icons/README.md)).

## Develop

```sh
cd desktop/tray/src-tauri
cargo tauri dev      # launches the tray + panel against the static ui/
cargo tauri build    # produces the platform bundle
```

> The command handlers are currently stubs that return shaped state; wiring them to the live IPC
> channel to `agentd` is the next step (search `TODO(ipc)`).
