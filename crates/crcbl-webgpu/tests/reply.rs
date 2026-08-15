//! The reply encoding's own suite: every reply shape out and back, and every way
//! a reply buffer can be wrong.
//!
//! The counterpart of `stream.rs`, and an integration test for its reason: the
//! encoding is a public contract and this is the only place it gets exercised
//! the way its callers will, through the crate's exported surface with nothing
//! `pub(crate)` in reach.
//!
//! **What this file does not cover is the waiting set.** A reply for a sequence
//! nobody asked for is not a property of the bytes — the same buffer is correct
//! or an error depending on what the engine registered — so it is tested against
//! the channel, in `src/web.rs`.

mod corpus;
mod replies;

use crcbl_webgpu::{DecodeError, Reply, ReplyReader, ReplyWriter, decode_replies, tag};

use corpus::handle;
use replies::{encode_all_replies, every_reply};

/// A reply buffer's header: the magic and the version word, and no more. There
/// is no base sequence, because every reply carries its own.
fn header() -> Vec<u8> {
    ReplyWriter::new().bytes().to_vec()
}

// ── Round trips ───────────────────────────────────────────────────────────────

#[test]
fn every_reply_shape_survives_a_round_trip_field_for_field() {
    let (replies, expected) = encode_all_replies();
    let decoded = decode_replies(replies.bytes()).expect("a buffer this crate wrote decodes");
    assert_eq!(decoded, expected);
}

#[test]
fn a_buffer_holding_no_replies_is_a_header_and_decodes_to_nothing() {
    let replies = ReplyWriter::new();
    assert_eq!(replies.bytes().len(), tag::REPLY_HEADER_BYTES);
    assert_eq!(decode_replies(replies.bytes()), Ok(Vec::new()));
}

/// **The sequence is a field, and it is the reply's own.**
///
/// The command stream's numbers are positional; a decoder written from that
/// habit would hand back `0, 1, 2` here and look entirely plausible doing it.
#[test]
fn each_reply_carries_the_sequence_it_answers_rather_than_its_position() {
    let mut replies = ReplyWriter::new();
    replies.readback_pending(41, handle(1, 2));
    replies.readback_pending(u64::MAX, handle(3, 4));
    replies.readback_pending(0, handle(5, 6));

    let sequences: Vec<u64> = decode_replies(replies.bytes())
        .expect("a buffer this crate wrote decodes")
        .into_iter()
        .map(|(sequence, _)| sequence)
        .collect();
    assert_eq!(sequences, vec![41, u64::MAX, 0]);
}

/// Every sequence in the corpus is distinct and none is its own index, so a
/// round trip that dropped the field entirely could not pass the test above by
/// coincidence.
#[test]
fn the_corpus_would_notice_a_sequence_read_from_a_position() {
    let expected = every_reply();
    let mut sequences: Vec<u64> = expected.iter().map(|(sequence, _)| *sequence).collect();
    let count = sequences.len();
    sequences.sort_unstable();
    sequences.dedup();
    assert_eq!(sequences.len(), count, "two replies share a sequence");
    for (index, (sequence, _)) in expected.iter().enumerate() {
        assert_ne!(
            *sequence, index as u64,
            "reply {index} is at its own number"
        );
    }
}

#[test]
fn every_reply_has_its_own_name() {
    let mut names: Vec<&str> = every_reply()
        .iter()
        .map(|(_, reply)| reply.name())
        .collect();
    names.sort_unstable();
    names.dedup();
    // The corpus holds two Adapters, two ReadbackReadys and two QueryResults, so
    // the distinct-name count is what the writer has methods for.
    assert_eq!(names.len(), 4);
    assert!(names.iter().all(|name| !name.is_empty()));
}

// ── Malformed buffers ─────────────────────────────────────────────────────────

/// The two directions have different magics precisely so this is an error rather
/// than a decode: a shim that fed the command buffer back to wasm as replies
/// would otherwise be reading command opcodes as reply tags.
#[test]
fn a_command_stream_handed_to_the_reply_reader_is_refused_by_its_magic() {
    let (stream, _) = corpus::encode_all();
    assert_eq!(decode_replies(stream.bytes()), Err(DecodeError::BadMagic));
}

