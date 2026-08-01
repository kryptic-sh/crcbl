//! The Web/canvas shell backend (P5).
//!
//! Implements [`Shell`] for a browser `<canvas>` driven by a hand-rolled JS
//! shim. The shim calls into wasm through the `extern "C"` entry points in
//! [`shim`]; each one appends to the queue [`pump`](WebShell::pump) drains.
//!
//! # Architecture
//!
//! `docs/plan/15-windowing.md` chooses a JS shim over `wasm-bindgen` /
//! `web-sys`. Everything the browser has to say arrives through one of the
//! `__crcbl_web_*` functions, and there is exactly **one** queue behind them:
//! the [`Bridge`], owned by the live [`WebShell`] through an [`Rc`] and reached
//! from the entry points through a thread-local. An entry point that could not
//! find a live window drops the event and says so in the log, because a browser
//! event with nowhere to go is a shim that started before the engine did.
//!
//! # Capabilities, and why so few
//!
//! [`caps`](WebShell::caps) reports only what this backend *does*, not what a
//! browser could do:
//!
//! | Bit | Why |
//! | --- | --- |
//! | [`HW_UPSCALE`](ShellCaps::HW_UPSCALE) | a CSS-sized canvas is scaled by the compositor for free |
//! | [`FRACTIONAL_SCALE`](ShellCaps::FRACTIONAL_SCALE) | `devicePixelRatio` is fractional and is reported verbatim |
//!
//! Everything else is clear, and each one is a *behaviour* that is missing
//! rather than a platform that lacks it.
//! [`RAW_POINTER_MOTION`](ShellCaps::RAW_POINTER_MOTION) is the one to
//! understand: the DOM does deliver `movementX`/`movementY` under Pointer Lock,
//! but they are accelerated and clamped by the same OS layer the bit exists to
//! bypass, so a first-person camera must not be told it has trustworthy aim
//! input here. [`TEXT_IME`](ShellCaps::TEXT_IME) is clear because nothing emits
//! [`TextCommit`](ShellEvent::TextCommit);
//! [`POINTER_LOCK`](ShellCaps::POINTER_LOCK) and
//! [`POINTER_CONFINE`](ShellCaps::POINTER_CONFINE) because no entry point calls
//! `requestPointerLock`; [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION) and
//! [`SERVER_DECORATIONS`](ShellCaps::SERVER_DECORATIONS) because a canvas is
//! placed and framed by the document, not by a window system;
//! [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED) because there is no
//! interactive resize for the browser to constrain.
//!
//! # The configure handshake
//!
//! A window created here has no size until the shim reports one through
//! [`__crcbl_web_resize`](shim::__crcbl_web_resize). The first call provides the
//! canvas's layout size; later ones come from `ResizeObserver`. This is the same
//! shape Wayland forces and the reason [`Shell::create_window`] returns nothing
//! but a handle.
//!
//! # Timestamps
//!
//! Every input entry point takes the DOM event's own `timeStamp`, in
//! milliseconds with the browser's sub-millisecond resolution, and never the
//! frame's — quantizing every event in a frame to one instant is exactly what
//! [`event`](crate::event) says makes a fast double-tap indistinguishable from a
//! slow one. `timeStamp` and `performance.now()` share the page's time origin,
//! so [`align_event_clock`](Shell::align_event_clock) only has to subtract the
//! `performance.now()` of the last frame from the engine's own elapsed reading.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use core::time::Duration;

use crcbl_core::{
    EventTime, KeyCode, SurfaceTarget,
    input::{Keysym, Scancode},
};

use crate::{
    ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode, MonitorId,
    MonitorInfo, PhysicalPoint, PhysicalRect, PhysicalSize, PointerMode, Shell, ShellBackend,
    ShellCaps, ShellError, ShellEvent, SizeConstraints, WindowConfiguration, WindowDesc, WindowId,
    WindowState,
};

// ---------------------------------------------------------------------------
// The bridge: the one queue the JS→wasm entry points and `pump` share
// ---------------------------------------------------------------------------

/// What the `extern "C"` entry points and the live [`WebShell`] share.
///
/// The shell owns it; the thread-local below is a *borrow* the entry points use
/// to find it. There is one because the browser calls into wasm with no
/// argument that could name a shell, and a second queue on the side is how the
/// first version of this backend managed to accumulate every browser event
/// forever without delivering one.
#[derive(Debug, Default)]
struct Bridge {
    /// Events the shim has pushed and [`pump`](WebShell::pump) has not drained.
    queue: VecDeque<ShellEvent>,
    /// The live window, or `None` before [`Shell::create_window`] and after
    /// [`Shell::destroy_window`].
    window: Option<WindowId>,
    /// Which canvas that window is on. An entry point naming a different one is
    /// talking about a canvas this shell does not own.
    canvas: u32,
    /// `performance.now()` as of the last `requestAnimationFrame` callback.
    frame_ms: f64,
    /// What to add to a DOM `timeStamp` to put it on the engine's clock. See
    /// [`Shell::align_event_clock`].
    clock_offset_ms: f64,
}

type Shared = Rc<RefCell<Bridge>>;

thread_local! {
    /// The live shell's bridge, or `None` when there is no shell on this
    /// thread. Thread-local rather than global because [`Shell`] is deliberately
    /// not `Send`.
    static BRIDGE: RefCell<Option<Shared>> = const { RefCell::new(None) };

    /// The canvas id the shim announced through
    /// [`__crcbl_web_canvas`](shim::__crcbl_web_canvas), read by [`canvas_id`].
    static CANVAS: Cell<u32> = const { Cell::new(0) };
}

