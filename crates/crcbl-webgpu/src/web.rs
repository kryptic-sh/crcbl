//! The transport: where the encoded stream lives, and how JS takes it.
//!
//! [`writer`](crate::writer) and [`tag`](crate::tag) say what the bytes *mean*;
//! this module
//! says how they cross. It is the wasm → JS direction and only that: wasm
//! appends commands into a buffer it owns, and once a frame the shim reads that
//! buffer in place, decodes it, and says it is done. **There is no reply channel
//! here** — the JS → wasm half arrives with the first call that needs an answer,
//! and until then nothing on this seam waits for one.
//!
//! Every export is a plain integer in and a plain integer out, wasm owns the
//! memory, and JS never passes a pointer in. That is the convention
//! `crcbl-store`'s fetch ABI, the OPFS entry points and `crcbl-shell`'s key
//! scratch already use; see `docs/plan/41-webgpu-stream.md` for why the stream
//! follows it rather than inventing a second one.
//!
//! # Exports
//!
//! Symbols are `#[unsafe(no_mangle)]` **only** on `wasm32`; a native build
//! exports none of them. The functions themselves are compiled everywhere, which
//! is what makes the lifecycle below testable without a browser.
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_gpu_stream_len`](shim::__crcbl_web_gpu_stream_len) | `() -> i32` | Bytes of encoded stream waiting, header included, or `0` when there is no channel to ask. The **readiness** test. |
//! | [`__crcbl_web_gpu_stream_ptr`](shim::__crcbl_web_gpu_stream_ptr) | `() -> i32` | Where those bytes start, or `0` when there is no channel to ask. |
//! | [`__crcbl_web_gpu_stream_release`](shim::__crcbl_web_gpu_stream_release) | `() -> i32` | The shim has finished with the buffer: drop the commands and begin the next frame's. `1`, or `0` when there is no channel to ask. |
//!
//! `len` is the readiness test rather than `ptr` because an installed channel
//! always holds at least a header — [`HEADER_BYTES`](crate::tag::HEADER_BYTES) —
//! so `0` there means
//! "nothing to ask" and nothing else. A frame that encoded no commands answers
//! [`HEADER_BYTES`](crate::tag::HEADER_BYTES) and decodes to an empty list, which
//! is a different fact
//! and stays a different number.
//!
//! ## Buffer ownership
//!
//! The buffer belongs to wasm; JS never passes a pointer in and never writes
//! into it.
//!
//! - It is **written by wasm** — by whatever encodes through
//!   [`StreamChannel::encode`] — and **read by JS**, between the frame that
//!   encoded it and the [`release`](shim::__crcbl_web_gpu_stream_release) that
//!   ends it.
//! - It is valid from the moment the frame export returns until that `release`.
//!   Afterwards the bytes are gone: `release` rewrites the header in place, so a
//!   view built before it still points at live memory and no longer describes
//!   the frame it described. **Decode before releasing, never after.**
//! - Nothing is copied out for JS. [`StreamWriter`] keeps one `Vec` for the life
//!   of the channel and `release` clears it without freeing, so the per-frame
//!   copy is one JS chooses to make or not.
//!
//! **The detached-view trap, and the answer for this ABI.** A `Uint8Array` over
//! `memory.buffer` is detached the moment wasm memory grows, and reading through
//! a detached view throws. **None of the three exports above can grow it**: two
//! only read a `Vec`'s pointer and length, and `release` clears that `Vec`, which
//! keeps its allocation. So the address is stable across every call in this
//! table, and JS may hold one view for the whole decode.
//!
//! What *does* move it is **encoding** — appending a command can reallocate the
//! buffer, and that allocation can grow wasm memory — and encoding happens inside
//! the engine's own per-frame export, which is not one of these three. The rule
//! that follows is therefore narrow and exact: build the view after the frame
//! export has returned, and drop it before calling back into wasm for the next
//! frame. `web/engine/wasm.js` enforces the general form of this by never
//! exporting a view at all.
//!
//! ## Call ordering
//!
//! 1. The app constructs a [`StreamChannel`] and calls [`install`] — before the
//!    shim's first call. Until then **every export answers `0`**, which is a shim
//!    that started before the engine did rather than a failure to report.
//! 2. Per frame, the engine encodes through [`StreamChannel::encode`].
//! 3. Per frame, after that: `len()` — `0` means there is nothing to do — then
//!    `ptr()`, then decode, then `release()`.
//! 4. `release` is the obligation, not the optional path. A shim that decodes and
//!    does not release leaves the frame's commands in the buffer, and the next
//!    frame appends to them. The stream stays correct — sequence numbers are
//!    monotonic across frames and the header still names the first command — but
//!    the buffer only grows. **A `len` that never falls is a shim that is not
//!    releasing.**
//!
//! Sequence numbers survive a `release`, because [`StreamWriter::clear`] keeps
//! the counter where it is. They have to: an error raised by a replayed command
//! surfaces a frame or more after the frame that encoded it, so a counter that
//! restarted each frame would name several different commands with the same
//! number.
//!
//! ## Failure behaviour
//!
//! | Situation | What the shim does | What the engine sees |
//! | --- | --- | --- |
//! | no channel installed (`len` is `0`) | nothing this frame; ask again next | nothing; the engine has not installed one yet |
//! | the channel was dropped mid-run | as above — every export goes back to `0` | nothing; the exports hold a [`Weak`] and stop finding it |
//! | decoding throws | release anyway, then report | the next frame starts from a clean buffer instead of the shim re-throwing on the same bytes forever |
//! | the shim stops releasing | — | `len` climbs and never falls |
//!
//! Nothing here can fail the way a fetch can: there is no slot to leak, no status
//! to report and no error to hand back. A stream that will not decode is a bug in
//! one of the two hand-written halves of the *format*, not a condition this ABI
//! models — which is what `crates/crcbl-webgpu/tests/fixture.rs` and
//! `web/tools/stream-decode.mjs` are for.
//!
//! ## Worked shim
//!
//! ```js
//! import { decodeStream } from './gpu-stream.js';
//!
//! function replayFrame(ex, memory) {
//!   const len = ex.__crcbl_web_gpu_stream_len();
//!   if (len === 0) return;                  // the engine has not installed one
//!   const ptr = ex.__crcbl_web_gpu_stream_ptr();
//!   try {
//!     // Neither call above allocates, so this view cannot already be detached,
//!     // and nothing below calls back into wasm before it is finished with.
//!     replay(decodeStream(new Uint8Array(memory.buffer, ptr, len)));
//!   } finally {
//!     ex.__crcbl_web_gpu_stream_release();  // even after a throw
//!   }
//! }
//! ```
//!
//! `web/engine/gpu-transport.js` is that shim written out, and
//! `web/tools/stream-transport.mjs` drives it against a synthetic
//! `WebAssembly.Memory` holding the committed fixture.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::StreamWriter;

