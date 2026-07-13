//! Detects interactive OS sessions and spawns/supervises a per-user capture helper in each
//! (Windows Session-0 can't screenshot user desktops; macOS TCC grants are per-user). Owns the
//! IPC transport (local socket / named pipe) to helpers.
//!
//! Skeleton placeholder; production wires `interprocess` + OS session enumeration.

#[derive(Default)]
pub struct SessionManager;

impl SessionManager {
    /// Spawn/attach a capture helper for every active interactive session; restart on crash.
    pub fn supervise(&self) {
        // TODO: enumerate sessions, spawn `capture-helper` per session, hold IPC channels.
    }
}
