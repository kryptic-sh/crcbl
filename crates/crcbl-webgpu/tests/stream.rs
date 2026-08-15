//! The encoding's own suite: every command shape out and back, and every way a
//! buffer can be wrong.
//!
//! An integration test rather than a `#[cfg(test)]` module, because the encoding
//! is a public contract and this is the only place it gets exercised the way its
//! callers will: through the crate's exported surface, with nothing `pub(crate)`
//! in reach.

mod corpus;

use crcbl_hal::{
    AdapterId, BufferDesc, BufferUsage, DeviceDesc, Extent3d, Features, Format, ImageAspect,
    ImageDesc, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewType,
    MemoryLocation, ShaderStages,
};
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

/// **The canvas key is a field of its own and not a second handle.** The surface
/// pair is one method away from the buffer pair in the writer, so the mistake it
/// invites is a body copied across with the trailing `u32` dropped or written
/// where the handle goes. The numbers below are chosen so neither survives: the
/// key differs from both halves of every handle here, so a transposition changes
/// the decoded value rather than reproducing it.
#[test]
fn a_surface_carries_its_canvas_key_as_well_as_its_handle() {
    let surface = handle(51, 52);
    let canvas_id = 53;
    let mut stream = StreamWriter::new();
    stream.create_surface(surface, canvas_id);
    stream.destroy_surface(handle(54, 55));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateSurface { surface, canvas_id },
            Command::DestroySurface {
                surface: handle(54, 55)
            },
        ])
    );
    assert!(
        ![surface.index(), surface.generation()].contains(&canvas_id),
        "the test would not notice the key written over a handle half otherwise"
    );
}

/// **An image's seven descriptor fields all cross, in the descriptor's order.**
///
/// The round trip over the corpus says the whole set survives; what this says is
/// that no two of them can be swapped without the decode changing, which a round
/// trip over equal values would not. Every number below is distinct, and the
/// assertions at the end are what state that out loud rather than leaving it to
/// the reader to notice.
#[test]
fn an_image_carries_every_field_of_its_descriptor_in_the_descriptors_order() {
    let image = handle(5, 6);
    let desc = ImageDesc {
        label: Some("depth pyramid"),
        image_type: ImageType::D3,
        extent: Extent3d {
            width: 1024,
            height: 512,
            depth_or_layers: 32,
        },
        format: Format::R32Float,
        mip_levels: 10,
        samples: 2,
        usage: ImageUsage::STORAGE | ImageUsage::SAMPLED,
    };
    let mut stream = StreamWriter::new();
    stream.create_image(image, &desc);
    stream.destroy_image(handle(7, 8));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateImage {
                image,
                label: Some("depth pyramid".into()),
                image_type: desc.image_type,
                extent: desc.extent,
                format: desc.format,
                mip_levels: desc.mip_levels,
                samples: desc.samples,
                usage: desc.usage,
            },
            Command::DestroyImage {
                image: handle(7, 8)
            },
        ])
    );

    let mut sorted = vec![
        desc.extent.width,
        desc.extent.height,
        desc.extent.depth_or_layers,
        desc.mip_levels,
        desc.samples,
    ];
    let count = sorted.len();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        count,
        "two of the image's numbers are equal, so the test would not notice them swapped"
    );
}

/// The same claim for a view, plus the one an image cannot make: **two handles
/// cross and they mean opposite things.** `view` is the id being filled in and
/// `image` the id being read, so a body that transposed them would create the
/// view at the image's id and leave the real one empty.
#[test]
fn an_image_view_carries_two_handles_that_are_not_interchangeable() {
    let view = handle(9, 10);
    let image = handle(11, 12);
    let desc = ImageViewDesc {
        label: None,
        image,
        view_type: ImageViewType::CubeArray,
        format: Format::Bgra8Unorm,
        range: ImageSubresourceRange {
            aspect: ImageAspect::COLOR,
            base_mip: 1,
            mip_count: 2,
            base_layer: 3,
            layer_count: ImageSubresourceRange::ALL,
        },
    };
    let mut stream = StreamWriter::new();
    stream.create_image_view(view, &desc);
    stream.destroy_image_view(handle(13, 14));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateImageView {
                view,
                label: None,
                image,
                view_type: desc.view_type,
                format: desc.format,
                range: desc.range,
            },
            Command::DestroyImageView {
                view: handle(13, 14)
            },
        ])
    );
    assert_ne!(
        view.to_bits(),
        image.to_bits(),
        "the test would not notice the two handles swapped otherwise"
    );
}