/// The buffer wasm encodes into and JS drains, once a frame.
///
/// A [`StreamWriter`] plus the interior mutability the entry points need, and
/// nothing else: the transport owns *when* the bytes cross, never what they say.
///
/// The app holds the [`Rc`]; [`install`] keeps only a [`Weak`], so dropping the
/// app's handle puts every export back to `0` with no `uninstall` to remember.
///
/// ```
/// use std::rc::Rc;
/// use crcbl_webgpu::tag;
/// use crcbl_webgpu::web::{StreamChannel, install, shim};
///
/// let channel = Rc::new(StreamChannel::new());
/// assert!(install(&channel));
///
/// // A frame's worth of recording.
/// assert_eq!(channel.encode(|stream| stream.draw(0..3, 0..1)), Some(0));
///
/// // What the shim reads.
/// let len = shim::__crcbl_web_gpu_stream_len() as usize;
/// assert!(len > tag::HEADER_BYTES, "the header plus one command");
/// assert!(!shim::__crcbl_web_gpu_stream_ptr().is_null());
///
/// // …and hands back.
/// assert_eq!(shim::__crcbl_web_gpu_stream_release(), 1);
/// assert_eq!(shim::__crcbl_web_gpu_stream_len() as usize, tag::HEADER_BYTES);
/// ```
#[derive(Debug, Default)]
pub struct StreamChannel {
    writer: RefCell<StreamWriter>,
}

impl StreamChannel {
    /// A channel holding one empty frame — a header and no commands.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `f` against the frame being recorded, and returns what it returned.
    ///
    /// The one door onto the buffer: an encoder reaches the stream through this,
    /// and the entry points below do nothing but publish and clear what it
    /// leaves.
    ///
    /// `None` if the buffer is already borrowed — which means `f` was reached
    /// from inside another `encode` on the same channel, and is a bug in the
    /// caller rather than something the shim can cause. Answering rather than
    /// panicking for the reason the entry points do: a panic inside a browser
    /// callback is an aborted page.
    pub fn encode<R>(&self, f: impl FnOnce(&mut StreamWriter) -> R) -> Option<R> {
        let mut writer = self.writer.try_borrow_mut().ok()?;
        Some(f(&mut writer))
    }
}

