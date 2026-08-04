//! [`Win32Shell`] — the shell itself, the message pump, and the [`Shell`]
//! implementation.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::time::Duration;
use std::collections::VecDeque;
use std::rc::Rc;

use crcbl_core::{EventTime, Pool, SurfaceTarget};

use crate::{
    ClipboardContent, ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode,
    LogicalSize, MimeType, MonitorId, MonitorInfo, PhysicalPoint, PhysicalSize, PointerMode,
    ReceivedMime, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent, SizeConstraints,
    WindowConfiguration, WindowDesc, WindowId, WindowState,
};

use super::TimeBase;
use super::clipboard::{self, Clipboard, Opened};
use super::events::RawEvent;
use super::ffi::{self, Handle, Msg, WindowPlacement, value};
use super::geometry;
use super::input;
use super::keys::Utf16;
use super::pointer::{RawMotion, Visibility};
use super::proc::{self, Shared};
use super::window;

/// Why [`Win32Shell::wait`] came back.
///
/// `MsgWaitForMultipleObjectsEx` has four answers and three of them return
/// *immediately*, so the duration of a wait does not say which one happened —
/// a failed call and a wait that found a message already queued look identical
/// from a clock, and both look like a wait that simply did not work. This names
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wake {
    /// The timeout elapsed with nothing to do: the wait slept, which is what
    /// [`ShellCaps::EVENT_WAIT`] promises is possible.
    TimedOut,
    /// A message is available — either it arrived during the wait, or it was
    /// already queued when the wait started and `MWMO_INPUTAVAILABLE` reported
    /// it. Correct behaviour, and indistinguishable from a broken wait unless
    /// the queue is known to have been empty.
    Message,
    /// `WAIT_FAILED`. Returns at once and sleeps not at all.
    Failed {
        /// `GetLastError` immediately afterwards.
        error: u32,
    },
    /// Something outside the documented set. With a handle count of zero
    /// `WAIT_ABANDONED` and `WAIT_IO_COMPLETION` should both be unreachable,
    /// which is exactly why one arriving is worth naming rather than folding
    /// into another arm.
    Unexpected {
        /// The raw return value.
        outcome: u32,
    },
}

/// The windowed style and placement a borderless window will be restored to.
///
/// Captured on the way into [`DisplayMode::Borderless`] and applied verbatim on
/// the way out; see [`window`](super::window) for why a placement rather than a
/// rectangle.
#[derive(Clone, Copy, Debug)]
pub(super) struct Saved {
    /// `GWL_STYLE` as it was.
    pub style: u32,
    /// `GWL_EXSTYLE` as it was.
    pub ex_style: u32,
    /// Show state, maximized position and restored rectangle.
    pub placement: WindowPlacement,
}

/// One window.
#[derive(Debug)]
pub(super) struct WinWindow {
    /// The `HWND`. Also what [`SurfaceTarget::Win32`] carries.
    hwnd: NonNull<c_void>,
    /// The same handle as the integer the raw queue matches on.
    pub key: isize,
    title: String,
    /// The **windowed** size that was asked for. Kept because a window created
    /// borderless has no saved placement to be restored to, and
    /// [`apply_mode`](Win32Shell::apply_mode) has to build one; see there.
    pub requested_size: LogicalSize,
    requested_mode: DisplayMode,
    pub requested_constraints: SizeConstraints,
    pub resizable: bool,
    /// The window's own DPI. Per window on this platform, unlike X11's global
    /// `Xft.dpi`.
    pub dpi: u32,
    pub scale_factor: f64,
    configuration: Option<WindowConfiguration>,
    /// The configuration the system has already told us about, waiting to be
    /// published on the next [`pump`](Shell::pump).
    ///
    /// The same shape as the X11 backend's, and for the same reason: the size
    /// is known synchronously, and the *only* thing making the caller wait is
    /// that [`WindowState::size`] is defined as `None` until a
    /// [`Resized`](ShellEvent::Resized) has been delivered. One pump, not a
    /// round trip, and not an invented delay either.
    pending: Option<WindowConfiguration>,
    /// The last size published, so the `WM_SIZE` that arrives during
    /// `CreateWindowExW` does not produce a second, identical
    /// [`Resized`](ShellEvent::Resized).
    last_size: Option<PhysicalSize>,
    /// The mode in effect. On Win32 this is whatever was last applied — there
    /// is no window manager to disagree — but the *monitor* in it is read back
    /// rather than assumed.
    pub effective_mode: DisplayMode,
    pub saved: Option<Saved>,
    /// The cursor last asked for: `None` for "never asked", `Some(None)` for
    /// hidden, `Some(Some(icon))` for a shape.
    cursor: Option<Option<CursorIcon>>,
    pub(super) pointer_mode: PointerMode,
    pub(super) focused: bool,
    visible: bool,
    close_pending: bool,
    /// Whether [`WindowDesc::accept_drops`] was set.
    ///
    /// The system already enforces this — no `WS_EX_ACCEPTFILES`, no
    /// `WM_DROPFILES` — so this is the second half of the gate rather than the
    /// gate, and [`dnd`](super::dnd) says why a second half is worth having.
    accept_drops: bool,
}

impl WinWindow {
    /// The raw handle, for the many calls that need it.
    pub(super) fn raw(&self) -> Handle {
        self.hwnd.as_ptr()
    }

    /// Whether this window asked for the cursor to be hidden.
    ///
    /// A window that has never asked is not asking for hidden — which is the
    /// difference between `None` and `Some(None)` and the reason the field is a
    /// nested `Option` rather than a flat one.
    pub(super) fn cursor_hidden(&self) -> bool {
        self.cursor == Some(None)
    }
}

/// A [`Shell`] backed by Win32.
///
/// Constructed through [`open`](crate::open) or
/// [`open_backend`](crate::open_backend); see the [module docs](super) for what
/// this backend implements, what waits for W2 and W3, and where Windows differs
/// from every other implementation of this seam.
pub struct Win32Shell {
    /// `HINSTANCE` of the module that registered the window class.
    instance: NonNull<c_void>,
    /// What the window procedure reaches through `GWLP_USERDATA`.
    ///
    /// An `Rc` because the procedure holds a raw pointer into it that must stay
    /// valid for as long as any window exists — which [`drop`](Self::drop)
    /// guarantees by destroying every window first.
    shared: Rc<Shared>,
    windows: Pool<WinWindow>,
    pub(super) monitors: Vec<MonitorInfo>,
    /// Device name → the [`MonitorId`] it was given, so an id is never reused
    /// within a session — the obligation [`monitor`](crate::monitor) states.
    pub(super) monitor_ids: Vec<(String, MonitorId)>,
    pub(super) next_monitor_id: u32,
    queue: VecDeque<ShellEvent>,
    time: TimeBase,
    /// The half-finished surrogate pair `WM_CHAR` may be in the middle of.
    ///
    /// Shell-wide rather than per window because the messages are a stream on
    /// one thread and only the focused window receives them: two windows cannot
    /// interleave halves of a pair.
    pub(super) text: Utf16,
    /// The previous absolute raw sample, for a device that reports positions.
    pub(super) raw_motion: RawMotion,
    /// `ShowCursor`'s reference count, kept balanced.
    pub(super) visibility: Visibility,
    /// The next [`ClipboardRequestId`], which is unique for the session.
    ///
    /// Every read is answered before
    /// [`clipboard_request`](Shell::clipboard_request) returns, so nothing is
    /// keyed by this — but the seam promises ids are unique within a shell, and
    /// a consumer with two reads in flight tells them apart by nothing else.
    next_request: u32,
    caps: ShellCaps,
}

impl core::fmt::Debug for Win32Shell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Win32Shell")
            .field("windows", &self.windows.len())
            .field("monitors", &self.monitors.len())
            .field("queued_events", &self.queue.len())
            .finish()
    }
}

/// Asks for per-monitor-v2 DPI awareness, once per process.
///
/// # Already set is not a failure
///
/// DPI awareness is a **process** property and something else may have set it
/// first — an application manifest, a host that embedded this engine, or an
/// earlier [`Win32Shell`] in the same process. All three answer
/// `ERROR_ACCESS_DENIED`, which means "it is already decided", not "it did not
/// work. Treating that as an error would make the second shell in a process
/// fail to open for a reason that is not a problem.
///
/// Anything else is logged and survived: a per-monitor-*v1* process still gets
/// correct scale factors, it just does not get `WM_DPICHANGED`'s suggested
/// rectangle, so a window dragged between monitors of different scales is left
/// for the user to resize. That is a degraded desktop, not a broken one, and
/// refusing to open a window over it would be worse.
fn set_dpi_awareness() {
    // SAFETY: the argument is a context pseudo-handle passed by value; the call
    // reads no memory of ours and cannot fail in a way that leaves state
    // half-changed.
    if unsafe { ffi::SetProcessDpiAwarenessContext(value::DPI_PER_MONITOR_AWARE_V2) } != 0 {
        log::debug!("per-monitor-v2 DPI awareness is on");
        return;
    }
    // SAFETY: reads this thread's last error code, which the call above set.
    let error = unsafe { ffi::GetLastError() };
    if error == value::ERROR_ACCESS_DENIED {
        log::debug!("the process DPI awareness was already set; leaving it alone");
        return;
    }
    log::warn!(
        "SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) failed with Win32 error {error}; \
         window scale factors will still be reported, but a window dragged between monitors of \
         different scales will not be resized for the new one"
    );
}

impl Win32Shell {
    /// Sets up the process's DPI awareness, registers the window class and
    /// enumerates the monitors.
    ///
    /// There is no connection to make — the window system is the kernel this
    /// process is already running on — so the only thing that can genuinely
    /// fail is the class registration, which is what a process with no usable
    /// window station cannot do. That makes it the honest failure point, and
    /// its `GetLastError` code the honest diagnostic.
    ///
    /// # Errors
    ///
    /// [`ShellError::Connect`] if the module handle or the window class could
    /// not be obtained.
    pub fn open() -> Result<Self, ShellError> {
        set_dpi_awareness();

        // SAFETY: a null module name asks for the handle of the executable that
        // started the process, which is documented never to fail for the
        // current module and is a pseudo-handle that needs no release.
        let instance = unsafe { ffi::GetModuleHandleW(ptr::null()) };
        let Some(instance) = NonNull::new(instance) else {
            return Err(ShellError::Connect {
                backend: ShellBackend::Win32,
                detail: "GetModuleHandleW(NULL) returned null".to_string(),
            });
        };
        window::register_class(instance.as_ptr())?;
        // Before any window exists, because the registration is per process and
        // the capability set is latched from whether it took.
        let raw_motion = input::register_raw_input();

        let mut shell = Self {
            instance,
            shared: Rc::new(Shared::default()),
            windows: Pool::new(),
            monitors: Vec::new(),
            monitor_ids: Vec::new(),
            next_monitor_id: 1,
            queue: VecDeque::new(),
            time: TimeBase::at(ffi::tick_nanos()),
            text: Utf16::default(),
            raw_motion: RawMotion::default(),
            visibility: Visibility::default(),
            next_request: 1,
            caps: Self::latch_caps(raw_motion),
        };
        shell.monitors = shell.enumerate_monitors();
        Ok(shell)
    }

    /// The capability set, which on this platform is a constant but one.
    ///
    /// See the [module docs](super) for why there is almost nothing to compute
    /// it from: every API this backend uses is present above its version floor
    /// or the process would not have started. The exception is
    /// [`RAW_POINTER_MOTION`](ShellCaps::RAW_POINTER_MOTION), which is latched
    /// from whether `RegisterRawInputDevices` actually took — the one call in
    /// this backend that can be refused for a reason outside the version floor.
    /// Each bit set here is exercised by a test in this module.
    const fn latch_caps(raw_motion: bool) -> ShellCaps {
        let caps = ShellCaps::MULTI_WINDOW
            .union(ShellCaps::EVENT_WAIT)
            .union(ShellCaps::WINDOW_POSITION)
            .union(ShellCaps::SERVER_DECORATIONS)
            .union(ShellCaps::FRACTIONAL_SCALE)
            .union(ShellCaps::ASPECT_HINT_HONORED)
            .union(ShellCaps::POINTER_LOCK)
            .union(ShellCaps::POINTER_CONFINE)
            .union(ShellCaps::POINTER_WARP)
            .union(ShellCaps::CLIPBOARD)
            .union(ShellCaps::DRAG_DROP);
        if raw_motion {
            caps.union(ShellCaps::RAW_POINTER_MOTION)
        } else {
            caps
        }
    }

    pub(super) fn instance(&self) -> Handle {
        self.instance.as_ptr()
    }

    pub(super) fn shared(&self) -> &Shared {
        &self.shared
    }

    /// The pointer the window procedure will find in `GWLP_USERDATA`.
    ///
    /// Borrowed from the `Rc`, not leaked out of it: the shell owns the
    /// allocation and outlives every window that holds this pointer.
    pub(super) fn shared_ptr(&self) -> *mut c_void {
        Rc::as_ptr(&self.shared) as *mut c_void
    }

