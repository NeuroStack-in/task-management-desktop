//! Shared types for the agent's three processes.
//!
//! - [`contract`] — re-export of the **shared agent↔backend** wire contract
//!   (`wp-agent-contract`): the batch envelope, ack, and tracking config. Depended on by the
//!   backend `ingest` binary too, so it can't drift.
//! - [`ipc`] — the agent-**internal** messages between the core service, the per-user capture
//!   helper, and the Tauri tray. These never leave the machine.

/// The agent↔backend wire contract (batch envelope, ack, config). Same crate the backend uses.
pub use wp_agent_contract as contract;

pub mod ipc;