/// Runs `f` against the live shell's bridge.
///
/// Returns `false` when there is no shell on this thread — before
/// [`WebShell::new`] or after the shell was dropped — which is the only honest
/// thing an entry point can do with an event that has nowhere to go.
fn with_bridge(f: impl FnOnce(&mut Bridge)) -> bool {
    BRIDGE.with(|slot| match slot.borrow().as_ref() {
        Some(shared) => {
            f(&mut shared.borrow_mut());
            true
        }
        None => false,
    })
}

/// Queues the event `make` builds, if `canvas` names the live window.
fn queue_for(canvas: u32, make: impl FnOnce(WindowId, &Bridge) -> ShellEvent) {
    let mut queued = false;
    let had_shell = with_bridge(|bridge| {
        if bridge.canvas != canvas {
            return;
        }
        let Some(window) = bridge.window else {
            return;
        };
        let event = make(window, bridge);
        bridge.queue.push_back(event);
        queued = true;
    });
    if !queued {
        // Not an error the shim can recover from, but the first thing to look
        // at when "the canvas never gets input": either the page started
        // pumping before `create_window`, or it is talking about a canvas this
        // shell was not opened for.
        log::debug!(
            "web shim event for canvas {canvas} dropped: {}",
            if had_shell {
                "no live window on that canvas"
            } else {
                "no shell on this thread"
            }
        );
    }
}

/// The canvas id the shim announced, or `0`.
///
/// The shim announces it; there is deliberately no environment variable, because
/// `std::env::var` on `wasm32-unknown-unknown` always answers `NotPresent` and a
/// canvas id read from it would silently always be the fallback.
pub fn canvas_id() -> u32 {
    CANVAS.with(Cell::get)
}

/// Puts a DOM `event.timeStamp` on the engine's clock.
///
/// Keeps the sub-millisecond fraction the browser reports: truncating to whole
/// milliseconds re-introduces most of the quantization the per-event timestamp
/// exists to avoid. A non-finite or negative result — a shim passing nonsense,
/// or an engine clock aligned to before the page's time origin — becomes
/// [`EventTime::ZERO`] rather than panicking inside `Duration`.
fn event_time(bridge: &Bridge, time_ms: f64) -> EventTime {
    let millis = time_ms + bridge.clock_offset_ms;
    if millis.is_finite() && millis > 0.0 {
        EventTime::from_duration(Duration::from_secs_f64(millis / 1000.0))
    } else {
        EventTime::ZERO
    }
}

/// A stable [`Scancode`] for a browser `KeyboardEvent.code`.
///
/// The browser states a key's *physical* identity as a string, and `Scancode` is
/// a `u32`, so this is a 32-bit FNV-1a of it. Like every other platform's
/// scancode numbering it is meaningless to compare against a literal; it exists
/// so a key this engine has no [`KeyCode`] name for is still bindable and
/// round-trips through a profile instead of collapsing to zero.
fn scancode_of(code: &str) -> Scancode {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in code.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    Scancode(hash)
}

/// The [`KeyCode`] for a browser `KeyboardEvent.code`.
///
/// A table lookup rather than a table: [`KeyCode::as_str`] is documented as the
/// stable name used in binding assets, and those names *are* the W3C
/// `code` values (`KeyA`, `Digit0`, `ArrowLeft`, `F1`, `NumpadEnter`, …). The
/// deprecated `keyCode` number this used to switch on is layout-dependent,
/// unspecified across browsers, and gone.
fn key_code_of(code: &str) -> Option<KeyCode> {
    KeyCode::from_name(code)
}

/// The [`Keysym`] for a browser `KeyboardEvent.key`.
///
/// `key` is the *composed* result, so it is a single character for anything
/// that produces one and a name like `"Shift"` or `"ArrowUp"` otherwise. Only
/// the first case has a keysym; the rest report [`Keysym::NONE`], as they do on
/// the Linux backends.
fn keysym_of(key: &str) -> Keysym {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(character), None) => Keysym::from_char(character),
        _ => Keysym::NONE,
    }
}

// ---------------------------------------------------------------------------
// extern "C" entry points — called from JS
// ---------------------------------------------------------------------------

/// The JS→wasm ABI.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`: these symbols exist for a browser to
/// call, and exporting `__crcbl_web_*` from every native binary that links the
/// engine — which an unconditional `no_mangle` did — is namespace pollution for
/// a shim that cannot be there.
pub(crate) mod shim {
    use super::{
        CANVAS, PhysicalPoint, PhysicalSize, ShellEvent, event_time, key_code_of, keysym_of,
        queue_for, scancode_of, with_bridge,
    };
    use crcbl_core::input::{ButtonState, DeviceId, Modifiers, PointerButton, ScrollDelta};

    /// `event.ctrlKey`.
    pub const STATE_CTRL: u32 = 1 << 0;
    /// `event.shiftKey`.
    pub const STATE_SHIFT: u32 = 1 << 1;
    /// `event.altKey`.
    pub const STATE_ALT: u32 = 1 << 2;
    /// `event.metaKey`.
    pub const STATE_SUPER: u32 = 1 << 3;
    /// The positive edge: a key or button went **down**, the pointer
    /// **entered**, the canvas gained focus.
    pub const STATE_EDGE: u32 = 1 << 4;
    /// `KeyboardEvent.repeat`.
    pub const STATE_REPEAT: u32 = 1 << 5;

