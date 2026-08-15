//! The encoding's own suite: every command shape out and back, and every way a
//! buffer can be wrong.
//!
//! An integration test rather than a `#[cfg(test)]` module, because the encoding
//! is a public contract and this is the only place it gets exercised the way its
//! callers will: through the crate's exported surface, with nothing `pub(crate)`
//! in reach.

mod corpus;

use crcbl_hal::{BufferDesc, BufferUsage, MemoryLocation, ShaderStages};
use crcbl_webgpu::{Command, DecodeError, StreamReader, StreamWriter, decode_stream, tag};

use corpus::{encode_all, every_command, handle};

// ── Round trips ───────────────────────────────────────────────────────────────

#[test]
fn every_command_shape_survives_a_round_trip_field_for_field() {
    let (stream, expected) = encode_all();
    let decoded = decode_stream(stream.bytes()).expect("a stream this crate wrote decodes");
    assert_eq!(decoded, expected);
}

#[test]
fn a_stream_holding_no_commands_is_a_header_and_decodes_to_nothing() {
    let stream = StreamWriter::new();
    assert_eq!(stream.bytes().len(), tag::HEADER_BYTES);
    assert_eq!(decode_stream(stream.bytes()), Ok(Vec::new()));
}

#[test]
fn sequence_numbers_count_from_the_header_and_carry_across_a_clear() {
    let mut stream = StreamWriter::new();
    assert_eq!(stream.base_sequence(), 0);
    let first = stream.draw(0..3, 0..1);
    let second = stream.draw(0..6, 0..2);
    assert_eq!((first, second), (0, 1));

    // What the reader recovers must be what the writer handed the caller: this
    // is the whole of the attribution story, since nothing per command is on
    // the wire.
    let mut reader = StreamReader::new(stream.bytes()).expect("the header is this crate's");
    assert_eq!(reader.base_sequence(), 0);
    let sequences: Vec<u64> = std::iter::from_fn(|| reader.next_command())
        .map(|next| next.expect("a stream this crate wrote decodes").0)
        .collect();
    assert_eq!(sequences, vec![first, second]);

    // A cleared buffer is a fresh header, and the counter does not restart: an
    // error raised by a replayed command surfaces a frame or more after the
    // frame that encoded it.
    stream.clear();
    assert_eq!(stream.bytes().len(), tag::HEADER_BYTES);
    assert_eq!(stream.base_sequence(), 2);
    let third = stream.draw(0..9, 0..3);
    assert_eq!(third, 2);

    let mut reader = StreamReader::new(stream.bytes()).expect("the header is this crate's");
    assert_eq!(reader.base_sequence(), 2);
    let (sequence, command) = reader
        .next_command()
        .expect("one command was encoded")
        .expect("a stream this crate wrote decodes");
    assert_eq!(sequence, third);
    assert_eq!(
        command,
        Command::Draw {
            vertices: 0..9,
            instances: 0..3
        }
    );
}

/// The argument `docs/plan/41-webgpu-stream.md` calls the one most easily
/// dropped: `bind_group` and `push_constants` both take the pipeline layout
/// *last*, after a variable-length field.
#[test]
fn the_pipeline_layout_that_comes_last_is_on_the_wire_and_not_confused_with_the_group() {
    let group = handle(1, 2);
    let layout = handle(3, 4);
    let mut stream = StreamWriter::new();
    stream.bind_group(0, group, &[7], layout);
    stream.push_constants(ShaderStages::COMPUTE, 0, &[9], layout);

    let decoded = decode_stream(stream.bytes()).expect("a stream this crate wrote decodes");
    match &decoded[0] {
        Command::BindGroup {
            group: g,
            layout: l,
            ..
        } => {
            assert_eq!(*g, group);
            assert_eq!(*l, layout);
            assert_ne!(
                g.to_bits(),
                l.to_bits(),
                "the test would not notice a swap otherwise"
            );
        }
        other => panic!("expected BindGroup, got {}", other.name()),
    }
    match &decoded[1] {
        Command::PushConstants { layout: l, .. } => assert_eq!(*l, layout),
        other => panic!("expected PushConstants, got {}", other.name()),
    }
}

/// `Some("")` means present and empty; `None` means absent. Conflating them is
/// how a truncated file turns into "this backend does not get WGSL" once the
/// shader-module command lands.
#[test]
fn an_empty_label_is_not_an_absent_one() {
    let mut empty = StreamWriter::new();
    empty.create_buffer(
        handle(1, 1),
        &BufferDesc {
            label: Some(""),
            size: 4,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        },
    );
    let mut absent = StreamWriter::new();
    absent.create_buffer(
        handle(1, 1),
        &BufferDesc {
            label: None,
            size: 4,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        },
    );

    assert_ne!(empty.bytes(), absent.bytes());
    let empty = decode_stream(empty.bytes()).expect("a stream this crate wrote decodes");
    let absent = decode_stream(absent.bytes()).expect("a stream this crate wrote decodes");
    match (&empty[0], &absent[0]) {
        (
            Command::CreateBuffer { label: empty, .. },
            Command::CreateBuffer { label: absent, .. },
        ) => {
            assert_eq!(empty.as_deref(), Some(""));
            assert_eq!(*absent, None);
        }
        _ => panic!("expected two CreateBuffers"),
    }
}

