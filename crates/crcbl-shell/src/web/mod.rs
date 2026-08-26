//! The Web/canvas shell backend (P5).
//!
//! Compiled on `wasm32`, where it is the real backend, and under `cfg(test)`,
//! where its own tests drive the JS→wasm entry points exactly as a browser
//! would. **Not** in a native build: there is no shim to call it, and compiling
//! it everywhere exported the shim's `__crcbl_web_*` symbols from every binary
//! that links the engine. The registry entry stays on every target regardless,
//! so `CRCBL_SHELL=web` off wasm names the problem instead of hanging — see
//! [`backend`](crate::backend).
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
//! | [`TOUCH`](ShellCaps::TOUCH) | the shim forwards every contact through [`__crcbl_web_touch`](shim::__crcbl_web_touch) |
//! | [`POINTER_LOCK`](ShellCaps::POINTER_LOCK) | the shim calls `requestPointerLock` from a gesture and answers with [`__crcbl_web_pointer_lock`](shim::__crcbl_web_pointer_lock) |
//! | [`RAW_POINTER_MOTION`](ShellCaps::RAW_POINTER_MOTION) | under that lock `movementX`/`movementY` are the whole event, asked for with `unadjustedMovement: true` |
//!
//! Everything else is clear, and each one is a *behaviour* that is missing
//! rather than a platform that lacks it. [`TEXT_IME`](ShellCaps::TEXT_IME) is
//! clear because nothing emits [`TextCommit`](ShellEvent::TextCommit);
//! [`POINTER_CONFINE`](ShellCaps::POINTER_CONFINE) because the Pointer Lock API
//! is the only pointer constraint a browser has and it pins rather than
//! confines; [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION) and
//! [`SERVER_DECORATIONS`](ShellCaps::SERVER_DECORATIONS) because a canvas is
//! placed and framed by the document, not by a window system;
//! [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED) because there is no
//! interactive resize for the browser to constrain.
//!
//! # Mouselook, and exactly what `RAW_POINTER_MOTION` promises here
//!
//! This backend used to clear both halves of
//! [`has_mouselook`](ShellCaps::has_mouselook) and say so at length: the DOM
//! delivers `movementX`/`movementY` under Pointer Lock, and those are
//! accelerated by the same OS layer
//! [`RAW_POINTER_MOTION`](ShellCaps::RAW_POINTER_MOTION) exists to bypass. That
//! argument was answered by the platform. `requestPointerLock` takes an options
//! dictionary, and `unadjustedMovement: true` **is** the bypass — the browser
//! asks the OS for raw device motion instead of the pointer's — so the shim
//! asks for it and a first-person camera in a browser gets what it gets
//! natively.
//!
//! The caveat is stated rather than buried, because the bit is a claim about
//! aim: where a browser declines the option the shim retries without it and the
//! deltas are then the OS-adjusted ones. Per MDN's compatibility data the
//! option is implemented by Chrome and Edge from 88, Chrome for Android from
//! 151, Firefox from 152 and Safari from 18.4 — and not by Safari on iOS or by
//! Firefox for Android, which is the one that reports it by rejecting with
//! `NotSupportedError` rather than ignoring the key. So the bit says *this
//! backend reads a relative-motion stream and asked for it unadjusted*, which
//! is the decision a camera needs — differencing absolute positions is wrong
//! here either way — and not *every browser granted it*.
//!
//! # The lock is taken from a gesture, so it is a request the shim completes
//!
//! [`set_pointer_mode`](Shell::set_pointer_mode) records
//! [`Locked`](PointerMode::Locked) and cannot do more, for the reason
//! [`__crcbl_web_fullscreen`](shim::__crcbl_web_fullscreen) gives about
//! fullscreen: a browser grants Pointer Lock only to code running inside a user
//! gesture, and wasm never is. Nor can the Rust side *call* the shim — the
//! whole ABI is exports-plus-polling, and `web/tools/check-exports.mjs` fails
//! the build over a single import — so the request is published rather than
//! pushed. [`__crcbl_web_pointer_lock_wanted`](shim::__crcbl_web_pointer_lock_wanted)
//! is what the shim polls; it arms a one-shot `pointerdown` on the canvas and
//! the player's next click is the gesture that takes the lock.
//!
//! What comes back is [`__crcbl_web_pointer_lock`](shim::__crcbl_web_pointer_lock),
//! and it is the one source of truth about whether the pointer is actually
//! pinned — like fullscreen, the answer may be one nobody asked for, since
//! Escape releases the lock without delivering a key anywhere. While it says
//! locked, [`PointerMotion`](ShellEvent::PointerMotion) carries the delta and
//! no `abs`, and [`Button`](ShellEvent::Button) and [`Wheel`](ShellEvent::Wheel)
//! carry no position: there is no meaningful absolute position under a lock,
//! and the frozen one would make anything reading it appear to work until the
//! day it does not.
//!
//! # Touch, and the pointer events that come with it
//!
//! A browser reports a finger twice: once as a `PointerEvent` whose
//! `pointerType` is `"touch"`, and — for the **primary** contact only — as the
//! mouse events every page written before touch existed depends on. This
//! backend keeps both. Every contact becomes a [`Touch`](ShellEvent::Touch)
//! through [`__crcbl_web_touch`](shim::__crcbl_web_touch), and the primary one
//! *additionally* becomes [`PointerMotion`](ShellEvent::PointerMotion) and
//! [`Button`](ShellEvent::Button) through the pointer entry points, which is
//! what [`ShellCaps::TOUCH`] obliges a backend that reports contacts to do.
//!
//! The alternative — contacts only, and let the engine synthesize a pointer —
//! was rejected because the browser is already doing that synthesis, correctly,
//! including the `isPrimary` rule that says a second finger landing does not
//! move the mouse. Doing it again a layer up would mean two answers to "where
//! is the pointer" that agree until they do not.
//!
//! # The configure handshake
//!
//! A window created here has no size until the shim reports one through
//! [`__crcbl_web_resize`](shim::__crcbl_web_resize). The first call provides the
//! canvas's layout size; later ones come from `ResizeObserver`. This is the same
//! shape Wayland forces and the reason [`Shell::create_window`] returns nothing
//! but a handle.
//!
//! # Strings cross in a buffer wasm owns
//!
//! [`__crcbl_web_key`](shim::__crcbl_web_key) is the only entry point that
//! takes pointers, and a browser cannot invent an address inside wasm memory —
//! it can only be given one. [`__crcbl_web_key_scratch_ptr`] and
//! [`__crcbl_web_key_scratch_capacity`] are that address and its bound; the
//! shim writes `KeyboardEvent.code` at the start of the buffer and
//! `KeyboardEvent.key` immediately after it, then passes the two `(ptr, len)`
//! pairs. This is the same shape `crcbl-store`'s fetch and OPFS entry points
//! use, and for the same reason.
//!
//! [`__crcbl_web_key_scratch_ptr`]: shim::__crcbl_web_key_scratch_ptr
//! [`__crcbl_web_key_scratch_capacity`]: shim::__crcbl_web_key_scratch_capacity
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
    /// Whether the canvas is the document's `fullscreenElement`, as the shim
    /// last reported through
    /// [`__crcbl_web_fullscreen`](shim::__crcbl_web_fullscreen).
    ///
    /// Not an event, because there is no `ShellEvent` for "the mode changed" —
    /// a mode change is observed through
    /// [`WindowConfiguration::mode`](crate::WindowConfiguration::mode), and the
    /// size change that comes with it arrives as an ordinary
    /// [`Resized`](ShellEvent::Resized) from the shim's `ResizeObserver`.
    /// [`pump`](WebShell::pump) copies this into the shell before it drains the
    /// queue, so the two orders the browser can produce — `fullscreenchange`
    /// before the resize, or after it — settle on the same configuration.
    fullscreen: bool,
    /// Whether [`set_pointer_mode`](Shell::set_pointer_mode) last asked for
    /// [`Locked`](PointerMode::Locked).
    ///
    /// Published for the shim to poll through
    /// [`__crcbl_web_pointer_lock_wanted`](shim::__crcbl_web_pointer_lock_wanted)
    /// rather than pushed at it, because a browser artifact imports nothing:
    /// see the module docs.
    pointer_lock_wanted: bool,
    /// Whether the canvas is the document's `pointerLockElement`, as the shim
    /// last reported through
    /// [`__crcbl_web_pointer_lock`](shim::__crcbl_web_pointer_lock).
    ///
    /// The *effective* half, and the one the pointer entry points read: a
    /// request that no gesture has completed yet, or one the browser released
    /// on its own, must not make an event claim a relative delta it does not
    /// have.
    pointer_locked: bool,
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

    /// Where the shim writes `KeyboardEvent.code` and `KeyboardEvent.key`.
    ///
    /// A fixed array in thread-local storage, so its address is stable for the
    /// life of the thread: a `Vec` could reallocate and a pointer the shim
    /// cached would then name freed memory. `Cell::as_ptr` hands the address
    /// out without a borrow and without `unsafe` — nothing on the Rust side
    /// ever reads *through* the cell, because
    /// [`__crcbl_web_key`](shim::__crcbl_web_key) reads the raw pointer the
    /// shim passed back in.
    static KEY_SCRATCH: Cell<[u8; KEY_SCRATCH_CAPACITY]> =
        const { Cell::new([0; KEY_SCRATCH_CAPACITY]) };
}