    /// The modifier half of a `state` word.
    fn modifiers(state: u32) -> Modifiers {
        let mut modifiers = Modifiers::empty();
        modifiers.set(Modifiers::CTRL, state & STATE_CTRL != 0);
        modifiers.set(Modifiers::SHIFT, state & STATE_SHIFT != 0);
        modifiers.set(Modifiers::ALT, state & STATE_ALT != 0);
        modifiers.set(Modifiers::SUPER, state & STATE_SUPER != 0);
        modifiers
    }

    /// Borrows a UTF-8 string the shim wrote into wasm memory.
    ///
    /// # Safety
    ///
    /// `ptr` must be null, or point to `len` initialised bytes that stay valid
    /// and unwritten for the duration of the call. That is the shim's half of
    /// the ABI: it copies the string into the wasm heap immediately before the
    /// call and does not touch it again until the call returns.
    unsafe fn borrow_str<'a>(ptr: *const u8, len: u32) -> Option<&'a str> {
        if ptr.is_null() || len == 0 {
            return None;
        }
        // SAFETY: the caller's obligation above is exactly `from_raw_parts`'s.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
        core::str::from_utf8(bytes).ok()
    }

    /// Announces which canvas this wasm instance drives. Call before `open`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_canvas(canvas: u32) {
        CANVAS.with(|slot| slot.set(canvas));
    }

    /// Called once per `requestAnimationFrame` with `performance.now()`.
    ///
    /// Nothing is stamped with this — every input event carries its own
    /// `timeStamp`. It is the reference point
    /// [`align_event_clock`](crate::Shell::align_event_clock) subtracts.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_frame(now_ms: f64) {
        with_bridge(|bridge| bridge.frame_ms = now_ms);
    }

    /// The canvas's size changed — initial layout, or `ResizeObserver`.
    ///
    /// `scale` is `devicePixelRatio`, which is legitimately below 1 on a
    /// zoomed-out page, so it is passed through rather than clamped up to 1.
    /// Only a non-positive value — which no browser reports — falls back.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_resize(canvas: u32, width: u32, height: u32, scale: f64) {
        let scale_factor = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        queue_for(canvas, |window, _| ShellEvent::Resized {
            window,
            size: PhysicalSize::new(width, height),
            scale_factor,
        });
    }

    /// The canvas gained or lost keyboard focus.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_focus(canvas: u32, state: u32) {
        queue_for(canvas, |window, _| ShellEvent::Focus {
            window,
            focused: state & STATE_EDGE != 0,
        });
    }

    /// The page is going away — `beforeunload`, or a close button the page
    /// draws itself.
    ///
    /// A *question*: the window stays open until
    /// [`reply_close_request`](crate::Shell::reply_close_request) answers it.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_close(canvas: u32) {
        queue_for(canvas, |window, _| ShellEvent::CloseRequested { window });
    }

    /// A `keydown` or `keyup`.
    ///
    /// `code` is `KeyboardEvent.code` (the physical key) and `key` is
    /// `KeyboardEvent.key` (what it produced); both are UTF-8 in wasm memory.
    /// `time_ms` is the event's own `timeStamp`.
    ///
    /// # Safety
    ///
    /// `code_ptr`/`key_ptr` must satisfy the contract on `borrow_str`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_key(
        canvas: u32,
        code_ptr: *const u8,
        code_len: u32,
        key_ptr: *const u8,
        key_len: u32,
        time_ms: f64,
        state: u32,
    ) {
        // SAFETY: forwarded from this function's own safety contract.
        let code = unsafe { borrow_str(code_ptr, code_len) }.unwrap_or_default();
        // SAFETY: likewise.
        let key = unsafe { borrow_str(key_ptr, key_len) }.unwrap_or_default();
        queue_for(canvas, |window, bridge| ShellEvent::Key {
            window,
            device: DeviceId::UNKNOWN,
            time: event_time(bridge, time_ms),
            scancode: scancode_of(code),
            key_code: key_code_of(code),
            keysym: keysym_of(key),
            state: edge(state),
            repeat: state & STATE_REPEAT != 0,
            modifiers: modifiers(state),
        });
    }

    /// A `pointermove`.
    ///
    /// No relative delta: `movementX`/`movementY` are accelerated and clamped by
    /// the OS, which is what
    /// [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) exists to
    /// rule out — so the bit is clear and the field is `None`, and the two agree.
    ///
    /// No `state` word either: [`ShellEvent::PointerMotion`] carries no
    /// modifiers, and a parameter that exists only to be discarded invites
    /// someone to use it for something it is not.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_pointer_motion(canvas: u32, time_ms: f64, x: f64, y: f64) {
        queue_for(canvas, |window, bridge| ShellEvent::PointerMotion {
            window,
            device: DeviceId::UNKNOWN,
            time: event_time(bridge, time_ms),
            abs: Some(PhysicalPoint::new(x, y)),
            raw_delta: None,
        });
    }

    /// A `pointerdown` or `pointerup`. `button` is `MouseEvent.button`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_pointer_button(
        canvas: u32,
        time_ms: f64,
        x: f64,
        y: f64,
        button: u32,
        state: u32,
    ) {
        let button = pointer_button(button);
        queue_for(canvas, |window, bridge| ShellEvent::Button {
            window,
            device: DeviceId::UNKNOWN,
            time: event_time(bridge, time_ms),
            button,
            state: edge(state),
            position: Some(PhysicalPoint::new(x, y)),
            modifiers: modifiers(state),
        });
    }

    /// A `wheel`, in pixels.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_pointer_wheel(
        canvas: u32,
        time_ms: f64,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        state: u32,
    ) {
        queue_for(canvas, |window, bridge| ShellEvent::Wheel {
            window,
            device: DeviceId::UNKNOWN,
            time: event_time(bridge, time_ms),
            delta: ScrollDelta::Pixels {
                x: delta_x,
                y: delta_y,
            },
            position: Some(PhysicalPoint::new(x, y)),
            modifiers: modifiers(state),
        });
    }

    /// A `pointerenter` or `pointerleave`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_pointer_focus(
        canvas: u32,
        time_ms: f64,
        x: f64,
        y: f64,
        state: u32,
    ) {
        let entered = state & STATE_EDGE != 0;
        queue_for(canvas, |window, bridge| ShellEvent::PointerFocus {
            window,
            device: DeviceId::UNKNOWN,
            time: event_time(bridge, time_ms),
            entered,
            position: entered.then(|| PhysicalPoint::new(x, y)),
        });
    }

    /// Pressed on the positive edge, released otherwise.
    fn edge(state: u32) -> ButtonState {
        if state & STATE_EDGE != 0 {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        }
    }

    /// `MouseEvent.button` in the engine's vocabulary.
    ///
    /// The DOM numbering is fixed for the first five; anything past it keeps the
    /// browser's index, saturated rather than truncated into the `u16` — a
    /// wrapped index would name a *different*, plausible button.
    fn pointer_button(button: u32) -> PointerButton {
        match button {
            0 => PointerButton::Left,
            1 => PointerButton::Middle,
            2 => PointerButton::Right,
            3 => PointerButton::Back,
            4 => PointerButton::Forward,
            other => PointerButton::Other(u16::try_from(other).unwrap_or(u16::MAX)),
        }
    }
}

