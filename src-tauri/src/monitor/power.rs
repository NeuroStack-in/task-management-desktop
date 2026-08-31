//! Windows power events — stop the timer when the machine goes away, **as it happens**.
//!
//! The monitor loop already notices a suspend *after the fact*, by spotting a jump in the wall
//! clock, and backdates the stop to the last awake tick. That keeps the hours honest but it has two
//! gaps this module exists to close:
//!
//!  - **It only fires on wake.** Between closing the lid at 18:00 and opening it at 09:00 the
//!    server still shows the session open, because the agent was frozen and could send nothing.
//!  - **Modern Standby (S0) never produces a gap.** On most current laptops "sleep" does not
//!    suspend the process outright; it keeps ticking at low power. The wall clock stays continuous,
//!    the gap check never trips, and the timer runs until the 15-minute idle stop catches it.
//!
//! Windows announces all of this — it is simply not exposed by tao, which handles neither
//! `WM_POWERBROADCAST` nor session-change notifications. So this creates its own **message-only
//! window** (`HWND_MESSAGE`: no pixels, no taskbar, exists purely to receive messages) and listens
//! directly:
//!
//! | Signal | Message | Covers |
//! |---|---|---|
//! | `PBT_APMSUSPEND` | classic S3 sleep / hibernate | "Sleep" on older machines |
//! | `GUID_LIDSWITCH_STATE_CHANGE` → closed | lid | closing the laptop |
//! | `WTS_SESSION_LOCK` | Win+L, switch user | walking away locked |
//!
//! Each means the same thing for time tracking: nobody is working. The timer stops.
//!
//! **The display state is deliberately not one of them.** `GUID_CONSOLE_DISPLAY_STATE` was listed
//! here and removed: it is the broadest signal available and it fires on an ordinary screen
//! timeout, which would stop the clock on someone reading, on a call, or watching a demo. Being
//! wrongly stopped is worse than being stopped late — the person has to notice and restart, and
//! the minutes in between are simply gone.
//!
//! What that costs: a **desktop** on Modern Standby, slept from the Start menu, with
//! sign-in-on-wake off — no lid, no S3 suspend, no lock — falls back to the 15-minute idle stop.
//! Laptops are covered by the lid, and Windows locks the session on sleep by default.
//!
//! **Stopping at `now` is right here**, unlike the wake-side detector which must backdate. These
//! arrive *before* the machine goes down, so `now` is the last moment of work — there is no gap to
//! subtract yet.
//!
//! The bucket is not sealed from this thread — the monitor loop owns it. It seals on its next tick
//! through the branch that already handles capture stopping, which is one second away at most.

#![cfg(windows)]

use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager};
use windows::core::GUID;
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
};

/// `dwFlags` for `WTSRegisterSessionNotification` — only this session's lock/unlock, not every
/// session on the machine.
const NOTIFY_FOR_THIS_SESSION: u32 = 0;

use crate::state::AppState;
use crate::{clock, events};
use wp_agent_contract::StopReason;

/// `wParam` of `WM_POWERBROADCAST`: the system is suspending.
const PBT_APMSUSPEND: usize = 0x0004;
/// `wParam` of `WM_POWERBROADCAST`: one of the settings we registered for changed.
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
/// `wParam` of `WM_WTSSESSION_CHANGE`: the session was locked.
const WTS_SESSION_LOCK: usize = 0x7;

/// The lid opened or closed. `Data[0]`: 0 = closed, 1 = open.
const GUID_LIDSWITCH_STATE_CHANGE: GUID = GUID::from_u128(0xBA3E0F4D_B817_4094_A2D1_D56379E6A0F3);

/// The handle the window procedure reaches the app through.
///
/// A `static` because `WNDPROC` is a bare `extern "system" fn` with nowhere to hang state. Set once,
/// before the window that uses it exists, so the procedure can never observe it unset.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Start listening. No-op if called twice.
pub fn spawn(app: AppHandle) {
    if APP.set(app).is_err() {
        return; // already running
    }
    std::thread::Builder::new()
        .name("wp-power".into())
        .spawn(|| unsafe { run() })
        .ok();
}

