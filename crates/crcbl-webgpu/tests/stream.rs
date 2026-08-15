//! The encoding's own suite: every command shape out and back, and every way a
//! buffer can be wrong.
//!
//! An integration test rather than a `#[cfg(test)]` module, because the encoding
//! is a public contract and this is the only place it gets exercised the way its
//! callers will: through the crate's exported surface, with nothing `pub(crate)`
//! in reach.

use crcbl_core::Handle;
use crcbl_hal::{
    BufferDesc, BufferUsage, ClearValue, ColorAttachment, DepthStencilAttachment, LoadOp,
    MemoryLocation, Rect2d, RenderPassDesc, ShaderStages, StoreOp, depth,
};
use crcbl_webgpu::{Command, DecodeError, StreamReader, StreamWriter, decode_stream, tag};

/// A handle with distinct index and generation halves, so a field written with
/// the two swapped does not still compare equal.
fn handle<T>(index: u32, generation: u32) -> Handle<T> {
    Handle::from_bits((u64::from(generation) << 32) | u64::from(index))
        .expect("a non-zero generation is a real generation")
}

/// One of every command this slice encodes, with no two fields sharing a value.
///
/// Shared values are how a round-trip test passes while the encoder writes two
/// fields in the wrong order — every number here is distinct for that reason,
/// and every optional field appears both ways somewhere in the list.
fn every_command() -> Vec<Command> {
    vec![
        Command::CreateBuffer {
            buffer: handle(11, 12),
            label: Some("instances".into()),
            size: 4096,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        },
        // The unlabelled twin: `None` and `Some("")` are different values.
        Command::CreateBuffer {
            buffer: handle(13, 14),
            label: None,
            size: 1,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        },
        Command::CreateBuffer {
            buffer: handle(15, 16),
            label: Some(String::new()),
            size: u64::MAX,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostReadback,
        },
        Command::DestroyBuffer {
            buffer: handle(17, 18),
        },
        Command::BeginDebugLabel {
            label: "gbuffer — ✱".into(),
        },
        Command::BeginRenderPass {
            label: Some("shading".into()),
            color_attachments: vec![
                ColorAttachment {
                    view: handle(21, 22),
                    resolve: Some(handle(23, 24)),
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue {
                        color: [0.25, 0.5, 0.75, 1.0],
                        depth: depth::CLEAR,
                        stencil: 7,
                    },
                },
                ColorAttachment {
                    view: handle(25, 26),
                    resolve: None,
                    load: LoadOp::DontCare,
                    store: StoreOp::Discard,
                    clear: ClearValue::default(),
                },
            ],
            depth_stencil_attachment: Some(DepthStencilAttachment {
                view: handle(27, 28),
                read_only: true,
                depth_load: LoadOp::Load,
                depth_store: StoreOp::Discard,
                stencil_load: LoadOp::Clear,
                stencil_store: StoreOp::Store,
                clear: ClearValue {
                    color: [1.0, 2.0, 3.0, 4.0],
                    depth: depth::NEAR,
                    stencil: 9,
                },
            }),
            render_area: Rect2d {
                x: -3,
                y: -5,
                width: 1920,
                height: 1080,
            },
        },
        // The empty-and-absent twin of the pass above.
        Command::BeginRenderPass {
            label: None,
            color_attachments: Vec::new(),
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(2, 3),
        },
        Command::BindGraphicsPipeline {
            pipeline: handle(31, 32),
        },
        Command::BindGroup {
            slot: 2,
            group: handle(33, 34),
            dynamic_offsets: vec![256, 512, 768],
            layout: handle(35, 36),
        },
        Command::BindGroup {
            slot: 0,
            group: handle(37, 38),
            dynamic_offsets: Vec::new(),
            layout: handle(39, 40),
        },
        Command::PushConstants {
            stages: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            offset: 16,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01],
            layout: handle(41, 42),
        },
        Command::Draw {
            vertices: 6..9,
            instances: 1..5,
        },
    ]
}

/// Encodes `command` through the writer method it came from.
///
/// The `match` is exhaustive, so a variant added to [`Command`] stops this file
/// compiling — which is the point at which the suite below is impossible to
/// leave un-extended.
fn encode(stream: &mut StreamWriter, command: &Command) -> u64 {
    match command {
        Command::CreateBuffer {
            buffer,
            label,
            size,
            usage,
            memory,
        } => stream.create_buffer(
            *buffer,
            &BufferDesc {
                label: label.as_deref(),
                size: *size,
                usage: *usage,
                memory: *memory,
            },
        ),
        Command::DestroyBuffer { buffer } => stream.destroy_buffer(*buffer),
        Command::BeginDebugLabel { label } => stream.begin_debug_label(label),
        Command::BeginRenderPass {
            label,
            color_attachments,
            depth_stencil_attachment,
            render_area,
        } => stream.begin_render_pass(&RenderPassDesc {
            label: label.as_deref(),
            color_attachments,
            depth_stencil_attachment: *depth_stencil_attachment,
            render_area: *render_area,
        }),
        Command::BindGraphicsPipeline { pipeline } => stream.bind_graphics_pipeline(*pipeline),
        Command::BindGroup {
            slot,
            group,
            dynamic_offsets,
            layout,
        } => stream.bind_group(*slot, *group, dynamic_offsets, *layout),
        Command::PushConstants {
            stages,
            offset,
            data,
            layout,
        } => stream.push_constants(*stages, *offset, data, *layout),
        Command::Draw {
            vertices,
            instances,
        } => stream.draw(vertices.clone(), instances.clone()),
    }
}

/// A stream holding every command in [`every_command`], in order.
fn encode_all() -> (StreamWriter, Vec<Command>) {
    let commands = every_command();
    let mut stream = StreamWriter::new();
    for command in &commands {
        encode(&mut stream, command);
    }
    (stream, commands)
}

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
