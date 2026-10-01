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
use super::devices::{Attribution, DeviceTable};
use super::events::RawEvent;
use super::ffi::{self, Handle, Msg, WindowPlacement, value};
use super::geometry;
use super::input;
use super::input::Contact;
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
/// the way out; see [`window`] for why a placement rather than a rectangle.
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
    /// Every touch contact currently down, with where it was last seen.
    pub(super) contacts: Vec<Contact>,
    /// Raw input handles and the stable [`DeviceId`](crcbl_core::input::DeviceId)s
    /// behind them.
    pub(super) devices: DeviceTable,
    /// Raw reports waiting for the key and button messages they produced.
    pub(super) attribution: Attribution,
    /// The high-resolution waitable timer a timed [`wait`](Self::wait) sleeps
    /// on, or `None` where the system refused one (before Windows 10 1803), in
    /// which case the wait falls back to its own millisecond timeout.
    wait_timer: Option<NonNull<c_void>>,
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
        crcbl_core::log::debug!("per-monitor-v2 DPI awareness is on");
        return;
    }
    // SAFETY: reads this thread's last error code, which the call above set.
    let error = unsafe { ffi::GetLastError() };
    if error == value::ERROR_ACCESS_DENIED {
        crcbl_core::log::debug!("the process DPI awareness was already set; leaving it alone");
        return;
    }
    crcbl_core::log::warn!(
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
            contacts: Vec::new(),
            devices: DeviceTable::default(),
            attribution: Attribution::default(),
            // SAFETY: null attributes and a null name ask for an unnamed timer
            // with default security; the flags and access mask are constants
            // the call documents. A null return is a refusal, kept as `None`.
            wait_timer: NonNull::new(unsafe {
                ffi::CreateWaitableTimerExW(
                    ptr::null(),
                    ptr::null(),
                    value::CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    value::TIMER_SET_AND_WAIT,
                )
            }),
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
            .union(ShellCaps::DRAG_DROP)
            .union(ShellCaps::TEXT_IME)
            .union(ShellCaps::TOUCH);
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
    /// is content rather than an owner to negotiate with — see [`clipboard`].
    /// The three outcomes are the three [`ClipboardContent`] means:
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
                crcbl_core::log::warn!(
                    "the clipboard was not readable within {:?}: {refused:?}",
                    clipboard::OPEN_BUDGET
                );
                return ClipboardContent::Unavailable;
            }
        };
        if let Opened::After { attempts } = board.opened() {
            crcbl_core::log::debug!(
                "the clipboard was held by another process for {attempts} attempts"
            );
        }
        let Some(bytes) = board.get(format) else {
            // A file list with no registered `text/uri-list` beside it is what
            // Explorer's "copy" leaves, and it is answered as one. The
            // registered format is asked first; see `clipboard`'s module docs.
            if mime == MimeType::UriList
                && let Some(list) = board.file_uri_list()
            {
                return ClipboardContent::Bytes(list);
            }
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
    /// `peek_pending` was added to print the `MSG` itself
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
    /// [`drain_messages`](Self::drain_messages) on the next pump; what changes
    /// is that one arriving *during* a sleep waits out the timeout instead of
    /// cutting it short. [`QS_ALL_EVENTS`](value::QS_ALL_EVENTS) states that
    /// trade in full, and `queue_status` keeps asking for `QS_ALLINPUT`,
    /// because a diagnosis wants every bit including the one the wait ignores.
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
        // **A timed wait sleeps on the high-resolution timer, not on its own
        // timeout.** `MsgWaitForMultipleObjectsEx`'s millisecond timeout expires
        // on the system clock tick, 15.6 ms by default, and nothing in this
        // process raises the resolution — so a 4 ms wait slept a whole tick
        // whenever no message arrived, capping a windowed game near 64 frames a
        // second (found by EW: 15.6 ms a frame windowed, 2.2 ms offscreen). The
        // timer is armed to the timeout in 100 ns units and waited on with no
        // timeout of its own; a zero timeout, or a system that refused the
        // timer, keeps the plain millisecond wait.
        let armed = match (timeout, self.wait_timer) {
            (Some(timeout), Some(timer)) if !timeout.is_zero() => {
                // Negative is relative. Saturating at `i64::MIN` is a wait of
                // some 29 000 years, which is "forever" for any caller.
                let due =
                    i64::try_from(timeout.as_nanos() / 100).map_or(i64::MIN, |ticks| -ticks.max(1));
                // SAFETY: `timer` is the live handle `open` created; `due` is a
                // live `i64` the call reads; no completion routine.
                let set = unsafe {
                    ffi::SetWaitableTimer(
                        timer.as_ptr(),
                        &raw const due,
                        0,
                        ptr::null(),
                        ptr::null(),
                        0,
                    )
                };
                (set != 0).then_some(timer.as_ptr())
            }
            _ => None,
        };
        let milliseconds = match (armed, timeout) {
            (Some(_), _) | (None, None) => value::INFINITE,
            // `INFINITE` is `u32::MAX`, so a timeout that saturates to it would
            // silently become "forever" — 49 days of sleep is close enough to
            // the caller's intent, but never waking is not.
            (None, Some(timeout)) => u32::try_from(timeout.as_millis())
                .unwrap_or(value::INFINITE - 1)
                .min(value::INFINITE - 1),
        };
        let handles = armed.map_or(0, |_| 1);
        let handle_array = armed.as_ref().map_or(ptr::null(), core::ptr::from_ref);
        // SAFETY: `handle_array` is null with a count of zero, or points at the
        // one live timer handle with a count of one, for the length of the
        // call. Zero flags is the documented "wait for a new message", which is
        // what the drain above makes correct.
        //
        // `QS_ALLEVENTS`, not `QS_ALLINPUT`: the difference is `QS_SENDMESSAGE`,
        // which `PeekMessageW` can neither return nor clear, so waiting on it is
        // waiting on a bit that is already set. See the doc comment.
        let outcome = unsafe {
            ffi::MsgWaitForMultipleObjectsEx(
                handles,
                handle_array,
                milliseconds,
                value::QS_ALL_EVENTS,
                0,
            )
        };
        // With the timer armed, `WAIT_OBJECT_0` is the timer firing and
        // `WAIT_OBJECT_0 + 1` a message; without it, `WAIT_OBJECT_0` is the
        // message.
        match outcome {
            value::WAIT_TIMEOUT => Wake::TimedOut,
            value::WAIT_OBJECT_0 if armed.is_some() => Wake::TimedOut,
            message if message == value::WAIT_OBJECT_0 + handles => Wake::Message,
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
                        self.forget_contacts(window);
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
                        crcbl_core::log::debug!(
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
                RawEvent::DeviceRemoved { device } => self.devices.remove(device),

                // The input half, which needs the layout, the modifier snapshot
                // and this shell's own surrogate and raw-motion state — see
                // [`input`](super::input).
                input_event @ (RawEvent::Key { .. }
                | RawEvent::Char { .. }
                | RawEvent::PointerMotion { .. }
                | RawEvent::PointerFocus { .. }
                | RawEvent::Button { .. }
                | RawEvent::Wheel { .. }
                | RawEvent::Touch { .. }
                | RawEvent::RawMotion { .. }
                | RawEvent::RawKey { .. }) => self.translate_input(input_event, window),
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
    ///   named by the windowing rules as this platform's form of it,
    ///   and implemented in [`geometry`].
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
    ///   [`clipboard`].
    /// * [`DRAG_DROP`](ShellCaps::DRAG_DROP) — `DragAcceptFiles` plus
    ///   `WM_DROPFILES`, honouring
    ///   [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops). The bit
    ///   is what [`DroppedFile`](ShellEvent::DroppedFile) documents it to be —
    ///   *files, in* — and no more: there is no drop cursor and no hover
    ///   feedback while a drag is in the air, because that is `IDropTarget` and
    ///   `IDropTarget` is COM. [`dnd`](super::dnd) gives the argument, and it is
    ///   a gap in *feedback* rather than in the capability the seam names.
    /// * [`TEXT_IME`](ShellCaps::TEXT_IME) — the bit claims only that composed
    ///   text reaches the engine through the platform's input method, which is
    ///   the bar X11 and Wayland set it on. Here the system composes:
    ///   `TranslateMessage` turns a dead key and the next key into one
    ///   `WM_CHAR`, and the `WM_IME_*` family falls through to `DefWindowProc`,
    ///   which delivers an IME's committed string as `WM_CHAR` too. Both arrive
    ///   as [`TextCommit`](ShellEvent::TextCommit), surrogate pairs joined.
    ///   `win32_e2e`'s dead-key test proves the composition on a real desktop.
    ///   What is **not** here, as on every backend, is a pre-edit: no
    ///   composition string reaches the seam and the candidate window is not
    ///   placed at the caret.
    /// * [`TOUCH`](ShellCaps::TOUCH) — `WM_POINTERDOWN`/`UPDATE`/`UP` for
    ///   `PT_TOUCH` pointers, with a capture change or a cancelled release as
    ///   [`Cancelled`](crcbl_core::input::TouchPhase::Cancelled). The primary
    ///   contact reaches `DefWindowProc`, which synthesizes the mouse messages
    ///   the seam obliges a touch backend to deliver; a secondary one does not,
    ///   because given two fingers the default handler turns a pinch into a
    ///   synthesized Ctrl press. A constant, like the rest: a touchscreen
    ///   plugged in mid-session delivers pointer messages to a window that
    ///   never asked for them, so there is nothing to latch on.
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
        self.forget_contacts(window);
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
    /// and `WM_SIZING` from numbers this call precomputes — see [`geometry`].
    /// Unlike X11's `WM_NORMAL_HINTS`, there is nobody who might not read them:
    /// the enforcement is in this process.
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
        self.keep_alive();
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

    /// Runs the message queue and keeps what it produced.
    ///
    /// The `PeekMessageW` inside [`drain_messages`](Self::drain_messages) is
    /// what the system times: a thread that makes one at least every five
    /// seconds is not hung, whatever it does in between.
    fn keep_alive(&mut self) {
        self.drain_messages();
        self.translate();
        self.publish_configurations();
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
                crcbl_core::log::warn!(
                    "MsgWaitForMultipleObjectsEx failed with error {error}; not sleeping"
                );
            }
            Wake::Unexpected { outcome } => {
                crcbl_core::log::warn!(
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
    /// to another client, so this cannot fail for a reason outside this
    /// process. It *can* stop being in effect, and does — see [`input`] and the
    /// window procedure, which release the clip the instant the window loses
    /// focus and re-establish it when focus returns, when the window moves and
    /// when it is resized.
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
            //
            // Logged rather than propagated: the lock itself is established by
            // this point — the clip is applied and the cursor hidden — and the
            // centring is a convenience. Refusing the whole mode change because
            // the pointer could not be moved would report a lock that is in
            // fact in force.
            if let Err(why) = self.warp_to_client(
                window,
                i32::try_from(size.width / 2).unwrap_or(0),
                i32::try_from(size.height / 2).unwrap_or(0),
            ) {
                crcbl_core::log::warn!("the pointer lock could not start from the middle: {why}");
            }
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
    /// recorded for the window procedure — see [`proc`] — and takes effect on
    /// the next movement, which is the first moment anything could have been
    /// drawn anyway.
    ///
    /// *Hiding* is `ShowCursor`, whose per-thread reference count is the classic
    /// bug in this API and is kept balanced by
    /// [`pointer::Visibility`](super::pointer::Visibility) rather than by
    /// counting calls by hand. It applies while the **focused** window wants it
    /// hidden; [`input`] states that rule and its one visible
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
        self.warp_to_client(window, position.x as i32, position.y as i32)
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
    /// Crucible losslessly. The reader picks; see [`clipboard`]. A
    /// `text/uri-list` offer is published a second time as the `CF_HDROP` file
    /// list Explorer pastes, from the URIs in it that name Windows files.
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
    /// [`clipboard::OPEN_BUDGET`] — another process is holding it — or if the
    /// system refused every format.
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
            crcbl_core::log::debug!(
                "the clipboard was held by another process for {attempts} attempts"
            );
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
                crcbl_core::log::warn!(
                    "a text/plain clipboard offer is not valid UTF-8; the invalid bytes will \
                     paste as replacement characters"
                );
            }
            if board.put(format, &clipboard::payload_bytes(encoding, offer.bytes)) {
                published += 1;
            }
            // Explorer pastes files from `CF_HDROP` alone; see `clipboard`.
            if offer.mime == MimeType::UriList && board.put_file_list(offer.bytes) {
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
    ///   is retried for [`clipboard::OPEN_BUDGET`]
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
    /// The window class is deliberately not unregistered; see [`window`].
    /// Neither is the raw-input registration; see [`input`].
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
        if let Some(timer) = self.wait_timer.take() {
            // SAFETY: the timer handle `open` created, closed once; no wait on
            // it can be in progress, because only `wait` waits on it and
            // `drop` has the shell exclusively.
            unsafe { ffi::CloseHandle(timer.as_ptr()) };
        }
    }
}

/// Tests that need a real desktop, and are therefore the point of the
/// `build + test (windows-latest)` CI job.
///
/// **Almost nothing here is `#[ignore]`d, and the exceptions are named where
/// they sit.** `docs/notes/process.md` calls a silently-skipped suite a known
/// trap, and this slice is deliberately how the project finds out whether a
/// GitHub runner gives a process a usable window station: if it does not,
/// [`Win32Shell::open`] fails and every one of these says so, which is an
/// answer. A skipped test is not.
///
/// The exceptions are the three pointer tests whose precondition is a
/// foreground window on an uncontended desktop, which a shared runner does not
/// provide. They are not skipped — `run-win32-e2e.ps1` passes `--run-ignored
/// all` and runs on a real interactive desktop, which is the only place that
/// precondition holds. Each carries its own comment saying so.
#[cfg(all(test, target_os = "windows"))]
mod tests;