// ---------------------------------------------------------------------------
// WebShell — the backend
// ---------------------------------------------------------------------------

/// The web backend: one `<canvas>`, the shim's event queue, and a virtual
/// monitor sized to the canvas.
#[derive(Debug)]
pub struct WebShell {
    canvas_id: u32,
    shared: Shared,
    /// The live window, mirrored from [`Bridge::window`] so every method can
    /// reject a stale handle without borrowing the shared cell.
    window: Option<WindowId>,
    /// The generation the next window is issued with. Bumped on every create so
    /// a handle from a destroyed window never resolves again.
    next_generation: u32,
    title: String,
    visible: bool,
    requested_mode: DisplayMode,
    requested_constraints: SizeConstraints,
    monitors: Vec<MonitorInfo>,
    configured: Option<WindowConfiguration>,
    focused: bool,
    pointer_mode: PointerMode,
    cursor: Option<CursorIcon>,
    close_pending: bool,
}

impl WebShell {
    /// Creates a web shell for the canvas registered as `canvas_id`.
    ///
    /// Registers this shell as the one the `extern "C"` entry points deliver to,
    /// replacing any previous registration on this thread. It has **no window**
    /// yet: [`create_window`](Shell::create_window) makes one, and until then
    /// every browser event is dropped with a log line.
    #[must_use]
    pub fn new(canvas_id: u32) -> Self {
        let monitors = vec![MonitorInfo {
            id: MonitorId(1),
            name: "canvas".to_string(),
            bounds: PhysicalRect::new(0, 0, 1280, 720),
            work_area: PhysicalRect::new(0, 0, 1280, 720),
            scale_factor: 1.0,
            refresh_millihertz: 60_000,
            is_primary: true,
        }];

        let shared: Shared = Rc::new(RefCell::new(Bridge {
            canvas: canvas_id,
            ..Bridge::default()
        }));
        BRIDGE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&shared)));

        Self {
            canvas_id,
            shared,
            window: None,
            next_generation: 1,
            title: String::new(),
            visible: false,
            requested_mode: DisplayMode::Windowed,
            requested_constraints: SizeConstraints::NONE,
            monitors,
            configured: None,
            focused: false,
            pointer_mode: PointerMode::Free,
            cursor: Some(CursorIcon::Default),
            close_pending: false,
        }
    }

    /// `Ok` only for the one live window.
    ///
    /// Trait obligation 1: a destroyed handle must fail cleanly rather than
    /// reach the canvas it used to name — which is why `destroy_window` clears
    /// `self.window` and every method routes through here.
    fn check(&self, window: WindowId) -> Result<(), ShellError> {
        if self.window == Some(window) {
            Ok(())
        } else {
            Err(ShellError::invalid_window(window))
        }
    }

    fn unsupported(what: &'static str) -> ShellError {
        ShellError::Unsupported {
            backend: ShellBackend::Web,
            what,
        }
    }

    fn configure(&mut self, size: PhysicalSize, scale_factor: f64) {
        self.configured = Some(WindowConfiguration {
            size,
            scale_factor,
            // A canvas sits inside a document: it is not fullscreen, and this
            // backend never calls `requestFullscreen`, so the effective mode is
            // windowed however borderless was requested. `mode_request_honoured`
            // reporting `false` for a borderless request is the honest answer.
            mode: DisplayMode::Windowed,
        });
        if let Some(monitor) = self.monitors.first_mut() {
            monitor.bounds = PhysicalRect::new(0, 0, size.width, size.height);
            monitor.work_area = PhysicalRect::new(0, 0, size.width, size.height);
            monitor.scale_factor = scale_factor;
        }
    }
}

