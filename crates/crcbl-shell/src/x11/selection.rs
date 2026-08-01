//! The X11 selection protocol's bookkeeping: `INCR` in both directions,
//! property chunking, and the deadline that keeps
//! [obligation 4](crate::Shell) satisfiable.
//!
//! Everything here is pure — integers, byte vectors and a clock passed in as a
//! parameter — so the state machine that is hardest to get right is the part
//! testable with no X server anywhere. [`super`] owns the requests; this owns
//! *when* and *what*.
//!
//! # The iceberg, and how much of it this slice carries
//!
//! Reading a selection on X11 is five steps, and only the third is obvious:
//!
//! 1. `GetSelectionOwner` says whether anyone owns it at all. Nobody is
//!    [`Empty`](ClipboardContent::Empty), synchronously, with no conversation.
//! 2. `ConvertSelection(…, TARGETS, …)` asks the owner **what formats it has**.
//!    This step is the one that looks skippable and is not: a mime type is one
//!    string to this engine and up to four *atoms* to X11, and an owner answers
//!    only the spelling it was asked for. `xterm` offers `STRING` and
//!    `UTF8_STRING`; a GTK application offers `text/plain;charset=utf-8`; a
//!    backend that guesses one of them refuses to paste from the other half of
//!    the desktop. [`choose_target`] is the pick.
//! 3. `ConvertSelection(…, chosen, property, requestor)` asks the **owning
//!    client** — not the server — to write the data.
//! 4. A `SelectionNotify` says it is done. `property == None` means it refused,
//!    which is [`ClipboardContent::Unavailable`] and *not* an empty clipboard.
//!    `GetProperty` then reads it — unless the property's **type** is `INCR`,
//!    in which case the value is not the data at all: it is a lower bound on
//!    the size, and the real transfer has not started yet.
//! 5. An `INCR` transfer is a handshake: the requestor deletes the property,
//!    the owner writes the next chunk, a `PropertyNotify(NewValue)` announces
//!    it, and a **zero-length** write ends it. Every step is an event; there is
//!    no bulk read.
//!
//! # Decision: a deadline on the whole read, not on the round trip
//!
//! [Obligation 4](crate::Shell) is "answered exactly once, and within a bounded
//! time", and P0.5c's prediction was that X11 would get it for free because a
//! read can be answered immediately. That prediction is **wrong for `INCR`**,
//! and this is the one place the X11 clipboard is harder than the Wayland one.
//!
//! On Wayland the transfer is a pipe, so a peer that stops writing shows up as
//! a descriptor that is never ready, and
//! [`wayland::fd::TIMEOUT`](crate::wayland::fd) bounds it. On X11 there is no
//! descriptor: a peer that stops answering simply stops sending
//! `PropertyNotify`, which is indistinguishable from a peer that is slow. Worse,
//! step 2 has the same problem — an owning client that has crashed *after*
//! taking the selection never sends `SelectionNotify` at all, and the X server
//! will not notice, because it never knew the conversation was happening.
//!
//! So every read carries [`TIMEOUT`] from the moment it was accepted, refreshed
//! on every chunk that actually arrives. A read that reaches it answers
//! [`Unavailable`](ClipboardContent::Unavailable) and is removed. Without this
//! the backend would satisfy obligation 4 for well-behaved peers and violate it
//! for exactly the peers a timeout exists for.
//!
//! There is exactly **one** case where an accepted read is not answered: its
//! window was destroyed. An answer names the window it was asked for, and
//! emitting one for a handle the consumer has already been told is stale would
//! break [obligation 1](crate::Shell), which is the stronger of the two — a
//! consumer can see a stale handle and cannot see a request that will never be
//! answered on a window it no longer has. Both routes to a destroyed window go
//! through `X11Shell::forget_selection_state`, and `HeadlessShell` makes the
//! same call in `resolve_reads`.
//!
//! # Decision: the same shape as `service_transfers`, for the same deadlock
//!
//! An X11 client can own the selection *and* read it, and P0.5c predicted the
//! deadlock would be worse here than on Wayland — it is, because there is one
//! connection rather than two file descriptors. The owner half of the
//! conversation is a `SelectionRequest` event that arrives on the very
//! connection the reader is waiting on, so **any** synchronous read is an
//! immediate self-deadlock.
//!
//! The answer is the alternating-progress shape the Wayland backend already
//! uses: nothing waits. [`Shell::clipboard_request`](crate::Shell::clipboard_request)
//! records a [`Read`] and returns; every [`pump`](crate::Shell::pump) drains
//! the connection, answers whatever `SelectionRequest`s arrived, advances
//! whatever `INCR` transfers arrived, and emits the reads that finished. Owner
//! and requestor make progress in the same loop because neither is ever inside
//! the other.

