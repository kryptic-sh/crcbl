//! The transport: where each buffer lives, and how the two halves hand them
//! over.
//!
//! [`writer`](crate::writer), [`reply`](crate::reply) and [`tag`]
//! say what the bytes *mean*; this module says how they cross. **Both
//! directions are here.** Wasm appends commands into a buffer it owns and the
//! shim drains it once a frame; JS writes replies into a second buffer wasm also
//! owns, and the engine drains that one.
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
//! | [`__crcbl_web_gpu_reply_capacity`](shim::__crcbl_web_gpu_reply_capacity) | `() -> i32` | The most bytes of replies wasm will accept in one buffer ([`MAX_REPLY_BYTES`](crate::tag::MAX_REPLY_BYTES)), or `0` when there is no channel. The **readiness** test for this direction. |
//! | [`__crcbl_web_gpu_reply_pending`](shim::__crcbl_web_gpu_reply_pending) | `() -> i32` | Bytes committed and not yet drained by the engine; `0` when there are none, or no channel. |
//! | [`__crcbl_web_gpu_reply_buffer`](shim::__crcbl_web_gpu_reply_buffer) | `(i32) -> i32` | Size the reply buffer to `len` bytes and return its address, or `0`. **May grow wasm memory** — the only export here that can. |
//! | [`__crcbl_web_gpu_reply_commit`](shim::__crcbl_web_gpu_reply_commit) | `(i32) -> i32` | The first `len` bytes of that buffer are a reply stream. `1`, or `0` if they were not accepted. |
//!
//! `len` is the command direction's readiness test rather than `ptr` because an
//! installed channel always holds at least a header —
//! [`HEADER_BYTES`](crate::tag::HEADER_BYTES) — so `0` there means "nothing to
//! ask" and nothing else. A frame that encoded no commands answers
//! [`HEADER_BYTES`](crate::tag::HEADER_BYTES) and decodes to an empty list, which
//! is a different fact and stays a different number. `capacity` is the reply
//! direction's, for the same shape of reason: an installed channel always
//! answers [`MAX_REPLY_BYTES`](crate::tag::MAX_REPLY_BYTES), which is never zero,
//! and `pending` cannot serve because zero there is the ordinary case.
//!
//! ## Buffer ownership
//!
//! Both buffers belong to wasm; JS never passes a pointer in.
//!
//! - The **command buffer** is written by wasm — by whatever encodes through
//!   [`StreamChannel::encode`] — and **read by JS**, between the frame that
//!   encoded it and the [`release`](shim::__crcbl_web_gpu_stream_release) that
//!   ends it. It is valid from the moment the frame export returns until that
//!   `release`. Afterwards the bytes are gone: `release` rewrites the header in
//!   place, so a view built before it still points at live memory and no longer
//!   describes the frame it described. **Decode before releasing, never after.**
//!   Nothing is copied out for JS: [`StreamWriter`] keeps one `Vec` for the life
//!   of the channel and `release` clears it without freeing, so the per-frame
//!   copy is one JS chooses to make or not.
//! - The **reply buffer** is allocated by wasm and **written by JS**, between
//!   [`reply_buffer`](shim::__crcbl_web_gpu_reply_buffer) and the matching
//!   [`reply_commit`](shim::__crcbl_web_gpu_reply_commit). Nothing else may
//!   touch it in that window. After the commit the bytes are wasm's until the
//!   engine drains them, and JS must not write again until it has: `reply_buffer`
//!   answers `0` while a committed buffer is still waiting, rather than
//!   overwriting replies nobody has read.
//!
//! **The detached-view trap, and the answer for this ABI.** A `Uint8Array` over
//! `memory.buffer` is detached the moment wasm memory grows, and reading through
//! a detached view throws.
//!
//! **[`__crcbl_web_gpu_reply_buffer`](shim::__crcbl_web_gpu_reply_buffer) is the
//! one export here that can grow memory**, because it is the one that allocates.
//! Build the view *after* that call returns, from the pointer it returned, and
//! never store it — the rule `crcbl-store`'s `__crcbl_web_fetch_buffer` sets and
//! for the same reason.
//!
//! None of the other five can: the three stream exports only read a `Vec`'s
//! pointer and length or clear it, and `capacity`, `pending` and `commit` only
//! read numbers. What *does* move the command buffer is **encoding** — appending
//! a command can reallocate it, and that allocation can grow wasm memory — and
//! encoding happens inside the engine's own per-frame export, which is not one of
//! these seven. The rule that follows is narrow and exact: build the view after
//! the frame export has returned, and drop it before calling back into wasm for
//! the next frame. `web/engine/wasm.js` enforces the general form of this by
//! never exporting a view at all.
//!
//! ## Call ordering
//!
//! 1. The app constructs a [`StreamChannel`] and calls [`install`] — before the
//!    shim's first call. Until then **every export answers `0`**, which is a shim
//!    that started before the engine did rather than a failure to report.
//! 2. Per frame, the engine encodes through [`StreamChannel::encode`], and
//!    registers [`expect_reply`](StreamChannel::expect_reply) for each command
//!    whose answer it will wait for.
//! 3. Per frame, after that: `stream_len()` — `0` means there is nothing to do —
//!    then `stream_ptr()`, then decode, then `stream_release()`.
//! 4. `release` is the obligation, not the optional path. A shim that decodes and
//!    does not release leaves the frame's commands in the buffer, and the next
//!    frame appends to them. The stream stays correct — sequence numbers are
//!    monotonic across frames and the header still names the first command — but
//!    the buffer only grows. **A `len` that never falls is a shim that is not
//!    releasing.**
//! 5. Whenever the shim has answers: `reply_capacity()` — `0` means there is
//!    nobody to answer — then `reply_buffer(len)`, then build the view, write the
//!    encoded replies, then `reply_commit(len)`. A `0` from `reply_buffer` means
//!    "not now": either the length was past capacity, or the engine has not
//!    drained the last one. Keep the replies and offer them again next frame.
//! 6. Per frame, the engine calls
//!    [`drain_replies`](StreamChannel::drain_replies), which decodes the
//!    committed bytes and frees the buffer for the next commit.
//!
//! Sequence numbers survive a `release`, because [`StreamWriter::clear`] keeps
//! the counter where it is. They have to: an error raised by a replayed command
//! surfaces a frame or more after the frame that encoded it, so a counter that
//! restarted each frame would name several different commands with the same
//! number. The reply direction depends on the same property from the other end —
//! a reply arriving three frames later still names a number no other command has
//! had.
//!
//! ## Failure behaviour
//!
//! | Situation | What the shim does | What the engine sees |
//! | --- | --- | --- |
//! | no channel installed (`stream_len` and `reply_capacity` are `0`) | nothing this frame; ask again next | nothing; the engine has not installed one yet |
//! | the channel was dropped mid-run | as above — every export goes back to `0` | nothing; the exports hold a [`Weak`] and stop finding it |
//! | decoding a frame throws | release anyway, then report | the next frame starts from a clean buffer instead of the shim re-throwing on the same bytes forever |
//! | the shim stops releasing | — | `stream_len` climbs and never falls |
//! | `reply_buffer` answers `0` | hold the replies; try again next frame | nothing yet; the replies are still in JS |
//! | the engine stops draining | `reply_buffer` keeps answering `0` | `reply_pending` stays non-zero |
//! | a reply names a sequence nothing waits on | — | [`DecodeError::UnexpectedSequence`] out of `drain_replies`, and the buffer is dropped |
//! | committed bytes that will not decode | — | the [`DecodeError`], and the buffer is dropped so the same bytes are not met for ever |
//!
//! A stream that will not decode is a bug in one of the two hand-written halves
//! of the *format*, not a condition this ABI models — which is what
//! `crates/crcbl-webgpu/tests/fixture.rs`, `web/tools/stream-decode.mjs` and
//! `web/tools/reply-encode.mjs` are for.
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
//!
//! function sendReplies(ex, memory, bytes) {       // bytes: a ReplyWriter's
//!   if (ex.__crcbl_web_gpu_reply_capacity() < bytes.length) return false;
//!   const ptr = ex.__crcbl_web_gpu_reply_buffer(bytes.length);  // may grow memory
//!   if (ptr === 0) return false;                  // not now — keep them
//!   new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes); // view built after
//!   return ex.__crcbl_web_gpu_reply_commit(bytes.length) === 1;
//! }
//! ```
//!
//! `web/engine/gpu-transport.js` is that shim written out, and
//! `web/tools/stream-transport.mjs` drives it against a synthetic
//! `WebAssembly.Memory` holding the committed fixtures.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::{Rc, Weak};