// ---------------------------------------------------------------------------
// The installed channel
// ---------------------------------------------------------------------------

thread_local! {
    /// The channel the `extern "C"` entry points reach.
    ///
    /// A [`Weak`] rather than an owning handle, for the reason `crcbl-store`'s
    /// fetch ABI keeps one: the app holds the [`Rc`] and encodes through it, and
    /// the entry points only need to find it. When the app drops it, the exports
    /// answer `0` again by themselves with no `uninstall` to forget.
    ///
    /// Thread-local because whichever thread the engine runs on is the one the
    /// shim calls, and a second one must not see the first's channel.
    static CHANNEL: RefCell<Weak<StreamChannel>> = const { RefCell::new(Weak::new()) };
}

/// Make `channel` the one the `__crcbl_web_gpu_*` entry points reach.
///
/// Returns `false` if a live channel is already installed — replacing one would
/// strand a frame the shim is part-way through reading, and would restart the
/// sequence counter that error attribution is keyed on. Call it before the
/// shim's first call; until then every export answers `0`.
pub fn install(channel: &Rc<StreamChannel>) -> bool {
    CHANNEL.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return false;
        };
        if slot.strong_count() > 0 {
            return false;
        }
        *slot = Rc::downgrade(channel);
        true
    })
}

/// Forget the installed channel, returning whether there was a live one.
///
/// Rarely needed — dropping the app's [`Rc`] has the same effect — but a test
/// that installs a channel must not leak the installation into the next one.
pub fn uninstall() -> bool {
    CHANNEL.with(|slot| match slot.try_borrow_mut() {
        Ok(mut slot) => {
            let had = slot.strong_count() > 0;
            *slot = Weak::new();
            had
        }
        Err(_) => false,
    })
}

/// Whether a live channel is installed on this thread.
#[must_use]
pub fn is_installed() -> bool {
    CHANNEL.with(|slot| slot.try_borrow().is_ok_and(|slot| slot.strong_count() > 0))
}

/// Run `f` against the installed channel's buffer, or answer `absent`.
///
/// `try_borrow` at both levels rather than `borrow`: an entry point reached
/// re-entrantly — from inside an [`encode`](StreamChannel::encode), or from
/// something the engine triggered — answers "no channel", which every caller
/// already handles, rather than panicking inside a browser callback where a
/// panic is an aborted page.
fn with_stream<R>(absent: R, f: impl FnOnce(&StreamWriter) -> R) -> R {
    with_channel(absent, |channel, absent| {
        match channel.writer.try_borrow() {
            Ok(writer) => f(&writer),
            Err(_) => absent,
        }
    })
}

/// [`with_stream`], for the one entry point that changes the buffer.
fn with_stream_mut<R>(absent: R, f: impl FnOnce(&mut StreamWriter) -> R) -> R {
    with_channel(absent, |channel, absent| {
        match channel.writer.try_borrow_mut() {
            Ok(mut writer) => f(&mut writer),
            Err(_) => absent,
        }
    })
}

/// The thread-local half of the two above: find the channel, or answer `absent`.
fn with_channel<R>(absent: R, f: impl FnOnce(&StreamChannel, R) -> R) -> R {
    CHANNEL.with(|slot| match slot.try_borrow() {
        Ok(slot) => match slot.upgrade() {
            Some(channel) => f(&channel, absent),
            None => absent,
        },
        Err(_) => absent,
    })
}

// ---------------------------------------------------------------------------
// extern "C" entry points — called from JS
// ---------------------------------------------------------------------------

/// The JS→wasm ABI. See the [module docs](self) for the whole contract.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`. None of these functions is `unsafe`:
/// none dereferences a pointer the caller supplied. One *returns* an address, and
/// what JS may do with it is the module docs' buffer-ownership section.
pub mod shim {
    use super::{with_stream, with_stream_mut};