use core::time::Duration;

use crate::{ClipboardContent, ClipboardRequestId, MimeType, WindowId};

/// How long a read may make no progress before it is abandoned.
///
/// Two seconds, matching [`wayland::fd::TIMEOUT`](crate::wayland::fd) so that
/// the two Linux backends fail a stalled paste on the same schedule — a UI that
/// tuned its "pasting…" spinner against one must not need retuning against the
/// other.
///
/// Refreshed on progress rather than measured from the start, so a genuinely
/// large `INCR` transfer over a slow connection is not cut off halfway.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// The most a single read may accumulate, in bytes.
///
/// The *peer* decides how much it sends and this process has to hold it. 64 MiB
/// is far past any real clipboard payload and far short of a memory problem.
/// The same number the Wayland backend caps a pipe at, and for the same reason:
/// without it, another application chooses how much of this engine's address
/// space it uses.
pub const MAX_BYTES: usize = 64 << 20;

/// Where a read has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadState {
    /// `ConvertSelection(TARGETS)` has been sent; waiting to hear what formats
    /// the owner has.
    AwaitingTargets,
    /// `ConvertSelection` for the chosen target has been sent; waiting for
    /// `SelectionNotify`.
    AwaitingNotify,
    /// The owner answered with type `INCR`; chunks are arriving as
    /// `PropertyNotify`.
    Incremental,
}

/// What a step of the state machine decided.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Delete the property to acknowledge a chunk and ask for the next.
    ///
    /// The `INCR` handshake is driven by the *requestor*: the owner will not
    /// write the next chunk until this one is deleted. A backend that skips it
    /// waits forever on a peer that is waiting on it.
    AcknowledgeChunk,
    /// The read is finished. Emit this and drop it.
    Done(ClipboardContent),
    /// `TARGETS` came back and this is the spelling to ask for next.
    Convert(u32),
}

/// One outstanding [`clipboard_request`](crate::Shell::clipboard_request).
#[derive(Clone, Debug)]
pub struct Read {
    /// Which request this answers.
    pub request: ClipboardRequestId,
    /// Which window asked. Also the requestor window on the wire.
    pub window: WindowId,
    /// The format that was asked for, for the answer's `mime` when there is no
    /// peer spelling to report.
    pub mime: MimeType,
    /// The property the answer is being written into.
    pub property: u32,
    /// The atoms this format may be spelled as, best first.
    pub candidates: Vec<u32>,
    /// The peer's spelling of the format, once `SelectionNotify` reports it.
    pub answered_target: u32,
    /// Bytes so far.
    pub buffer: Vec<u8>,
    /// Where the machine is.
    pub state: ReadState,
    /// `CLOCK_MONOTONIC` nanoseconds of the last progress.
    pub last_progress_nanos: u64,
}

impl Read {
    /// A read that has just had its `ConvertSelection` sent.
    #[must_use]
    pub fn new(
        request: ClipboardRequestId,
        window: WindowId,
        mime: MimeType,
        candidates: Vec<u32>,
        property: u32,
        now_nanos: u64,
    ) -> Self {
        Self {
            request,
            window,
            mime,
            candidates,
            property,
            answered_target: 0,
            buffer: Vec::new(),
            state: ReadState::AwaitingTargets,
            last_progress_nanos: now_nanos,
        }
    }