    pub(super) fn window(&self, window: WindowId) -> Result<&WinWindow, ShellError> {
        self.windows
            .get(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    pub(super) fn window_mut(&mut self, window: WindowId) -> Result<&mut WinWindow, ShellError> {
        self.windows
            .get_mut(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    /// The id a device name was given, if this session has seen it.
    pub(super) fn monitor_id_of(&self, device: &str) -> Option<MonitorId> {
        self.monitor_ids
            .iter()
            .find(|(known, _)| known == device)
            .map(|(_, id)| *id)
    }

    /// Every live window, by the handle the seam names it with.
    pub(super) fn windows_iter(&self) -> impl Iterator<Item = (WindowId, &WinWindow)> {
        self.windows
            .iter()
            .map(|(handle, window)| (handle.cast(), window))
    }

    /// A message's `GetMessageTime` on the engine's clock.
    ///
    /// The clock is read *now* rather than being cached, because
    /// [`TimeBase::widen`] resolves the message's 32 bits against a full-width
    /// reading and a stale one would resolve the wrap the wrong way for exactly
    /// as long as it was stale.
    pub(super) fn event_time(&self, millis: u32) -> EventTime {
        self.time.event_time_at(ffi::tick_nanos(), millis)
    }

    /// Queues an event for the current [`pump`](Shell::pump) to deliver.
    pub(super) fn queue_event(&mut self, event: ShellEvent) {
        self.queue.push_back(event);
    }

    /// Discards any clipboard answer queued for a window that has just died.
    ///
    /// [Obligation 4](Shell) is about *accepted* requests and
    /// [obligation 1](Shell) forbids an event naming a stale handle; where they
    /// meet, the stale handle wins, exactly as the X11 backend decides it. A
    /// consumer can see a handle it destroyed; it cannot see an answer it will
    /// never be given for a window it no longer has.
    ///
    /// The window here is narrow — this backend answers a read before
    /// [`clipboard_request`](Shell::clipboard_request) returns, so the answer is
    /// only ever queued for the length of one frame — but a consumer that reads
    /// the clipboard and then closes the window in the same frame is an ordinary
    /// "paste and dismiss the dialog", not a contrivance.
    fn drop_clipboard_answers(&mut self, window: WindowId) {
        self.queue.retain(|event| {
            !matches!(event, ShellEvent::ClipboardData { window: asked, .. } if *asked == window)
        });
    }

    /// The clipboard's content in `mime`, read now.
    ///
    /// The whole of the read, and it is synchronous because Win32's clipboard
    /// is content rather than an owner to negotiate with — see
    /// [`clipboard`](super::clipboard). The three outcomes are the three
    /// [`ClipboardContent`] means:
    ///
    /// * **The format is not there** — [`Empty`](ClipboardContent::Empty),
    ///   which the seam defines as merging "holds nothing" with "holds nothing
    ///   in that format". Both are true here and neither is a failure.
    /// * **The clipboard could not be opened or locked** —
    ///   [`Unavailable`](ClipboardContent::Unavailable). There may well be
    ///   something on it; we could not get at it.
    /// * **Anything else** — [`Bytes`](ClipboardContent::Bytes), possibly
    ///   empty, which is a successful transfer of nothing.
    fn read_clipboard(&self, hwnd: Handle, mime: MimeType) -> ClipboardContent {
        let Some(format) = clipboard::format_id(mime) else {
            // The window station would not name the format, so nothing can have
            // published anything under it. `format_id` has already logged.
            return ClipboardContent::Unavailable;
        };
        let board = match Clipboard::open(hwnd) {
            Ok(board) => board,
            Err(refused) => {
                log::warn!(
                    "the clipboard was not readable within {:?}: {refused:?}",
                    clipboard::OPEN_BUDGET
                );
                return ClipboardContent::Unavailable;
            }
        };
        if let Opened::After { attempts } = board.opened() {
            log::debug!("the clipboard was held by another process for {attempts} attempts");
        }
        let Some(bytes) = board.get(format) else {
            // `GetClipboardData` answering null is "no such format on the
            // clipboard", which is exactly `Empty`. A lock failure logs and
            // lands here too — rare enough that distinguishing it would mean
            // threading an outcome through for a case nobody has seen.
            return ClipboardContent::Empty;
        };
        match clipboard::encoding_for(mime) {
            clipboard::Encoding::UnicodeText => {
                ClipboardContent::Bytes(clipboard::utf8_from_utf16_bytes(&bytes))
            }
            clipboard::Encoding::Registered(_) => {
                ClipboardContent::Bytes(clipboard::trim_trailing_nuls(&bytes).to_vec())
            }
        }
    }

    /// The [`WindowId`] owning an `HWND`, for routing a raw event.
    fn window_by_key(&self, key: isize) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, window)| window.key == key)
            .map(|(handle, _)| handle.cast())
    }

    fn unsupported(what: &'static str) -> ShellError {
        ShellError::Unsupported {
            backend: ShellBackend::Win32,
            what,
        }
    }

    /// Whether a window is on screen and not iconified.
    ///
    /// Asked rather than assumed, because `ShowWindow` does not always do what
    /// it was told: showing an already-visible window is a no-op that still
    /// returns, and a minimized window is `IsWindowVisible` **and** `IsIconic`.
    /// [`WindowState::visible`] is defined as "mapped and not minimized", which
    /// is exactly this expression.
    fn is_showing(hwnd: Handle) -> bool {
        // SAFETY: `hwnd` is a live window of this shell; both calls only read.
        unsafe { ffi::IsWindowVisible(hwnd) != 0 && ffi::IsIconic(hwnd) == 0 }
    }

    /// Runs every message queued for this thread through the window procedure.
    ///
    /// `PeekMessageW` with a null window takes messages for **every** window on
    /// this thread, which is where all of this shell's windows deliver — the
    /// thread affinity the module docs describe, used rather than worked
    /// around. It returns zero when the queue is empty, so this terminates:
    /// `WM_PAINT` is the one message that would repeat forever if it were left
    /// unhandled, and `DefWindowProc` validates it.
    ///
    /// # `TranslateMessage` is not optional, and leaving it out was silent
    ///
    /// A `WM_KEYDOWN` names a key; the **character** it produces is a separate
    /// message that only exists because `TranslateMessage` ran, and it is where
    /// dead keys, AltGr and an input method's commit all arrive. This loop did
    /// not call it, so `WM_CHAR` was never generated for a real keystroke: the
    /// `Char` branch of the window procedure, the surrogate reassembly in
    /// [`keys`](super::keys) and every
    /// [`TextCommit`](ShellEvent::TextCommit) were unreachable from a keyboard,
    /// and typing into a Crucible window on Windows produced no text at all.
    ///
    /// **Nothing in the in-crate suite could see it.** Those tests send
    /// `WM_CHAR` with `SendMessageW` — the real procedure, the real
    /// reassembly — and a message sent directly does not pass through the queue
    /// this call is part of. It took a keystroke injected from another process
    /// to reach the gap, which is what `tests/win32_e2e.rs` is for.
    ///
    /// It is called on every message rather than on key messages only, which is
    /// what `winuser.h` documents: it inspects the message itself and does
    /// nothing for the ones it does not translate.
    /// [`wait_events`](Shell::wait_events), with the reason it came back.
    ///
    /// # Why the return value exists at all
    ///
    /// `Shell::wait_events` returns `()`, so the first version of this dropped
    /// `MsgWaitForMultipleObjectsEx`'s answer on the floor. Three of its four
    /// outcomes are *immediate* — a message was already queued, one arrived, or
    /// the call failed outright — and discarding the value makes them
    /// indistinguishable from each other and from a wait that slept properly.
    /// The first CI run on Windows produced exactly that: three 50 ms waits in
    /// 47 ms, with nothing to say which of the three had happened.
    ///
    /// [`Wake`] is what a test asserts on, because
    /// [`EVENT_WAIT`](ShellCaps::EVENT_WAIT) is a claim about a *mechanism* —
    /// "this sleeps" — and a wall clock cannot tell a wait that slept from a
    /// wait that failed on a loaded runner. Nothing in the backend branches on
    /// it; the trait method still ignores it, because a caller has nothing to do
    /// differently either way.
    ///
    /// # Decision: drain first, and no `MWMO_INPUTAVAILABLE`
    ///
    /// **That diagnosis then named the cause, and this is the fix it pointed
    /// at.** The second CI run reported `Wake::Message` after 14 ms with the
    /// queue holding `0x0040` in both halves of `GetQueueStatus` — which is
    /// `QS_SENDMESSAGE`, a message *sent* to a window of this thread rather than
    /// posted to its queue.
    ///
    /// `PeekMessageW` **dispatches** a sent message but does not retrieve it, so
    /// the `QS_SENDMESSAGE` bit is still set when the wait starts. Asking for
    /// `MWMO_INPUTAVAILABLE` is asking to be woken by exactly that bit, so the
    /// wait returned immediately, every time, forever — a shell that never
    /// sleeps and says nothing about it.
    ///
    /// So this drains the queue itself and then sleeps with **no flags**.
    /// Draining is what clears the state, because `PeekMessageW` returning
    /// `FALSE` is what marks the queue as seen; sleeping with no flags waits for
    /// something genuinely new, which `QS_ALLINPUT` still covers — sent messages
    /// included. `MWMO_INPUTAVAILABLE` exists for a caller that does *not* drain
    /// before sleeping, and draining ourselves is strictly better: it removes
    /// the reason for the flag and the spurious wake in one move.
    ///
    /// # What the third run said, and what the instrumentation answered
    ///
    /// **That change did what it was expected to do, and was not the whole
    /// answer.** The third CI run reported `0x80008` — `QS_POSTMESSAGE` in both
    /// halves of `GetQueueStatus` — on a queue drained microseconds earlier. So
    /// the failure was instrumented rather than guessed at a third time:
    /// [`peek_pending`](Self::peek_pending) was added to print the `MSG` itself
    /// beside the `QS_` word, on the reasoning that a message id is a number
    /// that can be looked up.
    ///
    /// The fourth run printed the observation that ended it:
    ///
    /// ```text
    /// it took 1.3311ms, the queue holds 0x400040,
    /// and the message still in it is None
    /// ```
    ///
    /// **A `QS_` bit is set and `PeekMessageW` has nothing to return.** That is
    /// documented behaviour rather than a defect in this code: `0x40` is
    /// `QS_SENDMESSAGE`, a message *sent* to a window of this thread, and
    /// `PeekMessageW` dispatches such a message without ever retrieving it — so
    /// it cannot report it and cannot clear its bit, while a wait that includes
    /// `QS_SENDMESSAGE` in its mask wakes on that bit at once. MSDN's own caveat
    /// covers it: a `QS_` flag being set does not guarantee that a subsequent
    /// `PeekMessage` will return a message. The third run's `0x80008` was the
    /// same phenomenon showing a different bit.
    ///
    /// # The fix: `QS_ALLEVENTS`, which is `QS_ALLINPUT` minus that bit
    ///
    /// The wait now asks for [`QS_ALL_EVENTS`](value::QS_ALL_EVENTS), which is
    /// exactly `QS_ALLINPUT` without `QS_SENDMESSAGE` — the one bit that cannot
    /// be cleared and therefore must not be waited on. Sent messages are still
    /// dispatched, by the `PeekMessageW` in
    /// [`drain_messages`](Self::drain_messages) on the next pump; what changes is
    /// that one arriving *during* a sleep waits out the timeout instead of
    /// cutting it short. [`QS_ALL_EVENTS`](value::QS_ALL_EVENTS) states that
    /// trade in full, and [`queue_status`](Self::queue_status) keeps asking for
    /// `QS_ALLINPUT`, because a diagnosis wants every bit including the one the
    /// wait ignores.
    ///
    /// **Nobody has run this on Windows yet.** The three previous rounds each
    /// looked like a fix too; what is different is that this one is a
    /// description of an observation rather than of a hypothesis. The next run
    /// says whether it is enough.
    fn wait(&mut self, timeout: Option<Duration>) -> Wake {
        // Before the wait, not after: an undrained queue is what
        // `MWMO_INPUTAVAILABLE` was there for, and this is the same guarantee
        // without the bit that never clears. Whatever the window procedure
        // records here is delivered by the next `pump`, exactly as if the
        // messages had been drained there.
        self.drain_messages();
        let milliseconds = timeout.map_or(value::INFINITE, |timeout| {
            // `INFINITE` is `u32::MAX`, so a timeout that saturates to it would
            // silently become "forever" — 49 days of sleep is close enough to
            // the caller's intent, but never waking is not.
            u32::try_from(timeout.as_millis())
                .unwrap_or(value::INFINITE - 1)
                .min(value::INFINITE - 1)
        });
        // SAFETY: a count of zero makes the handle array unused, so a null
        // pointer is correct for it; zero flags is the documented "wait for a
        // new message", which is what the drain above makes correct.
        //
        // `QS_ALLEVENTS`, not `QS_ALLINPUT`: the difference is `QS_SENDMESSAGE`,
        // which `PeekMessageW` can neither return nor clear, so waiting on it is
        // waiting on a bit that is already set. See the doc comment.
        let outcome = unsafe {
            ffi::MsgWaitForMultipleObjectsEx(0, ptr::null(), milliseconds, value::QS_ALL_EVENTS, 0)
        };
        match outcome {
            value::WAIT_TIMEOUT => Wake::TimedOut,
            value::WAIT_OBJECT_0 => Wake::Message,
            // SAFETY: reading the calling thread's last error, immediately
            // after the call that set it.
            value::WAIT_FAILED => Wake::Failed {
                error: unsafe { ffi::GetLastError() },
            },
            other => Wake::Unexpected { outcome: other },
        }
    }

    /// Which kinds of message this thread's queue holds, as `QS_*` bits.
    ///
    /// Only ever called to *explain* a [`Wake::Message`] that should have been
    /// a [`Wake::TimedOut`] — so it is not on the wait path and costs a wait
    /// nothing. Kept beside [`wait`](Self::wait) because that is the only
    /// question it answers, and test-only because only a test knows what the
    /// queue was supposed to hold.
    #[cfg(test)]
    fn queue_status() -> u32 {
        // SAFETY: reading the calling thread's own queue state. The call takes
        // no pointers and has no failure mode.
        unsafe { ffi::GetQueueStatus(value::QS_ALL_INPUT) }
    }

    /// The message at the head of this thread's queue, without taking it.
    ///
    /// The other half of the same question [`queue_status`](Self::queue_status)
    /// answers, and the half that ends an argument: a `QS_` word says a *posted
    /// message* is there, and this says **which** one — an id to look up rather
    /// than a bit to theorise about. `PM_NOREMOVE`, so asking changes nothing
    /// the next wait will see.
    ///
    /// Test-only for the same reason: only a test knows what the queue was
    /// supposed to hold, and nothing in the backend branches on the answer.
    #[cfg(test)]
    fn peek_pending() -> Option<Msg> {
        let mut message = Msg::default();
        // SAFETY: `message` is a live, initialised `MSG` the call writes into;
        // a null window and a zero filter range ask for every window and thread
        // message, and `PM_NOREMOVE` leaves the queue exactly as it was found.
        let pending = unsafe {
            ffi::PeekMessageW(&raw mut message, ptr::null_mut(), 0, 0, value::PM_NOREMOVE)
        };
        (pending != 0).then_some(message)
    }

    fn drain_messages(&mut self) {
        let mut message = Msg::default();
        loop {
            // SAFETY: `message` is a live, initialised `MSG` the call writes
            // into; a null window and a zero filter range ask for everything.
            let pending = unsafe {
                ffi::PeekMessageW(&raw mut message, ptr::null_mut(), 0, 0, value::PM_REMOVE)
            };
            if pending == 0 {
                break;
            }
            // SAFETY: `message` is the message this thread just took off its own
            // queue. `TranslateMessage` reads it and posts any `WM_CHAR` it
            // produces to the front of the same queue, where the next turn of
            // this loop picks it up; `DispatchMessageW` then calls the window
            // procedure re-entrantly, which is why nothing here holds a borrow
            // of `shared`.
            unsafe {
                ffi::TranslateMessage(&raw const message);
                ffi::DispatchMessageW(&raw const message);
            }
        }
    }

    /// Turns what the window procedure recorded into shell state and events.
    ///
    /// Takes the whole queue first, so no borrow is held while this calls back
    /// into the system — several of these branches do, and the procedure they
    /// re-enter would panic on a `RefCell` that was still borrowed.
    fn translate(&mut self) {
        for event in self.shared.take_events() {
            let window = event.hwnd().and_then(|key| self.window_by_key(key));
            match event {
                RawEvent::Resized { hwnd, size } => {
                    let Some(window) = window else { continue };
                    let showing = Self::is_showing(hwnd as Handle);
                    let Ok(state) = self.window_mut(window) else {
                        continue;
                    };
                    state.visible = showing;
                    // The `WM_SIZE` that `CreateWindowExW` and `SetWindowPos`
                    // send is a restatement of a size this shell already
                    // published; only a change is an event.
                    if state.last_size == Some(size) {
                        continue;
                    }
                    state.last_size = Some(size);
                    state.pending = Some(WindowConfiguration {
                        size,
                        scale_factor: state.scale_factor,
                        mode: state.effective_mode,
                    });
                }
                RawEvent::Minimized { .. } => {
                    let Some(window) = window else { continue };
                    if let Ok(state) = self.window_mut(window) {
                        // No size is published: a minimized window's client
                        // area is 0×0, and that is not an extent.
                        state.visible = false;
                    }
                }
                RawEvent::DpiChanged { hwnd, dpi } => {
                    let Some(window) = window else { continue };
                    let hwnd = hwnd as Handle;
                    let size = Self::client_size_of(hwnd);
                    let scale_factor = geometry::scale_from_dpi(dpi);
                    let Ok(state) = self.window_mut(window) else {
                        continue;
                    };
                    state.dpi = dpi;
                    state.scale_factor = scale_factor;
                    // Set unconditionally, and after the `Resized` that the
                    // procedure's own `SetWindowPos` produced: the two share
                    // one `pending` slot, so the configuration published names
                    // the new size *and* the new scale, which is the pairing
                    // `WindowConfiguration` exists to keep together.
                    state.last_size = Some(size);
                    state.pending = Some(WindowConfiguration {
                        size,
                        scale_factor,
                        mode: state.effective_mode,
                    });
                    // The frame scales with the DPI, so the constraints the
                    // window procedure enforces are now computed from a stale
                    // one.
                    if let Ok(state) = self.window(window) {
                        self.refresh_limits(state);
                    }
                }
                RawEvent::Focus { focused, .. } => {
                    let Some(window) = window else { continue };
                    if let Ok(state) = self.window_mut(window) {
                        state.focused = focused;
                    }
                    // The window procedure has already released or re-applied
                    // the *clip*, because one frame of a hostage desktop is one
                    // frame too many. Both are refreshed again here, and neither
                    // is redundant: `refresh_clip` is what establishes a
                    // confinement that was asked for **before** the window had
                    // the keyboard, and the visibility is what makes alt-tabbing
                    // out of mouselook give the pointer back. See
                    // [`input`](super::input).
                    self.refresh_clip();
                    self.refresh_cursor_visibility();
                    self.queue.push_back(ShellEvent::Focus { window, focused });
                }
                RawEvent::CloseRequested { .. } => {
                    let Some(window) = window else { continue };
                    if let Ok(state) = self.window_mut(window) {
                        state.close_pending = true;
                    }
                    self.queue.push_back(ShellEvent::CloseRequested { window });
                }
                RawEvent::Destroyed { .. } => {
                    // Only reachable when the window went away without this
                    // shell asking — `destroy_window` removes the pool entry
                    // *before* calling `DestroyWindow`, so its own
                    // `WM_DESTROY` finds nothing here and does not report a
                    // second `WindowDestroyed`.
                    let Some(window) = window else { continue };
                    if let Some(removed) = self.windows.remove(window.cast()) {
                        self.shared.forget(removed.key);
                        self.drop_clipboard_answers(window);
                        self.queue.push_back(ShellEvent::WindowDestroyed { window });
                    }
                }

                RawEvent::FilesDropped { hwnd, millis } => {
                    // **Taken before anything can `continue`.** The payload and
                    // its marker are one drop; leaving the payload behind when
                    // the marker is discarded would hand it to the next marker
                    // for that window, which is a scene importing the assets
                    // dropped on the previous one.
                    let Some(dropped) = self.shared.take_drop(hwnd) else {
                        continue;
                    };
                    let Some(window) = window else { continue };
                    if !self.window(window).is_ok_and(|state| state.accept_drops) {
                        // The system does not send `WM_DROPFILES` to a window
                        // without `WS_EX_ACCEPTFILES`, so this is the case
                        // where something outside this backend set the bit. The
                        // descriptor said no; that is the answer.
                        log::debug!(
                            "discarding a drop of {} file(s) on a window created without \
                             accept_drops",
                            dropped.paths.len()
                        );
                        continue;
                    }
                    let time = self.event_time(millis);
                    let position = Some(PhysicalPoint::new(
                        f64::from(dropped.x),
                        f64::from(dropped.y),
                    ));
                    // One event per file, which is what `DroppedFile`
                    // documents: a multi-file drop is several.
                    for path in dropped.paths {
                        self.queue.push_back(ShellEvent::DroppedFile {
                            window,
                            time,
                            path,
                            position,
                        });
                    }
                }
                RawEvent::MonitorsChanged => {
                    self.monitors = self.enumerate_monitors();
                    self.queue.push_back(ShellEvent::MonitorsChanged);
                }

                // The input half, which needs the layout, the modifier snapshot
                // and this shell's own surrogate and raw-motion state — see
                // [`input`](super::input).
                input_event @ (RawEvent::Key { .. }
                | RawEvent::Char { .. }
                | RawEvent::PointerMotion { .. }
                | RawEvent::PointerFocus { .. }
                | RawEvent::Button { .. }
                | RawEvent::Wheel { .. }
                | RawEvent::RawMotion { .. }) => self.translate_input(input_event, window),
            }
        }
    }

    /// Publishes configurations the system has already told us about.
    ///
    /// The one place Win32's synchronous geometry meets P0.4's asynchronous
    /// contract, and a copy of the X11 backend's for the same reason. A scale
    /// change is emitted **before** the resize it came with, because a consumer
    /// that recreates a swapchain on `Resized` must already know the scale it
    /// is recreating at.
    fn publish_configurations(&mut self) {
        let ready: Vec<(WindowId, WindowConfiguration)> = self
            .windows
            .iter_mut()
            .filter_map(|(handle, window)| {
                window.pending.take().map(|config| (handle.cast(), config))
            })
            .collect();
        for (window, config) in ready {
            let previous = self
                .windows
                .get(window.cast())
                .and_then(|state| state.configuration);
            if let Some(state) = self.windows.get_mut(window.cast()) {
                state.configuration = Some(config);
            }
            if previous.is_some_and(|previous| {
                (previous.scale_factor - config.scale_factor).abs() > f64::EPSILON
            }) {
                self.queue.push_back(ShellEvent::ScaleFactorChanged {
                    window,
                    scale_factor: config.scale_factor,
                    size: config.size,
                });
            }
            self.queue.push_back(ShellEvent::Resized {
                window,
                size: config.size,
                scale_factor: config.scale_factor,
            });
        }
    }
}

impl Shell for Win32Shell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::Win32
    }

    /// What this backend can do, fixed for the shell's lifetime.
    ///
    /// Each bit is a claim about code that exists in this backend:
    ///
    /// * [`MULTI_WINDOW`](ShellCaps::MULTI_WINDOW) — nothing about a window
    ///   class or a message queue is single-window.
    /// * [`EVENT_WAIT`](ShellCaps::EVENT_WAIT) —
    ///   [`wait_events`](Shell::wait_events) is
    ///   `MsgWaitForMultipleObjectsEx`, which genuinely blocks.
    /// * [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION) — one signed virtual
    ///   screen, `SetWindowPos` moves a window in it, and monitor bounds tile.
    ///   What that buys today is borderless on a *named* monitor, which the
    ///   Wayland backend can only hint at.
    /// * [`SERVER_DECORATIONS`](ShellCaps::SERVER_DECORATIONS) — a windowed
    ///   window is `WS_OVERLAPPEDWINDOW`, so the system draws the caption and
    ///   the borders and the UI layer must not.
    /// * [`FRACTIONAL_SCALE`](ShellCaps::FRACTIONAL_SCALE) — 125% is 120 DPI is
    ///   1.25, which is what the display settings offer and what
    ///   `GetDpiForWindow` reports.
    /// * [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED) — `WM_SIZING`,
    ///   named by `docs/plan/15-windowing.md` as this platform's form of it,
    ///   and implemented in [`geometry`](super::geometry).
    /// * [`POINTER_CONFINE`](ShellCaps::POINTER_CONFINE) and
    ///   [`POINTER_LOCK`](ShellCaps::POINTER_LOCK) — `ClipCursor` over the
    ///   client rectangle, plus a hidden cursor and a recentre for the second.
    ///   Re-established when the window moves, resizes or regains focus, and
    ///   released the instant it loses it.
    /// * [`POINTER_WARP`](ShellCaps::POINTER_WARP) — `SetCursorPos`, which
    ///   Windows has and Wayland refuses to.
    /// * [`RAW_POINTER_MOTION`](ShellCaps::RAW_POINTER_MOTION) — `WM_INPUT`, and
    ///   the **only bit here that is genuinely latched**: it is set if and only
    ///   if `RegisterRawInputDevices` succeeded at [`open`](Self::open). An
    ///   absolute-reporting device (remote desktop, a tablet) is differenced
    ///   rather than misread, so the bit means the same thing on `mstsc` as it
    ///   does on a desk.
    /// * [`CLIPBOARD`](ShellCaps::CLIPBOARD) — `OpenClipboard` in both
    ///   directions, with `CF_UNICODETEXT` for text and a
    ///   `RegisterClipboardFormatW` format named after the mime for everything
    ///   else, so an engine-to-engine copy is lossless while Notepad still gets
    ///   the text. A constant rather than a latch: the calls are `user32`'s and
    ///   are present or the process did not start. See
    ///   [`clipboard`](super::clipboard).
    /// * [`DRAG_DROP`](ShellCaps::DRAG_DROP) — `DragAcceptFiles` plus
    ///   `WM_DROPFILES`, honouring
    ///   [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops). The bit
    ///   is what [`DroppedFile`](ShellEvent::DroppedFile) documents it to be —
    ///   *files, in* — and no more: there is no drop cursor and no hover
    ///   feedback while a drag is in the air, because that is `IDropTarget` and
    ///   `IDropTarget` is COM. [`dnd`](super::dnd) gives the argument, and it is
    ///   a gap in *feedback* rather than in the capability the seam names.
    ///
    /// # `TEXT_IME` is clear, and that is a decision rather than a gap
    ///
    /// `WM_CHAR` is handled, so typing produces
    /// [`TextCommit`](ShellEvent::TextCommit) — surrogate pairs included, which
    /// is what makes an astral codepoint arrive whole. **That is not what this
    /// bit claims.** [`TEXT_IME`](ShellCaps::TEXT_IME) says the commit path is
    /// wired to a real input method, and nothing in this backend touches the
    /// `WM_IME_*` family: there is no composition string, no candidate-window
    /// placement, and no way for the seam to tell a pre-edit from a commit.
    ///
    /// Leaving `DefWindowProc` to run the default IME does deliver a committed
    /// Japanese string as `WM_CHAR`, and it would be easy to read that as the
    /// capability being met. It is not the standard the other backends are held
    /// to — Wayland latches this bit on having *bound* `text-input-v3` — and
    /// this backend has never been run on Windows, so setting it would be an
    /// unverified claim about a code path nobody has watched. A capability that
    /// overstates itself is worse than one that is missing.
    ///
    /// [`HW_UPSCALE`](ShellCaps::HW_UPSCALE) is clear for a reason rather than
    /// for want of work — a plain `HWND` presents at its own size, and the
    /// renderer does the upscale blit.
    fn caps(&self) -> ShellCaps {
        self.caps
    }

    /// Creates a window. **The window has no size yet.**
    ///
    /// # Windows knows the size, and this still returns without one
    ///
    /// `CreateWindowExW` takes a size and `GetClientRect` answers immediately,
    /// so — as on X11 and unlike Wayland — the size is known before this
    /// function returns. It is not *reported* before this function returns,
    /// because [`WindowState::size`] is defined as `None` until a
    /// [`Resized`](ShellEvent::Resized) has been delivered and delivering
    /// events is [`pump`](Shell::pump)'s job. The create-then-wait loop is real
    /// here as well; it just completes on the **first pump**.
    ///
    /// # `app_id` is validated and not yet applied
    ///
    /// Win32 has no `WM_CLASS`. The equivalent is the *Application User Model
    /// ID*, a process-wide string set with
    /// `SetCurrentProcessExplicitAppUserModelID` from `shell32`, and it is what
    /// decides taskbar grouping and which shortcut a window is matched to.
    /// This slice does not set it — that is a third system library for a
    /// property that is not per window — so
    /// [`WindowDesc::app_id`](crate::WindowDesc::app_id) is checked for
    /// validity and otherwise unused here. `docs/backlog.md` carries it.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] for a borderless request naming a monitor
    /// that is gone, [`ShellError::InvalidDescriptor`] for a title or app id
    /// containing a NUL byte or a size that rounds to nothing, and
    /// [`ShellError::WindowCreation`] carrying the `GetLastError` code if the
    /// system refused.
    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = desc.mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        // Validated here as well as in `create_native_window`, because the app
        // id never reaches a system call and would otherwise be accepted with
        // a NUL in it on this backend and rejected on the other two.
        if ffi::wide(desc.app_id).is_none() {
            return Err(ShellError::InvalidDescriptor(
                "app_id contains a NUL byte".to_string(),
            ));
        }
        let target = match desc.mode {
            DisplayMode::Borderless {
                monitor: Some(monitor),
            } => self.monitor(monitor).cloned(),
            _ => self
                .monitors
                .iter()
                .find(|monitor| monitor.is_primary)
                .or_else(|| self.monitors.first())
                .cloned(),
        };
        let created = self.create_native_window(desc, target.as_ref())?;
        let Some(hwnd) = NonNull::new(created.hwnd) else {
            return Err(ShellError::WindowCreation(
                "CreateWindowExW returned null".to_string(),
            ));
        };
        let scale_factor = geometry::scale_from_dpi(created.dpi);
        let effective_mode = match desc.mode {
            DisplayMode::Windowed => DisplayMode::Windowed,
            DisplayMode::Borderless { .. } => DisplayMode::Borderless {
                monitor: self.monitor_of(created.hwnd),
            },
        };
        let window = WinWindow {
            hwnd,
            key: proc::key(created.hwnd),
            title: desc.title.to_string(),
            requested_size: desc.size,
            requested_mode: desc.mode,
            requested_constraints: desc.constraints,
            resizable: desc.resizable,
            dpi: created.dpi,
            scale_factor,
            configuration: None,
            // Known now, delivered on the first pump; see the doc comment.
            pending: Some(WindowConfiguration {
                size: created.size,
                scale_factor,
                mode: effective_mode,
            }),
            last_size: Some(created.size),
            effective_mode,
            saved: None,
            cursor: None,
            pointer_mode: PointerMode::Free,
            focused: false,
            visible: Self::is_showing(created.hwnd),
            close_pending: false,
            accept_drops: desc.accept_drops,
        };
        let window = self.windows.insert(window).cast();
        let state = self.window(window)?;
        self.refresh_limits(state);
        Ok(window)
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        let removed = self
            .windows
            .remove(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))?;
        self.shared.forget(removed.key);
        self.drop_clipboard_answers(window);
        // The pool entry is gone **before** the system call, deliberately:
        // `DestroyWindow` dispatches `WM_DESTROY` into the window procedure
        // synchronously, and the `RawEvent::Destroyed` that produces must not
        // find a live window and report a second `WindowDestroyed` for it.
        //
        // SAFETY: destroying this shell's own window, from the thread that
        // created it — which is this one, because a `Shell` is not `Send`.
        unsafe { ffi::DestroyWindow(removed.raw()) };
        self.queue.push_back(ShellEvent::WindowDestroyed { window });
        Ok(())
    }

    fn window_state(&self, window: WindowId) -> Result<WindowState, ShellError> {
        let state = self.window(window)?;
        Ok(WindowState {
            configuration: state.configuration,
            requested_mode: state.requested_mode,
            requested_constraints: state.requested_constraints,
            focused: state.focused,
            visible: state.visible,
            pointer_mode: state.pointer_mode,
            close_pending: state.close_pending,
        })
    }

    /// Sets the title bar text.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale, or
    /// [`ShellError::InvalidDescriptor`] if the title contains a NUL byte —
    /// which would silently truncate it, since every `*W` entry point takes a
    /// NUL-terminated string.
    fn set_title(&mut self, window: WindowId, title: &str) -> Result<(), ShellError> {
        let hwnd = self.window(window)?.raw();
        let wide = ffi::wide(title).ok_or_else(|| {
            ShellError::InvalidDescriptor("title contains a NUL byte".to_string())
        })?;
        // SAFETY: `wide` is NUL-terminated UTF-16 that outlives the call, which
        // copies it. This dispatches `WM_SETTEXT` into the window procedure
        // synchronously; nothing is borrowed here.
        unsafe { ffi::SetWindowTextW(hwnd, wide.as_ptr()) };
        self.window_mut(window)?.title = title.to_string();
        Ok(())
    }

    /// Shows or hides the window — **really**, as on X11 and unlike Wayland.
    ///
    /// `ShowWindow` is reversible with no state lost, so this is one of the
    /// places the two desktop backends can do something the Wayland one
    /// documents that it cannot: there, a surface is mapped exactly while it
    /// has a buffer, and buffers belong to the renderer.
    ///
    /// [`WindowState::visible`] is read back from the system rather than set
    /// from the request, so a `SW_SHOWNORMAL` that was overruled — by a policy,
    /// or by the window being minimized — reports what happened.
    fn set_visible(&mut self, window: WindowId, visible: bool) -> Result<(), ShellError> {
        let hwnd = self.window(window)?.raw();
        let command = if visible {
            // `SW_SHOWNORMAL` rather than `SW_SHOW`: it also restores a window
            // that the user minimized, which is what "make it visible" means.
            value::SW_SHOW_NORMAL
        } else {
            value::SW_HIDE
        };
        // SAFETY: this shell's own window; dispatches `WM_SHOWWINDOW` and
        // possibly `WM_SIZE` into the window procedure synchronously.
        unsafe { ffi::ShowWindow(hwnd, command) };
        let showing = Self::is_showing(hwnd);
        self.window_mut(window)?.visible = showing;
        Ok(())
    }

    /// Asks for windowed or borderless.
    ///
    /// A request in the seam's sense, and on this platform one that is always
    /// granted: there is no window manager to refuse it, because borderless is
    /// this process changing its own window's style. What can still disagree
    /// with the request is the **monitor** — `Borderless { monitor: None }`
    /// means "wherever the window already is", and the answer names it — which
    /// is why [`mode_request_honoured`](WindowState::mode_request_honoured)
    /// compares with [`DisplayMode::satisfied_by`] rather than with `==`.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle,
    /// [`ShellError::NoSuchMonitor`] if the named monitor is gone, or
    /// [`ShellError::Backend`] if there is no monitor at all to be borderless
    /// on.
    fn set_mode(&mut self, window: WindowId, mode: DisplayMode) -> Result<(), ShellError> {
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        self.window_mut(window)?.requested_mode = mode;
        self.apply_mode(window, mode)?;

        // The `WM_SIZE` the style change produced is already in the raw queue,
        // and `translate` will discard it as a restatement — so the
        // configuration is published from here instead, which also covers the
        // case where the two modes happen to have the same client size.
        let config = Self::configuration_of(self.window(window)?);
        let state = self.window_mut(window)?;
        state.pending = Some(config);
        state.last_size = Some(config.size);
        Ok(())
    }

    /// Asks for resize limits and an aspect lock.
    ///
    /// Applied through the window procedure, which answers `WM_GETMINMAXINFO`
    /// and `WM_SIZING` from numbers this call precomputes — see
    /// [`geometry`](super::geometry). Unlike X11's `WM_NORMAL_HINTS`, there is
    /// nobody who might not read them: the enforcement is in this process.
    ///
    /// Nothing is resized here. A constraint bounds what the *user* may drag
    /// to; a window that is already outside it stays there until it is next
    /// resized, which is what every Windows application does and what
    /// [`SizeConstraints`] documents as "hints, whose only observable effect is
    /// whatever size the window system reports next".
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_constraints(
        &mut self,
        window: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        self.window_mut(window)?.requested_constraints = constraints;
        let state = self.window(window)?;
        self.refresh_limits(state);
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Runs the message queue and delivers what it produced.
    ///
    /// The order is the same as the two Linux backends': the system first, so
    /// that everything the window procedure recorded during this drain is
    /// visible; then the translation into shell state; then the configurations,
    /// so a resize and its scale change land together; then delivery.
    ///
    /// **This does not return during a user drag-resize.** That is a Win32
    /// fact, not a bug in this loop; the [module docs](super) state what it
    /// costs and why the alternative is a decision above this crate.
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        self.drain_messages();
        self.translate();
        self.publish_configurations();
        // Drain by count, not `while let`: a sink that creates a window must
        // not be able to spin this loop, and whatever it queued belongs to the
        // next frame — which is what the message queue would have done anyway.
        for _ in 0..self.queue.len() {
            let Some(event) = self.queue.pop_front() else {
                break;
            };
            sink(event);
        }
    }

    /// Blocks until a message arrives or `timeout` elapses.
    ///
    /// The hazard is a wait that sleeps out its whole timeout with work already
    /// queued — an editor that called [`pump`](Shell::pump), was handed
    /// something that queued another message, and then went to sleep. What
    /// closes it is that [`wait`](Self::wait) **drains the queue before it
    /// sleeps**, so there is never work waiting when the sleep starts. See
    /// there for why that is the drain rather than `MWMO_INPUTAVAILABLE`, and
    /// for the CI run that decided it.
    fn wait_events(&mut self, timeout: Option<Duration>) {
        // A wait that cannot wait turns an editor idling at zero frames per
        // second into one spinning a core, and says nothing while it does it.
        // Nothing can be done about it here — the caller has no alternative to
        // offer — so it is reported and the loop continues.
        match self.wait(timeout) {
            Wake::TimedOut | Wake::Message => {}
            Wake::Failed { error } => {
                log::warn!("MsgWaitForMultipleObjectsEx failed with error {error}; not sleeping");
            }
            Wake::Unexpected { outcome } => {
                log::warn!(
                    "MsgWaitForMultipleObjectsEx returned {outcome}, which is not one of its documented answers"
                );
            }
        }
    }

    fn align_event_clock(&mut self, elapsed: Duration) {
        self.time.align_at(
            ffi::tick_nanos(),
            u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
        );
    }

    /// The handles `crcbl-vk` needs for `vkCreateWin32SurfaceKHR`.
    ///
    /// Available the instant the window exists — Win32 creates a real, sized
    /// window synchronously — so a HAL surface can be created immediately and
    /// only the swapchain waits for the first
    /// [`Resized`](ShellEvent::Resized).
    ///
    /// The `HINSTANCE` is the module that registered the window class, which is
    /// what the WSI extension wants: it identifies the class the `HWND` belongs
    /// to, not the thread.
    ///
    /// Lifetime: the window lives as long as the [`WindowId`], so a HAL surface
    /// built from these must be destroyed before
    /// [`destroy_window`](Shell::destroy_window) and before the shell is
    /// dropped.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError> {
        let state = self.window(window)?;
        Ok(SurfaceTarget::Win32 {
            hinstance: self.instance,
            hwnd: state.hwnd,
        })
    }

    /// Frees, confines or locks the pointer.
    ///
    /// # Both captured modes are one `ClipCursor`
    ///
    /// [`Confined`](PointerMode::Confined) and [`Locked`](PointerMode::Locked)
    /// bound the pointer identically — the client rectangle, in screen
    /// coordinates — and differ in what is *reported*:
    ///
    /// * Confined keeps the cursor drawn and
    ///   [`abs`](ShellEvent::PointerMotion) flowing. The clip is the whole
    ///   implementation.
    /// * Locked additionally hides the cursor, suppresses `abs` — which
    ///   [`ShellEvent::PointerMotion`] documents as the invariant of the mode —
    ///   and recentres the pointer when it drifts, so leaving the mode does not
    ///   leave the cursor in a corner. The camera reads `WM_INPUT` instead, which
    ///   is unaffected by either the clip or the recentre.
    ///
    /// Unlike X11 there is no grab to be refused: `ClipCursor` is not a request
    /// to another client, so this cannot fail for a reason outside this process.
    /// It *can* stop being in effect, and does — see
    /// [`input`](super::input) and the window procedure, which release the clip
    /// the instant the window loses focus and re-establish it when focus
    /// returns, when the window moves and when it is resized.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::Unsupported`] naming the mode — which is what a caller
    /// that checked [`PointerMode::required_cap`] against
    /// [`caps`](Shell::caps) never sees.
    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        self.window(window)?;
        if !self.caps.contains(mode.required_cap()) {
            return Err(Self::unsupported(mode.as_str()));
        }
        // The clip is per shell rather than per window — there is one cursor —
        // so a second window taking it releases the first's, exactly as the X11
        // backend's grab does.
        let previous: Vec<WindowId> = self
            .windows_iter()
            .filter(|(handle, state)| *handle != window && state.pointer_mode.is_captured())
            .map(|(handle, _)| handle)
            .collect();
        for held in previous {
            if let Ok(state) = self.window_mut(held) {
                state.pointer_mode = PointerMode::Free;
            }
        }
        self.window_mut(window)?.pointer_mode = mode;
        self.refresh_clip();
        self.refresh_cursor_visibility();
        if mode == PointerMode::Locked
            && let Some(size) = self.window(window)?.configuration.map(|config| config.size)
        {
            // Start from the middle, so the first recentre is not immediate.
            self.warp_to_client(
                window,
                i32::try_from(size.width / 2).unwrap_or(0),
                i32::try_from(size.height / 2).unwrap_or(0),
            );
        }
        Ok(())
    }

    /// Sets the cursor shape, or hides the cursor with `None`.
    ///
    /// # Two mechanisms, because Windows has two questions
    ///
    /// A *shape* is answered from `WM_SETCURSOR`, and only from there: the
    /// system asks on every pointer movement and applies the window class's
    /// cursor if nothing answers, so a `SetCursor` called from here would be
    /// overwritten before the next frame. The loaded `HCURSOR` is therefore
    /// recorded for the window procedure — see [`proc`](super::proc) — and takes
    /// effect on the next movement, which is the first moment anything could
    /// have been drawn anyway.
    ///
    /// *Hiding* is `ShowCursor`, whose per-thread reference count is the classic
    /// bug in this API and is kept balanced by
    /// [`pointer::Visibility`](super::pointer::Visibility) rather than by
    /// counting calls by hand. It applies while the **focused** window wants it
    /// hidden; [`input`](super::input) states that rule and its one visible
    /// consequence.
    ///
    /// Unlike the X11 backend, a named shape really is applied here: Windows
    /// ships the stock cursor set in `user32`, so there is no theme to load and
    /// no dependency to take on. Three of the seam's shapes have no exact stock
    /// equivalent and are approximated;
    /// [`pointer::cursor_id`](super::pointer::cursor_id) names which and to what.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        self.window_mut(window)?.cursor = Some(cursor);
        if let Some(icon) = cursor {
            self.apply_cursor_shape(window, icon)?;
        }
        self.refresh_cursor_visibility();
        Ok(())
    }

    /// Moves the pointer to a position in the window.
    ///
    /// `ClientToScreen` then `SetCursorPos`, which is the capability
    /// [`POINTER_WARP`](ShellCaps::POINTER_WARP) names and which Wayland
    /// deliberately does not have. The warning on the trait method applies here
    /// as it does on X11 and is worth repeating: a camera must not be built on
    /// this. A warp produces a `WM_MOUSEMOVE` indistinguishable from the user's
    /// own, so a warp-based camera fights every real movement — which is why
    /// [`PointerMode::Locked`] plus `WM_INPUT` exists.
    ///
    /// The position is **clamped by the system** to the current clip rectangle
    /// and to the virtual screen, so a warp outside a confined window lands on
    /// its edge rather than being refused.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn warp_pointer(
        &mut self,
        window: WindowId,
        position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        self.window(window)?;
        self.warp_to_client(window, position.x as i32, position.y as i32);
        Ok(())
    }

    fn reply_close_request(
        &mut self,
        window: WindowId,
        reply: CloseReply,
    ) -> Result<(), ShellError> {
        let state = self.window_mut(window)?;
        if !state.close_pending {
            return Err(ShellError::NoPendingCloseRequest { window });
        }
        state.close_pending = false;
        match reply {
            CloseReply::Keep => Ok(()),
            CloseReply::Close => self.destroy_window(window),
        }
    }

    /// Publishes content, or empties the clipboard with an empty slice.
    ///
    /// Every offer is written under its own format — `CF_UNICODETEXT` for text
    /// and a registered format named after the mime for everything else — so a
    /// `[text, ron]` pair reaches Notepad *and* round-trips through another
    /// Crucible losslessly. The reader picks; see
    /// [`clipboard`](super::clipboard).
    ///
    /// # "Release" means "empty", because that is what Win32 has
    ///
    /// The seam says an empty slice "releases the clipboard, where the platform
    /// can express that". X11 and Wayland can: both have an *owner*, and giving
    /// it up leaves whatever the previous owner published in place. Windows has
    /// no owner to give up — the clipboard is content, and the only thing a
    /// process can do to content it put there is discard it. So an empty slice
    /// is `EmptyClipboard`, and afterwards the clipboard holds nothing rather
    /// than holding what it held before this shell wrote to it.
    ///
    /// # Win32 never returns `NeedsUserInteraction`, and that is complete
    ///
    /// [Obligation 6](Shell) says a backend on a platform with no
    /// user-interaction rule never returns
    /// [`ShellError::NeedsUserInteraction`]. Windows has no such rule: any
    /// window of any process may open the clipboard at any time, with no serial,
    /// no timestamp and no focus involved. A background process can take the
    /// clipboard here, which Wayland forbids by design — a real difference in
    /// what the two platforms permit, not a gap in this backend. The same
    /// sentence the X11 backend writes, for the same reason.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::Backend`] if the clipboard could not be opened within
    /// [`clipboard::OPEN_BUDGET`](super::clipboard::OPEN_BUDGET) — another
    /// process is holding it — or if the system refused every format.
    fn clipboard_offer(
        &mut self,
        window: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        let hwnd = self.window(window)?.raw();
        let board = Clipboard::open(hwnd).map_err(|refused| {
            ShellError::Backend(format!(
                "the clipboard could not be opened within {:?} ({refused:?}); another process is \
                 holding it",
                clipboard::OPEN_BUDGET
            ))
        })?;
        if let Opened::After { attempts } = board.opened() {
            log::debug!("the clipboard was held by another process for {attempts} attempts");
        }
        // Required before a write, and the whole of a release. It also makes
        // this process the clipboard's owner, which is what lets the system
        // synthesize `CF_TEXT` from the `CF_UNICODETEXT` written below.
        if !board.empty() {
            return Err(ShellError::Backend(
                "EmptyClipboard was refused, so nothing could be published".to_string(),
            ));
        }
        if offers.is_empty() {
            return Ok(());
        }

        let mut published = 0usize;
        for offer in offers {
            let Some(format) = clipboard::format_id(offer.mime) else {
                continue;
            };
            let encoding = clipboard::encoding_for(offer.mime);
            if encoding == clipboard::Encoding::UnicodeText
                && core::str::from_utf8(offer.bytes).is_err()
            {
                // `ClipboardOffer` is explicit that it does not validate, and
                // `CF_UNICODETEXT` cannot carry a byte that is not a character.
                // Said out loud, because the replacement characters are
                // otherwise found by whoever pastes.
                log::warn!(
                    "a text/plain clipboard offer is not valid UTF-8; the invalid bytes will \
                     paste as replacement characters"
                );
            }
            if board.put(format, &clipboard::payload_bytes(encoding, offer.bytes)) {
                published += 1;
            }
        }
        if published == 0 {
            // The clipboard has already been emptied, which is the honest state
            // to leave it in: this process claimed it and then published
            // nothing, rather than leaving somebody else's content under our
            // ownership.
            return Err(ShellError::Backend(
                "no offered format could be published to the clipboard".to_string(),
            ));
        }
        Ok(())
    }

    /// Asks for the clipboard's content in `mime`.
    ///
    /// # Which obligations this satisfies, and how
    ///
    /// * **Exactly one answer** ([obligation 4](Shell)): the read happens inside
    ///   this call and its [`ClipboardData`](ShellEvent::ClipboardData) is
    ///   queued before it returns, so it is delivered by the next
    ///   [`pump`](Shell::pump). There is no list of outstanding reads to lose
    ///   one from. The single exception is the one the X11 backend names too —
    ///   a window destroyed before that pump, where obligation 1 wins; see
    ///   [`drop_clipboard_answers`](Self::drop_clipboard_answers).
    /// * **Within a bounded time** (also 4): the only wait is
    ///   `OpenClipboard` being refused by a process that has it open, and that
    ///   is retried for [`clipboard::OPEN_BUDGET`](super::clipboard::OPEN_BUDGET)
    ///   and then answered [`Unavailable`](ClipboardContent::Unavailable). There
    ///   is no transfer to stall: `GetClipboardData` hands over memory rather
    ///   than starting a conversation with another process, so this backend has
    ///   no equivalent of X11's `INCR` deadline or Wayland's pipe timeout.
    /// * **Held rather than answered empty** ([obligation 5](Shell)): there is
    ///   nothing to hold. Any window may open the clipboard at any time —
    ///   **Win32 has neither Wayland's focus gate nor its serial requirement** —
    ///   so "hold until answerable" collapses to "answer", and the obligation is
    ///   discharged by answering. [`clipboard_readable`](Shell::clipboard_readable)
    ///   is therefore left at the trait's provided default, which that method's
    ///   own documentation names this platform as being right for.
    ///
    /// # The answer names the format that was asked for
    ///
    /// [`ClipboardData::mime`](ShellEvent::ClipboardData) is "the format that
    /// was delivered, spelled the way the *other* application spelled it". A
    /// Win32 clipboard format is a **number**, not a string: there is no peer
    /// spelling in existence to report, so the answer echoes the request. That
    /// is not a shortcut — it is what `ReceivedMime` documents for the case
    /// where there is nothing else to say.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle. An empty clipboard is
    /// an answer, not an error.
    fn clipboard_request(
        &mut self,
        window: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        let hwnd = self.window(window)?.raw();
        let request = ClipboardRequestId(self.next_request);
        self.next_request = self.next_request.wrapping_add(1);
        let content = self.read_clipboard(hwnd, mime);
        // Queued rather than returned, even though it is known now: a consumer
        // written against the asynchronous shape is one that also works on X11,
        // where the answer is several round trips away.
        self.queue_event(ShellEvent::ClipboardData {
            window,
            request,
            mime: ReceivedMime::from(mime),
            content,
        });
        Ok(request)
    }
}