    /// Bytes of encoded stream waiting, header included.
    ///
    /// `0` when there is no channel to ask, and only then: an installed channel
    /// always holds at least [`tag::HEADER_BYTES`](crate::tag::HEADER_BYTES), so
    /// a frame that encoded nothing is a different number from a shim that
    /// started before the engine did.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_stream_len() -> u32 {
        with_stream(0, |stream| {
            u32::try_from(stream.bytes().len()).unwrap_or(u32::MAX)
        })
    }

    /// The address of those bytes, or `0` when there is no channel to ask.
    ///
    /// **Does not allocate**, and neither does anything else in this module, so
    /// the address holds for as long as nothing encodes — see the module docs on
    /// the detached-view trap. Valid until
    /// [`__crcbl_web_gpu_stream_release`], which does not move the buffer but
    /// does overwrite what is in it.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_stream_ptr() -> *const u8 {
        with_stream(core::ptr::null(), |stream| stream.bytes().as_ptr())
    }

    /// Drop the frame the shim has finished decoding and begin the next one.
    ///
    /// `1` if there was a channel, `0` if there was not. The buffer keeps its
    /// allocation and the sequence counter keeps its place, so the next frame
    /// costs no allocation and its commands are still named by numbers no earlier
    /// frame used.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_stream_release() -> u32 {
        with_stream_mut(0, |stream| {
            stream.clear();
            1
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, StreamReader, decode_stream, tag};

    /// The entry points share one thread-local, so the tests that touch it run
    /// under a mutex and clean up after themselves. `nextest` runs each test in
    /// its own process, but `cargo test` does not.
    static SHIM: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn shim_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = SHIM.lock().unwrap_or_else(|e| e.into_inner());
        uninstall();
        guard
    }

    /// The bytes the shim would decode, read exactly as it reads them: the
    /// length first, then the pointer, then a window on the two together.
    fn published() -> Vec<u8> {
        let len = shim::__crcbl_web_gpu_stream_len() as usize;
        let ptr = shim::__crcbl_web_gpu_stream_ptr();
        assert!(!ptr.is_null(), "a length with no pointer");
        // SAFETY: `ptr` and `len` describe the installed channel's buffer, which
        // lives until the channel is dropped; nothing encodes or releases
        // between the two calls above and this read. This is the shim's
        // `new Uint8Array(memory.buffer, ptr, len)`.
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    }

    /// Encodes one draw, and returns the sequence number it was given.
    fn draw(channel: &StreamChannel, vertices: core::ops::Range<u32>) -> u64 {
        channel
            .encode(|stream| stream.draw(vertices, 0..1))
            .expect("nothing else holds the channel")
    }

    #[test]
    fn the_entry_points_answer_zero_until_a_channel_is_installed() {
        let _guard = shim_guard();
        assert!(!is_installed());
        assert_eq!(shim::__crcbl_web_gpu_stream_len(), 0);
        assert!(shim::__crcbl_web_gpu_stream_ptr().is_null());
        assert_eq!(shim::__crcbl_web_gpu_stream_release(), 0);
    }

    /// **A frame is published as bytes the shim can decode, and released.**
    ///
    /// The whole wasm → JS direction in one test: encode, read the two numbers
    /// the shim reads, decode what they describe, hand the buffer back.
    #[test]
    fn a_frame_is_published_where_the_shim_can_decode_it() {
        let _guard = shim_guard();
        let channel = Rc::new(StreamChannel::new());
        assert!(install(&channel));
        assert!(!install(&channel), "a second install must not replace it");
        assert!(is_installed());

        // An installed channel with nothing encoded is a header, not nothing:
        // "no commands this frame" and "no engine yet" are different facts and
        // the shim tells them apart by this number alone.
        assert_eq!(
            shim::__crcbl_web_gpu_stream_len() as usize,
            tag::HEADER_BYTES
        );
        assert_eq!(decode_stream(&published()), Ok(Vec::new()));

        let sequences = channel
            .encode(|stream| (stream.draw(0..3, 0..1), stream.begin_debug_label("pass")))
            .expect("nothing else holds the channel");
        assert_eq!(sequences, (0, 1));

        assert_eq!(
            decode_stream(&published()),
            Ok(vec![
                Command::Draw {
                    vertices: 0..3,
                    instances: 0..1,
                },
                Command::BeginDebugLabel {
                    label: "pass".into(),
                },
            ])
        );

        assert_eq!(shim::__crcbl_web_gpu_stream_release(), 1);
        assert_eq!(
            shim::__crcbl_web_gpu_stream_len() as usize,
            tag::HEADER_BYTES,
            "the released frame's commands are gone"
        );
        assert_eq!(decode_stream(&published()), Ok(Vec::new()));
    }

    /// **`release` does not move the buffer, and does not restart the counter.**
    ///
    /// The address is the detached-view question a JS author has to get right,
    /// and the counter is what error attribution is keyed on. Both are properties
    /// of `Vec::clear` plus [`StreamWriter::clear`] that nothing else in the
    /// suite would notice breaking.
    #[test]
    fn a_release_keeps_the_address_and_the_sequence_counter() {
        let _guard = shim_guard();
        let channel = Rc::new(StreamChannel::new());
        assert!(install(&channel));

        draw(&channel, 0..3);
        let before = shim::__crcbl_web_gpu_stream_ptr();
        assert_eq!(shim::__crcbl_web_gpu_stream_release(), 1);
        assert_eq!(
            shim::__crcbl_web_gpu_stream_ptr(),
            before,
            "release moved the buffer, so a JS view built over it would be stale"
        );

        // The next frame's first command carries the number after the last one,
        // and the header says so — which is where the shim reads it from.
        assert_eq!(draw(&channel, 0..6), 1);
        let next_frame = published();
        let reader = StreamReader::new(&next_frame).expect("a stream this crate wrote");
        assert_eq!(reader.base_sequence(), 1);
    }

    /// **A frame the shim never releases is kept, not dropped.**
    ///
    /// The documented degradation: a shim that stops draining batches rather than
    /// losing commands, and a climbing `len` is how that is visible at all.
    #[test]
    fn a_frame_that_is_never_released_batches_into_the_next() {
        let _guard = shim_guard();
        let channel = Rc::new(StreamChannel::new());
        assert!(install(&channel));

        draw(&channel, 0..3);
        let after_one = shim::__crcbl_web_gpu_stream_len();
        draw(&channel, 0..6);
        assert!(
            shim::__crcbl_web_gpu_stream_len() > after_one,
            "a length that never climbs would hide a shim that stopped releasing"
        );
        assert_eq!(
            decode_stream(&published()),
            Ok(vec![
                Command::Draw {
                    vertices: 0..3,
                    instances: 0..1,
                },
                Command::Draw {
                    vertices: 0..6,
                    instances: 0..1,
                },
            ]),
            "an unreleased frame must keep its commands, not lose them"
        );
    }

    /// **A dropped channel puts every export back to zero.**
    ///
    /// The exports hold a [`Weak`], so this is the whole of the teardown story:
    /// there is no id to alias and no state to strand.
    #[test]
    fn a_dropped_channel_answers_zero_again_with_no_uninstall() {
        let _guard = shim_guard();
        let channel = Rc::new(StreamChannel::new());
        assert!(install(&channel));
        draw(&channel, 0..3);
        assert_ne!(shim::__crcbl_web_gpu_stream_len(), 0);

        drop(channel);
        assert!(!is_installed());
        assert_eq!(shim::__crcbl_web_gpu_stream_len(), 0);
        assert!(shim::__crcbl_web_gpu_stream_ptr().is_null());
        assert_eq!(shim::__crcbl_web_gpu_stream_release(), 0);

        // …and the slot is free for the next one, which starts its own counter.
        let next = Rc::new(StreamChannel::new());
        assert!(install(&next));
        assert_eq!(
            shim::__crcbl_web_gpu_stream_len() as usize,
            tag::HEADER_BYTES
        );
    }

    /// **`uninstall` reports whether there was anything to forget.**
    ///
    /// Only a test needs it — dropping the [`Rc`] is the real path — but a test
    /// that leaked its installation into the next one would make the
    /// "answers zero until installed" case above pass for the wrong reason.
    #[test]
    fn uninstall_forgets_a_live_channel_once() {
        let _guard = shim_guard();
        let channel = Rc::new(StreamChannel::new());
        assert!(install(&channel));
        assert!(uninstall());
        assert!(!uninstall(), "there was nothing left to forget");
        assert!(!is_installed());
        assert_eq!(shim::__crcbl_web_gpu_stream_len(), 0);

        // The channel is still alive and still usable; it is simply not the one
        // the entry points reach any more.
        assert_eq!(draw(&channel, 0..3), 0);
    }

    /// **Nothing on this seam panics when it is re-entered.**
    ///
    /// A panic inside a browser callback aborts the page, which is the one thing
    /// the entry points must never do. The `encode` here stands in for the frame
    /// the engine would be part-way through.
    #[test]
    fn a_reentrant_call_answers_zero_rather_than_panicking() {
        let _guard = shim_guard();
        let channel = Rc::new(StreamChannel::new());
        assert!(install(&channel));

        let inner = channel.encode(|stream| {
            stream.draw(0..3, 0..1);
            (
                channel.encode(|stream| stream.draw(0..6, 0..2)),
                shim::__crcbl_web_gpu_stream_len(),
                shim::__crcbl_web_gpu_stream_ptr(),
                shim::__crcbl_web_gpu_stream_release(),
            )
        });
        assert_eq!(inner, Some((None, 0, core::ptr::null(), 0)));

        // The frame it was holding is untouched: nothing above cleared it.
        assert_eq!(
            decode_stream(&published()),
            Ok(vec![Command::Draw {
                vertices: 0..3,
                instances: 0..1,
            }])
        );
    }
}