impl Drop for WebShell {
    fn drop(&mut self) {
        // Only if it is still ours: a second shell created on this thread has
        // taken the slot, and clearing it would silence that one instead.
        BRIDGE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, &self.shared))
            {
                *slot = None;
            }
        });
    }
}

impl Shell for WebShell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::Web
    }

    fn caps(&self) -> ShellCaps {
        // Built up rather than subtracted from `DESKTOP`: a bit added to
        // `ShellCaps` must be opted into here by someone who implemented it,
        // not inherited by a backend that has never heard of it. See the module
        // docs for why each of the others is clear.
        ShellCaps::HW_UPSCALE | ShellCaps::FRACTIONAL_SCALE
    }

    /// Creates the canvas window.
    ///
    /// `desc.size` is dropped, and that is the platform rather than an
    /// omission: a canvas's size is the document's, set by CSS and reported back
    /// through [`__crcbl_web_resize`](shim::__crcbl_web_resize) — exactly as a
    /// Wayland compositor decides and answers with a configure. Everything else
    /// in the descriptor is recorded and readable from
    /// [`window_state`](Shell::window_state).
    ///
    /// `desc.accept_drops` is likewise inert: this backend clears
    /// [`ShellCaps::DRAG_DROP`] and never emits
    /// [`DroppedFile`](ShellEvent::DroppedFile), so the hint has nobody to
    /// reach — the same answer X11 gives it.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] for a second window: this backend clears
    /// [`ShellCaps::MULTI_WINDOW`] and there is one canvas.
    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        if self.window.is_some() {
            return Err(Self::unsupported("a second window"));
        }
        // A fresh generation every time, so a handle from a destroyed window
        // never resolves against the canvas that replaced it.
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let window = WindowId::from_bits((u64::from(generation) << 32) | u64::from(self.canvas_id))
            .expect("the generation is never zero");

        self.window = Some(window);
        self.title = desc.title.to_string();
        self.visible = desc.visible;
        self.requested_mode = desc.mode;
        self.requested_constraints = desc.constraints;
        // No size until the shim reports one; see the module docs.
        self.configured = None;
        self.focused = false;
        self.close_pending = false;
        self.pointer_mode = PointerMode::Free;
        self.cursor = Some(CursorIcon::Default);
        self.shared.borrow_mut().window = Some(window);
        Ok(window)
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        self.check(window)?;
        self.window = None;
        self.configured = None;
        self.focused = false;
        self.close_pending = false;
        {
            let mut bridge = self.shared.borrow_mut();
            bridge.window = None;
            // Anything the shim queued for this window before it went away
            // would be delivered naming a handle the consumer has just been
            // told is dead.
            bridge.queue.retain(|event| event.window() != Some(window));
            bridge
                .queue
                .push_back(ShellEvent::WindowDestroyed { window });
        }
        Ok(())
    }

    fn window_state(&self, window: WindowId) -> Result<WindowState, ShellError> {
        self.check(window)?;
        Ok(WindowState {
            configuration: self.configured,
            requested_mode: self.requested_mode,
            requested_constraints: self.requested_constraints,
            focused: self.focused,
            visible: self.visible,
            pointer_mode: self.pointer_mode,
            close_pending: self.close_pending,
        })
    }

    /// Records the title.
    ///
    /// A canvas has no title bar; a shim is free to mirror this into
    /// `document.title`, and nothing here can check whether it did.
    fn set_title(&mut self, window: WindowId, title: &str) -> Result<(), ShellError> {
        self.check(window)?;
        self.title = title.to_string();
        Ok(())
    }

    fn set_visible(&mut self, window: WindowId, visible: bool) -> Result<(), ShellError> {
        self.check(window)?;
        self.visible = visible;
        Ok(())
    }

    /// Records the requested mode.
    ///
    /// Never honoured: this backend does not call `requestFullscreen`, so
    /// [`WindowState::mode_request_honoured`] stays `false` for a borderless
    /// request — the same answer a tiling window manager produces, and the
    /// reason the seam keeps request and effect apart.
    fn set_mode(&mut self, window: WindowId, mode: DisplayMode) -> Result<(), ShellError> {
        self.check(window)?;
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        self.requested_mode = mode;
        Ok(())
    }

    /// Records the constraints.
    ///
    /// Hints only, and this backend has nobody to hint to — the canvas is sized
    /// by the document. See [`Shell::set_constraints`], which says no platform
    /// reports effective constraints back.
    fn set_constraints(
        &mut self,
        window: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        self.check(window)?;
        self.requested_constraints = constraints;
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        // Drained into a local first: the sink may call back into a shim entry
        // point, which borrows the same cell.
        let events: Vec<ShellEvent> = self.shared.borrow_mut().queue.drain(..).collect();
        for event in events {
            match &event {
                ShellEvent::Resized {
                    size, scale_factor, ..
                } => self.configure(*size, *scale_factor),
                ShellEvent::Focus { focused, .. } => self.focused = *focused,
                ShellEvent::CloseRequested { .. } => self.close_pending = true,
                _ => {}
            }
            sink(event);
        }
    }

    /// Returns immediately, always — a browser main thread cannot block, which
    /// is why [`ShellCaps::EVENT_WAIT`] is clear.
    fn wait_events(&mut self, _timeout: Option<Duration>) {}

    /// Puts the shim's `event.timeStamp` values on the engine's clock.
    ///
    /// `timeStamp` and `performance.now()` share the page's time origin, so the
    /// whole conversion is one subtraction: the engine says how long it has been
    /// running, the last `requestAnimationFrame` said what the browser called
    /// that same instant, and the difference is the offset. Discharges the
    /// trait's "timestamps are rebased" obligation, which this backend
    /// previously left to a default that assumed the two clocks already
    /// coincided.
    fn align_event_clock(&mut self, elapsed: Duration) {
        let mut bridge = self.shared.borrow_mut();
        bridge.clock_offset_ms = elapsed.as_secs_f64() * 1000.0 - bridge.frame_ms;
    }

    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError> {
        self.check(window)?;
        Ok(SurfaceTarget::Web {
            canvas_id: self.canvas_id,
        })
    }

    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        self.check(window)?;
        if !self.caps().contains(mode.required_cap()) {
            return Err(Self::unsupported(mode.as_str()));
        }
        self.pointer_mode = mode;
        Ok(())
    }

    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        self.check(window)?;
        self.cursor = cursor;
        Ok(())
    }

    fn warp_pointer(
        &mut self,
        window: WindowId,
        _position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        self.check(window)?;
        Err(Self::unsupported("pointer warp"))
    }

    fn reply_close_request(
        &mut self,
        window: WindowId,
        reply: CloseReply,
    ) -> Result<(), ShellError> {
        self.check(window)?;
        if !self.close_pending {
            return Err(ShellError::NoPendingCloseRequest { window });
        }
        self.close_pending = false;
        match reply {
            // The two answers differ, which they did not before: agreeing to
            // close is what actually ends the window's life here, exactly as
            // calling `destroy_window` would.
            CloseReply::Close => self.destroy_window(window),
            CloseReply::Keep => Ok(()),
        }
    }

    fn clipboard_offer(
        &mut self,
        _window: WindowId,
        _offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        Err(Self::unsupported("clipboard"))
    }

    fn clipboard_request(
        &mut self,
        _window: WindowId,
        _mime: crate::MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        Err(Self::unsupported("clipboard"))
    }

    fn clipboard_readable(&self, _window: WindowId) -> bool {
        false
    }
}

