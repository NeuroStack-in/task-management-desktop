# WorkPulse desktop agent — task runner. ONE Tauri process (BUILD-PLAN §0/§1). Run `just` to list.

default:
    @just --list

# Type-check the workspace.
check:
    cargo check --workspace --all-targets

# Run all unit tests.
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

# Everything CI runs on the Rust core, in order.
ci: fmt clippy test

# Run the agent (needs the Tauri CLI + webview deps: `just ui-install` and `cargo install tauri-cli`).
dev:
    cargo tauri dev

# Produce the platform bundle.
build:
    cargo tauri build

# Install the webview (ui/) dependencies.
ui-install:
    npm --prefix ui install

# Regenerate app icons from a source logo (see src-tauri/icons/README.md).
icons logo:
    cargo tauri icon {{logo}}
