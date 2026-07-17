//! Persist the panel window size across launches — **M6**. Debounced ~400 ms so a drag-resize writes
//! once, not per frame. Companion-widget bounds (min 380×480, max 550×650) are enforced in
//! `tauri.conf.json`.

// TODO(M6): 400 ms-debounced save/restore of the panel size.