impl Drop for Win32Shell {
    /// Destroys every window this shell still owns.
    ///
    /// The order is the whole soundness argument for the raw pointer in
    /// `GWLP_USERDATA`: `DestroyWindow` dispatches `WM_DESTROY` and
    /// `WM_NCDESTROY` into the window procedure **synchronously**, and the
    /// procedure reads [`Shared`] through that pointer. Both messages therefore
    /// have to run while this `Rc` is still alive, which is exactly what doing
    /// it here — before the field is dropped — guarantees.
    ///
    /// The window class is deliberately not unregistered; see
    /// [`window`](super::window). Neither is the raw-input registration; see
    /// [`input`](super::input).
    ///
    /// # Two pieces of desktop state have to be handed back
    ///
    /// Both are process- or thread-wide rather than per window, so destroying
    /// the windows does not undo them, and both are user-visible if they are
    /// left behind:
    ///
    /// * The **cursor clip**. A process that exits with the cursor clipped
    ///   leaves it clipped to a rectangle no window occupies. Windows does clean
    ///   this up when the *process* exits, which is exactly why it must be done
    ///   here — a shell dropped by a host application that keeps running would
    ///   otherwise hold the desktop hostage with no window to point at.
    /// * The **`ShowCursor` count**, for the same reason and with the same
    ///   asymmetry: one hide outstanding is an invisible cursor over every
    ///   window this thread owns afterwards.
    fn drop(&mut self) {
        input::release_clip();
        Self::show_cursor(self.visibility.want(false));
        let windows: Vec<Handle> = self
            .windows
            .iter()
            .map(|(_, window)| window.raw())
            .collect();
        for hwnd in windows {
            // SAFETY: destroying this shell's own windows from the thread that
            // created them — a `Shell` is not `Send`, so this is that thread.
            unsafe { ffi::DestroyWindow(hwnd) };
        }
    }
}