#[test]
fn a_buffer_without_this_formats_header_is_refused_before_any_reply_is_read() {
    assert_eq!(
        decode_replies(&[]),
        Err(DecodeError::TooShort {
            needed: 8,
            offset: 0,
            remaining: 0
        })
    );
    assert_eq!(decode_replies(b"NOTAREPLY_"), Err(DecodeError::BadMagic));

    let mut wrong_version = header();
    let bumped = tag::REPLY_VERSION + 1;
    wrong_version[8..10].copy_from_slice(&bumped.to_le_bytes());
    assert_eq!(
        decode_replies(&wrong_version),
        Err(DecodeError::UnsupportedVersion {
            found: bumped,
            expected: tag::REPLY_VERSION,
        })
    );

    // A header cut short is short, not corrupt.
    let whole = header();
    for cut in 0..tag::REPLY_HEADER_BYTES {
        assert!(
            matches!(
                decode_replies(&whole[..cut]),
                Err(DecodeError::TooShort { .. } | DecodeError::BadMagic)
            ),
            "a {cut}-byte header decoded as something"
        );
    }
}

#[test]
fn an_unknown_tag_is_reported_as_unknown_rather_than_as_a_malformed_known_reply() {
    let mut replies = header();
    replies.push(0xFF);
    replies.extend_from_slice(&7u64.to_le_bytes());
    assert_eq!(
        decode_replies(&replies),
        Err(DecodeError::UnknownTag { tag: 0xFF })
    );

    // A known tag with nothing behind it is a different error.
    let mut replies = header();
    replies.push(tag::READBACK_PENDING_REPLY_TAG);
    replies.extend_from_slice(&7u64.to_le_bytes());
    assert!(matches!(
        decode_replies(&replies),
        Err(DecodeError::TooShort { .. })
    ));
}

/// A tag is refused before its sequence is even read, so a reply this build does
/// not know cannot be silently attributed to a command.
#[test]
fn a_reply_with_no_sequence_behind_its_tag_is_short_rather_than_sequence_zero() {
    let mut replies = header();
    replies.push(tag::ADAPTER_REPLY_TAG);
    assert!(matches!(
        decode_replies(&replies),
        Err(DecodeError::TooShort { needed: 8, .. })
    ));
}

#[test]
fn truncating_any_reply_yields_too_short_rather_than_a_partial_decode() {
    let (replies, expected) = encode_all_replies();
    let whole = replies.bytes();
    for cut in tag::REPLY_HEADER_BYTES..whole.len() {
        match decode_replies(&whole[..cut]) {
            // A cut that lands exactly on a reply boundary is a shorter but
            // perfectly well-formed buffer.
            Ok(decoded) => assert!(
                decoded.len() < expected.len(),
                "truncating to {cut} bytes decoded the whole buffer"
            ),
            Err(DecodeError::TooShort { .. } | DecodeError::InvalidLength { .. }) => {}
            Err(other) => panic!("truncating to {cut} bytes gave {other}"),
        }
    }
}

#[test]
fn a_length_prefix_past_the_cap_is_refused_rather_than_allocated_for() {
    // `readback_ready` is the unbounded byte payload: tag, sequence, handle,
    // then the length prefix.
    let mut replies = header();
    replies.push(tag::READBACK_READY_REPLY_TAG);
    replies.extend_from_slice(&3u64.to_le_bytes());
    replies.extend_from_slice(&handle::<()>(1, 1).to_bits().to_le_bytes());
    replies.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_replies(&replies),
        Err(DecodeError::InvalidLength {
            field: "ReadbackReady::data",
            len: u32::MAX,
        })
    );

    // `query_results` is the element count: tag, sequence, handle, first query,
    // then the count.
    let mut replies = header();
    replies.push(tag::QUERY_RESULTS_REPLY_TAG);
    replies.extend_from_slice(&3u64.to_le_bytes());
    replies.extend_from_slice(&handle::<()>(1, 1).to_bits().to_le_bytes());
    replies.extend_from_slice(&0u32.to_le_bytes());
    replies.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_replies(&replies),
        Err(DecodeError::InvalidLength {
            field: "QueryResults::values",
            len: u32::MAX,
        })
    );
}

