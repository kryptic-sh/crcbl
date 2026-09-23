//! The `__crcbl_web_pad_*` entry points `web/engine/gamepad.js` calls, and
//! the one frame of reports behind them.
//!
//! Exports-plus-polling, like every other ABI a page drives (see
//! `crcbl::web`): the shim calls in, writes into buffers wasm owns, and is
//! never called back. **Once per `requestAnimationFrame`, before the demo's
//! frame**, it calls [`__crcbl_web_pad_begin`], then for every non-null entry
//! of `navigator.getGamepads()`:
//!
//! 1. writes every standard button's `value` and then every axis into
//!    [`__crcbl_web_pad_values_ptr`], as `f32`s, [`VALUES`] of
//!    them — a pad with fewer writes zeros for the rest;
//! 2. writes `Gamepad.id` as UTF-8 into [`__crcbl_web_pad_id_ptr`] if it fits
//!    in [`__crcbl_web_pad_id_capacity`] bytes, and nothing if it does not —
//!    half a UTF-8 string is a different string, and the id only names the
//!    pad's family;
//! 3. calls [`__crcbl_web_pad`] with the pad's `index`, its flags, the
//!    `pressed` bits and the id's length.
//!
//! [`WebGamepads::poll`](super::WebGamepads::poll) then takes that frame. A
//! frame the shim reported nothing for — no `begin` since the last poll — is
//! not "no pads": the poll changes nothing, so a skipped pump never reads as
//! every pad unplugging.
//!
//! The scratch buffers are thread-locals and never move, so the shim may read
//! their addresses once; it must build a fresh typed-array view on
//! `memory.buffer` for every write, because a `memory.grow()` detaches the
//! old one.

use std::cell::{Cell, RefCell};

use super::{ID_CAPACITY, Report, Source, VALUES};

/// [`__crcbl_web_pad`]'s `flags`: `Gamepad.connected`.
pub const PAD_CONNECTED: u32 = 1 << 0;
/// [`__crcbl_web_pad`]'s `flags`: `Gamepad.mapping === "standard"`.
pub const PAD_STANDARD: u32 = 1 << 1;

/// The reports since the last `begin`, and whether there was a `begin` since
/// the last poll.
#[derive(Debug)]
struct Frame {
    reported: bool,
    pads: Vec<Report>,
}

thread_local! {
    /// Where the shim writes one pad's button values and axes.
    static VALUE_SCRATCH: Cell<[f32; VALUES]> = const { Cell::new([0.0; VALUES]) };
    /// Where it writes that pad's `Gamepad.id`.
    static ID_SCRATCH: Cell<[u8; ID_CAPACITY]> = const { Cell::new([0; ID_CAPACITY]) };
    /// This frame's reports.
    static FRAME: RefCell<Frame> = const {
        RefCell::new(Frame {
            reported: false,
            pads: Vec::new(),
        })
    };
}

/// The address of the value scratch buffer: [`VALUES`] `f32`s,
/// every standard button's `value` in order and then every standard axis.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn __crcbl_web_pad_values_ptr() -> *mut f32 {
    VALUE_SCRATCH.with(|slot| slot.as_ptr().cast::<f32>())
}

/// How many `f32`s the value scratch holds. Read it rather than assuming the
/// constant, as the key scratch's capacity is read.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn __crcbl_web_pad_values_capacity() -> u32 {
    u32::try_from(VALUES).unwrap_or(u32::MAX)
}

/// The address of the id scratch buffer.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn __crcbl_web_pad_id_ptr() -> *mut u8 {
    ID_SCRATCH.with(|slot| slot.as_ptr().cast::<u8>())
}

/// How many bytes the id scratch holds: [`ID_CAPACITY`].
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn __crcbl_web_pad_id_capacity() -> u32 {
    u32::try_from(ID_CAPACITY).unwrap_or(u32::MAX)
}

/// A new frame of reports: forgets the last frame's, and marks this one as
/// reported even if no pad follows — which is how "no pads" is said.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn __crcbl_web_pad_begin() {
    FRAME.with_borrow_mut(|frame| {
        frame.reported = true;
        frame.pads.clear();
    });
}

/// One entry of `navigator.getGamepads()`, whose values and id the shim has
/// just written into the scratch buffers.
///
/// `flags` is [`PAD_CONNECTED`] and [`PAD_STANDARD`]; `pressed` holds
/// `buttons[i].pressed` in bit `i`; `id_len` is how many id bytes were
/// written, and is clamped to the buffer. Reads only buffers wasm owns, so
/// there is no pointer to trust. A report with no `begin` before it in this
/// frame joins the next frame's `begin`, which forgets it.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn __crcbl_web_pad(index: u32, flags: u32, pressed: u32, id_len: u32) {
    let values = VALUE_SCRATCH.with(Cell::get);
    let id = ID_SCRATCH.with(Cell::get);
    let id_len = usize::try_from(id_len).map_or(ID_CAPACITY, |len| len.min(ID_CAPACITY));
    FRAME.with_borrow_mut(|frame| {
        frame.pads.push(Report {
            index,
            connected: flags & PAD_CONNECTED != 0,
            standard: flags & PAD_STANDARD != 0,
            pressed,
            values,
            id,
            id_len,
        });
    });
}

/// The frame the entry points above filled, as the poller's source.
#[derive(Debug)]
pub(super) struct Bridge;

impl Source for Bridge {
    fn take(&mut self, into: &mut Vec<Report>) -> bool {
        FRAME.with_borrow_mut(|frame| {
            if !frame.reported {
                return false;
            }
            frame.reported = false;
            into.clear();
            std::mem::swap(into, &mut frame.pads);
            true
        })
    }
}