use crate::bytes::DecodeError;
use crate::reply::{Reply, ReplyReader};
use crate::{StreamWriter, tag};

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
///
/// The other direction, from the same channel. What JS writes into the buffer is
/// a reply stream; [`ReplyWriter`](crate::ReplyWriter) is what encodes one
/// outside a browser.
///
/// ```
/// use std::rc::Rc;
/// use crcbl_webgpu::{Reply, ReplyWriter};
/// use crcbl_webgpu::web::{StreamChannel, install, shim};
/// # use crcbl_core::Handle;
/// # let readback = Handle::from_bits((7 << 32) | 3).unwrap();
///
/// let channel = Rc::new(StreamChannel::new());
/// assert!(install(&channel));
///
/// // A command that will be answered later, and the wait it registers.
/// let sequence = channel.encode(|stream| stream.draw(0..3, 0..1)).unwrap();
/// assert!(channel.expect_reply(sequence));
///
/// // What JS encodes…
/// let mut replies = ReplyWriter::new();
/// replies.readback_pending(sequence, readback);
///
/// // …and how it hands the bytes over. `reply_buffer` is the call that can
/// // grow wasm memory, so a JS view is built from what it returns, after it.
/// let len = replies.bytes().len() as u32;
/// assert!(shim::__crcbl_web_gpu_reply_capacity() >= len);
/// let ptr = shim::__crcbl_web_gpu_reply_buffer(len);
/// assert!(!ptr.is_null());
/// // SAFETY: `ptr` and `len` are what the call above just handed out, and
/// // nothing has called back into wasm since. This is the shim's
/// // `new Uint8Array(memory.buffer, ptr, len).set(bytes)`.
/// unsafe { std::ptr::copy_nonoverlapping(replies.bytes().as_ptr(), ptr, len as usize) };
/// assert_eq!(shim::__crcbl_web_gpu_reply_commit(len), 1);
///
/// // The engine reads them next frame, and stops waiting for that sequence.
/// assert_eq!(
///     channel.drain_replies().unwrap(),
///     Ok(vec![(sequence, Reply::ReadbackPending { readback })]),
/// );
/// assert_eq!(channel.waiting_replies(), 0);
/// ```
#[derive(Debug, Default)]
pub struct StreamChannel {
    writer: RefCell<StreamWriter>,
    replies: RefCell<ReplyInbox>,
}

