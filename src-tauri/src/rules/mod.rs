//! Synced app/URL rules applied **on-device** (edge-first: the cloud receives pre-classified spans).
//! Rules ride the CONFIG rail from the server (contract `AppUrlRules`).
//!
//! M0 keeps the classifier + its test. M4 wires it to its three consumers — set `AppSpan.category`,
//! suppress the screenshot **and** the span for an `exceptions` match, and emit `PolicyViolation`
//! for a `blocked` match — **enforcement only while the timer runs** (off-timer: none, ever;
//! BUILD-PLAN §2).

pub mod classifier;
pub use classifier::{classify, CategoryRule};