/// **Zero `mip_levels` and zero `samples` cross rather than being refused**, and
/// this is the test that pins the decision.
///
/// Both are meaningless to a device, and neither is a malformed *stream*: the
/// wire form of a `u32` claims every value, so there is nothing here for a
/// decoder to reject that a `from_bits` or an enum table would reject. Refusing
/// would also have to happen in the writer, which the crate's own rule requires
/// — it asserts what the reader enforces — and the writer has only a panic to
/// refuse with, in the middle of a frame's recording, for a call whose contract
/// is to return `Ok(handle)` immediately. So an invalid descriptor stays a
/// creation failure and arrives through `Device::take_error`.
#[test]
fn a_zero_mip_count_or_sample_count_is_carried_rather_than_refused() {
    let mut stream = StreamWriter::new();
    stream.create_image(
        handle(1, 1),
        &ImageDesc {
            label: None,
            image_type: ImageType::D2,
            extent: Extent3d::d2(4, 4),
            format: Format::R8Unorm,
            mip_levels: 0,
            samples: 0,
            usage: ImageUsage::SAMPLED,
        },
    );

    match &decode_stream(stream.bytes()).expect("a stream this crate wrote decodes")[0] {
        Command::CreateImage {
            mip_levels,
            samples,
            ..
        } => assert_eq!((*mip_levels, *samples), (0, 0)),
        other => panic!("expected CreateImage, got {}", other.name()),
    }
}

/// A dimensionality code no variant claims is an error rather than the
/// neighbour one byte away — the rule `tag::image_type_from_code` and
/// `tag::image_view_type_from_code` state, seen through a whole command.
#[test]
fn a_dimensionality_code_no_variant_claims_is_refused_rather_than_folded_into_a_neighbour() {
    // Tag, the handle, the absent label's presence byte — then the image type.
    let mut stream = StreamWriter::new();
    stream.create_image(
        handle(1, 1),
        &ImageDesc {
            label: None,
            image_type: ImageType::D2,
            extent: Extent3d::d2(8, 8),
            format: Format::R8Unorm,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::SAMPLED,
        },
    );
    let mut bytes = stream.bytes().to_vec();
    bytes[tag::HEADER_BYTES + 1 + 8 + 1] = 0x7F;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "ImageDesc::image_type",
            code: 0x7F,
        })
    );

    // The format is the byte after the extent, and it is the table this slice
    // reuses rather than writes a second copy of.
    let mut bytes = stream.bytes().to_vec();
    bytes[tag::HEADER_BYTES + 1 + 8 + 1 + 1 + 12] = 0x7F;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "ImageDesc::format",
            code: 0x7F,
        })
    );

    // A view's own dimensionality sits behind a second handle: tag, the view's
    // id, the absent label, then the image's id.
    let mut stream = StreamWriter::new();
    stream.create_image_view(
        handle(1, 1),
        &ImageViewDesc {
            label: None,
            image: handle(2, 2),
            view_type: ImageViewType::D2,
            format: Format::R8Unorm,
            range: ImageSubresourceRange::all(Format::R8Unorm),
        },
    );
    let mut bytes = stream.bytes().to_vec();
    bytes[tag::HEADER_BYTES + 1 + 8 + 1 + 8] = 0x7F;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "ImageViewDesc::view_type",
            code: 0x7F,
        })
    );
}

/// The two new bitflags words go over as `bits()` and come back through
/// `from_bits`, so a bit no flag claims is refused rather than truncated away —
/// which for a usage word would create an image the caller cannot bind, and for
/// an aspect a view onto a plane nobody named.
#[test]
fn an_image_usage_or_aspect_bit_no_flag_claims_is_refused_rather_than_dropped() {
    let mut stream = StreamWriter::new();
    stream.create_image(
        handle(1, 1),
        &ImageDesc {
            label: None,
            image_type: ImageType::D2,
            extent: Extent3d::d2(8, 8),
            format: Format::R8Unorm,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::SAMPLED,
        },
    );
    // The usage word is the last four bytes of the body.
    let mut bytes = stream.bytes().to_vec();
    let usage_at = bytes.len() - 4;
    bytes[usage_at..].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "ImageDesc::usage",
            code: u32::MAX.into(),
        })
    );

    let mut stream = StreamWriter::new();
    stream.create_image_view(
        handle(1, 1),
        &ImageViewDesc {
            label: None,
            image: handle(2, 2),
            view_type: ImageViewType::D2,
            format: Format::R8Unorm,
            range: ImageSubresourceRange::all(Format::R8Unorm),
        },
    );
    // The aspect opens the range, which is the last twenty bytes of the body.
    let mut bytes = stream.bytes().to_vec();
    let aspect_at = bytes.len() - 20;
    bytes[aspect_at..aspect_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "ImageSubresourceRange::aspect",
            code: u32::MAX.into(),
        })
    );
}