impl StreamChannel {
    /// A channel holding one empty frame — a header and no commands — and no
    /// replies.
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

    /// Wait for a reply to the command that was assigned `sequence`.
    ///
    /// Register this for every command whose answer the engine intends to read.
    /// It is what makes an answer to a command nobody asked reportable rather
    /// than plausible: [`drain_replies`](Self::drain_replies) refuses a reply
    /// whose sequence is not registered here.
    ///
    /// `false` when [`MAX_WAITING_REPLIES`](crate::tag::MAX_WAITING_REPLIES) are
    /// already waiting, or the set is borrowed. A caller that gets `false` has
    /// not registered anything: the call it was about to make has nowhere to put
    /// an answer, so it should fail rather than issue it.
    pub fn expect_reply(&self, sequence: u64) -> bool {
        let Ok(mut replies) = self.replies.try_borrow_mut() else {
            return false;
        };
        if replies.waiting.len() >= tag::MAX_WAITING_REPLIES && !replies.waiting.contains(&sequence)
        {
            return false;
        }
        replies.waiting.insert(sequence);
        true
    }

    /// How many sequences are waiting for an answer.
    ///
    /// A number that only climbs is a shim that has stopped answering — the same
    /// diagnostic a `stream_len` that never falls is in the other direction.
    #[must_use]
    pub fn waiting_replies(&self) -> usize {
        self.replies
            .try_borrow()
            .map_or(0, |replies| replies.waiting.len())
    }

