# Timer transitions — immediate flush (out-of-band)

**Status:** ✅ **Applied 2026-07-19.** Small change to the agent's sender loop, reusing the existing
send path. `state.flush: Arc<Notify>` wakes the sender loop on any timer transition; the loop
`select!`s the 300 s cycle against it. Notified from `commands/panel.rs` (start/stop — the UI path),
`commands/mod.rs` (start/stop), and the monitor's idle auto-stop. Verified with `cargo clippy -D
warnings` + tests. Still needs a **redeploy** for the effect to reach the live backend.

## The problem

A timer start/stop currently rides the **300 s cycle**, like everything else. The path today
(`src-tauri/src/`):

```
commands/mod.rs  timer_start / timer_stop  →  state.pending_events.push(event)   [buffered]
api/mod.rs       spawn_sender loop:  sleep(300s) → cycle::assemble_and_enqueue → drain → POST /v1/agent/batch
```

So `pending_events` sits until the next 300 s tick assembles it. The backend does not learn a timer
started for **up to 5 minutes** (avg ~2.5 min). This is the lag called out in **LLD §4** — and the LLD
names this exact fix: *"post `TimerStarted` immediately instead of waiting for the cycle."*

## The fix — wake the sender loop on a timer transition

The sender loop already assembles + drains + POSTs. We don't need a new route, a new batch path, or a
second network call — we just need the loop to **run now** when a timer event is enqueued, instead of
only every 300 s. One `tokio::sync::Notify` does it.

On a timer transition:
1. `timer_start` / `timer_stop` push the event to `pending_events` (unchanged) **and notify**.
2. The sender loop, waiting on `select!(300s timer, notify)`, wakes immediately.
3. `assemble_and_enqueue` **drains** `pending_events` (the timer event) → outbox.
4. `drain` POSTs `/v1/agent/batch` — the backend learns in **seconds**.

Reuses everything. No double-send: `assemble` empties the buffer, so the next 300 s tick finds nothing.
The batch it sends is exactly what LLD §4 describes — a heartbeat + the timer event, `activity: []`.

## The change — three edits

### 1. `src-tauri/src/state.rs` — add the notify to `AppState`

```rust
use std::sync::Arc;
use tokio::sync::Notify;

pub struct AppState {
    pub timer: Mutex<TimerEngine>,
    pub outbox: Mutex<Outbox>,
    // …
    pub pending_events: Mutex<Vec<AgentEvent>>,
    /// Signalled on any timer transition so the sender loop flushes immediately instead of
    /// waiting for the 300 s cycle (LLD §4 "post TimerStarted immediately"). One permit is
    /// enough — a burst of transitions collapses to a single extra flush.
    pub flush: Arc<Notify>,        // ← add
}

// in the constructor (around line 86):
            pending_events: Mutex::new(Vec::new()),
            flush: Arc::new(Notify::new()),   // ← add
```

### 2. `src-tauri/src/api/mod.rs` — the sender loop waits on the timer *or* the notify

```rust
// spawn_sender loop, replacing the bare `sleep(CYCLE_SECS)`:
loop {
    // Wake on whichever comes first: the 300 s heartbeat cycle, or a timer transition asking
    // for an immediate flush. Either way we assemble + drain exactly as before.
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(CYCLE_SECS)) => {}
        _ = state.flush.notified() => {}
    }
    // …unchanged: the signed-in guard, then:
    cycle::assemble_and_enqueue(&state);
    drain(&http, &upload, &ingest_url, &id_token, &state).await;
}
```

`state.flush` is an `Arc<Notify>`, so clone it into the task alongside the other `state` handles.

### 3. `src-tauri/src/commands/mod.rs` — notify after buffering

```rust
// timer_start, after the push:
    state.pending_events.lock().unwrap().push(event);
    state.flush.notify_one();      // ← flush now, don't wait for the cycle
    Ok(())

// timer_stop, after the push:
    if let Some(ev) = event {
        state.pending_events.lock().unwrap().push(ev);
        state.flush.notify_one();  // ← same
    }
    Ok(())
```

**Also do this for non-user stops.** Idle / logout / shutdown stops come from the monitor
(`monitor.rs`), not the UI command — wherever the monitor pushes a `timer_stopped` event to
`pending_events`, add the same `state.flush.notify_one()`. A stop should reflect as fast as a start.

## Result, combined with the frontend

| | before | after |
|---|---|---|
| Agent → backend | up to **5 min** (waits for cycle) | **seconds** (immediate flush) |
| Backend → website | manual refresh only | **~30 s** (frontend polling — already built, `use-poll.ts`) |
| **Timer start/stop visible on the web** | ~2.5–5 min **+ refresh** | **seconds to ~30 s, no refresh** |

The frontend half is done: `useTimesheet` now background-polls every 30 s (`hooks/use-poll.ts`, paused
when the tab is hidden, refetch on focus). This agent change is the other half — without it, polling
still only sees the timer whenever the 300 s batch eventually carries it.

## Cost & safety

- **Cost:** a timer start + stop = at most two extra `/v1/agent/batch` POSTs beyond the 300 s cadence.
  Negligible (well under a cent/user/month) — the same conclusion HLD §3 reached for out-of-band
  transitions.
- **Idempotency:** the batch already dedups on its id; a retried immediate flush is a safe replay.
- **Offline:** unchanged — `notify_one()` just wakes the loop; if there's no network, the outbox holds
  the event and the normal retry/backoff applies. The flush is best-effort, the outbox is the
  guarantee.
- **No new route, no new API, no contract change** — the batch envelope and `fold_timer_events/` are
  untouched. This is purely *when* the agent sends, not *what*.

## Verify

1. `just check` (fmt + clippy + test) — the change is small; watch for the `Arc<Notify>` clone into the
   spawned task and the `select!` arms.
2. Manual: start a timer in the tray → within a few seconds `GET /v1/me/timesheet/today` shows
   `running: true` (not 5 min later). Stop it → `running: false` just as fast.
3. Web: with the frontend polling live, the running indicator appears within ~30 s of the tray action,
   no page refresh.
