//! Timer state machine. Split into `engine` so the pure logic is testable without Tauri.
pub mod engine;
pub use engine::TimerEngine;