    /// Decode the replies JS has committed, and stop waiting for them.
    ///
    /// `Ok` with an empty list when nothing has been committed, which is the
    /// ordinary case for most frames. The buffer is released either way — on an
    /// error too, so a buffer that will not decode is not met again for ever.
    ///
    /// `None` if the inbox is already borrowed, which is the guard
    /// [`encode`](Self::encode) has and for the same reason.
    ///
    /// # Errors
    ///
    /// [`DecodeError::UnexpectedSequence`] if any reply names a sequence
    /// [`expect_reply`](Self::expect_reply) was not told about — **the whole
    /// buffer is refused, not just that reply**, because a replayer answering a
    /// command nobody asked is not a condition to accept the rest of. Any other
    /// [`DecodeError`] the bytes produce, which is a bug in one of the two
    /// hand-written halves of the format.
    pub fn drain_replies(&self) -> Option<Result<Vec<(u64, Reply)>, DecodeError>> {
        let mut replies = self.replies.try_borrow_mut().ok()?;
        Some(replies.drain())
    }
}

// ---------------------------------------------------------------------------
// The reply buffer
// ---------------------------------------------------------------------------

/// The buffer JS writes replies into, and the sequences wasm is waiting for.
///
/// One buffer, not a queue of them: replies are drained once a frame like
/// commands, and a second buffer arriving before the first is drained is a shim
/// running ahead of the engine rather than a case to buffer for. It is told so —
/// [`shim::__crcbl_web_gpu_reply_buffer`] answers `0` — rather than having its
/// replies overwritten.
#[derive(Debug, Default)]
struct ReplyInbox {
    /// The bytes themselves. Kept across frames so a steady stream of replies
    /// costs no allocation after the first.
    buf: Vec<u8>,
    /// What the last [`buffer`](Self::buffer) handed out. A commit longer than
    /// this is refused: JS would be claiming bytes it was never given.
    handed_out: usize,
    /// Bytes committed and not yet drained, or `None` when nothing waits.
    committed: Option<usize>,
    /// Sequences a reply is expected for. A `BTreeSet` because it is searched
    /// far more often than it is grown, and because an ordered set makes a
    /// dump of "what is this frame still waiting on" readable.
    waiting: BTreeSet<u64>,
}

impl ReplyInbox {
    /// Bytes committed and not yet drained.
    fn pending(&self) -> usize {
        self.committed.unwrap_or(0)
    }

    /// Size the buffer to `len` and hand back where it starts, or `None`.
    ///
    /// **This is the call that can grow wasm memory**, and the only one in this
    /// module that can: it is the only one that allocates.
    fn buffer(&mut self, len: usize) -> Option<*mut u8> {
        if len > tag::MAX_REPLY_BYTES || self.committed.is_some() {
            return None;
        }
        self.buf.resize(len, 0);
        self.handed_out = len;
        Some(self.buf.as_mut_ptr())
    }

    /// Accept the first `len` bytes of the buffer as a reply stream.
    ///
    /// `false` when `len` is longer than what was handed out, shorter than a
    /// header — bytes that cannot be a reply stream at all — or when a committed
    /// buffer is still waiting to be drained. Refused at the boundary rather
    /// than documented as a rule, so a shim that gets it wrong is told at the
    /// call rather than a frame later at the decode.
    fn commit(&mut self, len: usize) -> bool {
        if len > self.handed_out || len < tag::REPLY_HEADER_BYTES || self.committed.is_some() {
            return false;
        }
        self.committed = Some(len);
        true
    }