    /// Applies the `SelectionNotify` answering the `TARGETS` conversion.
    ///
    /// `offered` is the atom list the owner published, or empty when it refused
    /// the query. **Refusing is not an error**: `TARGETS` is ICCCM and there are
    /// still clients that do not implement it, so the fallback is to ask for the
    /// best-guess spelling directly and let *that* refusal be the failure. A
    /// backend that treated a missing `TARGETS` as "nothing on the clipboard"
    /// would silently stop pasting from a handful of old but real applications.
    pub fn on_targets(&mut self, offered: &[u32], now_nanos: u64) -> Step {
        self.last_progress_nanos = now_nanos;
        self.state = ReadState::AwaitingNotify;
        if offered.is_empty() {
            return match self.candidates.first() {
                Some(target) => Step::Convert(*target),
                None => Step::Done(ClipboardContent::Unavailable),
            };
        }
        match choose_target(&self.candidates, offered) {
            Some(target) => Step::Convert(target),
            // The owner has something, but nothing in the format that was
            // asked for. `Empty` is defined to merge "holds nothing" with
            // "holds nothing in *that* format", precisely for this.
            None => Step::Done(ClipboardContent::Empty),
        }
    }

    /// Applies a `SelectionNotify`.
    ///
    /// `property` is the event's property field — **`0` means the owner
    /// refused**, which is [`Unavailable`](ClipboardContent::Unavailable) and
    /// not an empty clipboard. `is_incr` says the property's type was the
    /// `INCR` atom, in which case its *value* is a size estimate and is
    /// deliberately ignored: it is a lower bound the spec does not require to
    /// be accurate, and trusting it would mean allocating on a peer's say-so.
    pub fn on_selection_notify(
        &mut self,
        property: u32,
        target: u32,
        is_incr: bool,
        value: &[u8],
        now_nanos: u64,
    ) -> Step {
        self.last_progress_nanos = now_nanos;
        self.answered_target = target;
        if property == 0 {
            return Step::Done(ClipboardContent::Unavailable);
        }
        if is_incr {
            self.state = ReadState::Incremental;
            // Deleting the property is what tells the owner to start.
            return Step::AcknowledgeChunk;
        }
        // A property that exists and is empty is a *successful* transfer of
        // nothing, which `ClipboardContent` separates from both other cases
        // precisely so this can be said.
        Step::Done(ClipboardContent::Bytes(value.to_vec()))
    }

    /// Applies one `INCR` chunk, read out of the property.
    ///
    /// A **zero-length** write is the end of the transfer, per ICCCM. It is not
    /// a peer that had nothing to say: a peer with nothing to say sends the
    /// terminator immediately, and the accumulated buffer — possibly empty — is
    /// the answer either way.
    pub fn on_chunk(&mut self, value: &[u8], now_nanos: u64) -> Step {
        self.last_progress_nanos = now_nanos;
        if value.is_empty() {
            return Step::Done(ClipboardContent::Bytes(core::mem::take(&mut self.buffer)));
        }
        if self.buffer.len().saturating_add(value.len()) > MAX_BYTES {
            log::warn!(
                "clipboard peer sent more than {MAX_BYTES} bytes for {}; abandoning",
                self.request
            );
            return Step::Done(ClipboardContent::Unavailable);
        }
        self.buffer.extend_from_slice(value);
        Step::AcknowledgeChunk
    }