/// The replayer's obligation seen from the decoder's side: the decode consults
/// no table, so a destroy naming an id nothing created is a well-formed command
/// rather than corruption. The replayer turns it into a no-op.
#[test]
fn a_destroy_for_an_id_the_stream_never_created_decodes_cleanly() {
    let mut stream = StreamWriter::new();
    stream.destroy_buffer(handle(9999, 7));
    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![Command::DestroyBuffer {
            buffer: handle(9999, 7)
        }])
    );
}

#[test]
fn every_command_has_its_own_name() {
    let commands = every_command();
    let mut names: Vec<&str> = commands.iter().map(Command::name).collect();
    names.sort_unstable();
    names.dedup();
    // `every_command` holds three CreateBuffers, two BeginRenderPasses and two
    // BindGroups, so the distinct-name count is what the writer has methods for.
    assert_eq!(names.len(), 8);
    assert!(names.iter().all(|name| !name.is_empty()));
}

// ── Malformed streams ─────────────────────────────────────────────────────────

#[test]
fn a_buffer_without_this_formats_header_is_refused_before_any_command_is_read() {
    assert_eq!(
        decode_stream(&[]),
        Err(DecodeError::TooShort {
            needed: 8,
            offset: 0,
            remaining: 0
        })
    );
    assert_eq!(
        decode_stream(b"NOTASTREAM__________"),
        Err(DecodeError::BadMagic)
    );

    let mut wrong_version = StreamWriter::new().bytes().to_vec();
    let bumped = tag::STREAM_VERSION + 1;
    wrong_version[8..10].copy_from_slice(&bumped.to_le_bytes());
    assert_eq!(
        decode_stream(&wrong_version),
        Err(DecodeError::UnsupportedVersion {
            found: bumped,
            expected: tag::STREAM_VERSION,
        })
    );

    // A header cut short is short, not corrupt.
    let whole = StreamWriter::new();
    for cut in 0..tag::HEADER_BYTES {
        assert!(
            matches!(
                decode_stream(&whole.bytes()[..cut]),
                Err(DecodeError::TooShort { .. } | DecodeError::BadMagic)
            ),
            "a {cut}-byte header decoded as something"
        );
    }
}

/// The stated reason the house style puts the tag first: without it, "unknown
/// command" and "malformed known command" are the same error.
#[test]
fn an_unknown_tag_is_reported_as_unknown_rather_than_as_a_malformed_known_command() {
    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(0xFF);
    assert_eq!(
        decode_stream(&stream),
        Err(DecodeError::UnknownTag { tag: 0xFF })
    );

    // A known tag with nothing behind it is a different error, from the same
    // number of bytes.
    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(tag::DRAW_TAG);
    assert!(matches!(
        decode_stream(&stream),
        Err(DecodeError::TooShort { .. })
    ));
}

#[test]
fn truncating_any_command_yields_too_short_rather_than_a_partial_decode() {
    let (stream, _) = encode_all();
    let whole = stream.bytes();
    for cut in tag::HEADER_BYTES..whole.len() {
        let decoded = decode_stream(&whole[..cut]);
        match decoded {
            // A cut that lands exactly on a command boundary is a shorter but
            // perfectly well-formed stream.
            Ok(commands) => assert!(
                commands.len() < every_command().len(),
                "truncating to {cut} bytes decoded the whole stream"
            ),
            Err(DecodeError::TooShort { .. } | DecodeError::InvalidLength { .. }) => {}
            Err(other) => panic!("truncating to {cut} bytes gave {other}"),
        }
    }
}

#[test]
fn a_length_prefix_past_the_cap_is_refused_rather_than_allocated_for() {
    // `push_constants` is the unbounded byte payload: tag, stages, offset, then
    // the length prefix.
    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(tag::PUSH_CONSTANTS_TAG);
    stream.extend_from_slice(&ShaderStages::VERTEX.bits().to_le_bytes());
    stream.extend_from_slice(&0u32.to_le_bytes());
    stream.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&stream),
        Err(DecodeError::InvalidLength {
            field: "PushConstants::data",
            len: u32::MAX,
        })
    );

    // `bind_group` is the element count: tag, slot, group, then the count.
    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(tag::BIND_GROUP_TAG);
    stream.extend_from_slice(&0u32.to_le_bytes());
    stream.extend_from_slice(&handle::<()>(1, 1).to_bits().to_le_bytes());
    stream.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&stream),
        Err(DecodeError::InvalidLength {
            field: "BindGroup::dynamic_offsets",
            len: u32::MAX,
        })
    );
}