/// How many bytes the key scratch buffer holds.
///
/// The two strings a `keydown` carries share it: the shim writes
/// `KeyboardEvent.code` at the start and `KeyboardEvent.key` immediately after,
/// then passes both pointers. `code` is a W3C identifier — the longest is
/// `MediaTrackPrevious`, 18 bytes — and `key` is one composed character or a
/// name of similar length, so 256 is roughly an order of magnitude of headroom
/// over anything a browser can produce. A shim whose two strings do not fit
/// must drop the event rather than truncate it: half a `code` is a *different*
/// key, and would bind to one.
pub const KEY_SCRATCH_CAPACITY: usize = 256;

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
        crcbl_core::log::debug!(
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
        CANVAS, KEY_SCRATCH, KEY_SCRATCH_CAPACITY, PhysicalPoint, PhysicalSize, ShellEvent,
        event_time, key_code_of, keysym_of, queue_for, scancode_of, with_bridge,
    };
    use crcbl_core::input::{
        ButtonState, ContactId, DeviceId, Modifiers, PointerButton, ScrollDelta, TouchPhase,
    };

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

    /// A contact landed: a touch `pointerdown`.
    ///
    /// The four phases are a small enumeration rather than bits in a `state`
    /// word because they are exclusive — a contact is in exactly one of them —
    /// and a bitmask would let a shim describe a finger that both began and
    /// ended.
    pub const TOUCH_BEGAN: u32 = 0;
    /// A contact moved: a touch `pointermove`.
    pub const TOUCH_MOVED: u32 = 1;
    /// A contact was lifted: a touch `pointerup`.
    pub const TOUCH_ENDED: u32 = 2;
    /// A contact was taken away: a touch `pointercancel`.
    pub const TOUCH_CANCELLED: u32 = 3;

    /// The phase word of [`__crcbl_web_touch`], or `None` for one this ABI does
    /// not name.
    const fn touch_phase(phase: u32) -> Option<TouchPhase> {
        match phase {
            TOUCH_BEGAN => Some(TouchPhase::Began),
            TOUCH_MOVED => Some(TouchPhase::Moved),
            TOUCH_ENDED => Some(TouchPhase::Ended),
            TOUCH_CANCELLED => Some(TouchPhase::Cancelled),
            _ => None,
        }
    }

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

    /// The address the shim writes a key event's two strings to.
    ///
    /// Stable for the life of the thread, so a shim may read it once. It is
    /// **wasm's** buffer: JS writes into it and calls
    /// [`__crcbl_web_key`] immediately, and nothing else may be interleaved
    /// between the write and the call.
    ///
    /// This exists because `__crcbl_web_key` takes pointers *in*, and a browser
    /// has no way to obtain an address inside wasm memory except from an export
    /// that hands one out — the same reason
    /// [`crcbl-store`](https://docs.rs/crcbl-store)'s fetch and OPFS entry
    /// points each publish a scratch buffer. Without it the key ABI was
    /// specified but not callable: the module docs said the shim "copies the
    /// string into the wasm heap immediately before the call" and gave it
    /// nowhere to copy to. Found while writing that shim (P5.8).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_key_scratch_ptr() -> *mut u8 {
        KEY_SCRATCH.with(|slot| slot.as_ptr().cast::<u8>())
    }

    /// How many bytes may be written there: [`KEY_SCRATCH_CAPACITY`].
    ///
    /// Read it rather than assuming the constant — the two are the same number
    /// today and a shim that hard-codes it is a shim that keeps working after
    /// the number changes and then starts corrupting the frame after the buffer.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_key_scratch_capacity() -> u32 {
        u32::try_from(KEY_SCRATCH_CAPACITY).unwrap_or(u32::MAX)
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

    /// The canvas entered or left the document's fullscreen element.
    ///
    /// **The page owns the gesture, the engine owns the state.** A browser
    /// grants `requestFullscreen` only from inside a user-gesture handler, and
    /// wasm is never inside one: the shim's `keydown` listener is, a
    /// `requestAnimationFrame` callback is not, and the engine only sees a key
    /// on the frame *after* it was pressed. So the shim calls
    /// `requestFullscreen` itself and reports the outcome here, exactly as a
    /// compositor answers [`Shell::set_mode`](crate::Shell::set_mode) with a
    /// configure rather than obeying it. The answer may also be one nobody
    /// asked for — Escape leaves fullscreen without any key reaching the engine
    /// — which is the case
    /// [`WindowState::mode_request_honoured`](crate::WindowState::mode_request_honoured)
    /// exists for.
    ///
    /// `state & STATE_EDGE` means the canvas *is* the fullscreen element.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_fullscreen(canvas: u32, state: u32) {
        let entered = state & STATE_EDGE != 0;
        with_bridge(|bridge| {
            if bridge.canvas == canvas {
                bridge.fullscreen = entered;
            }
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
    /// `x`/`y` are the position in device pixels and `dx`/`dy` are
    /// `movementX`/`movementY` scaled the same way. **Which of the two pairs
    /// survives is not the shim's call**: it passes both and the bridge's
    /// `pointer_locked` decides, because [`__crcbl_web_pointer_button`] and
    /// [`__crcbl_web_pointer_wheel`] have to suppress their positions under the
    /// same lock and only the bridge sees all three. A locked event carries the
    /// delta and no `abs`; an unlocked one carries the position and no delta,
    /// since `movementX` off the lock is a difference of adjusted positions and
    /// [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) means more
    /// than "a number arrived".
    ///
    /// No `state` word: [`ShellEvent::PointerMotion`] carries no modifiers, and
    /// a parameter that exists only to be discarded invites someone to use it
    /// for something it is not.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_pointer_motion(
        canvas: u32,
        time_ms: f64,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    ) {
        queue_for(canvas, |window, bridge| {
            let locked = bridge.pointer_locked;
            ShellEvent::PointerMotion {
                window,
                device: DeviceId::UNKNOWN,
                time: event_time(bridge, time_ms),
                abs: (!locked).then(|| PhysicalPoint::new(x, y)),
                raw_delta: locked.then_some((dx, dy)),
            }
        });
    }

    /// Whether the engine wants the pointer locked, for the shim to poll.
    ///
    /// The *request* — what [`set_pointer_mode`](crate::Shell::set_pointer_mode)
    /// was last given — and not whether the browser granted it. The shim polls
    /// this once per frame and arms the gesture that can complete it; see the
    /// module docs for why the request is published rather than pushed.
    ///
    /// `0` for a canvas this shell does not own, and `0` when there is no shell
    /// on this thread at all: the same "an event with nowhere to go" case every
    /// other entry point has, answered with the only value that cannot make a
    /// page act.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_pointer_lock_wanted(canvas: u32) -> u32 {
        let mut wanted = 0;
        with_bridge(|bridge| {
            if bridge.canvas == canvas && bridge.pointer_lock_wanted {
                wanted = 1;
            }
        });
        wanted
    }

    /// The canvas became, or stopped being, the document's `pointerLockElement`.
    ///
    /// `state & STATE_EDGE` means the pointer *is* locked to this canvas. As
    /// with [`__crcbl_web_fullscreen`], the answer may be one nobody asked for
    /// — Escape releases the lock, and a `pointerlockerror` ends a request that
    /// no key or click will ever report — so this is the engine's only source
    /// of truth about it. Not an event: there is no `ShellEvent` for "the
    /// pointer mode changed", and what a consumer actually reads is the shape
    /// of the [`PointerMotion`](ShellEvent::PointerMotion) that follows.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_pointer_lock(canvas: u32, state: u32) {
        let locked = state & STATE_EDGE != 0;
        with_bridge(|bridge| {
            if bridge.canvas == canvas {
                bridge.pointer_locked = locked;
            }
        });
    }

    /// A `pointerdown` or `pointerup`. `button` is `MouseEvent.button`.
    ///
    /// The position is dropped while the pointer is locked, for the reason
    /// [`__crcbl_web_pointer_motion`] drops its `abs`: `clientX`/`clientY` are
    /// frozen at wherever the lock was taken, so a click reported there is a
    /// click somewhere the player is not looking.
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
            position: (!bridge.pointer_locked).then(|| PhysicalPoint::new(x, y)),
            modifiers: modifiers(state),
        });
    }

    /// A `wheel`, in pixels. No position while the pointer is locked, for the
    /// reason [`__crcbl_web_pointer_button`] has none.
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
            position: (!bridge.pointer_locked).then(|| PhysicalPoint::new(x, y)),
            modifiers: modifiers(state),
        });
    }

    /// One contact of a touch gesture: `pointerdown`, `pointermove`,
    /// `pointerup` or `pointercancel` whose `pointerType` is `"touch"`.
    ///
    /// **Every contact, primary or not**, which is the difference this entry
    /// point exists to make: the shim's pointer calls above carry the primary
    /// contact only — a browser's own mouse emulation, and what a game bound to
    /// [`PointerButton`] plays with — and a second finger had nowhere to go
    /// before this. The two streams overlap on the primary contact by design;
    /// [`ShellEvent::Touch`] carries the argument.
    ///
    /// `contact` is the DOM `pointerId`. The browser guarantees exactly what
    /// [`ContactId`] promises — distinct for contacts that are down together,
    /// constant for the life of a gesture, reusable afterwards — so it is
    /// passed through rather than renumbered into ids of our own, which would
    /// be a second identity to keep in step with the browser's.
    ///
    /// `phase` is one of [`TOUCH_BEGAN`], [`TOUCH_MOVED`], [`TOUCH_ENDED`] or
    /// [`TOUCH_CANCELLED`]. Anything else is dropped with a log line rather
    /// than guessed at: an unknown phase mapped onto `Moved` would report a
    /// finger that is still down after it has gone.
    ///
    /// **The pointer lock does not reach this entry point**, unlike
    /// [`__crcbl_web_pointer_motion`] and the two beside it. A contact's
    /// coordinate is the whole content of a touch event — there is no delta to
    /// fall back on — and Pointer Lock is a statement about a *cursor*, which a
    /// finger does not have. The shim keeps the two apart at the source by
    /// letting only a mouse gesture take the lock, so a locked canvas is one
    /// nobody is touching.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub unsafe extern "C" fn __crcbl_web_touch(
        canvas: u32,
        time_ms: f64,
        x: f64,
        y: f64,
        contact: u32,
        phase: u32,
    ) {
        let Some(phase) = touch_phase(phase) else {
            crcbl_core::log::debug!("web shim touch with unknown phase {phase} dropped");
            return;
        };
        queue_for(canvas, |window, bridge| ShellEvent::Touch {
            window,
            device: DeviceId::UNKNOWN,
            time: event_time(bridge, time_ms),
            contact: ContactId(contact),
            phase,
            position: PhysicalPoint::new(x, y),
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
    /// The canvas is the document's fullscreen element, as of the last
    /// [`pump`](Shell::pump). This is the *effective* half of the mode: see
    /// [`Bridge::fullscreen`].
    fullscreen: bool,
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
            fullscreen: false,
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

    /// The mode the canvas is actually presented in.
    ///
    /// [`Borderless`](DisplayMode::Borderless) exactly when the canvas is the
    /// document's fullscreen element — a fullscreen canvas *is* a frameless
    /// surface covering a display with the video mode untouched, which is what
    /// that variant means. `monitor` is `None` because a page cannot choose
    /// one; the browser puts the element on whichever display the window is on,
    /// the same limitation Wayland has and the same reason `None` is documented
    /// as the only honourable value there.
    const fn effective_mode(&self) -> DisplayMode {
        if self.fullscreen {
            DisplayMode::Borderless { monitor: None }
        } else {
            DisplayMode::Windowed
        }
    }

    fn configure(&mut self, size: PhysicalSize, scale_factor: f64) {
        self.configured = Some(WindowConfiguration {
            size,
            scale_factor,
            mode: self.effective_mode(),
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
        ShellCaps::HW_UPSCALE
            | ShellCaps::FRACTIONAL_SCALE
            | ShellCaps::TOUCH
            | ShellCaps::POINTER_LOCK
            | ShellCaps::RAW_POINTER_MOTION
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
        // The lock request the bridge publishes is already clear: a second
        // window is refused above until the first is destroyed, and that is
        // where it is withdrawn.
        self.shared.borrow_mut().window = Some(window);
        Ok(window)
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        self.check(window)?;
        self.window = None;
        self.configured = None;
        self.focused = false;
        self.close_pending = false;
        self.pointer_mode = PointerMode::Free;
        {
            let mut bridge = self.shared.borrow_mut();
            bridge.window = None;
            // The shim's next poll releases the lock, and this is the only
            // place that withdraws the request: a canvas with no window behind
            // it must not keep the pointer pinned, and the frames between a
            // destroy and the next create are frames the shim is polling.
            bridge.pointer_lock_wanted = false;
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
    /// **A request, and this backend cannot grant it itself.** A browser gives
    /// fullscreen only to code running inside a user-gesture handler, and
    /// nothing on the Rust side ever is: the engine reads a key one frame after
    /// the `keydown` that carried it. The page's shim calls `requestFullscreen`
    /// from the gesture and reports the outcome through
    /// [`__crcbl_web_fullscreen`](shim::__crcbl_web_fullscreen), which is what
    /// moves [`WindowConfiguration::mode`] — so a request made here is honoured
    /// a frame or two later on a page whose shim does that, and never on one
    /// that does not. Read [`WindowState::mode_request_honoured`] rather than
    /// the request, exactly as on a compositor that may refuse.
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
        let (events, fullscreen): (Vec<ShellEvent>, bool) = {
            let mut bridge = self.shared.borrow_mut();
            (bridge.queue.drain(..).collect(), bridge.fullscreen)
        };
        // Before the queue, and applied to the *existing* configuration too: a
        // `fullscreenchange` and the resize it causes arrive in either order,
        // and one that arrives second must not leave the mode describing the
        // other. Taking it here rather than in an event handler is also what
        // makes an Escape-driven exit — which produces no key event at all —
        // visible to the frame that follows it.
        if fullscreen != self.fullscreen {
            self.fullscreen = fullscreen;
            let mode = self.effective_mode();
            if let Some(config) = &mut self.configured {
                config.mode = mode;
            }
        }
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

    /// Frees or locks the pointer.
    ///
    /// **A request the shim completes**, like [`set_mode`](Shell::set_mode)
    /// above and for the same reason: `requestPointerLock` is granted only
    /// inside a user gesture. `Ok` here means the mode was recorded and
    /// published for the shim to poll through
    /// [`__crcbl_web_pointer_lock_wanted`](shim::__crcbl_web_pointer_lock_wanted),
    /// not that the pointer is pinned — the player's next click on the canvas
    /// is what does that, and a page with no shim never does it at all.
    /// [`window_state`](Shell::window_state) reports the request, exactly as
    /// [`WindowState::requested_mode`] does; what a consumer can observe of the
    /// *effective* lock is a [`PointerMotion`](ShellEvent::PointerMotion) with
    /// a `raw_delta` and no `abs`.
    ///
    /// Releasing is not gated on anything: `Free` reaches the shim by the same
    /// poll and `exitPointerLock` needs no gesture.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] for [`Confined`](PointerMode::Confined): the
    /// Pointer Lock API pins the pointer or leaves it alone, and there is no
    /// third answer for [`POINTER_CONFINE`](ShellCaps::POINTER_CONFINE) to
    /// name.
    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        self.check(window)?;
        if !self.caps().contains(mode.required_cap()) {
            return Err(Self::unsupported(mode.as_str()));
        }
        self.pointer_mode = mode;
        self.shared.borrow_mut().pointer_lock_wanted = mode == PointerMode::Locked;
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
    use crcbl_core::input::{ButtonState, ContactId, Modifiers, PointerButton, TouchPhase};

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

    /// Where each event says the pointer is — motion, button and wheel alike.
    ///
    /// One reader for all three because the lock's rule is one rule: under it
    /// none of them has a position, and a test that only looked at motion would
    /// pass on a backend that still reported the frozen point on every click.
    fn positions(events: &[ShellEvent]) -> Vec<Option<PhysicalPoint>> {
        events
            .iter()
            .map(|event| match event {
                ShellEvent::PointerMotion { abs, .. } => *abs,
                ShellEvent::Button { position, .. } | ShellEvent::Wheel { position, .. } => {
                    *position
                }
                other => panic!("not a pointer event: {other:?}"),
            })
            .collect()
    }

    /// The relative motion of every [`PointerMotion`](ShellEvent::PointerMotion),
    /// and nothing else — the other two carry none.
    fn raw_deltas(events: &[ShellEvent]) -> Vec<Option<(f64, f64)>> {
        events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::PointerMotion { raw_delta, .. } => Some(*raw_delta),
                _ => None,
            })
            .collect()
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

    /// A key event delivered exactly the way the JS shim delivers one.
    ///
    /// The other key tests build `&str`s in Rust and hand out their pointers,
    /// which no browser can do: JS has no address inside wasm memory except one
    /// an export gave it. This drives the real path — read the scratch address
    /// and its capacity, copy both strings in, pass the two `(ptr, len)` pairs
    /// — and it is the test whose absence let `__crcbl_web_key` be specified
    /// without being callable (P5.8).
    #[test]
    fn the_key_scratch_is_how_a_browser_gets_a_string_into_wasm() {
        let (mut shell, window) = shell_with_window(2);

        let code = "KeyA";
        let key = "a";
        let capacity = shim::__crcbl_web_key_scratch_capacity() as usize;
        assert!(capacity >= code.len() + key.len());
        let base = shim::__crcbl_web_key_scratch_ptr();
        assert!(!base.is_null());
        // The address must not move between calls, or a shim that reads it once
        // — as the sample shim does — writes into freed memory forever after.
        assert_eq!(base, shim::__crcbl_web_key_scratch_ptr());

        // What `new Uint8Array(memory.buffer, base, n).set(bytes)` does.
        // SAFETY: `base` is the start of a `KEY_SCRATCH_CAPACITY`-byte buffer
        // in thread-local storage that lives for the whole thread, and the two
        // strings were just checked to fit in it. Nothing else aliases it: the
        // buffer is only ever reached through this pointer.
        unsafe {
            core::ptr::copy_nonoverlapping(code.as_ptr(), base, code.len());
            core::ptr::copy_nonoverlapping(key.as_ptr(), base.add(code.len()), key.len());
        }

        // SAFETY: both pointers name initialised bytes inside that buffer, and
        // nothing writes to it for the duration of the call — the shim's half
        // of the contract on `borrow_str`.
        unsafe {
            shim::__crcbl_web_key(
                2,
                base,
                code.len() as u32,
                base.add(code.len()),
                key.len() as u32,
                1234.5,
                STATE_EDGE | STATE_SHIFT,
            );
        }

        let events = drain(&mut shell);
        let [
            ShellEvent::Key {
                window: got_window,
                key_code,
                keysym,
                state,
                repeat,
                modifiers,
                ..
            },
        ] = events.as_slice()
        else {
            panic!("expected exactly one key event, got {events:?}");
        };
        assert_eq!(*got_window, window);
        assert_eq!(*key_code, KeyCode::from_name("KeyA"));
        assert_eq!(*keysym, Keysym::from_char('a'));
        assert_eq!(*state, ButtonState::Pressed);
        assert!(!*repeat);
        assert_eq!(*modifiers, Modifiers::SHIFT);
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

    /// **A borderless request is honoured only once the page says it was.**
    ///
    /// The whole of the browser's fullscreen story in one sequence: the request
    /// is recorded and is not yet fact; the shim's `fullscreenchange` makes it
    /// fact; an exit the engine never asked for — Escape, which reaches no key
    /// handler — takes it away again and the window reports the mode it really
    /// has rather than the one that was asked for.
    #[test]
    fn fullscreen_is_the_pages_answer_not_the_engines_request() {
        let (mut shell, window) = shell_with_window(9);
        // SAFETY: the shim ABI; nothing is dereferenced by these entry points.
        unsafe { shim::__crcbl_web_resize(9, 800, 600, 1.0) };
        drain(&mut shell);

        shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("the window is live");
        let state = shell.window_state(window).expect("state");
        assert_eq!(
            state.requested_mode,
            DisplayMode::Borderless { monitor: None }
        );
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Windowed),
            "a request the page has not answered is not a fact",
        );
        assert!(!state.mode_request_honoured());

        // The page granted it, and the canvas grew to the display.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_fullscreen(9, shim::STATE_EDGE) };
        unsafe { shim::__crcbl_web_resize(9, 1920, 1080, 1.0) };
        drain(&mut shell);
        let state = shell.window_state(window).expect("state");
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Borderless { monitor: None })
        );
        assert!(state.mode_request_honoured());
        assert_eq!(state.size(), Some(PhysicalSize::new(1920, 1080)));

        // Escape. No key reaches the engine and no `set_mode` is made, so the
        // request still says borderless while the window is not.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_fullscreen(9, 0) };
        drain(&mut shell);
        let state = shell.window_state(window).expect("state");
        assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
        assert_eq!(
            state.requested_mode,
            DisplayMode::Borderless { monitor: None }
        );
        assert!(
            !state.mode_request_honoured(),
            "a toggle reading the request back would still be showing fullscreen",
        );
    }

    /// The mode survives the two orders a browser can deliver the change in.
    ///
    /// `fullscreenchange` and the `ResizeObserver` callback it causes are two
    /// separate tasks, and nothing specifies which runs first. Reading the flag
    /// inside the resize handler would be right in one order and wrong in the
    /// other; `pump` takes it before the queue for exactly that reason.
    #[test]
    fn the_mode_is_right_whichever_order_the_resize_and_the_change_arrive_in() {
        for resize_first in [true, false] {
            let (mut shell, window) = shell_with_window(11);
            // SAFETY: the shim ABI.
            unsafe { shim::__crcbl_web_resize(11, 800, 600, 1.0) };
            drain(&mut shell);

            if resize_first {
                // SAFETY: the shim ABI.
                unsafe { shim::__crcbl_web_resize(11, 1920, 1080, 1.0) };
                unsafe { shim::__crcbl_web_fullscreen(11, shim::STATE_EDGE) };
            } else {
                // SAFETY: the shim ABI.
                unsafe { shim::__crcbl_web_fullscreen(11, shim::STATE_EDGE) };
                unsafe { shim::__crcbl_web_resize(11, 1920, 1080, 1.0) };
            }
            drain(&mut shell);

            let state = shell.window_state(window).expect("state");
            assert_eq!(
                state.effective_mode(),
                Some(DisplayMode::Borderless { monitor: None }),
                "resize_first = {resize_first}",
            );
            assert_eq!(state.size(), Some(PhysicalSize::new(1920, 1080)));
        }
    }

    #[test]
    fn caps_and_behaviour_agree() {
        let (mut shell, window) = shell_with_window(0);
        let caps = shell.caps();
        assert_eq!(
            caps,
            ShellCaps::HW_UPSCALE
                | ShellCaps::FRACTIONAL_SCALE
                | ShellCaps::TOUCH
                | ShellCaps::POINTER_LOCK
                | ShellCaps::RAW_POINTER_MOTION,
            "every other bit names something this backend does not do"
        );
        // The three that used to be advertised with nothing behind them.
        assert!(!caps.contains(ShellCaps::TEXT_IME));
        assert!(!caps.contains(ShellCaps::WINDOW_POSITION));
        assert!(!caps.contains(ShellCaps::SERVER_DECORATIONS));

        // And the two that are set are set because there is something behind
        // them, which is the whole point of this test: the lock is asked for
        // through an entry point the shim polls, and while the shim reports it
        // granted a motion carries the delta the camera reads and no position.
        assert!(caps.has_mouselook());
        assert_eq!(shim::__crcbl_web_pointer_lock_wanted(0), 0);
        shell
            .set_pointer_mode(window, PointerMode::Locked)
            .expect("the lock is asked for, not refused");
        assert_eq!(
            shim::__crcbl_web_pointer_lock_wanted(0),
            1,
            "the request has to reach the shim, or no gesture can complete it"
        );
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_lock(0, STATE_EDGE) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 1.0, 640.0, 360.0, 3.0, -4.0) };
        let events = drain(&mut shell);
        let [ShellEvent::PointerMotion { abs, raw_delta, .. }] = events.as_slice() else {
            panic!("expected one PointerMotion, got {events:?}");
        };
        assert_eq!(*raw_delta, Some((3.0, -4.0)));
        assert_eq!(*abs, None, "there is no position under a lock");

        // Confine is the one pointer mode a browser has no answer for, and it
        // still says so.
        assert!(!caps.contains(ShellCaps::POINTER_CONFINE));
        assert!(matches!(
            shell.set_pointer_mode(window, PointerMode::Confined),
            Err(ShellError::Unsupported { .. })
        ));
    }

    /// The lock's two halves are separate, and only the *effective* one shapes
    /// an event.
    ///
    /// A request nobody has clicked on yet is exactly the state a page is in
    /// between `set_pointer_mode` and the player's next gesture, and reporting a
    /// delta there would be a camera turning while the cursor is still visible.
    /// Releasing has to put the position back, or a menu opened after a
    /// firefight has nothing to hit-test.
    #[test]
    fn a_lock_nobody_granted_is_not_a_lock() {
        let (mut shell, window) = shell_with_window(0);
        shell
            .set_pointer_mode(window, PointerMode::Locked)
            .expect("recorded");
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 1.0, 10.0, 20.0, 5.0, 5.0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_button(0, 2.0, 10.0, 20.0, 0, STATE_EDGE) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_wheel(0, 3.0, 10.0, 20.0, 0.0, -1.0, 0) };
        let events = drain(&mut shell);
        let expected = Some(PhysicalPoint::new(10.0, 20.0));
        assert_eq!(
            positions(&events),
            vec![expected, expected, expected],
            "the browser has not locked anything yet"
        );
        assert_eq!(raw_deltas(&events), vec![None]);

        // Granted, and every one of the three loses its position.
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_lock(0, STATE_EDGE) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 4.0, 10.0, 20.0, 5.0, 5.0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_button(0, 5.0, 10.0, 20.0, 0, STATE_EDGE) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_wheel(0, 6.0, 10.0, 20.0, 0.0, -1.0, 0) };
        let events = drain(&mut shell);
        assert_eq!(positions(&events), vec![None, None, None]);
        assert_eq!(raw_deltas(&events), vec![Some((5.0, 5.0))]);

        // And released — by Escape, which reaches nothing else — it all comes
        // back.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("recorded");
        assert_eq!(shim::__crcbl_web_pointer_lock_wanted(0), 0);
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_lock(0, 0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 7.0, 11.0, 21.0, 5.0, 5.0) };
        let events = drain(&mut shell);
        assert_eq!(
            positions(&events),
            vec![Some(PhysicalPoint::new(11.0, 21.0))]
        );
        assert_eq!(raw_deltas(&events), vec![None]);
    }

    /// A canvas with no window behind it must not keep the pointer pinned.
    #[test]
    fn destroying_the_window_withdraws_the_lock_request() {
        let (mut shell, window) = shell_with_window(0);
        shell
            .set_pointer_mode(window, PointerMode::Locked)
            .expect("recorded");
        assert_eq!(shim::__crcbl_web_pointer_lock_wanted(0), 1);
        shell.destroy_window(window).expect("destroy");
        assert_eq!(shim::__crcbl_web_pointer_lock_wanted(0), 0);
        // And a shim talking about some other canvas is answered about that
        // one, not about this shell's request.
        shell
            .create_window(&WindowDesc::default())
            .expect("the canvas is still there");
        assert_eq!(shim::__crcbl_web_pointer_lock_wanted(9), 0);
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
        unsafe { shim::__crcbl_web_pointer_motion(0, 4.0, 10.0, 20.0, 1.0, 0.0) };
        // SAFETY: the shim ABI.
        unsafe { shim::__crcbl_web_pointer_motion(0, 12.0, 11.0, 21.0, 1.0, 1.0) };
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

    /// Two fingers reach the queue as two contacts, and a lift ends the right
    /// one.
    ///
    /// The shim's job in one sequence: both contacts arrive, they keep their own
    /// ids and their own positions, moving one leaves the other where it was,
    /// and the phases are the ones the DOM event types mean. Before this entry
    /// point the second contact was dropped in JS and there was nothing here to
    /// test.
    #[test]
    fn every_contact_reaches_the_queue_with_its_own_id_and_place() {
        use shim::{TOUCH_BEGAN, TOUCH_CANCELLED, TOUCH_ENDED, TOUCH_MOVED};
        let (mut shell, window) = shell_with_window(0);
        // SAFETY: the shim ABI; nothing is dereferenced by this entry point.
        unsafe {
            shim::__crcbl_web_touch(0, 1.0, 10.0, 20.0, 3, TOUCH_BEGAN);
            shim::__crcbl_web_touch(0, 2.0, 300.0, 400.0, 4, TOUCH_BEGAN);
            shim::__crcbl_web_touch(0, 3.0, 310.0, 405.0, 4, TOUCH_MOVED);
            shim::__crcbl_web_touch(0, 4.0, 10.0, 20.0, 3, TOUCH_ENDED);
            shim::__crcbl_web_touch(0, 5.0, 310.0, 405.0, 4, TOUCH_CANCELLED);
            // A phase this ABI does not name is dropped, not guessed at.
            shim::__crcbl_web_touch(0, 6.0, 0.0, 0.0, 5, 99);
        }

        let events = drain(&mut shell);
        let contacts: Vec<(WindowId, ContactId, TouchPhase, PhysicalPoint)> = events
            .iter()
            .map(|event| {
                let ShellEvent::Touch {
                    window,
                    contact,
                    phase,
                    position,
                    ..
                } = event
                else {
                    panic!("expected only touch events, got {events:?}");
                };
                (*window, *contact, *phase, *position)
            })
            .collect();
        assert_eq!(
            contacts,
            vec![
                (
                    window,
                    ContactId(3),
                    TouchPhase::Began,
                    PhysicalPoint::new(10.0, 20.0)
                ),
                (
                    window,
                    ContactId(4),
                    TouchPhase::Began,
                    PhysicalPoint::new(300.0, 400.0)
                ),
                (
                    window,
                    ContactId(4),
                    TouchPhase::Moved,
                    PhysicalPoint::new(310.0, 405.0)
                ),
                (
                    window,
                    ContactId(3),
                    TouchPhase::Ended,
                    PhysicalPoint::new(10.0, 20.0)
                ),
                (
                    window,
                    ContactId(4),
                    TouchPhase::Cancelled,
                    PhysicalPoint::new(310.0, 405.0)
                ),
            ]
        );
        // Each contact carries the DOM event's own timestamp, like every other
        // input this backend queues.
        assert_eq!(events[2].time(), Some(EventTime::from_millis(3)));
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
        assert!(
            shell
                .set_pointer_mode(window, PointerMode::Confined)
                .is_err()
        );
        assert!(shell.clipboard_offer(window, &[]).is_err());
        assert!(
            shell
                .clipboard_request(window, crate::MimeType::TextUtf8)
                .is_err()
        );
        assert!(!shell.clipboard_readable(window));
        // Free needs no capability, and the lock is the one this backend grew.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("free is always available");
        shell
            .set_pointer_mode(window, PointerMode::Locked)
            .expect("the Pointer Lock API is what POINTER_LOCK names");
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
