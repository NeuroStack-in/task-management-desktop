//! Local cache of assignable projects/tasks for the tray timer picker; refreshed via a version
//! stamp in the batch ack (same pattern as config_sync). Lets the timer work offline.
//!
//! Skeleton placeholder.

#[derive(Default)]
pub struct TaskCache {
    version: u64,
}

impl TaskCache {
    pub fn version(&self) -> u64 {
        self.version
    }
    // TODO: store [{task_id, title, project_id}], refresh on ack version bump.
}