/// `u32::MAX` above pins that *a* cap exists; it does not pin **which**. Both
/// bounds in `read_count` — the cap, and "more elements than there are bytes
/// left" — raise the same variant with the same field, so a count huge enough
/// to trip the cap trips the second one too and the assertion above survives
/// the cap being moved. Doubling `MAX_ELEMENT_COUNT` was confirmed to leave the
/// whole suite green.
///
/// So this one asks for exactly one element past the cap and supplies more than
/// enough bytes to hold them, which is the only shape where the two bounds
/// disagree. With the cap where it is, the count is refused. With the cap
/// raised, decoding runs on past the count and fails somewhere else with a
/// different variant — which is why the variant is asserted and not merely the
/// fact of an error.
#[test]
fn the_element_cap_is_the_one_the_tag_module_names() {
    // **Written out, not derived.** `MAX_ELEMENT_COUNT + 1` is one past the cap
    // whatever the cap is, so a test using it agrees with itself for ever — that
    // was the first version of this, and doubling the constant left it green.
    // The literal is what does the pinning; the equality below is what says so
    // out loud when someone moves the cap on purpose.
    const PAST_THE_CAP: u32 = 65_537;
    assert_eq!(
        tag::MAX_ELEMENT_COUNT,
        PAST_THE_CAP as usize - 1,
        "the cap moved, so this test's literal has to move with it — deliberately"
    );

    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(tag::BIND_GROUP_TAG);
    stream.extend_from_slice(&0u32.to_le_bytes());
    stream.extend_from_slice(&handle::<()>(1, 1).to_bits().to_le_bytes());
    stream.extend_from_slice(&PAST_THE_CAP.to_le_bytes());
    // One filler byte per element the count claims, so `count > remaining()` is
    // false and the cap is the only bound that can refuse this.
    stream.resize(stream.len() + PAST_THE_CAP as usize, 0);

    assert_eq!(
        decode_stream(&stream),
        Err(DecodeError::InvalidLength {
            field: "BindGroup::dynamic_offsets",
            len: PAST_THE_CAP,
        })
    );
}

#[test]
#[should_panic(expected = "past the stream's cap")]
fn the_writer_refuses_to_encode_a_field_the_reader_would_refuse() {
    let oversized = vec![0u8; crcbl_webgpu::tag::MAX_FIELD_BYTES + 1];
    StreamWriter::new().push_constants(ShaderStages::COMPUTE, 0, &oversized, handle(1, 1));
}

#[test]
fn a_code_no_variant_claims_is_refused_rather_than_folded_into_a_neighbour() {
    // The last byte of a `create_buffer` body is the memory location.
    let mut stream = StreamWriter::new();
    stream.create_buffer(
        handle(1, 1),
        &BufferDesc {
            label: None,
            size: 4,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        },
    );
    let mut bytes = stream.bytes().to_vec();
    *bytes.last_mut().expect("the body ends in a memory code") = 0x7F;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BufferDesc::memory",
            code: 0x7F,
        })
    );

    // A usage bit no `BufferUsage` flag claims is the same kind of refusal.
    let mut bytes = stream.bytes().to_vec();
    let usage_at = bytes.len() - 5;
    bytes[usage_at..usage_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BufferDesc::usage",
            code: u32::MAX,
        })
    );
}

#[test]
fn a_handle_field_that_cannot_be_absent_refuses_zero_bits() {
    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(tag::DESTROY_BUFFER_TAG);
    stream.extend_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        decode_stream(&stream),
        Err(DecodeError::NullHandle {
            field: "DestroyBuffer::buffer"
        })
    );
}

#[test]
fn a_label_that_is_not_utf8_is_refused_rather_than_replaced_with_question_marks() {
    let mut stream = StreamWriter::new();
    stream.begin_debug_label("ok");
    let mut bytes = stream.bytes().to_vec();
    let len = bytes.len();
    bytes[len - 2..].copy_from_slice(&[0xFF, 0xFE]);
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::NotUtf8 {
            field: "BeginDebugLabel::label"
        })
    );
}

#[test]
fn a_reader_that_has_failed_stays_failed_rather_than_resyncing_mid_body() {
    let mut stream = StreamWriter::new().bytes().to_vec();
    stream.push(0xFF);
    stream.push(tag::DRAW_TAG);
    stream.extend_from_slice(&[0; 16]);

    let mut reader = StreamReader::new(&stream).expect("the header is this crate's");
    assert_eq!(
        reader.next_command(),
        Some(Err(DecodeError::UnknownTag { tag: 0xFF }))
    );
    assert_eq!(
        reader.next_command(),
        None,
        "the cursor is inside a body, so the next byte is not a tag"
    );
}

/// Arbitrary bytes behind a valid header must produce an error, never a panic
/// and never a plausible-looking command built out of noise.
#[test]
fn no_byte_sequence_makes_the_decoder_panic() {
    let header = StreamWriter::new().bytes().to_vec();
    // Deterministic xorshift64, seeded by the format's own magic.
    let mut state = u64::from_le_bytes(*tag::STREAM_MAGIC);
    for _ in 0..2_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;

        let mut stream = header.clone();
        let len = (state % 96) as usize;
        let mut bits = state;
        for _ in 0..len {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            stream.push(bits as u8);
        }
        let _ = decode_stream(&stream);
        // …and without a valid header, so `new` is fuzzed too.
        let _ = decode_stream(&stream[..stream.len().min(tag::HEADER_BYTES + len / 2)]);
    }
}
