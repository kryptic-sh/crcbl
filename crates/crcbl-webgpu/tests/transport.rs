//! The transport's own suite: the seven `__crcbl_web_gpu_*` entry points, driven
//! the way the shim drives them.
//!
//! An integration test rather than a `#[cfg(test)]` module in `web.rs`, for
//! `stream.rs`'s reason and one of its own. The reason it shares: the ABI is a
//! public contract, and this is the only place it gets exercised the way its
//! caller will — through the exported surface, with nothing `pub(crate)` in
//! reach, so a test that needed the private `ReplyInbox` to say what it meant
//! would be saying something the shim cannot check. The reason of its own:
//! `web.rs` is a long module of ABI documentation, and its suite was half its
//! length.
//!
//! **The entry points share one thread-local**, so every test here runs under a
//! mutex and uninstalls before it starts. `nextest` runs each test in its own
//! process; `cargo test` does not, and a leaked installation would make the
//! "answers zero until installed" case pass for the wrong reason.

use std::rc::Rc;

use crcbl_webgpu::web::{StreamChannel, install, is_installed, shim, uninstall};
use crcbl_webgpu::{Command, DecodeError, Reply, ReplyWriter, StreamReader, decode_stream, tag};

/// The lock every test here holds; see the module docs for why.
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

/// Writes `replies` into the channel exactly as the shim does: ask for a
/// buffer, build the window on what came back, copy, commit. Returns what
/// `commit` answered.
fn deliver(replies: &ReplyWriter) -> u32 {
    let bytes = replies.bytes();
    let len = u32::try_from(bytes.len()).expect("a test-sized buffer");
    let ptr = shim::__crcbl_web_gpu_reply_buffer(len);
    if ptr.is_null() {
        return 0;
    }
    // SAFETY: `ptr` addresses `len` bytes the call above just sized and
    // handed out, and nothing has called back into wasm since — which is
    // the same window the shim's `new Uint8Array(memory.buffer, ptr, len)`
    // is valid in. The two regions are distinct allocations.
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
    shim::__crcbl_web_gpu_reply_commit(len)
}

/// A readback handle with distinct halves, so a field written with the two
/// swapped would not still compare equal.
fn readback() -> crcbl_hal::ReadbackHandle {
    crcbl_core::Handle::from_bits((7 << 32) | 3).expect("a non-zero generation")
}

#[test]
fn the_entry_points_answer_zero_until_a_channel_is_installed() {
    let _guard = shim_guard();
    assert!(!is_installed());
    assert_eq!(shim::__crcbl_web_gpu_stream_len(), 0);
    assert!(shim::__crcbl_web_gpu_stream_ptr().is_null());
    assert_eq!(shim::__crcbl_web_gpu_stream_release(), 0);

    // The reply direction answers nothing too — including `capacity`, which
    // is the readiness test on that side and would otherwise be a constant a
    // shim could read before there was anyone to answer it.
    assert_eq!(shim::__crcbl_web_gpu_reply_capacity(), 0);
    assert_eq!(shim::__crcbl_web_gpu_reply_pending(), 0);
    assert!(shim::__crcbl_web_gpu_reply_buffer(64).is_null());
    assert_eq!(shim::__crcbl_web_gpu_reply_commit(64), 0);
}

/// **A reply crosses, is decoded, and stops being waited on.**
///
/// The whole JS → wasm direction in one test: register the wait, write the
/// bytes the way the shim writes them, commit, drain.
#[test]
fn a_reply_reaches_the_engine_and_clears_the_wait_it_answers() {
    let _guard = shim_guard();
    let channel = Rc::new(StreamChannel::new());
    assert!(install(&channel));
    assert_eq!(
        shim::__crcbl_web_gpu_reply_capacity() as usize,
        tag::MAX_REPLY_BYTES
    );
    assert_eq!(shim::__crcbl_web_gpu_reply_pending(), 0);
    assert_eq!(
        channel.drain_replies(),
        Some(Ok(Vec::new())),
        "a frame with nothing committed is empty, not an error"
    );

    let first = draw(&channel, 0..3);
    let second = draw(&channel, 0..6);
    assert!(channel.expect_reply(first));
    assert!(channel.expect_reply(second));
    assert_eq!(channel.waiting_replies(), 2);

    let mut replies = ReplyWriter::new();
    replies.readback_pending(first, readback());
    replies.adapter(second, 1, "llvmpipe");
    let committed = replies.bytes().len();
    assert_eq!(deliver(&replies), 1);
    assert_eq!(
        shim::__crcbl_web_gpu_reply_pending() as usize,
        committed,
        "committed bytes are visible until the engine drains them"
    );

    assert_eq!(
        channel.drain_replies(),
        Some(Ok(vec![
            (
                first,
                Reply::ReadbackPending {
                    readback: readback()
                }
            ),
            (
                second,
                Reply::Adapter {
                    id: 1,
                    name: "llvmpipe".into()
                }
            ),
        ]))
    );
    assert_eq!(channel.waiting_replies(), 0);
    assert_eq!(shim::__crcbl_web_gpu_reply_pending(), 0);
    assert_eq!(channel.drain_replies(), Some(Ok(Vec::new())));
}