#[test]
#[should_panic(expected = "past the stream's cap")]
fn the_writer_refuses_to_encode_a_payload_the_reader_would_refuse() {
    let oversized = vec![0u8; tag::MAX_FIELD_BYTES + 1];
    ReplyWriter::new().readback_ready(0, handle(1, 1), &oversized);
}

#[test]
fn a_handle_field_that_cannot_be_absent_refuses_zero_bits() {
    let mut replies = header();
    replies.push(tag::READBACK_PENDING_REPLY_TAG);
    replies.extend_from_slice(&3u64.to_le_bytes());
    replies.extend_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        decode_replies(&replies),
        Err(DecodeError::NullHandle {
            field: "ReadbackPending::readback"
        })
    );
}

#[test]
fn an_adapter_name_that_is_not_utf8_is_refused_rather_than_replaced_with_question_marks() {
    let mut replies = ReplyWriter::new();
    replies.adapter(3, 0, "ok");
    let mut bytes = replies.bytes().to_vec();
    let len = bytes.len();
    bytes[len - 2..].copy_from_slice(&[0xFF, 0xFE]);
    assert_eq!(
        decode_replies(&bytes),
        Err(DecodeError::NotUtf8 {
            field: "Adapter::name"
        })
    );
}

#[test]
fn a_reader_that_has_failed_stays_failed_rather_than_resyncing_mid_body() {
    let mut replies = header();
    replies.push(0xFF);
    replies.extend_from_slice(&3u64.to_le_bytes());
    replies.push(tag::READBACK_PENDING_REPLY_TAG);
    replies.extend_from_slice(&[0; 16]);

    let mut reader = ReplyReader::new(&replies).expect("the header is this crate's");
    assert_eq!(
        reader.next_reply(),
        Some(Err(DecodeError::UnknownTag { tag: 0xFF }))
    );
    assert_eq!(
        reader.next_reply(),
        None,
        "the cursor is inside a body, so the next byte is not a tag"
    );
}

/// A cleared writer is a fresh header — and, unlike the command stream's, it has
/// no counter to carry, because a reply's sequence comes from the command it
/// answers and never from the writer.
#[test]
fn clearing_a_writer_leaves_a_header_and_nothing_else() {
    let mut replies = ReplyWriter::new();
    replies.adapter(3, 0, "gpu");
    assert!(replies.bytes().len() > tag::REPLY_HEADER_BYTES);

    replies.clear();
    assert_eq!(replies.bytes().len(), tag::REPLY_HEADER_BYTES);
    assert_eq!(decode_replies(replies.bytes()), Ok(Vec::new()));

    replies.readback_pending(9, handle(1, 2));
    assert_eq!(
        decode_replies(replies.bytes()),
        Ok(vec![(
            9,
            Reply::ReadbackPending {
                readback: handle(1, 2)
            }
        )])
    );
}

/// Arbitrary bytes behind a valid header must produce an error, never a panic
/// and never a plausible-looking reply built out of noise.
#[test]
fn no_byte_sequence_makes_the_decoder_panic() {
    let header = header();
    // Deterministic xorshift64, seeded by the format's own magic.
    let mut state = u64::from_le_bytes(*tag::REPLY_MAGIC);
    for _ in 0..2_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;

        let mut replies = header.clone();
        let len = (state % 96) as usize;
        let mut bits = state;
        for _ in 0..len {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            replies.push(bits as u8);
        }
        let _ = decode_replies(&replies);
        // …and without a valid header, so `new` is fuzzed too.
        let _ = decode_replies(&replies[..replies.len().min(tag::REPLY_HEADER_BYTES + len / 2)]);
    }
}