/// Creates a web shell for `canvas_id`.
///
/// Called from the backend registry for `CRCBL_SHELL=web` and
/// `open_backend(Web)`; see [`canvas_id`] for where the id comes from.
///
/// # Errors
///
/// None today — the browser objects only when a canvas is used.
pub fn open(canvas_id: u32) -> Result<WebShell, ShellError> {
    Ok(WebShell::new(canvas_id))
}

#[cfg(test)]
mod tests {
    use super::shim::{STATE_ALT, STATE_CTRL, STATE_EDGE, STATE_REPEAT, STATE_SHIFT, STATE_SUPER};
    use super::*;
    use crcbl_core::input::{ButtonState, Modifiers, PointerButton};

    /// A shell with the one window the backend allows, ready for the shim.
    fn shell_with_window(canvas: u32) -> (WebShell, WindowId) {
        let mut shell = WebShell::new(canvas);
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("the first window");
        (shell, window)
    }

    fn drain(shell: &mut WebShell) -> Vec<ShellEvent> {
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        events
    }

    #[test]
    fn a_window_has_no_size_until_the_shim_reports_one() {
        let (shell, window) = shell_with_window(7);
        assert_eq!(shell.backend(), ShellBackend::Web);
        let state = shell.window_state(window).expect("valid window");
        assert!(!state.is_configured());
        assert_eq!(state.size(), None);
    }

    /// The test whose absence let the entry points stay wired to nothing: a
    /// browser resize must reach `pump` and configure the window.
    #[test]
    fn a_shim_resize_reaches_pump_and_configures_the_window() {
        let (mut shell, window) = shell_with_window(3);
        // SAFETY: the shim ABI; nothing is dereferenced by this entry point.
        unsafe { shim::__crcbl_web_resize(3, 1920, 1080, 2.0) };

        let events = drain(&mut shell);
        assert_eq!(
            events,
            vec![ShellEvent::Resized {
                window,
                size: PhysicalSize::new(1920, 1080),
                scale_factor: 2.0,
            }]
        );
        let state = shell.window_state(window).expect("valid window");
        assert_eq!(state.size(), Some(PhysicalSize::new(1920, 1080)));
        assert_eq!(state.scale_factor(), Some(2.0));
        // The virtual monitor follows the canvas.
        assert_eq!(
            shell.monitors()[0].bounds,
            PhysicalRect::new(0, 0, 1920, 1080)
        );
    }

    #[test]
    fn shim_events_before_a_window_exists_are_dropped_not_accumulated() {
        let mut shell = WebShell::new(0);
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_resize(0, 800, 600, 1.0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_close(0) };
        assert_eq!(drain(&mut shell), Vec::new());

        let window = shell
            .create_window(&WindowDesc::default())
            .expect("the first window");
        // And a resize for a canvas this shell does not own goes nowhere.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_resize(9, 800, 600, 1.0) };
        assert_eq!(drain(&mut shell), Vec::new());
        assert_eq!(shell.window_state(window).expect("state").size(), None);
    }

    #[test]
    fn focus_and_close_come_from_the_shim_rather_than_being_assumed() {
        let (mut shell, window) = shell_with_window(0);
        assert!(
            !shell.window_state(window).expect("state").focused,
            "a fresh canvas is not focused until the browser says so"
        );
        assert!(matches!(
            shell.reply_close_request(window, CloseReply::Keep),
            Err(ShellError::NoPendingCloseRequest { .. })
        ));

        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_focus(0, STATE_EDGE) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_close(0) };
        let events = drain(&mut shell);
        assert_eq!(events.len(), 2);
        let state = shell.window_state(window).expect("state");
        assert!(state.focused);
        assert!(state.close_pending);

