# WorkPulse desktop agent — task runner. Run `just` to list.
# The core (agentd + capture-helper) and the tray build separately (see README).

default:
    @just --list

# ---- core (headless): agentd + capture-helper + agent-shared ----

# Type-check the core workspace.
check:
    cargo check --workspace --all-targets

# Run all core unit tests.
test:
    cargo test --workspace

# Lint (matches CI: warnings are errors).
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Format check / apply.
fmt:
    cargo fmt --all --check
fmt-fix:
    cargo fmt --all

# Everything CI runs on the core, in order.
ci: fmt clippy test

# Run the headless core service locally.
run-core:
    cargo run --bin workpulse-agentd

# Run the capture helper locally (normally spawned by the core).
run-helper:
    cargo run --bin workpulse-capture-helper

# ---- tray (Tauri 2; needs `cargo install tauri-cli`) ----

tray-dev:
    cd tray/src-tauri && cargo tauri dev

tray-build:
    cd tray/src-tauri && cargo tauri build

# Regenerate tray icons from a source logo (see tray/src-tauri/icons/README.md).
tray-icons logo:
    cd tray/src-tauri && cargo tauri icon {{logo}}