/// **The capability query is a tag and nothing else**, so the byte after it is
/// the next command's tag.
///
/// The command after it is what makes that checkable: a writer that still put a
/// handle and an adapter id on the wire, or a reader that still consumed twelve
/// bytes, does not merely mis-read one command — it walks into or over its
/// neighbour, and the buffer decodes to the wrong number of commands. A query on
/// its own would decode cleanly either way, because a reader running off the end
/// of a one-command buffer is a `TooShort` that a writer's extra bytes cover up.
///
/// `create_surface` before it, so the pair is also the shape the HAL call is
/// made in: a surface exists, and the query names it nowhere.
#[test]
fn a_surface_capability_query_is_a_tag_with_no_body_after_it() {
    let surface = handle(63, 64);
    let mut stream = StreamWriter::new();
    stream.create_surface(surface, 19);
    stream.surface_caps();
    stream.enumerate_adapters();

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateSurface {
                surface,
                canvas_id: 19,
            },
            Command::SurfaceCaps,
            Command::EnumerateAdapters,
        ])
    );
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

/// **The two feature words are not interchangeable**, and they are adjacent on
/// the wire, which is how a pair gets swapped. Required and optional are given
/// disjoint values so a swap cannot compare equal — and the required word holds
/// a flag WebGPU cannot satisfy, because dropping those in the encoder is the
/// mistake that would turn a refusal into a device.
#[test]
fn a_device_request_carries_both_feature_words_the_way_round_the_descriptor_had_them() {
    let desc = DeviceDesc {
        label: Some("engine"),
        adapter: AdapterId(2),
        required_features: Features::COMPUTE | Features::TIMELINE_SEMAPHORE,
        optional_features: Features::TIMESTAMP_QUERY,
        compatible_surface: Some(handle(5, 6)),
    };
    let mut stream = StreamWriter::new();
    stream.request_device(&desc);

    let decoded = decode_stream(stream.bytes()).expect("a stream this crate wrote decodes");
    assert_eq!(
        decoded,
        vec![Command::RequestDevice {
            adapter: AdapterId(2),
            label: Some("engine".into()),
            required_features: desc.required_features,
            optional_features: desc.optional_features,
            compatible_surface: desc.compatible_surface,
        }]
    );
    assert!(
        desc.required_features
            .intersection(desc.optional_features)
            .is_empty(),
        "the test would not notice the two words swapped otherwise"
    );
}

/// A feature bit no flag claims is refused on the command side too. Truncating
/// would quietly move a required feature out of the request, which is the one
/// thing `required` cannot survive.
#[test]
fn a_feature_bit_no_flag_claims_in_a_device_request_is_refused_rather_than_dropped() {
    let mut stream = StreamWriter::new();
    stream.request_device(&DeviceDesc {
        label: None,
        adapter: AdapterId(0),
        required_features: Features::COMPUTE,
        optional_features: Features::empty(),
        compatible_surface: None,
    });
    let mut bytes = stream.bytes().to_vec();
    // Tag, the adapter id, the absent label's presence byte, then the word.
    let at = tag::HEADER_BYTES + 1 + 4 + 1;
    let unclaimed = Features::all().bits() | (1 << 32);
    bytes[at..at + 8].copy_from_slice(&unclaimed.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "DeviceDesc::required_features",
            code: unclaimed,
        })
    );

    // …and the optional word is checked separately, one field further on.
    let mut bytes = stream.bytes().to_vec();
    bytes[at + 8..at + 16].copy_from_slice(&unclaimed.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "DeviceDesc::optional_features",
            code: unclaimed,
        })
    );
}

#[test]
fn every_command_has_its_own_name() {
    let commands = every_command();
    let mut names: Vec<&str> = commands.iter().map(Command::name).collect();
    names.sort_unstable();
    names.dedup();
    // `every_command` holds three CreateBuffers, three CreateImages, six
    // CreateImageViews and two each of BeginRenderPass, BindGroup and
    // RequestDevice, so the distinct-name count is what the writer has methods
    // for.
    assert_eq!(names.len(), 17);
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
            code: u32::MAX.into(),
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