    /// Decode what is committed, checking every sequence against the waiting
    /// set, and free the buffer whatever the outcome.
    fn drain(&mut self) -> Result<Vec<(u64, Reply)>, DecodeError> {
        let Some(len) = self.committed.take() else {
            return Ok(Vec::new());
        };
        self.handed_out = 0;

        // Decoded whole and checked whole before anything is removed from the
        // waiting set: a buffer that turns out to answer a command nobody asked
        // must leave the set exactly as it found it, or the engine would stop
        // waiting for answers it never got.
        let mut reader = ReplyReader::new(&self.buf[..len])?;
        let mut decoded = Vec::new();
        let mut answered = BTreeSet::new();
        while let Some(next) = reader.next_reply() {
            let (sequence, reply) = next?;
            // `answered` is what catches the *second* reply to one command
            // inside a single buffer. Removing from `waiting` as it went would
            // have caught that too, and would have left the set half-drained
            // when a later reply turned out to be unexpected.
            if !self.waiting.contains(&sequence) || !answered.insert(sequence) {
                return Err(DecodeError::UnexpectedSequence { sequence });
            }
            decoded.push((sequence, reply));
        }
        for sequence in &answered {
            self.waiting.remove(sequence);
        }
        Ok(decoded)
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

/// [`with_stream`], for the reply direction.
fn with_replies<R>(absent: R, f: impl FnOnce(&ReplyInbox) -> R) -> R {
    with_channel(absent, |channel, absent| {
        match channel.replies.try_borrow() {
            Ok(replies) => f(&replies),
            Err(_) => absent,
        }
    })
}

/// [`with_stream_mut`], for the reply direction.
fn with_replies_mut<R>(absent: R, f: impl FnOnce(&mut ReplyInbox) -> R) -> R {
    with_channel(absent, |channel, absent| {
        match channel.replies.try_borrow_mut() {
            Ok(mut replies) => f(&mut replies),
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
    use super::{tag, with_channel, with_replies, with_replies_mut, with_stream, with_stream_mut};

    /// Bytes of encoded stream waiting, header included.
    ///
    /// `0` when there is no channel to ask, and only then: an installed channel
    /// always holds at least [`tag::HEADER_BYTES`], so
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

    /// The most bytes of replies wasm will accept in one buffer.
    ///
    /// [`tag::MAX_REPLY_BYTES`], or `0` when there
    /// is no channel to answer — which makes this the readiness test for this
    /// direction, since the cap is never zero and
    /// [`__crcbl_web_gpu_reply_pending`] is zero on almost every frame.
    ///
    /// Asked rather than assumed: the number is a Rust constant, and a shim that
    /// hard-coded it would keep its old value across a wasm update it did not
    /// know had happened.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_reply_capacity() -> u32 {
        with_channel(0, |_channel, _absent| {
            u32::try_from(tag::MAX_REPLY_BYTES).unwrap_or(u32::MAX)
        })
    }

    /// Bytes committed and not yet drained by the engine, or `0`.
    ///
    /// A number that never falls is an engine that has stopped draining, and it
    /// is why [`__crcbl_web_gpu_reply_buffer`] is answering `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_reply_pending() -> u32 {
        with_replies(0, |replies| {
            u32::try_from(replies.pending()).unwrap_or(u32::MAX)
        })
    }

    /// Size the reply buffer to `len` bytes and return its address.
    ///
    /// `0` when there is no channel, when `len` is past
    /// [`__crcbl_web_gpu_reply_capacity`], or when a committed buffer is still
    /// waiting to be drained — in every case the shim keeps its replies and
    /// offers them again next frame rather than losing them.
    ///
    /// **May grow wasm memory**, detaching any `Uint8Array` built before the
    /// call. It is the one export in this module that can, because it is the one
    /// that allocates. Build the view afterwards, from the pointer this
    /// returned, and do not store it.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_reply_buffer(len: u32) -> *mut u8 {
        with_replies_mut(core::ptr::null_mut(), |replies| {
            replies
                .buffer(len as usize)
                .unwrap_or(core::ptr::null_mut())
        })
    }

    /// The first `len` bytes of the reply buffer are a reply stream.
    ///
    /// `1` when they were accepted; `0` when there is no channel, when `len` is
    /// longer than the buffer that was handed out, when it is shorter than a
    /// reply header, or when a committed buffer is still waiting. A `0` means the
    /// bytes were **not** delivered and the shim still owns them.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_reply_commit(len: u32) -> u32 {
        with_replies_mut(0, |replies| u32::from(replies.commit(len as usize)))
    }
}