    /// Whether this read has made no progress for [`TIMEOUT`].
    ///
    /// The check that turns obligation 4 from an aspiration into a property.
    #[must_use]
    pub fn is_stalled(&self, now_nanos: u64) -> bool {
        let elapsed = now_nanos.saturating_sub(self.last_progress_nanos);
        elapsed >= u64::try_from(TIMEOUT.as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The largest property payload one `ChangeProperty` can carry, in bytes.
///
/// `maximum_request_length` is in **4-byte units** and covers the whole
/// request, header included. `ChangeProperty`'s header is six words, and a
/// margin of two more is kept because a request that is exactly at the limit is
/// rejected by some servers rather than accepted — the failure mode being a
/// `BadLength` in the middle of a paste.
///
/// The result is rounded down to a multiple of four so that a chunk boundary
/// never lands mid-word for a format-32 property.
/// The first spelling of a format that the owner actually has.
///
/// Order is preference, not availability: `candidates` is best-first, so a peer
/// offering both `UTF8_STRING` and Latin-1 `STRING` is read as UTF-8. Answering
/// on availability order instead — taking whatever the peer listed first —
/// would mean pasting mojibake from a client whose `TARGETS` happens to be
/// alphabetical.
#[must_use]
pub fn choose_target(candidates: &[u32], offered: &[u32]) -> Option<u32> {
    candidates
        .iter()
        .copied()
        .find(|candidate| *candidate != 0 && offered.contains(candidate))
}

#[must_use]
pub fn max_property_bytes(maximum_request_length: u16) -> usize {
    /// `ChangeProperty`'s fixed part, in 4-byte units.
    const HEADER_WORDS: usize = 6;
    /// Slack, in 4-byte units.
    const MARGIN_WORDS: usize = 2;
    let words = usize::from(maximum_request_length).saturating_sub(HEADER_WORDS + MARGIN_WORDS);
    // A server that advertised something absurdly small still has to be able to
    // send *something*, and 4 KiB is below every real server's limit (the
    // protocol floor is 4096 words = 16 KiB).
    words.saturating_mul(4).max(4096)
}

/// A payload being handed to a peer one `INCR` chunk at a time.
#[derive(Clone, Debug)]
pub struct Write {
    /// The window that asked. Chunks are written to *its* property.
    pub requestor: u32,
    /// The property on the requestor to write into.
    pub property: u32,
    /// The target atom that was asked for, echoed in the reply.
    pub target: u32,
    /// The payload.
    pub bytes: Vec<u8>,
    /// How far through `bytes` we are.
    pub offset: usize,
    /// Bytes per chunk.
    pub chunk: usize,
    /// Whether the zero-length terminator has been written.
    pub terminated: bool,
    /// `CLOCK_MONOTONIC` nanoseconds of the last chunk we managed to write.
    pub last_progress_nanos: u64,
}

impl Write {
    /// A transfer of `bytes`, chunked at `chunk`.
    #[must_use]
    pub fn new(
        requestor: u32,
        property: u32,
        target: u32,
        bytes: Vec<u8>,
        chunk: usize,
        now_nanos: u64,
    ) -> Self {
        Self {
            requestor,
            property,
            target,
            bytes,
            offset: 0,
            chunk: chunk.max(1),
            terminated: false,
            last_progress_nanos: now_nanos,
        }
    }

    /// The next chunk to write, or `None` when the transfer is over.
    ///
    /// The last chunk is **empty**, and is a real write rather than a skipped
    /// one: ICCCM defines the terminator as a zero-length property write, so a
    /// transfer whose payload happens to be a multiple of the chunk size still
    /// owes the peer one more `ChangeProperty`.
    pub fn next_chunk(&mut self, now_nanos: u64) -> Option<&[u8]> {
        if self.terminated {
            return None;
        }
        self.last_progress_nanos = now_nanos;
        if self.offset >= self.bytes.len() {
            self.terminated = true;
            return Some(&[]);
        }
        let end = self.bytes.len().min(self.offset + self.chunk);
        let start = self.offset;
        self.offset = end;
        Some(&self.bytes[start..end])
    }

    /// Whether the peer has stopped acknowledging chunks.
    ///
    /// The write side needs the same bound as the read side and for a sharper
    /// reason: an abandoned write holds a `Vec` of the payload *and* keeps
    /// answering `PropertyNotify` events for a window that may already be gone.
    #[must_use]
    pub fn is_stalled(&self, now_nanos: u64) -> bool {
        let elapsed = now_nanos.saturating_sub(self.last_progress_nanos);
        elapsed >= u64::try_from(TIMEOUT.as_nanos()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Pool;

    fn window() -> WindowId {
        let mut pool: Pool<u8> = Pool::new();
        pool.insert(0).cast()
    }

    fn pending(now: u64) -> Read {
        let mut read = Read::new(
            ClipboardRequestId(1),
            window(),
            MimeType::TextUtf8,
            vec![11],
            12,
            now,
        );
        // Every test below starts from the point where the format has already
        // been negotiated; `on_targets` is exercised on its own.
        let _ = read.on_targets(&[11], now);
        read
    }

    #[test]
    fn a_refused_conversion_is_unavailable_and_not_an_empty_clipboard() {
        // The distinction P0.5c added `ClipboardContent::Unavailable` for, in
        // its X11 form: `property == None` on the `SelectionNotify` means the
        // owner could not produce that target. There may well be something on
        // the clipboard.
        let mut read = pending(0);
        assert_eq!(
            read.on_selection_notify(0, 11, false, &[], 1_000),
            Step::Done(ClipboardContent::Unavailable)
        );
    }

    #[test]
    fn a_direct_answer_completes_in_one_step_and_zero_bytes_is_a_success() {
        let mut read = pending(0);
        assert_eq!(
            read.on_selection_notify(12, 11, false, b"hello", 1_000),
            Step::Done(ClipboardContent::Bytes(b"hello".to_vec()))
        );

        // A property that exists and is empty is a transfer of nothing, not a
        // failure — `Bytes(vec![])`, which the seam is explicit is distinct
        // from `Empty`.
        let mut empty = pending(0);
        let step = empty.on_selection_notify(12, 11, false, &[], 1_000);
        assert_eq!(step, Step::Done(ClipboardContent::Bytes(Vec::new())));
        assert_ne!(step, Step::Done(ClipboardContent::Empty));
    }

    #[test]
    fn an_incr_transfer_acknowledges_every_chunk_and_ends_on_the_empty_one() {
        // The handshake that a backend which forgets to delete the property
        // deadlocks on: the owner writes nothing more until the requestor
        // acknowledges.
        let mut read = pending(0);
        assert_eq!(
            read.on_selection_notify(12, 11, true, &4_096_u32.to_ne_bytes(), 10),
            Step::AcknowledgeChunk,
            "the INCR size estimate is ignored; the delete is what starts it"
        );
        assert_eq!(read.state, ReadState::Incremental);
        assert!(read.buffer.is_empty(), "the estimate is not data");

        assert_eq!(read.on_chunk(b"one ", 20), Step::AcknowledgeChunk);
        assert_eq!(read.on_chunk(b"two ", 30), Step::AcknowledgeChunk);
        assert_eq!(read.on_chunk(b"three", 40), Step::AcknowledgeChunk);
        assert_eq!(
            read.on_chunk(&[], 50),
            Step::Done(ClipboardContent::Bytes(b"one two three".to_vec()))
        );
        assert_eq!(read.last_progress_nanos, 50);
    }

    #[test]
    fn an_incr_transfer_of_nothing_is_a_terminator_and_an_empty_success() {
        let mut read = pending(0);
        assert_eq!(
            read.on_selection_notify(12, 11, true, &[], 10),
            Step::AcknowledgeChunk
        );
        assert_eq!(
            read.on_chunk(&[], 20),
            Step::Done(ClipboardContent::Bytes(Vec::new()))
        );
    }

    #[test]
    fn a_stalled_read_times_out_rather_than_staying_outstanding_forever() {
        // Obligation 4, and the one thing X11 does *not* get for free: a peer
        // that took the selection and then crashed sends no `SelectionNotify`
        // at all, and the server never notices.
        let read = pending(1_000);
        assert!(!read.is_stalled(1_000));
        assert!(!read.is_stalled(1_000 + TIMEOUT.as_nanos() as u64 - 1));
        assert!(read.is_stalled(1_000 + TIMEOUT.as_nanos() as u64));

        // Progress refreshes the deadline, so a slow-but-live transfer is not
        // cut off halfway.
        let mut slow = pending(0);
        let _ = slow.on_selection_notify(12, 11, true, &[], 0);
        for step in 1..=10u64 {
            let now = step * (TIMEOUT.as_nanos() as u64 - 1);
            assert!(!slow.is_stalled(now), "chunk {step}");
            assert_eq!(slow.on_chunk(b"x", now), Step::AcknowledgeChunk);
        }
    }

    #[test]
    fn a_peer_that_sends_too_much_is_cut_off_rather_than_believed() {
        let mut read = pending(0);
        let _ = read.on_selection_notify(12, 11, true, &[], 0);
        read.buffer = vec![0; MAX_BYTES - 1];
        assert_eq!(
            read.on_chunk(&[1, 2, 3], 10),
            Step::Done(ClipboardContent::Unavailable)
        );
    }

    #[test]
    fn the_best_available_spelling_wins_over_the_first_one_offered() {
        // Preference order, not the peer's listing order: a client offering
        // Latin-1 `STRING` first and `UTF8_STRING` second must still be read as
        // UTF-8, or every accented character pastes as mojibake.
        let utf8 = 101;
        let latin1 = 31;
        let candidates = [utf8, latin1];
        assert_eq!(choose_target(&candidates, &[latin1, utf8]), Some(utf8));
        assert_eq!(choose_target(&candidates, &[latin1]), Some(latin1));
        assert_eq!(choose_target(&candidates, &[utf8]), Some(utf8));
        // Nothing in common is not a failure — it is an empty clipboard *in
        // that format*, which is what `Empty` merges.
        assert_eq!(choose_target(&candidates, &[999]), None);
        assert_eq!(choose_target(&candidates, &[]), None);
        // An atom the server refused to intern is never chosen.
        assert_eq!(choose_target(&[0], &[0]), None);
    }

    #[test]
    fn a_peer_with_no_targets_support_is_asked_directly_rather_than_given_up_on() {
        // `TARGETS` is ICCCM and some real clients still do not implement it.
        // Treating a refusal as "nothing on the clipboard" would silently stop
        // pasting from them.
        let mut read = Read::new(
            ClipboardRequestId(2),
            window(),
            MimeType::TextUtf8,
            vec![77, 78],
            12,
            0,
        );
        assert_eq!(read.state, ReadState::AwaitingTargets);
        assert_eq!(read.on_targets(&[], 10), Step::Convert(77));
        assert_eq!(read.state, ReadState::AwaitingNotify);

        let mut nothing_in_common = Read::new(
            ClipboardRequestId(3),
            window(),
            MimeType::CrcblRon,
            vec![55],
            12,
            0,
        );
        assert_eq!(
            nothing_in_common.on_targets(&[77, 78], 10),
            Step::Done(ClipboardContent::Empty),
            "the owner has something, just not this"
        );
    }

    #[test]
    fn the_property_chunk_size_respects_the_servers_own_limit() {
        // `maximum_request_length` is in 4-byte units and covers the header, so
        // a chunk sized from it directly is one that the server rejects with
        // `BadLength` — halfway through a paste, where it looks like data loss.
        assert_eq!(
            max_property_bytes(65_535),
            (65_535 - 8) * 4,
            "a big-requests server"
        );
        assert_eq!(max_property_bytes(4_096), (4_096 - 8) * 4);
        // Never below one page, whatever a degenerate server claims.
        assert_eq!(max_property_bytes(0), 4_096);
        assert_eq!(max_property_bytes(8), 4_096);
        assert!(
            max_property_bytes(u16::MAX).is_multiple_of(4),
            "word-aligned"
        );
    }

    #[test]
    fn a_write_always_ends_with_an_empty_chunk_even_when_it_divides_evenly() {
        // The terminator is a real `ChangeProperty`, not the absence of one. A
        // transfer whose length is a multiple of the chunk size is exactly
        // where an implementation forgets it, and the peer then waits forever.
        let mut write = Write::new(1, 2, 3, b"abcdef".to_vec(), 3, 0);
        assert_eq!(write.next_chunk(1), Some(&b"abc"[..]));
        assert_eq!(write.next_chunk(2), Some(&b"def"[..]));
        assert_eq!(write.next_chunk(3), Some(&[][..]), "the terminator");
        assert_eq!(write.next_chunk(4), None, "and nothing after it");
        assert!(write.terminated);

        let mut ragged = Write::new(1, 2, 3, b"abcde".to_vec(), 3, 0);
        assert_eq!(ragged.next_chunk(1), Some(&b"abc"[..]));
        assert_eq!(ragged.next_chunk(2), Some(&b"de"[..]));
        assert_eq!(ragged.next_chunk(3), Some(&[][..]));
        assert_eq!(ragged.next_chunk(4), None);

        // An empty payload is one terminator and nothing else.
        let mut nothing = Write::new(1, 2, 3, Vec::new(), 4096, 0);
        assert_eq!(nothing.next_chunk(1), Some(&[][..]));
        assert_eq!(nothing.next_chunk(2), None);
    }

    #[test]
    fn a_write_to_a_peer_that_stops_acknowledging_is_abandoned() {
        let mut write = Write::new(1, 2, 3, vec![0; 10], 4, 1_000);
        assert!(!write.is_stalled(1_000));
        let _ = write.next_chunk(2_000);
        assert!(!write.is_stalled(2_000));
        assert!(write.is_stalled(2_000 + TIMEOUT.as_nanos() as u64));
        // A zero chunk size would be an infinite loop; the constructor floors
        // it at one byte.
        assert_eq!(Write::new(1, 2, 3, vec![7], 0, 0).chunk, 1);
    }
}