/// Stop the running session because the machine is going away.
///
/// Idempotent: `TimerEngine::stop` returns `None` when nothing is running, so overlapping signals
/// (a lid close that also turns the display off and then suspends) do the work once and no-op after.
fn stop_for(reason: &'static str) {
    let Some(app) = APP.get() else { return };
    let state = app.state::<AppState>();
    // `now`, not backdated: these arrive *before* the machine goes down, so this instant is the
    // last moment of work. The wake-side detector is the one that has a gap to subtract.
    let ts = clock::now_epoch_ms();
    // Remember the running task **before** the stop consumes it, so returning — unlock, or wake +
    // input — auto-resumes the same work. This is the path that actually fires for a lock / lid /
    // suspend (the monitor loop's gap detector only catches suspends this module missed), so without
    // arming here the feature never triggered for the case people hit most: Win+L and back.
    super::arm_auto_resume(&state, ts);
    let ev = state.timer.lock().unwrap().stop(ts, StopReason::Idle);
    let Some(ev) = ev else { return };
    state.pending_events.lock().unwrap().push(ev);
    // Queue synchronously: on `PBT_APMSUSPEND` the machine may be down within seconds, and the
    // async sender will not get a turn. `assemble_and_enqueue` writes the durable outbox on this
    // thread, so the stop survives the suspend and ships on resume.
    let seq = crate::cycle::assemble_and_enqueue(&state);
    state.flush.notify_one();
    crate::session_state::update(|s| s.open = None);
    let _ = app.emit(events::TRACKING_CHANGED, ());
    tracing::info!(batch_seq = seq, reason, "power event: stopped the timer");
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_POWERBROADCAST => {
            match w.0 {
                PBT_APMSUSPEND => stop_for("suspend"),
                PBT_POWERSETTINGCHANGE if l.0 != 0 => {
                    let s = &*(l.0 as *const POWERBROADCAST_SETTING);
                    // `Data` is a 1-byte array in the binding; the real payload length is in
                    // `DataLength`. Both settings we register for are a single byte.
                    let value = s.Data[0];
                    if s.PowerSetting == GUID_LIDSWITCH_STATE_CHANGE && value == 0 {
                        stop_for("lid-closed");
                    }
                }
                _ => {}
            }
            LRESULT(1) // TRUE: we handled it and are not vetoing the transition.
        }
        WM_WTSSESSION_CHANGE if w.0 == WTS_SESSION_LOCK => {
            stop_for("session-locked");
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn run() {
    let Ok(instance) = GetModuleHandleW(None) else {
        tracing::warn!("power: no module handle; power events unavailable");
        return;
    };
    let class = windows::core::w!("WorkPulsePowerListener");
    let hinstance = HINSTANCE(instance.0);
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    if RegisterClassW(&wc) == 0 {
        tracing::warn!("power: could not register the window class");
        return;
    }

    // HWND_MESSAGE: a message-only window. Never shown, never in the taskbar, not painted — it
    // exists solely so Windows has somewhere to deliver these notifications.
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        class,
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        None,
        hinstance,
        None,
    );
    // 0.57 models HWND as an isize; a null window is 0, not a null pointer.
    if hwnd.0 == 0 {
        tracing::warn!("power: could not create the listener window");
        return;
    }

    // A failure warns and continues rather than abandoning the rest: a desktop with no lid switch
    // still wants suspend and lock, neither of which needs registering.
    if RegisterPowerSettingNotification(
        HANDLE(hwnd.0),
        &GUID_LIDSWITCH_STATE_CHANGE,
        DEVICE_NOTIFY_WINDOW_HANDLE,
    )
    .is_err()
    {
        tracing::warn!("power: could not subscribe to lid notifications");
    }
    if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).is_err() {
        tracing::warn!("power: could not subscribe to session lock notifications");
    }

    tracing::info!("power: listening for suspend / lid / lock");

    // A message loop this thread owns. `GetMessageW` blocks, so it costs nothing while idle.
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