/// Tests that need a real desktop, and are therefore the point of the
/// `build + test (windows-latest)` CI job.
///
/// **Nothing here is `#[ignore]`d.** `docs/plan/12-testing.md` calls a
/// silently-skipped suite a known trap, and this slice is deliberately how the
/// project finds out whether a GitHub runner gives a process a usable window
/// station: if it does not, [`Win32Shell::open`] fails and every one of these
/// says so, which is an answer. A skipped test is not.
#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::{AspectRatio, LogicalSize, MimeType};
    use crcbl_core::KeyCode;
    use crcbl_core::input::{ButtonState, Keysym, PointerButton, Scancode, ScrollDelta};
    use std::path::PathBuf;
    use std::time::Instant;

    use super::super::ffi::{DropFiles, Lparam, MinMaxInfo, Point, Rect, msg};

    /// `VK_W`, which is the letter's virtual key on every layout that has one.
    const VK_W: usize = 0x57;
    /// `VK_UP`.
    const VK_UP: usize = 0x26;

    /// A shell, or a failure that names what the runner did not provide.
    fn shell() -> Win32Shell {
        Win32Shell::open().expect(
            "opening the Win32 shell needs a usable window station; a failure here is the \
             runner's answer to whether it has one",
        )
    }

    fn window(shell: &mut Win32Shell) -> WindowId {
        shell
            .create_window(&WindowDesc {
                title: "crcbl P5C W2",
                ..WindowDesc::default()
            })
            .expect("creating a top-level window")
    }

    /// The `HWND`, for the messages only a user's mouse would otherwise send.
    fn hwnd_of(shell: &Win32Shell, window: WindowId) -> Handle {
        shell.window(window).expect("a live window").raw()
    }

    /// Sends a key message with the `lParam` the keyboard driver would build.
    ///
    /// The whole of input is otherwise unreachable from CI, which has no
    /// keyboard and no mouse. `SendMessageW` runs the real window procedure
    /// against the real cached state, so everything below the driver — the scan
    /// code table, the modifier snapshot, the layout lookup — is exercised.
    fn send_key(
        hwnd: Handle,
        message: u32,
        virtual_key: usize,
        scancode: isize,
        extended: bool,
        previous: bool,
    ) {
        let mut l_param = (scancode << 16) | 1;
        if extended {
            l_param |= 1 << 24;
        }
        if previous {
            l_param |= 1 << 30;
        }
        if message == msg::KEY_UP {
            // The transition bit, which is set on every release and which this
            // backend must not confuse with the previous-state bit beside it.
            l_param |= 1 << 31;
        }
        // SAFETY: a keyboard message to this shell's own window, with the
        // `wParam`/`lParam` encoding `winuser.h` documents for it.
        unsafe { ffi::SendMessageW(hwnd, message, virtual_key, l_param) };
    }

    /// Sends a mouse message with two coordinates packed into `lParam`.
    fn send_mouse(hwnd: Handle, message: u32, w_param: usize, x: i32, y: i32) {
        let l_param = (((y as u16 as usize) << 16) | (x as u16 as usize)) as Lparam;
        // SAFETY: a mouse message to this shell's own window, with the packed
        // coordinate encoding `winuser.h` documents.
        unsafe { ffi::SendMessageW(hwnd, message, w_param, l_param) };
    }

    /// Sends the `WM_MOUSELEAVE` a real pointer leaving would produce.
    ///
    /// The one-shot notification this backend arms with `TrackMouseEvent`, sent
    /// by hand because CI has no mouse to move out — and, at the *start* of a
    /// test, the only way to put the derived "is the pointer inside?" state on a
    /// known edge when a real cursor may already be over the window.
    fn send_leave(hwnd: Handle) {
        // SAFETY: a pointer-crossing message to this shell's own window, with
        // the `wParam`/`lParam` of zero `winuser.h` documents for it.
        unsafe { ffi::SendMessageW(hwnd, msg::MOUSE_LEAVE, 0, 0) };
    }

    /// Makes a window the foreground one.
    ///
    /// `ClipCursor` and `SetCursorPos` are both restricted to the process that
    /// owns the foreground window — the desktop's protection against exactly the
    /// hostage-taking this backend is careful not to do — so a test that
    /// exercises either has to arrange it. A runner where this does not take is
    /// a runner where those two tests fail with the clip unchanged, which is the
    /// honest report.
    fn make_foreground(hwnd: Handle) {
        // SAFETY: this shell's own window, from the thread that created it.
        unsafe { ffi::SetForegroundWindow(hwnd) };
    }

    /// Sends `WM_SETFOCUS` or `WM_KILLFOCUS`.
    ///
    /// The clip and the cursor both follow the focus, and CI cannot click on a
    /// window to give it one.
    fn send_focus(hwnd: Handle, focused: bool) {
        let message = if focused {
            msg::SET_FOCUS
        } else {
            msg::KILL_FOCUS
        };
        // SAFETY: a focus message to this shell's own window.
        unsafe { ffi::SendMessageW(hwnd, message, 0, 0) };
    }

    /// Every [`ShellEvent::Key`] one pump produced, flattened for assertion.
    fn pump_keys(
        shell: &mut Win32Shell,
    ) -> Vec<(Scancode, Option<KeyCode>, Keysym, ButtonState, bool)> {
        let mut keys = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::Key {
                scancode,
                key_code,
                keysym,
                state,
                repeat,
                ..
            } = event
            {
                keys.push((scancode, key_code, keysym, state, repeat));
            }
        });
        keys
    }

    /// The rectangle the cursor is currently clipped to.
    fn clip_rect() -> Rect {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live, initialised `RECT` the call writes into.
        unsafe { ffi::GetClipCursor(&raw mut rect) };
        rect
    }

    /// The whole desktop, as one rectangle.
    ///
    /// The origin is read rather than assumed to be zero: a monitor to the left
    /// of the primary puts the virtual screen's left edge at a negative
    /// coordinate, and a test that assumed the origin would pass on this
    /// developer's desk and fail on somebody else's.
    fn virtual_screen() -> Rect {
        // SAFETY: four metric queries by value; each reads no memory of ours
        // and answers zero for an index it does not know.
        unsafe {
            let left = ffi::GetSystemMetrics(value::SM_X_VIRTUAL_SCREEN);
            let top = ffi::GetSystemMetrics(value::SM_Y_VIRTUAL_SCREEN);
            Rect {
                left,
                top,
                right: left + ffi::GetSystemMetrics(value::SM_CX_VIRTUAL_SCREEN),
                bottom: top + ffi::GetSystemMetrics(value::SM_CY_VIRTUAL_SCREEN),
            }
        }
    }

    /// Whether any window of this process has the clipboard open.
    ///
    /// The observable behind "closed on every path out": the guard's `Drop` is
    /// what has to have run, and this is the only thing that can see whether it
    /// did.
    fn clipboard_is_open() -> bool {
        // SAFETY: the call takes no arguments and only reads window-station
        // state.
        !unsafe { ffi::GetOpenClipboardWindow() }.is_null()
    }

    /// A window that accepts file drops, which is off by default.
    fn drop_window(shell: &mut Win32Shell) -> WindowId {
        shell
            .create_window(&WindowDesc {
                title: "crcbl P5C W3 — drops",
                accept_drops: true,
                ..WindowDesc::default()
            })
            .expect("creating a drop target")
    }

    /// Builds the `HDROP` a file manager's drop would have handed over.
    ///
    /// CI has nobody to drag a file onto a window, so this is the drag: the
    /// block is exactly what the shell puts behind an `HDROP`, and everything
    /// downstream of it — `DragQueryFileW`, `DragQueryPoint`, `DragFinish`, the
    /// window procedure, the shared drop queue and the translation — is the
    /// real path. The handle is **not** freed here: `DragFinish` inside the
    /// procedure owns it, and that is part of what this exercises.
    fn make_hdrop(paths: &[&str], x: i32, y: i32) -> Handle {
        let mut list: Vec<u16> = Vec::new();
        for path in paths {
            list.extend(path.encode_utf16());
            list.push(0);
        }
        // The list itself is terminated by an empty entry, which is the second
        // NUL after the last path — and the whole list when there are none.
        list.push(0);

        let header = size_of::<DropFiles>();
        let bytes = header + list.len() * 2;
        // SAFETY: an allocation request by value. `GMEM_MOVEABLE` is what an
        // `HDROP` is, which is why `DragQueryFileW` can lock it.
        let mem = unsafe { ffi::GlobalAlloc(value::GMEM_MOVEABLE, bytes) };
        assert!(!mem.is_null(), "GlobalAlloc for a synthetic HDROP");
        // SAFETY: `mem` is a live moveable block this function owns until the
        // message is sent.
        let locked = unsafe { ffi::GlobalLock(mem) };
        assert!(!locked.is_null(), "GlobalLock of a synthetic HDROP");
        // SAFETY: `locked` points at `bytes` writable bytes, which is
        // `size_of::<DropFiles>()` followed by exactly `list.len()` code units;
        // `GlobalAlloc` returns at least 8-byte alignment, so both the header
        // write and the `u16` copy at the even offset `header` are aligned.
        unsafe {
            ptr::write_bytes(locked.cast::<u8>(), 0, bytes);
            locked.cast::<DropFiles>().write(DropFiles {
                p_files: header as u32,
                pt: Point { x, y },
                f_nc: 0,
                f_wide: 1,
            });
            ptr::copy_nonoverlapping(
                list.as_ptr(),
                locked.cast::<u8>().add(header).cast::<u16>(),
                list.len(),
            );
            ffi::GlobalUnlock(mem);
        }
        mem
    }

    /// Sends a `WM_DROPFILES` carrying a synthetic `HDROP`.
    fn send_drop(hwnd: Handle, paths: &[&str], x: i32, y: i32) {
        let hdrop = make_hdrop(paths, x, y);
        // SAFETY: a drop message to this shell's own window, with the `wParam`
        // `shell32` documents for it. The procedure finishes the handle.
        unsafe { ffi::SendMessageW(hwnd, msg::DROP_FILES, hdrop as usize, 0) };
    }

    /// Every [`ShellEvent::DroppedFile`] one pump produced.
    fn pump_drops(shell: &mut Win32Shell) -> Vec<(WindowId, PathBuf, Option<PhysicalPoint>)> {
        let mut dropped = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::DroppedFile {
                window,
                path,
                position,
                ..
            } = event
            {
                dropped.push((window, path, position));
            }
        });
        dropped
    }

    /// Reads the clipboard and pumps until the answer arrives.
    ///
    /// One pump is always enough on this backend — the read happens inside
    /// `clipboard_request` — and the loop is not a retry: it asserts that.
    fn paste(shell: &mut Win32Shell, window: WindowId, requested: MimeType) -> ClipboardContent {
        let request = shell
            .clipboard_request(window, requested)
            .expect("CLIPBOARD is claimed");
        let mut answers = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::ClipboardData {
                window: asked,
                request: answered,
                mime,
                content,
            } = event
            {
                assert_eq!(asked, window, "the answer names the window that asked");
                assert_eq!(answered, request, "and the request it answers");
                assert!(
                    mime.matches(requested),
                    "a Win32 format is a number, so the answer echoes the request: {mime}"
                );
                answers.push(content);
            }
        });
        assert_eq!(
            answers.len(),
            1,
            "exactly one answer per accepted request, on the first pump: {answers:?}"
        );
        answers.remove(0)
    }

    /// The cursor's display count, read without changing it.
    ///
    /// There is no read-only accessor: `ShowCursor` returns the count *after*
    /// its own adjustment, so the only way to see it is to move it and put it
    /// back. The pair is balanced, which is exactly what the code under test has
    /// to be as well.
    fn cursor_display_count() -> i32 {
        // SAFETY: two integers by value; neither call can fail, and together
        // they leave the count where they found it.
        unsafe {
            let raised = ffi::ShowCursor(1);
            ffi::ShowCursor(0);
            raised - 1
        }
    }

    #[test]
    fn a_window_has_no_size_until_the_first_pump_and_exactly_one_is_enough() {
        // The P0.4 contract, and the Win32 shape of it: the system knew the
        // size before `create_window` returned, so the wait is one pump — not a
        // round trip, and not an invented delay to look like Wayland.
        let mut shell = shell();
        let window = window(&mut shell);
        assert_eq!(
            shell.window_state(window).expect("live").size(),
            None,
            "a freshly created window has not been configured yet"
        );

        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        let size = shell
            .window_state(window)
            .expect("live")
            .size()
            .expect("one pump is enough on Win32");
        assert!(!size.is_empty(), "{size}");
        assert!(
            events.iter().any(|event| matches!(
                event,
                ShellEvent::Resized { window: reported, .. } if *reported == window
            )),
            "the size arrives as an event, not only as state: {events:?}"
        );

        // And it is not re-delivered: the `WM_SIZE` that `CreateWindowExW`
        // sent is a restatement of a size already published.
        let mut again = Vec::new();
        shell.pump(&mut |event| again.push(event));
        assert!(
            !again
                .iter()
                .any(|event| matches!(event, ShellEvent::Resized { .. })),
            "{again:?}"
        );
    }

    #[test]
    fn a_window_opens_flips_to_borderless_and_back_and_is_destroyed() {
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let windowed = shell
            .window_state(window)
            .expect("live")
            .size()
            .expect("configured");

        let primary = shell
            .monitors()
            .iter()
            .find(|monitor| monitor.is_primary)
            .expect("a desktop session has a primary monitor")
            .id;
        let bounds = shell.monitor(primary).expect("just found it").bounds;

        shell
            .set_mode(
                window,
                DisplayMode::Borderless {
                    monitor: Some(primary),
                },
            )
            .expect("borderless on a named monitor is what WINDOW_POSITION claims");
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        let state = shell.window_state(window).expect("live");
        assert_eq!(
            state.size(),
            Some(bounds.size()),
            "a borderless window covers its monitor exactly"
        );
        assert!(
            state.mode_request_honoured(),
            "Win32 has no window manager to refuse: {state:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ShellEvent::Resized { .. })),
            "the mode change is reported as a resize: {events:?}"
        );

        shell
            .set_mode(window, DisplayMode::Windowed)
            .expect("and back");
        shell.pump(&mut |_| {});
        let state = shell.window_state(window).expect("live");
        assert_eq!(
            state.size(),
            Some(windowed),
            "the saved WINDOWPLACEMENT restores the windowed size exactly"
        );
        assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
        assert!(state.mode_request_honoured());

        shell.destroy_window(window).expect("destroy");
        let mut destroyed = false;
        shell.pump(&mut |event| {
            destroyed |=
                matches!(event, ShellEvent::WindowDestroyed { window: gone } if gone == window);
        });
        assert!(destroyed, "destruction is reported");
        assert!(matches!(
            shell.window_state(window),
            Err(ShellError::InvalidWindow { .. })
        ));
    }

    #[test]
    fn a_window_created_borderless_can_still_be_made_windowed() {
        // The case with no saved placement to restore: the window has never
        // had a windowed style, so one is built from the descriptor rather
        // than the recorded mode being changed under a window that is still
        // `WS_POPUP`.
        let mut shell = shell();
        let primary = shell
            .monitors()
            .iter()
            .find(|monitor| monitor.is_primary)
            .expect("a primary monitor")
            .id;
        let bounds = shell.monitor(primary).expect("just found it").bounds;
        let window = shell
            .create_window(&WindowDesc {
                title: "crcbl P5C W1 — borderless from the start",
                mode: DisplayMode::Borderless {
                    monitor: Some(primary),
                },
                size: LogicalSize::new(800.0, 600.0),
                ..WindowDesc::default()
            })
            .expect("create borderless");
        shell.pump(&mut |_| {});
        let state = shell.window_state(window).expect("live");
        assert_eq!(state.size(), Some(bounds.size()), "it covers the monitor");
        assert!(state.mode_request_honoured());

        shell
            .set_mode(window, DisplayMode::Windowed)
            .expect("and back to a mode it has never been in");
        shell.pump(&mut |_| {});
        let state = shell.window_state(window).expect("live");
        assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
        let size = state.size().expect("configured");
        assert_ne!(size, bounds.size(), "it is no longer the monitor: {size}");
        let scale = shell.window(window).expect("live").scale_factor;
        assert_eq!(
            size,
            LogicalSize::new(800.0, 600.0).to_physical(scale),
            "the descriptor's windowed size is what it becomes"
        );
    }

    #[test]
    fn the_capabilities_are_exactly_what_this_backend_implements() {
        let mut shell = shell();
        let window = window(&mut shell);
        let caps = shell.caps();
        for present in [
            ShellCaps::MULTI_WINDOW,
            ShellCaps::EVENT_WAIT,
            ShellCaps::WINDOW_POSITION,
            ShellCaps::SERVER_DECORATIONS,
            ShellCaps::FRACTIONAL_SCALE,
            ShellCaps::ASPECT_HINT_HONORED,
            ShellCaps::POINTER_LOCK,
            ShellCaps::POINTER_CONFINE,
            ShellCaps::POINTER_WARP,
            ShellCaps::CLIPBOARD,
            ShellCaps::DRAG_DROP,
        ] {
            assert!(caps.contains(present), "{present:?} is implemented");
        }
        // Latched from the registration rather than assumed — the one bit on
        // this backend that is not a constant.
        assert!(
            caps.contains(ShellCaps::RAW_POINTER_MOTION),
            "RegisterRawInputDevices was refused on this runner, which is a finding"
        );
        assert!(caps.has_mouselook(), "both halves, which is the point");

        // Clear, and each for a stated reason: `TEXT_IME` is not earned by
        // `WM_CHAR` alone, and `HW_UPSCALE` is not something a plain `HWND` can
        // do. A capability that overstates itself is worse than one that is
        // missing.
        for absent in [ShellCaps::TEXT_IME, ShellCaps::HW_UPSCALE] {
            assert!(!caps.contains(absent), "{absent:?} is not implemented");
        }
        assert_eq!(caps, shell.caps(), "latched for the shell's lifetime");

        // And the methods agree with the bits, which is what makes them
        // checkable rather than decorative.
        assert!(shell.clipboard_offer(window, &[]).is_ok());
        assert!(shell.clipboard_request(window, MimeType::TextUtf8).is_ok());
        assert!(
            shell.clipboard_readable(window),
            "Win32 has no focus gate, so the trait's default is exactly right"
        );
        for mode in [
            PointerMode::Free,
            PointerMode::Confined,
            PointerMode::Locked,
            PointerMode::Free,
        ] {
            assert!(
                shell.set_pointer_mode(window, mode).is_ok(),
                "{mode:?} is claimed by the capability set"
            );
            assert_eq!(shell.window_state(window).expect("live").pointer_mode, mode);
        }
        assert!(shell.warp_pointer(window, PhysicalPoint::ORIGIN).is_ok());
        assert!(
            shell
                .set_cursor(window, Some(CursorIcon::Crosshair))
                .is_ok()
        );
        assert!(shell.set_cursor(window, None).is_ok());
    }

    #[test]
    fn a_key_press_carries_its_position_its_symbol_and_its_repeat_flag() {
        // The real window procedure, the real scan-code table, the real
        // `GetKeyboardState` and the real `MapVirtualKeyW` — driven by a
        // message CI has no keyboard to send.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);

        send_key(hwnd, msg::KEY_DOWN, VK_W, 0x0011, false, false);
        send_key(hwnd, msg::KEY_DOWN, VK_W, 0x0011, false, true);
        send_key(hwnd, msg::KEY_UP, VK_W, 0x0011, false, true);
        // An extended key, which is the half a dropped `E0` bit gets wrong.
        send_key(hwnd, msg::KEY_DOWN, VK_UP, 0x0048, true, false);

        let keys = pump_keys(&mut shell);
        assert_eq!(keys.len(), 4, "{keys:?}");
        let (scancode, key_code, keysym, state, repeat) = keys[0];
        assert_eq!(scancode, Scancode(0x0011));
        assert_eq!(key_code, Some(KeyCode::KeyW));
        assert_eq!(state, ButtonState::Pressed);
        assert!(!repeat, "the first press is not a repeat");
        assert_eq!(
            keysym.to_char(),
            Some('w'),
            "a US layout labels this key w; a different layout is a different \
             letter and still a character"
        );

        assert!(keys[1].4, "the second press is the system repeating it");
        assert_eq!(keys[2].3, ButtonState::Released);
        assert!(
            !keys[2].4,
            "bit 30 is always set on a release and must not be read there"
        );

        assert_eq!(keys[3].0, Scancode(0xE048), "the E0 prefix is folded in");
        assert_eq!(keys[3].1, Some(KeyCode::ArrowUp));
        assert_eq!(keys[3].2, Keysym(0xFF52), "XK_Up, from the named table");
    }

    #[test]
    fn typing_commits_text_and_a_control_key_commits_nothing() {
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);

        // A BMP character, then a surrogate pair, then the carriage return
        // Windows really does deliver when Enter is pressed.
        for unit in [
            u32::from(b'a'),
            0x3042, // あ
            0xD83C, // 🎮, high half
            0xDFAE, // 🎮, low half
            0x000D, // Enter
            0x0008, // Backspace
        ] {
            // SAFETY: `WM_CHAR` to this shell's own window, with `wParam` the
            // UTF-16 code unit the system would have put there.
            unsafe { ffi::SendMessageW(hwnd, msg::CHAR, unit as usize, 0) };
        }

        let mut text = String::new();
        shell.pump(&mut |event| {
            if let ShellEvent::TextCommit { text: piece, .. } = event {
                text.push_str(&piece);
            }
        });
        assert_eq!(
            text, "aあ🎮",
            "the pair is one character and the controls are not text"
        );
    }

    #[test]
    fn the_pointer_enters_moves_clicks_scrolls_and_leaves() {
        /// Where this test's **first** movement puts the pointer, and therefore
        /// the position the arrival derived from it carries.
        ///
        /// This is the payload that identifies a crossing as ours: a real
        /// movement reports where the physical cursor actually is, so no event
        /// the desktop contributes carries this point.
        const ARRIVED_AT: PhysicalPoint = PhysicalPoint::new(40.0, 30.0);
        /// Where the **second** movement puts it. No arrival may carry this:
        /// the pointer is already inside by then, and `TrackMouseEvent` is
        /// re-armed only on a transition.
        const MOVED_TO: PhysicalPoint = PhysicalPoint::new(41.0, 31.0);

        let mut shell = shell();
        let window = window(&mut shell);
        let hwnd = hwnd_of(&shell, window);

        // **A Windows desktop has a real cursor on it, and it is somewhere.**
        // Showing a window under it delivers a genuine `WM_MOUSEMOVE` that
        // derives the arrival before this test sends anything, so the first
        // synthetic movement below is then not the first movement at all. That
        // is not a runner quirk — it is true of any machine where the pointer
        // happens to be over the new window, this developer's included.
        //
        // The leave is what puts the derived state on a known edge: the backend
        // ignores one for a pointer it already thinks is outside, so this is
        // either a no-op or exactly the transition needed. Nothing here moves the
        // cursor, which would only trade one assumption about the environment for
        // another.
        //
        // # The drain comes *before* the leave, and the order is the whole fix
        //
        // It used to be the other way round, and a `windows-latest` run collected
        // `[(false, None)]` — a leave with no arrival in front of it. The
        // sequence explains itself once written out: the leave marked the pointer
        // outside, then the discarding pump dispatched a **real** `WM_MOUSEMOVE`
        // the desktop had queued, which derived the arrival and threw it away,
        // and the synthetic movements that followed found the pointer already
        // inside. The runner delivers real motion every few milliseconds, so this
        // was never a rare interleaving.
        //
        // Draining first and leaving second closes the gap completely, because
        // `send_mouse` and `send_leave` are `SendMessageW`: they call the window
        // procedure **synchronously**, and nothing between the leave and the
        // first movement pumps. No queued message can be processed in that
        // window, so the arrival below is derived from this test's own movement
        // and from nothing else. What the desktop queued meanwhile is dispatched
        // during the collecting pump — *after* everything sent here, because
        // `pump` translates what the procedure recorded in the order it recorded
        // it. What that does **not** buy is a known position in the sequence:
        // the discarding pump above runs before any of it and can leave a
        // crossing of the desktop's in front, which is what the assertions below
        // are written around.
        shell.pump(&mut |_| {});
        let frame = super::input::client_screen_rect(hwnd).expect("a live window has one");
        send_leave(hwnd);

        // A movement into a window nothing has been over is an arrival, and
        // Windows sends no message for that — it is derived.
        send_mouse(hwnd, msg::MOUSE_MOVE, 0, 40, 30);
        send_mouse(hwnd, msg::MOUSE_MOVE, 0, 41, 31);
        // A press and its release, which also takes and gives back the capture.
        send_mouse(hwnd, msg::L_BUTTON_DOWN, 0, 41, 31);
        send_mouse(hwnd, msg::L_BUTTON_UP, 0, 41, 31);
        // The thumb button, whose identity is in the *high* word.
        send_mouse(hwnd, msg::X_BUTTON_DOWN, 1 << 16, 41, 31);
        send_mouse(hwnd, msg::X_BUTTON_UP, 1 << 16, 41, 31);
        // The wheel, whose position is in **screen** coordinates.
        send_mouse(
            hwnd,
            msg::MOUSE_WHEEL,
            (120usize) << 16,
            frame.left + 41,
            frame.top + 31,
        );
        send_leave(hwnd);

        // Collected rather than compared position by position: a runner whose
        // physical cursor happens to sit over the window contributes real
        // messages of its own, and the claim under test is that ours produced
        // the events below — not that nothing else did.
        let mut names = Vec::new();
        let mut motions = Vec::new();
        let mut buttons = Vec::new();
        let mut wheel = None;
        // Every crossing, carried with the number of movements seen ahead of
        // it. That count is what pairs an arrival with the movement it was
        // derived from: the procedure pushes the derived arrival *before* the
        // motion that triggered it, so the motion sits at exactly that index.
        // Kept alongside rather than compared on the spot so the failure can
        // print both sides.
        let mut crossings: Vec<(bool, Option<PhysicalPoint>, usize)> = Vec::new();
        shell.pump(&mut |event| {
            names.push(event.name());
            match event {
                ShellEvent::PointerMotion { abs, raw_delta, .. } => {
                    motions.push(abs);
                    assert_eq!(raw_delta, None, "WM_MOUSEMOVE is not raw motion");
                }
                ShellEvent::Button {
                    button,
                    state,
                    position,
                    ..
                } => buttons.push((button, state, position)),
                ShellEvent::Wheel {
                    delta, position, ..
                } => wheel = Some((delta, position)),
                ShellEvent::PointerFocus {
                    entered, position, ..
                } => crossings.push((entered, position, motions.len())),
                _ => {}
            }
        });

        // **Our own crossings are found by their payload, never by their
        // index**, and nothing at all is claimed about the ones the desktop
        // contributes.
        //
        // # Three runs, three failures, and the fourth shape is different in kind
        //
        // This assertion has now been rewritten three times, each version
        // assuming a slightly quieter machine than the last, and `windows-latest`
        // voted against every one. The last of them required the first crossing
        // to be an arrival, and came back with:
        //
        // ```text
        // the first crossing after a leave is the arrival:
        //   [(false, None), (true, Some(PhysicalPoint { x: 40.0, y: 30.0 })), (false, None)]
        //   out of ["PointerFocus", "PointerFocus", "PointerMotion", "PointerMotion",
        //           "Button", ×4, "Wheel", "PointerFocus", "Focus"]
        // ```
        //
        // A leave in front, then our arrival at the injected point, then our
        // leave. The discarding pump above had dispatched a real `WM_MOUSEMOVE`
        // whose derived arrival it threw away, so `send_leave` was a genuine
        // transition rather than the no-op it is on a still desktop — and a
        // crossing nothing in this test's own sequence accounts for was sitting
        // ahead of everything.
        //
        // So the shape changed rather than tightening again: **a live desktop is
        // entitled to insert crossings before, between and after ours**, and any
        // claim about position-in-the-sequence is a claim that it held still.
        // What this test injected is identifiable — the arrival it derives
        // carries [`ARRIVED_AT`], which is a point no real movement reports —
        // so the property is stated about that pair and about nothing else:
        //
        // * an arrival carries the position this test moved to,
        // * the movement it was derived from is the very next motion in the
        //   stream, since the procedure pushes the arrival ahead of it,
        // * the crossing straight after it is a leave carrying no position —
        //   this test's own, and nothing can be dispatched between the two
        //   because every `send_*` here is a synchronous `SendMessageW`, and
        // * **no arrival carries [`MOVED_TO`]**, which is the one-shot claim:
        //   `TrackMouseEvent` is re-armed only on a transition, so a backend
        //   deriving an arrival per movement would announce the second movement
        //   as an arrival too.
        let ours = crossings
            .iter()
            .position(|(entered, at, _)| *entered && *at == Some(ARRIVED_AT))
            .unwrap_or_else(|| {
                panic!(
                    "no arrival carried {ARRIVED_AT:?}, which is where this test's first \
                     movement put the pointer: {crossings:?} out of {names:?}"
                )
            });
        let derived_from = crossings[ours].2;
        assert_eq!(
            motions.get(derived_from).copied(),
            Some(Some(ARRIVED_AT)),
            "an arrival carries the position of the movement it was derived from, and that \
             movement is still delivered: motion {derived_from} of {motions:?}. Events {names:?}"
        );
        assert_eq!(
            crossings
                .get(ours + 1)
                .map(|(entered, at, _)| (*entered, *at)),
            Some((false, None)),
            "the leave this test sent follows its arrival and carries no position; every \
             message between the two is a synchronous SendMessageW, so nothing else can be \
             dispatched in there: {crossings:?} out of {names:?}"
        );
        assert!(
            !crossings
                .iter()
                .any(|(entered, at, _)| *entered && *at == Some(MOVED_TO)),
            "the pointer was already inside by the second movement, so {MOVED_TO:?} is a \
             movement and not an arrival; announcing it would mean TrackMouseEvent is re-armed \
             per message rather than per transition: {crossings:?} out of {names:?}"
        );
        assert!(motions.contains(&Some(MOVED_TO)), "{motions:?}");
        assert_eq!(
            buttons,
            vec![
                (PointerButton::Left, ButtonState::Pressed, Some(MOVED_TO)),
                (PointerButton::Left, ButtonState::Released, Some(MOVED_TO)),
                (PointerButton::Back, ButtonState::Pressed, Some(MOVED_TO)),
                (PointerButton::Back, ButtonState::Released, Some(MOVED_TO)),
            ],
            "{names:?}"
        );
        let (delta, position) = wheel.expect("the wheel scrolled: {names:?}");
        assert_eq!(delta, ScrollDelta::Lines { x: 0.0, y: 1.0 });
        assert_eq!(
            position,
            Some(MOVED_TO),
            "the screen coordinates a wheel message carries were converted"
        );
    }

    #[test]
    fn confining_the_pointer_clips_it_and_losing_focus_gives_the_desktop_back() {
        // The hazard that a compile cannot catch: a process that keeps the
        // cursor clipped after it stops being the foreground window has taken
        // the desktop hostage. The clip is read back from the *system*, because
        // that is the thing being claimed.
        //
        // What the clip is compared against is **not** the client rectangle,
        // and the difference is the runner's answer rather than a loosening.
        // `ClipCursor` intersects with the virtual screen before it applies: the
        // Windows runner's display is 1024×768, which is smaller than the
        // default 1280×720 window once the frame is added, so the clip came back
        // 12 px narrower on the right and the right edge was exactly the screen
        // width. Asserting the client rectangle asserts something the API does
        // not promise; asserting the intersection asserts what it does, and it
        // is the same clamp a game meets on a 1366×768 laptop.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);
        let client = super::input::client_screen_rect(hwnd).expect("a live window has one");
        let clipped = client.intersect(virtual_screen());
        assert_ne!(clip_rect(), clipped, "nothing is clipped to start with");

        // The clip follows the focus, so the window has to have it. CI cannot
        // click on a window; the message is what a click would have produced,
        // and the foreground call is what makes `ClipCursor` allowed at all.
        make_foreground(hwnd);
        send_focus(hwnd, true);
        shell.pump(&mut |_| {});
        assert!(shell.window_state(window).expect("live").focused);

        shell
            .set_pointer_mode(window, PointerMode::Confined)
            .expect("POINTER_CONFINE is claimed");
        assert_eq!(clip_rect(), clipped, "the pointer is bounded by the window");

        // **Released synchronously**, inside the window procedure, without a
        // pump — one frame of a hostage desktop is one frame too many.
        send_focus(hwnd, false);
        assert_ne!(clip_rect(), clipped, "focus loss releases the clip");

        // And it comes back with the focus, because the request has not been
        // withdrawn.
        send_focus(hwnd, true);
        assert_eq!(clip_rect(), clipped);
        shell.pump(&mut |_| {});

        // The other order: a mode asked for while the window does **not** have
        // the keyboard gets no clip — the hostage rule does not care why the
        // focus is missing — and takes effect when the keyboard arrives, rather
        // than being lost.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("start from nothing");
        send_focus(hwnd, false);
        shell.pump(&mut |_| {});
        shell
            .set_pointer_mode(window, PointerMode::Confined)
            .expect("asking is allowed whether or not it takes effect now");
        assert_ne!(
            clip_rect(),
            clipped,
            "an unfocused window does not get the pointer"
        );
        send_focus(hwnd, true);
        assert_eq!(
            clip_rect(),
            clipped,
            "and gets it when the keyboard arrives"
        );
        shell.pump(&mut |_| {});

        // Going back to free withdraws it for good.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("free always works");
        assert_ne!(clip_rect(), clipped);
        send_focus(hwnd, true);
        assert_ne!(clip_rect(), clipped, "and focus does not resurrect it");
    }

    #[test]
    fn minimizing_a_captured_window_releases_the_clip() {
        // A minimized window is 0×0, and `client_screen_rect` maps both corners
        // of that through `ClientToScreen` to the *same* point — so a clip
        // re-applied from it pins the cursor there for the whole minimized
        // period. The window is minimized for real rather than handed a
        // synthetic `WM_SIZE(SIZE_MINIMIZED)`: the degenerate 0×0 client
        // rectangle only exists for an actually-iconic window.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);

        make_foreground(hwnd);
        send_focus(hwnd, true);
        shell.pump(&mut |_| {});
        let client = super::input::client_screen_rect(hwnd).expect("a live window has one");
        let clipped = client.intersect(virtual_screen());

        shell
            .set_pointer_mode(window, PointerMode::Confined)
            .expect("POINTER_CONFINE is claimed");
        assert_eq!(clip_rect(), clipped, "the pointer is bounded by the window");

        // SAFETY: this shell's own window; SW_MINIMIZE is a documented command.
        unsafe { ffi::ShowWindow(hwnd, value::SW_MINIMIZE) };
        shell.pump(&mut |_| {});
        let released = clip_rect();
        assert!(
            released.right > released.left && released.bottom > released.top,
            "a minimized captured window must not pin the cursor to a point: {released:?}"
        );
        assert_ne!(
            released, clipped,
            "the clip is released, not re-applied from the 0×0 client area"
        );

        // SAFETY: this shell's own window; SW_RESTORE is a documented command.
        unsafe { ffi::ShowWindow(hwnd, value::SW_RESTORE) };
        shell.pump(&mut |_| {});
        assert_eq!(
            clip_rect(),
            clipped,
            "restore re-clips from the real client rectangle"
        );

        // Going back to free withdraws it for good.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("free always works");
        assert_ne!(clip_rect(), clipped);
    }

    #[test]
    fn a_second_window_takes_the_pointer_capture_from_the_first() {
        // One cursor, so one clip. A shell with two windows must not leave the
        // first one reporting `Locked` while the second holds the clip.
        let mut shell = shell();
        let first = window(&mut shell);
        let second = shell
            .create_window(&WindowDesc {
                title: "crcbl P5C W2 — second",
                size: LogicalSize::new(640.0, 480.0),
                ..WindowDesc::default()
            })
            .expect("a second window");
        shell.pump(&mut |_| {});

        shell
            .set_pointer_mode(first, PointerMode::Locked)
            .expect("lock");
        shell
            .set_pointer_mode(second, PointerMode::Confined)
            .expect("confine");
        assert_eq!(
            shell.window_state(first).expect("live").pointer_mode,
            PointerMode::Free,
            "the first window no longer holds anything"
        );
        assert_eq!(
            shell.window_state(second).expect("live").pointer_mode,
            PointerMode::Confined
        );
    }

    #[test]
    fn hiding_the_cursor_is_balanced_however_many_times_it_is_asked_for() {
        // The `ShowCursor` reference-count bug, observed through the count
        // itself: two hides and one show leave the cursor invisible for the
        // rest of the process's life, with no error anywhere.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);
        let baseline = cursor_display_count();

        make_foreground(hwnd);
        send_focus(hwnd, true);
        shell.pump(&mut |_| {});

        shell.set_cursor(window, None).expect("hide");
        assert_eq!(cursor_display_count(), baseline - 1, "one hide is owed");
        shell.set_cursor(window, None).expect("hide again");
        assert_eq!(
            cursor_display_count(),
            baseline - 1,
            "asking twice must not owe two shows"
        );

        shell
            .set_cursor(window, Some(CursorIcon::Text))
            .expect("a shape reveals it again");
        assert_eq!(cursor_display_count(), baseline);
        shell
            .set_cursor(window, Some(CursorIcon::Crosshair))
            .expect("and another shape changes nothing about visibility");
        assert_eq!(cursor_display_count(), baseline);

        // Losing focus gives the cursor back, which is what stops a game in
        // mouselook leaving the desktop without a pointer.
        shell.set_cursor(window, None).expect("hide");
        assert_eq!(cursor_display_count(), baseline - 1);
        send_focus(hwnd, false);
        shell.pump(&mut |_| {});
        assert_eq!(cursor_display_count(), baseline, "focus loss reveals it");
    }

    #[test]
    fn a_cursor_shape_is_answered_from_the_window_procedure() {
        // `WM_SETCURSOR` is the only message allowed to set a shape, and the
        // system uses `DefWindowProc`'s answer — the class arrow — unless ours
        // says it handled it. Sending the message runs exactly the path a
        // mouse movement would.
        let mut shell = shell();
        let window = window(&mut shell);
        shell
            .set_cursor(window, Some(CursorIcon::Text))
            .expect("an I-beam is a stock Windows cursor");
        let hwnd = hwnd_of(&shell, window);

        // SAFETY: `WM_SETCURSOR` to this shell's own window. `wParam` is the
        // window under the cursor and `lParam` packs the hit-test code in its
        // low word and the triggering message in its high one.
        let handled = unsafe {
            ffi::SendMessageW(
                hwnd,
                msg::SET_CURSOR,
                hwnd as usize,
                value::HT_CLIENT as isize,
            )
        };
        assert_eq!(handled, 1, "the shape was applied, not the class cursor");

        // The frame is the system's: answering for it would replace the resize
        // arrows on the window border with an I-beam.
        const HT_BOTTOM_RIGHT: isize = 17;
        // SAFETY: as above, with the hit-test code for a corner of the frame.
        let frame =
            unsafe { ffi::SendMessageW(hwnd, msg::SET_CURSOR, hwnd as usize, HT_BOTTOM_RIGHT) };
        assert_ne!(frame, 1, "the non-client area is left to DefWindowProc");
    }

    #[test]
    fn warping_the_pointer_moves_it_to_a_position_in_the_window() {
        // What `POINTER_WARP` claims, and the conversion it depends on: the
        // seam is in window pixels and `SetCursorPos` is in screen ones.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);
        let client = super::input::client_screen_rect(hwnd).expect("a live window has one");
        make_foreground(hwnd);

        shell
            .warp_pointer(window, PhysicalPoint::new(30.0, 20.0))
            .expect("POINTER_WARP is claimed");
        let mut point = super::super::ffi::Point::default();
        // SAFETY: `point` is a live, initialised `POINT` the call writes into.
        let read = unsafe { ffi::GetCursorPos(&raw mut point) };
        assert_ne!(read, 0, "a session with a desktop has a cursor position");
        assert_eq!(
            (point.x, point.y),
            (client.left + 30, client.top + 20),
            "the warp landed in screen space at the client offset asked for"
        );
    }

    #[test]
    fn a_decorated_window_really_is_decorated() {
        // What `SERVER_DECORATIONS` claims, observed rather than asserted from
        // the style word: a windowed window's outer rectangle is larger than
        // its client area, and a borderless one's is not.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let client = shell
            .window_state(window)
            .expect("live")
            .size()
            .expect("configured");
        let frame = Win32Shell::frame_for(
            super::super::geometry::styles(DisplayMode::Windowed, true),
            shell.window(window).expect("live").dpi,
        );
        assert!(frame.height > 0, "a caption has a height: {frame:?}");
        assert!(frame.width > 0, "a border has a width: {frame:?}");
        assert!(!client.is_empty());

        let flush = Win32Shell::frame_for(
            super::super::geometry::styles(DisplayMode::Borderless { monitor: None }, true),
            shell.window(window).expect("live").dpi,
        );
        assert_eq!(
            flush,
            super::super::geometry::Frame::default(),
            "WS_POPUP has no frame at all"
        );
    }

    #[test]
    fn the_size_constraints_reach_the_window_procedure() {
        // `WM_GETMINMAXINFO` and `WM_SIZING` are otherwise only sent by a user
        // dragging a window edge. Sending them by hand runs the real procedure
        // against the real cached limits, which is the only way to test the
        // aspect lock and the track sizes without a mouse.
        let mut shell = shell();
        let window = window(&mut shell);
        shell
            .set_constraints(
                window,
                SizeConstraints {
                    min: Some(LogicalSize::new(640.0, 480.0)),
                    max: Some(LogicalSize::new(1920.0, 1080.0)),
                    aspect: Some(AspectRatio::WIDESCREEN),
                },
            )
            .expect("constraints are a request every backend accepts");
        let hwnd = hwnd_of(&shell, window);
        let scale = shell.window(window).expect("live").scale_factor;

        let mut info = MinMaxInfo::default();
        // SAFETY: sending a message to this shell's own window on the thread
        // that owns it; `info` is a live, initialised `MINMAXINFO` and
        // `WM_GETMINMAXINFO` is documented to take a pointer to one.
        unsafe {
            ffi::SendMessageW(
                hwnd,
                msg::GET_MIN_MAX_INFO,
                0,
                (&raw mut info) as super::super::ffi::Lparam,
            );
        }
        let min_client_width = (640.0 * scale).round() as i32;
        assert!(
            info.pt_min_track_size.x >= min_client_width,
            "the minimum is the client minimum plus the frame: {info:?}"
        );
        assert!(
            info.pt_max_track_size.x >= (1920.0 * scale).round() as i32,
            "{info:?}"
        );

        let mut rect = Rect {
            left: 100,
            top: 100,
            right: 1100,
            bottom: 1100,
        };
        // SAFETY: as above; `WM_SIZING` is documented to take a pointer to the
        // `RECT` the system is about to apply, and rewriting it is how a resize
        // is constrained.
        let handled = unsafe {
            ffi::SendMessageW(
                hwnd,
                msg::SIZING,
                super::super::ffi::value::WMSZ_RIGHT,
                (&raw mut rect) as super::super::ffi::Lparam,
            )
        };
        assert_eq!(handled, 1, "the rectangle was rewritten");
        assert_eq!(rect.width(), 1000, "the dragged edge never moves");
        assert!(
            rect.height() < 700,
            "16:9 makes a 1000-wide window a little over 560 tall: {rect:?}"
        );
        assert!(rect.height() > 450, "{rect:?}");
    }

    #[test]
    fn a_close_request_is_intercepted_and_can_be_refused_then_accepted() {
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        assert!(matches!(
            shell.reply_close_request(window, CloseReply::Keep),
            Err(ShellError::NoPendingCloseRequest { .. })
        ));

        let hwnd = hwnd_of(&shell, window);
        // SAFETY: `WM_CLOSE` to this shell's own window — exactly what the
        // title bar's close button sends.
        unsafe { ffi::SendMessageW(hwnd, msg::CLOSE, 0, 0) };
        let mut asked = false;
        shell.pump(&mut |event| {
            asked |= matches!(event, ShellEvent::CloseRequested { window: asked } if asked == window);
        });
        assert!(asked, "WM_CLOSE is a question, not a notification");
        assert!(
            shell
                .window_state(window)
                .expect("still open")
                .close_pending
        );

        shell
            .reply_close_request(window, CloseReply::Keep)
            .expect("refusing is allowed");
        assert!(
            shell.window_state(window).is_ok(),
            "the window survives a refusal — DefWindowProc would have destroyed it"
        );
        assert!(!shell.window_state(window).expect("open").close_pending);

        // SAFETY: as above.
        unsafe { ffi::SendMessageW(hwnd, msg::CLOSE, 0, 0) };
        shell.pump(&mut |_| {});
        shell
            .reply_close_request(window, CloseReply::Close)
            .expect("and accepting closes it");
        assert!(matches!(
            shell.window_state(window),
            Err(ShellError::InvalidWindow { .. })
        ));
    }

    #[test]
    fn both_offered_formats_round_trip_and_the_reader_picks() {
        // What `CLIPBOARD` claims, against the real window station: text for
        // everything else on the desktop, a registered format for the engine's
        // own, and both on the clipboard at once so the reader chooses. The RON
        // has to come back **byte for byte** — a blob carrying the heap's
        // padding would not parse.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});

        const RON: &str = "(kind:\"node\",id:7,name:\"日本語 🎮\")";
        shell
            .clipboard_offer(
                window,
                &[ClipboardOffer::text("hello 🎮"), ClipboardOffer::ron(RON)],
            )
            .expect("claiming the clipboard needs no user interaction on Win32");
        assert!(
            !clipboard_is_open(),
            "the guard closed the clipboard on the way out of the offer"
        );

        assert_eq!(
            paste(&mut shell, window, MimeType::TextUtf8).text(),
            Some("hello 🎮"),
            "an astral codepoint survives the UTF-16 round trip"
        );
        assert_eq!(
            paste(&mut shell, window, MimeType::CrcblRon),
            ClipboardContent::Bytes(RON.as_bytes().to_vec()),
            "the engine's own format is lossless, padding and all"
        );
        assert!(
            !clipboard_is_open(),
            "and on the way out of every read as well"
        );

        // A format that was not offered is `Empty` — "holds nothing in that
        // format", which the seam merges with an empty clipboard on purpose —
        // and emphatically not `Unavailable`, which would mean the read failed.
        assert_eq!(
            paste(&mut shell, window, MimeType::UriList),
            ClipboardContent::Empty
        );
    }

    #[test]
    fn an_empty_offer_empties_the_clipboard_and_an_empty_payload_does_not() {
        // The two cases `ClipboardContent` exists to keep apart, and the one
        // place they are easy to conflate: releasing the clipboard and
        // publishing nothing.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});

        // A successful transfer of zero bytes. `Bytes(vec![])`, not `Empty` —
        // reporting `Empty` here tells an editor its paste failed when it did
        // not.
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("")])
            .expect("an empty payload is a payload");
        assert_eq!(
            paste(&mut shell, window, MimeType::TextUtf8),
            ClipboardContent::Bytes(Vec::new())
        );

        // An empty *slice* is the release. Windows has no owner to give up, so
        // this is `EmptyClipboard` and afterwards there is nothing there.
        shell
            .clipboard_offer(window, &[])
            .expect("an empty slice releases");
        assert_eq!(
            paste(&mut shell, window, MimeType::TextUtf8),
            ClipboardContent::Empty
        );
        assert!(!clipboard_is_open());
    }

    #[test]
    fn a_read_is_answered_exactly_once_and_needs_no_second_pump() {
        // Obligation 4 in the form this backend takes: the read happens inside
        // `clipboard_request`, so the answer is queued before it returns and
        // there is no list of outstanding reads to lose one from. `paste`
        // asserts the "exactly one, on the first pump" half; this asserts that
        // nothing arrives afterwards, which is the half a duplicated answer
        // would break.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("once")])
            .expect("publish");

        let first = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("accepted");
        let second = shell
            .clipboard_request(window, MimeType::CrcblRon)
            .expect("accepted");
        assert_ne!(first, second, "ids are unique within a shell");

        let mut answered = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::ClipboardData {
                request, content, ..
            } = event
            {
                answered.push((request, content));
            }
        });
        assert_eq!(answered.len(), 2, "one each: {answered:?}");
        assert_eq!(answered[0].0, first, "in the order they were asked");
        assert_eq!(answered[1].0, second);
        assert_eq!(answered[0].1.text(), Some("once"));
        assert_eq!(
            answered[1].1,
            ClipboardContent::Empty,
            "the RON format was not published"
        );

        let mut again = Vec::new();
        shell.pump(&mut |event| {
            if matches!(event, ShellEvent::ClipboardData { .. }) {
                again.push(event.name());
            }
        });
        assert!(
            again.is_empty(),
            "answered once, not once per pump: {again:?}"
        );
    }

    #[test]
    fn a_window_destroyed_before_the_pump_is_not_answered_at_all() {
        // Where obligation 4 meets obligation 1, and the stronger of the two
        // wins: an answer naming a handle the consumer has already been told is
        // stale is worse than a request that ends without one, because the
        // consumer can see the stale handle and cannot see the missing answer.
        // The X11 backend decides this the same way.
        let mut shell = shell();
        let window = window(&mut shell);
        let survivor = shell
            .create_window(&WindowDesc {
                title: "crcbl P5C W3 — survivor",
                size: LogicalSize::new(640.0, 480.0),
                ..WindowDesc::default()
            })
            .expect("a second window");
        shell.pump(&mut |_| {});
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("gone")])
            .expect("publish");

        let doomed = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("accepted while the window was live");
        let kept = shell
            .clipboard_request(survivor, MimeType::TextUtf8)
            .expect("accepted");
        shell.destroy_window(window).expect("destroy");

        let mut answered = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::ClipboardData { request, .. } = event {
                answered.push(request);
            }
        });
        assert_eq!(
            answered,
            [kept],
            "the dead window's answer went with it and the live one's did not"
        );
        assert_ne!(doomed, kept);
    }

    #[test]
    fn a_drop_becomes_one_event_per_file_at_the_position_it_landed() {
        // What `DRAG_DROP` claims, driven through the real `WM_DROPFILES` path:
        // shell32's `DragQueryFileW` reads a real `HDROP`, the procedure
        // finishes it, and the paths reach the pump through the side queue that
        // keeps them out of a `Copy` event.
        let mut shell = shell();
        let window = drop_window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);

        send_drop(
            hwnd,
            &[r"C:\assets\My Scene.ron", r"C:\assets\プロジェクト.png"],
            12,
            34,
        );
        let dropped = pump_drops(&mut shell);
        assert_eq!(dropped.len(), 2, "one event per file: {dropped:?}");
        assert_eq!(dropped[0].0, window);
        assert_eq!(dropped[0].1, PathBuf::from(r"C:\assets\My Scene.ron"));
        assert_eq!(
            dropped[1].1,
            PathBuf::from(r"C:\assets\プロジェクト.png"),
            "a non-ASCII name survives UTF-16 without a lossy step"
        );
        assert_eq!(
            dropped[0].2,
            Some(PhysicalPoint::new(12.0, 34.0)),
            "the drop point is in client pixels, which is what the seam means"
        );

        // Two drops in one pump stay two drops, and each marker claims its own
        // payload — the pairing that hands one scene the other's assets when it
        // is wrong.
        send_drop(hwnd, &[r"C:\a.png"], 1, 2);
        send_drop(hwnd, &[r"C:\b.png", r"C:\c.png"], 3, 4);
        let batch = pump_drops(&mut shell);
        let paths: Vec<PathBuf> = batch.iter().map(|(_, path, _)| path.clone()).collect();
        assert_eq!(
            paths,
            [
                PathBuf::from(r"C:\a.png"),
                PathBuf::from(r"C:\b.png"),
                PathBuf::from(r"C:\c.png")
            ],
            "in order: {batch:?}"
        );
        assert_eq!(batch[0].2, Some(PhysicalPoint::new(1.0, 2.0)));
        assert_eq!(
            batch[1].2,
            Some(PhysicalPoint::new(3.0, 4.0)),
            "the second drop's point, not the first's"
        );
    }

    #[test]
    fn a_window_that_did_not_ask_for_drops_reports_none() {
        // `accept_drops` is off by default and load-bearing rather than
        // advisory. The system enforces it — no `WS_EX_ACCEPTFILES`, no
        // message — so the message here is one the system would never send, and
        // what is under test is the backend's own half of the gate.
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);

        send_drop(hwnd, &[r"C:\unwanted.png"], 5, 5);
        assert!(
            pump_drops(&mut shell).is_empty(),
            "the descriptor said no, and that is the answer"
        );
    }

    #[test]
    fn the_drop_registration_survives_a_trip_through_borderless() {
        // `DragAcceptFiles` sets `WS_EX_ACCEPTFILES`, and a mode change
        // *rewrites the extended style word*. Carrying the bit is what stops
        // drops silently ceasing to arrive the first time a window goes
        // fullscreen — with nothing anywhere reporting it.
        let mut shell = shell();
        let window = drop_window(&mut shell);
        shell.pump(&mut |_| {});
        let hwnd = hwnd_of(&shell, window);
        let accepts = || {
            // SAFETY: reading this shell's own window's extended style.
            let ex_style = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_EX_STYLE) } as u32;
            ex_style & super::super::ffi::style::EX_ACCEPT_FILES
        };
        assert_ne!(accepts(), 0, "created with drops accepted");

        let primary = shell
            .monitors()
            .iter()
            .find(|monitor| monitor.is_primary)
            .expect("a primary monitor")
            .id;
        shell
            .set_mode(
                window,
                DisplayMode::Borderless {
                    monitor: Some(primary),
                },
            )
            .expect("borderless");
        assert_ne!(accepts(), 0, "the style rewrite carried the bit");
        shell
            .set_mode(window, DisplayMode::Windowed)
            .expect("and back");
        assert_ne!(accepts(), 0);
        shell.pump(&mut |_| {});

        // And it still works, which is the thing the bit is a proxy for.
        send_drop(hwnd, &[r"C:\after.ron"], 7, 8);
        let dropped = pump_drops(&mut shell);
        assert_eq!(dropped.len(), 1, "{dropped:?}");
        assert_eq!(dropped[0].1, PathBuf::from(r"C:\after.ron"));
    }

    #[test]
    fn opening_the_clipboard_reports_how_it_went_and_always_closes_it() {
        // The mechanism, not a wall clock: an open that took it first time and
        // one that retried seven times take indistinguishable amounts of time on
        // a loaded runner, and only `Opened` can say which happened. What is
        // asserted is that it was not *refused* and that the guard's `Drop` ran
        // — the second being what stops this process locking every other
        // application out of the clipboard.
        assert!(!clipboard_is_open(), "nothing is open before we start");
        let mut shell = shell();
        let window = window(&mut shell);
        let hwnd = hwnd_of(&shell, window);

        let opened = {
            let board = super::super::clipboard::Clipboard::open(hwnd)
                .expect("an idle desktop lets a process open the clipboard");
            assert!(clipboard_is_open(), "held for the guard's lifetime");
            board.opened()
        };
        assert!(
            !clipboard_is_open(),
            "and given back the moment the guard leaves scope"
        );
        assert!(
            !matches!(opened, Opened::Refused { .. }),
            "another process held the clipboard for the whole budget: {opened:?}"
        );
        if let Opened::After { attempts } = opened {
            assert!(
                attempts <= super::super::clipboard::OPEN_ATTEMPTS,
                "the retry is bounded: {attempts}"
            );
        }
    }

    #[test]
    fn every_method_taking_a_window_rejects_a_stale_handle() {
        // Implementor obligation 1, method by method: a destroyed handle must
        // never act on whatever window took the slot.
        let mut shell = shell();
        let stale = window(&mut shell);
        shell.destroy_window(stale).expect("destroy");

        let invalid = |result: Result<(), ShellError>| {
            assert!(
                matches!(result, Err(ShellError::InvalidWindow { .. })),
                "{result:?}"
            );
        };
        invalid(shell.window_state(stale).map(|_| ()));
        invalid(shell.set_title(stale, "gone"));
        invalid(shell.set_visible(stale, true));
        invalid(shell.set_mode(stale, DisplayMode::Windowed));
        invalid(shell.set_constraints(stale, SizeConstraints::NONE));
        invalid(shell.surface_target(stale).map(|_| ()));
        invalid(shell.set_cursor(stale, None));
        invalid(shell.set_pointer_mode(stale, PointerMode::Free));
        invalid(shell.warp_pointer(stale, PhysicalPoint::ORIGIN));
        invalid(shell.reply_close_request(stale, CloseReply::Keep));
        invalid(shell.clipboard_offer(stale, &[]));
        invalid(
            shell
                .clipboard_request(stale, MimeType::TextUtf8)
                .map(|_| ()),
        );
        invalid(shell.destroy_window(stale));
        assert!(!shell.clipboard_readable(stale));
    }

    #[test]
    fn two_windows_coexist_and_are_told_apart() {
        // What `MULTI_WINDOW` claims.
        let mut shell = shell();
        let first = window(&mut shell);
        let second = shell
            .create_window(&WindowDesc {
                title: "crcbl P5C W1 — second",
                size: LogicalSize::new(640.0, 480.0),
                ..WindowDesc::default()
            })
            .expect("a second window");
        assert_ne!(first, second);
        assert_ne!(hwnd_of(&shell, first), hwnd_of(&shell, second));

        let mut resized = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::Resized { window, size, .. } = event {
                resized.push((window, size));
            }
        });
        assert_eq!(resized.len(), 2, "one configuration each: {resized:?}");
        let sizes: Vec<PhysicalSize> = resized.iter().map(|(_, size)| *size).collect();
        assert_ne!(sizes[0], sizes[1], "and they are not the same window twice");

        // Destroying one leaves the other alone.
        shell.destroy_window(first).expect("destroy");
        assert!(shell.window_state(second).is_ok());
        assert!(shell.window_state(first).is_err());
    }

    #[test]
    fn wait_events_genuinely_blocks() {
        // What `EVENT_WAIT` claims, asserted as the *mechanism* it is rather
        // than as a duration. The first version of this measured three 50 ms
        // waits against a wall clock and failed on the runner at 47 ms with
        // nothing to say why — a wait that found a queued message, a wait that
        // failed outright, and a wait that never slept are all instant, and a
        // clock cannot tell them apart. `Wake` can.
        //
        // A window that has just been created and shown has messages waiting,
        // so the queue is drained to quiescence first: `Wake::Message` before
        // that point is correct behaviour and asserting against it would be
        // asserting that the drain does not work.
        //
        // Whether it reached quiescence is *reported* rather than asserted, and
        // that is this test's second correction — see below. On a runner where
        // the desktop delivers messages unbidden every few milliseconds, "the
        // queue went quiet" is a fact about the machine's mood.
        let mut shell = shell();
        let _window = window(&mut shell);
        let mut settled = false;
        for _ in 0..16 {
            shell.pump(&mut |_| {});
            if shell.wait(Some(Duration::from_millis(0))) == Wake::TimedOut {
                settled = true;
                break;
            }
        }

        // # The observable is "it slept", not "it reached the timeout"
        //
        // **This assertion has been wrong twice, in the same direction, and the
        // evidence for the current shape is worth keeping so nobody tightens it
        // back.** It first measured a wall clock and failed at 47 ms of a 50 ms
        // timeout. It then asked for `Wake::TimedOut` on at least one of five
        // attempts — and a `windows-latest` run printed all five:
        //
        // ```text
        // attempt 0: Message after  5.7433ms, queue 0x80008, message id 799
        // attempt 1: Message after 16.0896ms, queue 0x400040, message None
        // attempt 2: Message after 31.6668ms, queue 0x400040, message None
        // attempt 3: Message after   299.7µs, queue 0x80008, message id 96
        // attempt 4: Message after 10.9571ms, queue 0x20002, message id 512
        // ```
        //
        // 799 is `WM_DWMNCRENDERINGCHANGED` and 512 is `WM_MOUSEMOVE`: the
        // desktop is delivering compositor notifications and **real mouse
        // movement** to this window every few milliseconds. An idle window with a
        // drained queue does not exist there, and no amount of draining will make
        // one.
        //
        // Read those numbers again, though, because they also say the wait
        // *works*: four of the five blocked for between 5.7 ms and 31.7 ms before
        // something woke them. That is what blocking looks like on a busy
        // desktop. Only attempt 3, at 299 µs, returned to an already-full queue.
        //
        // So the assertion is now the thing that actually separates a blocking
        // wait from a no-op one. [`ShellCaps::EVENT_WAIT`] claims the call
        // *sleeps until something happens*; it never claimed the machine would go
        // quiet. A wait that does nothing returns in microseconds **every single
        // time** — five of those in a row is unmistakable and is what fails here.
        // A wait woken after milliseconds slept, whatever woke it.
        //
        // Every attempt carries its evidence, because two of the CI rounds spent
        // on this were settled only by the failure printing more than the last
        // one did. The queue word says what kind of message woke it; the `MSG`
        // beside it says which one — an id, an `hwnd` and two parameters, which
        // is a thing that can be looked up rather than guessed at. `None` there,
        // with a bit set, is the observation that identified `QS_SENDMESSAGE`;
        // see [`Win32Shell::wait`].
        const ATTEMPTS: usize = 5;
        const TIMEOUT: Duration = Duration::from_millis(50);
        // The full timeout, loose on the low side because Windows' timer
        // granularity is 15.6 ms. Only a `TimedOut` is held to it: a
        // `WAIT_TIMEOUT` that arrived instantly would mean the timeout was not
        // the one that was asked for, so it is not evidence of sleeping.
        const FLOOR: Duration = Duration::from_millis(40);
        // The line between "it slept and was woken" and "it never slept", drawn
        // between the two populations the run above measured: 299 µs for a return
        // to a full queue, 5.7 ms for the shortest genuine block. An order of
        // magnitude of clearance on each side, and no dependence on the desktop
        // being quiet.
        const BLOCKED: Duration = Duration::from_millis(2);

        let mut slept = false;
        let mut woken = Vec::new();
        for attempt in 0..ATTEMPTS {
            let start = Instant::now();
            let woke = shell.wait(Some(TIMEOUT));
            let waited = start.elapsed();
            woken.push(format!(
                "attempt {attempt}: {woke:?} after {waited:?}, queue {:#06x}, message {:?}",
                Win32Shell::queue_status(),
                Win32Shell::peek_pending()
            ));
            slept = match woke {
                Wake::TimedOut => waited >= FLOOR,
                // Woken early, which on a live desktop is the ordinary case —
                // but a wait has to have slept to be woken out of.
                _ => waited >= BLOCKED,
            };
            if slept {
                break;
            }
            // Whatever woke it is still in the queue; leaving it there would
            // wake the next attempt too.
            shell.pump(&mut |_| {});
        }
        assert!(
            slept,
            "every one of {ATTEMPTS} waits came back inside {BLOCKED:?} without reaching its \
             {TIMEOUT:?} timeout, so this backend's EVENT_WAIT never sleeps at all (the queue \
             {}): {woken:#?}",
            if settled {
                "was drained to quiescence first"
            } else {
                "never went quiet in 16 drain-and-check rounds, so a return to a full queue is \
                 expected — but not five in a row, each in microseconds"
            }
        );
    }

    #[test]
    fn the_monitors_are_enumerated_with_one_primary_and_a_scale_factor() {
        let shell = shell();
        let monitors = shell.monitors();
        assert!(
            !monitors.is_empty(),
            "a session with a window station has at least one display"
        );
        assert_eq!(
            monitors.iter().filter(|monitor| monitor.is_primary).count(),
            1,
            "exactly one primary: {monitors:?}"
        );
        for monitor in monitors {
            assert!(!monitor.name.is_empty(), "the device name: {monitor:?}");
            assert!(!monitor.size().is_empty(), "{monitor:?}");
            assert!(monitor.scale_factor >= 0.5, "{monitor:?}");
            assert!(
                monitor.work_area.width <= monitor.bounds.width
                    && monitor.work_area.height <= monitor.bounds.height,
                "the work area is the bounds minus the taskbar: {monitor:?}"
            );
            assert_eq!(
                shell.monitor(monitor.id).map(|found| &found.name),
                Some(&monitor.name),
                "every id resolves"
            );
        }
        let ids: Vec<MonitorId> = monitors.iter().map(|monitor| monitor.id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "ids are not reused: {ids:?}");
        assert_eq!(shell.monitor(MonitorId(u32::MAX)), None);
    }

    #[test]
    fn hiding_a_window_is_real_and_reversible() {
        let mut shell = shell();
        let window = window(&mut shell);
        shell.pump(&mut |_| {});
        assert!(shell.window_state(window).expect("live").visible);

        shell.set_visible(window, false).expect("hide");
        assert!(
            !shell.window_state(window).expect("live").visible,
            "visibility is read back from the system, not assumed from the request"
        );
        shell.set_visible(window, true).expect("show");
        assert!(shell.window_state(window).expect("live").visible);

        // A window created hidden is hidden, which is the fix for the
        // black-window flash on startup.
        let hidden = shell
            .create_window(&WindowDesc {
                visible: false,
                ..WindowDesc::default()
            })
            .expect("create hidden");
        assert!(!shell.window_state(hidden).expect("live").visible);
    }

    #[test]
    fn the_title_round_trips_and_a_nul_is_refused() {
        let mut shell = shell();
        let window = window(&mut shell);
        shell
            .set_title(window, "crcbl — 日本語 🎮")
            .expect("UTF-16");
        assert!(matches!(
            shell.set_title(window, "save\0nothing"),
            Err(ShellError::InvalidDescriptor(_))
        ));
        assert!(matches!(
            shell.create_window(&WindowDesc {
                app_id: "sh.kryptic\0crcbl",
                ..WindowDesc::default()
            }),
            Err(ShellError::InvalidDescriptor(_)),
        ));
    }

    #[test]
    fn the_surface_target_exists_before_the_window_is_configured() {
        // The seam's promise: the *handle* exists as soon as the window does,
        // so a HAL surface can be created immediately and only the swapchain
        // waits for the first `Resized`.
        let mut shell = shell();
        let window = window(&mut shell);
        assert_eq!(shell.window_state(window).expect("live").size(), None);

        let target = shell.surface_target(window).expect("available already");
        let SurfaceTarget::Win32 { hinstance, hwnd } = target else {
            panic!("the Win32 backend produces a Win32 target: {target:?}");
        };
        assert_eq!(hwnd.as_ptr(), hwnd_of(&shell, window));
        assert_eq!(hinstance, shell.instance);

        // And it does not change across a resize, which is what makes
        // "re-query after a mode change" a complete rule.
        shell.pump(&mut |_| {});
        assert_eq!(shell.surface_target(window).expect("still live"), target);
    }

    #[test]
    fn the_backend_reports_itself_and_is_reachable_by_name() {
        let shell = shell();
        assert_eq!(shell.backend(), ShellBackend::Win32);
        assert_eq!(ShellBackend::Win32.as_str(), "win32");
        let boxed = crate::open_backend(ShellBackend::Win32).expect("registered on Windows");
        assert_eq!(boxed.backend(), ShellBackend::Win32);
        // And the automatic path finds it, because it is the only backend on
        // this platform. Skipped when the environment has an opinion, since
        // `open` is documented to obey it.
        if std::env::var(crate::BACKEND_ENV_VAR).is_err() {
            let automatic = crate::open().expect("auto-selection has one candidate here");
            assert_eq!(automatic.backend(), ShellBackend::Win32);
        }
    }
}