/// **A reply for a sequence nobody waits on is reported, not dropped.**
///
/// The bug this channel could otherwise hide: a replayer answering the wrong
/// command looks exactly like an answer, and the engine would take it. The
/// buffer is refused whole, and the wait it did not answer is still there.
#[test]
fn a_reply_naming_a_sequence_nothing_waits_on_is_an_error_the_engine_can_report() {
    let _guard = shim_guard();
    let channel = Rc::new(StreamChannel::new());
    assert!(install(&channel));

    let sequence = draw(&channel, 0..3);
    assert!(channel.expect_reply(sequence));

    let mut replies = ReplyWriter::new();
    replies.readback_pending(sequence, readback());
    replies.readback_pending(sequence + 1, readback());
    assert_eq!(deliver(&replies), 1);
    assert_eq!(
        channel.drain_replies(),
        Some(Err(DecodeError::UnexpectedSequence {
            sequence: sequence + 1
        }))
    );
    assert_eq!(
        channel.waiting_replies(),
        1,
        "a refused buffer must not half-answer the waits it did name"
    );
    assert_eq!(
        shim::__crcbl_web_gpu_reply_pending(),
        0,
        "the buffer is released even when it is refused"
    );

    // The same reply, twice in one buffer, is the same error: the second one
    // answers a command that — after the first — nothing is waiting on.
    let mut replies = ReplyWriter::new();
    replies.readback_pending(sequence, readback());
    replies.readback_pending(sequence, readback());
    assert_eq!(deliver(&replies), 1);
    assert_eq!(
        channel.drain_replies(),
        Some(Err(DecodeError::UnexpectedSequence { sequence }))
    );

    // …and once it has been answered properly, answering it again is too.
    let mut replies = ReplyWriter::new();
    replies.readback_pending(sequence, readback());
    assert_eq!(deliver(&replies), 1);
    assert!(matches!(channel.drain_replies(), Some(Ok(answered)) if answered.len() == 1));
    assert_eq!(deliver(&replies), 1);
    assert_eq!(
        channel.drain_replies(),
        Some(Err(DecodeError::UnexpectedSequence { sequence }))
    );
}

/// **A committed buffer is not overwritten before the engine reads it.**
///
/// The alternative — letting the second `buffer` call resize over the first
/// frame's replies — loses answers silently, which is the failure this whole
/// channel exists to make impossible.
#[test]
fn a_second_reply_buffer_is_refused_until_the_first_is_drained() {
    let _guard = shim_guard();
    let channel = Rc::new(StreamChannel::new());
    assert!(install(&channel));
    let sequence = draw(&channel, 0..3);
    assert!(channel.expect_reply(sequence));

    let mut replies = ReplyWriter::new();
    replies.readback_pending(sequence, readback());
    assert_eq!(deliver(&replies), 1);

    assert!(
        shim::__crcbl_web_gpu_reply_buffer(64).is_null(),
        "an undrained buffer must not be handed out again"
    );
    assert_eq!(shim::__crcbl_web_gpu_reply_commit(64), 0);

    assert!(matches!(channel.drain_replies(), Some(Ok(_))));
    assert!(
        !shim::__crcbl_web_gpu_reply_buffer(64).is_null(),
        "and must be handed out again once it has been drained"
    );
}

/// **A length the buffer never handed out is refused at the commit.**
///
/// Enforced rather than documented: a shim that commits more than it asked
/// for would otherwise have wasm decode bytes nobody wrote.
#[test]
fn a_commit_that_claims_more_than_it_was_given_is_refused() {
    let _guard = shim_guard();
    let channel = Rc::new(StreamChannel::new());
    assert!(install(&channel));

    assert_eq!(
        shim::__crcbl_web_gpu_reply_commit(32),
        0,
        "nothing was handed out yet"
    );

    assert!(!shim::__crcbl_web_gpu_reply_buffer(32).is_null());
    assert_eq!(shim::__crcbl_web_gpu_reply_commit(33), 0);
    assert_eq!(
        shim::__crcbl_web_gpu_reply_commit(tag::REPLY_HEADER_BYTES as u32 - 1),
        0,
        "bytes that cannot even be a header are not a reply stream"
    );
    assert_eq!(shim::__crcbl_web_gpu_reply_commit(32), 1);
    assert_eq!(shim::__crcbl_web_gpu_reply_pending(), 32);

    // Those 32 bytes are zeroes, so they are not a reply stream at all. The
    // engine gets the decode error and the buffer is released regardless —
    // otherwise the same bytes would be met on every frame for ever.
    assert_eq!(
        channel.drain_replies(),
        Some(Err(DecodeError::BadMagic)),
        "a zeroed buffer is not this format"
    );
    assert_eq!(shim::__crcbl_web_gpu_reply_pending(), 0);
}

/// **A length past the cap is refused before anything is allocated.**
///
/// The length comes from JS, and it is the one number on this seam that
/// drives an allocation wasm makes.
#[test]
fn a_reply_buffer_past_the_cap_is_refused_rather_than_allocated_for() {
    let _guard = shim_guard();
    let channel = Rc::new(StreamChannel::new());
    assert!(install(&channel));

    let cap = shim::__crcbl_web_gpu_reply_capacity();
    assert!(shim::__crcbl_web_gpu_reply_buffer(cap + 1).is_null());
    assert!(shim::__crcbl_web_gpu_reply_buffer(u32::MAX).is_null());
    assert_eq!(shim::__crcbl_web_gpu_reply_pending(), 0);
    drop(channel);
}

/// **The waiting set is bounded**, so a shim that stops answering costs a
/// refusal rather than unbounded memory.
#[test]
fn the_waiting_set_stops_growing_at_its_cap() {
    let _guard = shim_guard();
    let channel = Rc::new(StreamChannel::new());
    assert!(install(&channel));

    for sequence in 0..tag::MAX_WAITING_REPLIES as u64 {
        assert!(channel.expect_reply(sequence), "sequence {sequence}");
    }
    assert_eq!(channel.waiting_replies(), tag::MAX_WAITING_REPLIES);
    assert!(
        !channel.expect_reply(tag::MAX_WAITING_REPLIES as u64),
        "one past the cap is refused"
    );
    assert!(
        channel.expect_reply(0),
        "…while a sequence already waited on is not a new one"
    );
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
