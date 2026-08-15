//! The canonical replies: one of every reply shape, with no two fields alike.
//!
//! A sibling of [`corpus`](crate::corpus) rather than part of it, because the
//! two are needed by different test binaries: `stream.rs` has no use for a reply
//! and every integration test compiles the modules it declares. Splitting by
//! direction is what lets each binary declare only what it exercises — and what
//! keeps `cargo clippy --all-targets` honest about the rest, since a helper
//! nothing calls is a helper nothing checks.
//!
//! Shared between `reply.rs` and `fixture.rs` for the reason the command corpus
//! is shared: both are about the *same* bytes, and a second copy would be a
//! second thing to keep in step.

use crcbl_webgpu::{Reply, ReplyWriter};

use crate::corpus::handle;

/// A payload big enough to force a writer past its initial buffer, with a value
/// per byte so a truncation or a stale write shows up as a byte difference
/// rather than as a length.
///
/// Generated rather than spelled out — the one place in either corpus that is —
/// because five hundred literals would be unreadable and would say nothing a
/// reader could check. `web/tools/reply-encode.mjs` computes the same series
/// from the same rule, which is what keeps it a shared expectation rather than
/// a shared decoder.
fn growth_payload() -> Vec<u8> {
    (0..512u32).map(|i| ((i * 7) % 251) as u8).collect()
}

/// One of every reply this slice encodes, each with the sequence it answers.
///
/// Distinct values throughout, for [`every_command`]'s reason. Two things here
/// are deliberate beyond that:
///
/// * **The sequences are not in order and not contiguous.** Replies arrive when
///   the browser has an answer, so a decoder that assumed position — as the
///   command stream legitimately does — would be wrong here, and this corpus is
///   what would catch it.
/// * **One sequence and one value are past 2³².** The JavaScript half must carry
///   both as `BigInt`; read as numbers they round, and round to something that
///   still looks plausible.
pub fn every_reply() -> Vec<(u64, Reply)> {
    vec![
        (
            9,
            Reply::Adapter {
                id: 3,
                name: "Apple M2 — ✱".into(),
            },
        ),
        // The empty twin: a name the browser declined to give is still a name
        // field, and its length prefix still has to be read.
        (
            2,
            Reply::Adapter {
                id: 0,
                name: String::new(),
            },
        ),
        (
            17,
            Reply::ReadbackPending {
                readback: handle(51, 52),
            },
        ),
        (
            5,
            Reply::ReadbackReady {
                readback: handle(53, 54),
                data: vec![0x0B, 0xAD, 0xF0, 0x0D],
            },
        ),
        (
            23,
            Reply::ReadbackReady {
                readback: handle(55, 56),
                data: Vec::new(),
            },
        ),
        (
            31,
            Reply::ReadbackReady {
                readback: handle(61, 62),
                // **Long enough to make a writer grow its buffer**, which is
                // the case a small corpus never reaches: the JavaScript writer
                // starts at a fixed capacity and doubles, and a growth that
                // wrote through the view it had before growing would land here
                // rather than in a browser. It did, the first time this corpus
                // was short enough to fit.
                data: growth_payload(),
            },
        ),
        (
            0x0000_0001_0000_002A,
            Reply::QueryResults {
                set: handle(57, 58),
                first_query: 4,
                values: vec![u64::MAX, 0, 1_234_567_890_123],
            },
        ),
        (
            11,
            Reply::QueryResults {
                set: handle(59, 60),
                first_query: 0,
                values: Vec::new(),
            },
        ),
        // The other half of an enumeration's answer: one command is answered
        // exactly once, so "no adapter" cannot be an `Adapter` carrying a
        // sentinel and has a reply of its own.
        //
        // **Appended rather than filed beside the two `Adapter`s**, where it
        // reads better: every entry below an insertion point moves down one, and
        // two of these sequences would then land on their own index — which is
        // the coincidence `the_corpus_would_notice_a_sequence_read_from_a_position`
        // exists to rule out.
        (
            13,
            Reply::NoAdapter {
                reason: "requestAdapter() resolved null — ✱".into(),
            },
        ),
        // Its empty twin. There is no absent/present distinction to make here —
        // the reason is a bare length-prefixed string — but a browser that
        // refused without saying why still has to encode and decode as a
        // refusal rather than as a short buffer.
        (
            1,
            Reply::NoAdapter {
                reason: String::new(),
            },
        ),
    ]
}

/// Encodes `reply` through the writer method it came from.
///
/// Exhaustive, so a variant added to [`Reply`] stops this file compiling — which
/// is the point at which the suites that use it are impossible to leave
/// un-extended.
pub fn encode_reply(replies: &mut ReplyWriter, sequence: u64, reply: &Reply) {
    match reply {
        Reply::Adapter { id, name } => replies.adapter(sequence, *id, name),
        Reply::NoAdapter { reason } => replies.no_adapter(sequence, reason),
        Reply::ReadbackPending { readback } => replies.readback_pending(sequence, *readback),
        Reply::ReadbackReady { readback, data } => {
            replies.readback_ready(sequence, *readback, data);
        }
        Reply::QueryResults {
            set,
            first_query,
            values,
        } => replies.query_results(sequence, *set, *first_query, values),
    }
}

/// A buffer holding every reply in [`every_reply`], in order.
pub fn encode_all_replies() -> (ReplyWriter, Vec<(u64, Reply)>) {
    let expected = every_reply();
    let mut replies = ReplyWriter::new();
    for (sequence, reply) in &expected {
        encode_reply(&mut replies, *sequence, reply);
    }
    (replies, expected)
}
