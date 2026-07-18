//! App lifecycle — **M6**. Auto-sign-out on quit (idempotent, 5 s-bounded, fired on tray-Quit /
//! Ctrl-C / SIGTERM), single-instance guard, and autostart registration. On quit the timer stops
//! with `StopReason::Shutdown` and the outbox flushes what it can before exit.

// TODO(M6): on_exit flush + auto-sign-out; single-instance; autostart.
