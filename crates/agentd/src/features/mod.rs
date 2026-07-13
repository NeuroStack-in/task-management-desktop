//! Core-service feature slices (LLD Appendix A). Each is self-contained.

pub mod attendance_events;
pub mod config_sync;
pub mod heartbeat;
pub mod outbox;
pub mod self_update;
pub mod session_manager;
pub mod task_cache;
pub mod timer_engine;
pub mod uploader;