        shell
            .reply_close_request(window, CloseReply::Keep)
            .expect("answered");
        assert!(!shell.window_state(window).expect("state").close_pending);
        assert!(shell.window_state(window).is_ok(), "still open");
    }

    #[test]
    fn agreeing_to_close_destroys_the_window() {
        let (mut shell, window) = shell_with_window(0);
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_close(0) };
        drain(&mut shell);
        shell
            .reply_close_request(window, CloseReply::Close)
            .expect("answered");
        assert_eq!(
            drain(&mut shell),
            vec![ShellEvent::WindowDestroyed { window }]
        );
        assert!(shell.window_state(window).is_err());
    }

    #[test]
    fn a_destroyed_window_stops_answering_every_method() {
        let (mut shell, window) = shell_with_window(4);
        shell.destroy_window(window).expect("destroy");
        assert_eq!(
            drain(&mut shell),
            vec![ShellEvent::WindowDestroyed { window }]
        );

        assert!(shell.window_state(window).is_err());
        assert!(shell.surface_target(window).is_err());
        assert!(shell.set_title(window, "gone").is_err());
        assert!(shell.set_visible(window, true).is_err());
        assert!(shell.set_mode(window, DisplayMode::Windowed).is_err());
        assert!(
            shell
                .set_constraints(window, SizeConstraints::NONE)
                .is_err()
        );
        assert!(shell.set_cursor(window, None).is_err());
        assert!(shell.set_pointer_mode(window, PointerMode::Free).is_err());
        assert!(shell.destroy_window(window).is_err());
        assert!(shell.reply_close_request(window, CloseReply::Keep).is_err());

        // A replacement canvas window is a different handle.
        let next = shell
            .create_window(&WindowDesc::default())
            .expect("the canvas is still there");
        assert_ne!(next, window);
        assert!(shell.window_state(window).is_err());
        assert!(shell.window_state(next).is_ok());
    }

    #[test]
    fn a_second_window_is_unsupported_because_caps_say_so() {
        let (mut shell, _) = shell_with_window(0);
        assert!(!shell.caps().contains(ShellCaps::MULTI_WINDOW));
        assert!(matches!(
            shell.create_window(&WindowDesc::default()),
            Err(ShellError::Unsupported { .. })
        ));
    }

    #[test]
    fn the_descriptor_is_recorded_rather_than_dropped() {
        let mut shell = WebShell::new(0);
        let constraints = SizeConstraints::min(crate::LogicalSize::new(320.0, 240.0));
        let window = shell
            .create_window(&WindowDesc {
                title: "sandbox",
                visible: false,
                mode: DisplayMode::Borderless { monitor: None },
                constraints,
                ..WindowDesc::default()
            })
            .expect("window");
        let state = shell.window_state(window).expect("state");
        assert!(!state.visible);
        assert_eq!(
            state.requested_mode,
            DisplayMode::Borderless { monitor: None }
        );
        assert_eq!(state.requested_constraints, constraints);
        assert_eq!(shell.title, "sandbox");
    }

    #[test]
    fn caps_and_behaviour_agree() {
        let shell = WebShell::new(0);
        let caps = shell.caps();
        assert_eq!(
            caps,
            ShellCaps::HW_UPSCALE | ShellCaps::FRACTIONAL_SCALE,
            "every other bit names something this backend does not do"
        );
        // The four that used to be advertised with nothing behind them.
        assert!(!caps.contains(ShellCaps::RAW_POINTER_MOTION));
        assert!(!caps.contains(ShellCaps::TEXT_IME));
        assert!(!caps.contains(ShellCaps::WINDOW_POSITION));
        assert!(!caps.contains(ShellCaps::SERVER_DECORATIONS));
        assert!(!caps.has_mouselook());
    }

    #[test]
    fn keys_come_from_the_physical_code_and_carry_their_own_timestamp() {
        let (mut shell, window) = shell_with_window(0);
        // The engine has been running 40 ms; the browser called that instant
        // 1000 ms, so an event stamped 1002.5 ms is 42.5 ms on our clock.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_frame(1000.0) };
        shell.align_event_clock(Duration::from_millis(40));

        let code = b"KeyW";
        let key = "w".as_bytes();
        // SAFETY: both slices outlive the call and are valid UTF-8.
        unsafe {
            shim::__crcbl_web_key(
                0,
                code.as_ptr(),
                code.len() as u32,
                key.as_ptr(),
                key.len() as u32,
                1002.5,
                STATE_EDGE | STATE_SHIFT | STATE_CTRL | STATE_REPEAT,
            );
        }
        let events = drain(&mut shell);
        let [
            ShellEvent::Key {
                window: got,
                time,
                scancode,
                key_code,
                keysym,
                state,
                repeat,
                modifiers,
                ..
            },
        ] = events.as_slice()
        else {
            panic!("expected one Key event, got {events:?}");
        };
        assert_eq!(*got, window);
        assert_eq!(*key_code, Some(KeyCode::KeyW));
        assert_eq!(*keysym, Keysym::from_char('w'));
        assert_eq!(*state, ButtonState::Pressed);
        assert!(*repeat);
        assert_eq!(*modifiers, Modifiers::CTRL | Modifiers::SHIFT);
        assert_ne!(*scancode, Scancode(0), "an unnamed key is still bindable");
        // Sub-millisecond precision survives.
        assert_eq!(*time, EventTime::from_micros(42_500));
    }

    #[test]
    fn two_events_in_one_frame_have_different_timestamps() {
        let (mut shell, _) = shell_with_window(0);
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_frame(0.0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 4.0, 10.0, 20.0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 12.0, 11.0, 21.0) };
        let events = drain(&mut shell);
        let times: Vec<Option<EventTime>> = events.iter().map(ShellEvent::time).collect();
        assert_eq!(
            times,
            vec![
                Some(EventTime::from_millis(4)),
                Some(EventTime::from_millis(12))
            ],
            "a frame must not quantize the events inside it"
        );
    }

    #[test]
    fn pointer_buttons_and_wheels_arrive_with_modifiers() {
        let (mut shell, _) = shell_with_window(0);
        // SAFETY: the shim ABI.
        unsafe {
            shim::__crcbl_web_pointer_button(
                0,
                1.0,
                5.0,
                6.0,
                2,
                STATE_EDGE | STATE_ALT | STATE_SUPER,
            );
        }
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_wheel(0, 2.0, 5.0, 6.0, 0.0, -120.0, 0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_focus(0, 3.0, 5.0, 6.0, 0) };
        let events = drain(&mut shell);
        assert_eq!(
            events.iter().map(ShellEvent::name).collect::<Vec<&str>>(),
            ["Button", "Wheel", "PointerFocus"]
        );
        let ShellEvent::Button {
            button,
            position,
            modifiers,
            ..
        } = &events[0]
        else {
            panic!("expected a Button");
        };
        assert_eq!(*button, PointerButton::Right);
        assert_eq!(*position, Some(PhysicalPoint::new(5.0, 6.0)));
        assert_eq!(*modifiers, Modifiers::ALT | Modifiers::SUPER);
        let ShellEvent::PointerFocus {
            entered, position, ..
        } = &events[2]
        else {
            panic!("expected a PointerFocus");
        };
        assert!(!*entered);
        assert_eq!(*position, None, "a leave has no position");
    }

    #[test]
    fn a_sub_unit_device_pixel_ratio_is_reported_verbatim() {
        let (mut shell, window) = shell_with_window(0);
        // A zoomed-out page genuinely reports below 1; clamping it up produced a
        // swapchain larger than the canvas.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_resize(0, 640, 360, 0.5) };
        drain(&mut shell);
        assert_eq!(
            shell.window_state(window).expect("state").scale_factor(),
            Some(0.5)
        );
        // Nonsense still falls back rather than poisoning the swapchain size.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_resize(0, 640, 360, f64::NAN) };
        drain(&mut shell);
        assert_eq!(
            shell.window_state(window).expect("state").scale_factor(),
            Some(1.0)
        );
    }

    #[test]
    fn unsupported_operations_name_the_capability() {
        let (mut shell, window) = shell_with_window(0);
        assert!(
            shell
                .warp_pointer(window, PhysicalPoint::new(100.0, 200.0))
                .is_err()
        );
        assert!(shell.set_pointer_mode(window, PointerMode::Locked).is_err());
        assert!(shell.clipboard_offer(window, &[]).is_err());
        assert!(
            shell
                .clipboard_request(window, crate::MimeType::TextUtf8)
                .is_err()
        );
        assert!(!shell.clipboard_readable(window));
        // Free needs no capability and is the only mode this backend has.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("free is always available");
    }

    #[test]
    fn surface_target_is_the_canvas() {
        let (shell, window) = shell_with_window(3);
        assert_eq!(
            shell.surface_target(window).expect("valid window"),
            SurfaceTarget::Web { canvas_id: 3 }
        );
    }

    #[test]
    fn foreign_handles_are_rejected() {
        let (shell, _) = shell_with_window(0);
        let foreign = WindowId::from_bits(0xDEAD_BEEF_0001_0001).expect("valid bits");
        assert!(shell.window_state(foreign).is_err());
        assert!(shell.surface_target(foreign).is_err());
    }

    #[test]
    fn key_codes_are_the_w3c_code_names() {
        assert_eq!(key_code_of("KeyA"), Some(KeyCode::KeyA));
        assert_eq!(key_code_of("Space"), Some(KeyCode::Space));
        assert_eq!(key_code_of("ArrowLeft"), Some(KeyCode::ArrowLeft));
        assert_eq!(key_code_of("F1"), Some(KeyCode::F1));
        assert_eq!(key_code_of(""), None);
        assert_eq!(key_code_of("Fn"), None);
        // Distinct scancodes for keys with no name, so both stay bindable.
        assert_ne!(scancode_of("Fn"), scancode_of("Lang1"));
        assert_eq!(keysym_of("Shift"), Keysym::NONE);
        assert_eq!(keysym_of("é"), Keysym::from_char('é'));
    }

    #[test]
    fn the_canvas_id_comes_from_the_shim() {
        // Not from `CRCBL_CANVAS_ID`: `std::env::var` on
        // `wasm32-unknown-unknown` always answers `NotPresent`, so a canvas id
        // read from the environment was always the fallback.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_canvas(11) };
        assert_eq!(canvas_id(), 11);
        let shell = open(canvas_id()).expect("opens");
        assert!(
            shell
                .surface_target(WindowId::from_bits(0x0000_0001_0000_000B).expect("bits"))
                .is_err(),
            "and it has no window until one is created"
        );
    }
}
