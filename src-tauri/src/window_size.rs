//! Panel window geometry — **decided in `tauri.conf.json`, deliberately not persisted.**
//!
//! There is no code here on purpose. The panel is a fixed **420 × 560** companion widget that opens
//! **centred** on every launch, and all three of those facts are declared in `tauri.conf.json`
//! (`width` / `height` / `center`). Nothing at runtime should override them.
//!
//! ## The M6 TODO that used to live here is closed, not pending
//!
//! This module previously said: *"Persist the panel window size across launches — M6. Debounced
//! ~400 ms so a drag-resize writes once, not per frame."* That work should **not** be built:
//!
//! - The window is `resizable: false`, so there is no user-chosen size to remember. The doc also
//!   claimed bounds (min 380×480, max 550×650) that `tauri.conf.json` has never actually declared.
//! - `tauri-plugin-window-state` already implemented exactly this, and it was **removed on
//!   2026-07-22** because it was the cause of a real bug, not a feature: its default
//!   `StateFlags::all()` restored a saved size *over* the config, so the window silently ignored
//!   every config change. `%APPDATA%/com.workpulse.agent/.window-state.json` held `775 × 738`
//!   — precisely the old 620 × 590 captured at 125 % display scaling — and replayed it on every
//!   launch. It also persisted `"visible": false`, so a saved hidden state could restore a
//!   background agent that never appears.
//!
//! Position is not persisted either: the panel is meant to open centred, the way a tray companion
//! does, rather than wherever it was last dragged.
//!
//! **If size or position ever needs to be remembered again**, re-add the plugin with an explicit
//! `with_state_flags(...)` naming only the fields intended, never `default()` — and make the window
//! resizable first, so there is something real to remember.
