//! The encoding's own suite: every command shape out and back, and every way a
//! buffer can be wrong.
//!
//! An integration test rather than a `#[cfg(test)]` module, because the encoding
//! is a public contract and this is the only place it gets exercised the way its
//! callers will: through the crate's exported surface, with nothing `pub(crate)`
//! in reach.

mod corpus;

use crcbl_hal::{
    AdapterId, BindGroupDesc, BindGroupEntry, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendFactor, BlendOp,
    BlendState, BufferDesc, BufferImageCopy, BufferUsage, ColorTargetState, ColorWrites,
    CommandEncoderDesc, CompareOp, ComputePipelineDesc, CullMode, DepthBias, DepthStencilState,
    DeviceDesc, Extent3d, Features, FilterMode, Format, FrontFace, GraphicsPipelineDesc,
    ImageAspect, ImageDesc, ImageSubresourceLayers, ImageSubresourceRange, ImageType, ImageUsage,
    ImageViewDesc, ImageViewType, MemoryLocation, MultisampleState, Offset3d, PipelineLayoutDesc,
    PolygonMode, PrimitiveState, PrimitiveTopology, PushConstantRange, ReadbackDesc, SampleType,
    SamplerAddressMode, SamplerDesc, SemaphoreSignal, SemaphoreWait, ShaderEntry, ShaderModuleDesc,
    ShaderStages, StencilFaceState, StencilOp, StencilState, SubmitInfo,
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

/// A sampler with no label, so every field below sits at a fixed offset. The
/// values differ from each other in every position a neighbour could be read
/// from; the tests that use it say which pair each one pins.
fn probe_sampler() -> SamplerDesc<'static> {
    SamplerDesc {
        label: None,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Nearest,
        mip_filter: FilterMode::Nearest,
        address_mode: [
            SamplerAddressMode::Repeat,
            SamplerAddressMode::MirrorRepeat,
            SamplerAddressMode::ClampToBorder,
        ],
        lod_min: 0.5,
        lod_max: 12.25,
        anisotropy: 4.5,
        compare: Some(CompareOp::Greater),
    }
}

/// Where each field of an unlabelled `create_sampler` body starts: the tag, the
/// handle and the absent label's presence byte come first.
const SAMPLER_MAG_AT: usize = tag::HEADER_BYTES + 1 + 8 + 1;
/// The first of the three address bytes, behind the three filter bytes.
const SAMPLER_ADDRESS_AT: usize = SAMPLER_MAG_AT + 3;
/// `lod_min`, behind the three address bytes.
const SAMPLER_LOD_MIN_AT: usize = SAMPLER_ADDRESS_AT + 3;
/// `compare`'s presence byte, behind the three floats.
const SAMPLER_COMPARE_AT: usize = SAMPLER_LOD_MIN_AT + 4 + 4 + 4;

/// **A sampler's nine descriptor fields all cross, in the descriptor's order**,
/// and this is the command where that claim is worth the most: `mag`, `min` and
/// `mip` are three identically typed bytes in a row and `address_mode` is three
/// more, so six of the nine fields are a byte each and could hold each other's
/// values.
///
/// The round trip over the corpus says the whole set survives; what this says is
/// that no two of them can be swapped without the decode changing. The
/// assertions at the end are what state that out loud rather than leaving it to
/// a reader to notice — with only two `FilterMode` variants the trio cannot be
/// all-distinct, so what is pinned is that `mag` differs from both of the
/// others, and the corpus carries the two commands that pin `min` against `mip`.
#[test]
fn a_sampler_carries_every_field_of_its_descriptor_in_the_descriptors_order() {
    let sampler = handle(5, 6);
    let desc = probe_sampler();
    let mut stream = StreamWriter::new();
    stream.create_sampler(sampler, &desc);
    stream.destroy_sampler(handle(7, 8));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateSampler {
                sampler,
                label: None,
                mag_filter: desc.mag_filter,
                min_filter: desc.min_filter,
                mip_filter: desc.mip_filter,
                address_mode: desc.address_mode,
                lod_min: desc.lod_min,
                lod_max: desc.lod_max,
                anisotropy: desc.anisotropy,
                compare: desc.compare,
            },
            Command::DestroySampler {
                sampler: handle(7, 8)
            },
        ])
    );

    assert_ne!(
        desc.mag_filter, desc.min_filter,
        "the test would not notice mag and min swapped otherwise"
    );
    assert_ne!(
        desc.mag_filter, desc.mip_filter,
        "the test would not notice mag and mip swapped otherwise"
    );
    let [u, v, w] = desc.address_mode;
    assert!(
        u != v && v != w && u != w,
        "two of U, V and W are equal, so the test would not notice them swapped"
    );
    let mut floats = vec![
        desc.lod_min.to_bits(),
        desc.lod_max.to_bits(),
        desc.anisotropy.to_bits(),
    ];
    floats.sort_unstable();
    floats.dedup();
    assert_eq!(
        floats.len(),
        3,
        "two of the sampler's floats are equal, so the test would not notice them swapped"
    );
}

/// **The floats survive bit for bit, and one of them is not a short decimal.**
///
/// `0.1` is not representable in binary at all: the nearest `f32` is
/// `0.100000001490116119384765625`, so an encoding that went through a decimal
/// string — or that widened to `f64` and narrowed back through a different
/// rounding — lands on a neighbouring value. Comparing with `==` would not see
/// every such slip, because two `f32`s that differ in the last bit still compare
/// unequal but a `f64` round trip can land back on the same `f32`; comparing
/// `to_bits` is what makes this about the bytes.
#[test]
fn a_samplers_floats_survive_bit_for_bit_including_one_no_short_decimal_names() {
    let desc = SamplerDesc {
        lod_min: 0.1,
        lod_max: 1.0 / 3.0,
        anisotropy: core::f32::consts::PI,
        ..probe_sampler()
    };
    let mut stream = StreamWriter::new();
    stream.create_sampler(handle(1, 1), &desc);

    match &decode_stream(stream.bytes()).expect("a stream this crate wrote decodes")[0] {
        Command::CreateSampler {
            lod_min,
            lod_max,
            anisotropy,
            ..
        } => {
            assert_eq!(lod_min.to_bits(), desc.lod_min.to_bits());
            assert_eq!(lod_max.to_bits(), desc.lod_max.to_bits());
            assert_eq!(anisotropy.to_bits(), desc.anisotropy.to_bits());
        }
        other => panic!("expected CreateSampler, got {}", other.name()),
    }

    // …and the bytes are the little-endian bit pattern, which is the house rule
    // the header of `crate::tag` states and the thing a decimal encoding would
    // fail even though it round-tripped through this crate's own reader.
    let bytes = stream.bytes();
    assert_eq!(
        &bytes[SAMPLER_LOD_MIN_AT..SAMPLER_LOD_MIN_AT + 4],
        &desc.lod_min.to_le_bytes(),
    );
    assert_ne!(
        f64::from(desc.lod_min).to_bits(),
        0.1_f64.to_bits(),
        "the value under test has to be one no short decimal names, or an \
         encoding that went through one would pass this test"
    );
}

/// **`lod_max`'s `f32::MAX` crosses as itself and is not resolved here.**
///
/// It is [`SamplerDesc::default`]'s "no limit", and the sentinel rule in
/// `docs/plan/41-webgpu-stream.md` is that a sentinel is a value the seam
/// defines and that an encoder resolving one is answering a question only the
/// replayer has the information to answer. So this asserts the four bytes on the
/// wire rather than the decoded value alone: a writer that turned the sentinel
/// into WebGPU's own `lodMaxClamp` default — a *number*, not "the rest" —
/// would still round-trip through this crate's reader and would silently change
/// which mips every sampler in the engine can reach.
#[test]
fn the_no_limit_sentinel_in_lod_max_crosses_as_itself_rather_than_being_resolved() {
    let desc = SamplerDesc {
        lod_max: f32::MAX,
        ..probe_sampler()
    };
    assert_eq!(
        desc.lod_max.to_bits(),
        SamplerDesc::default().lod_max.to_bits(),
        "the sentinel under test is the one the seam's own default carries"
    );

    let mut stream = StreamWriter::new();
    stream.create_sampler(handle(1, 1), &desc);

    let at = SAMPLER_LOD_MIN_AT + 4;
    assert_eq!(
        &stream.bytes()[at..at + 4],
        &f32::MAX.to_le_bytes(),
        "the encoder resolved the sentinel instead of carrying it"
    );
    match &decode_stream(stream.bytes()).expect("a stream this crate wrote decodes")[0] {
        Command::CreateSampler { lod_max, .. } => {
            assert_eq!(lod_max.to_bits(), f32::MAX.to_bits());
        }
        other => panic!("expected CreateSampler, got {}", other.name()),
    }
}

/// **An absent comparison is not a present one**, which is the optional-field
/// rule for the first optional *enum* on this stream.
///
/// `CompareOp::Never` is the trap: it is the code `0x00`, so a decoder that read
/// the presence byte as the code — or an encoder that spent a reserved code on
/// "absent" instead of a byte — would turn "no comparison" into "a comparison
/// that always fails", which is a shadow sampler that returns zero everywhere.
#[test]
fn an_absent_comparison_is_distinguishable_from_a_present_one() {
    let mut absent = StreamWriter::new();
    absent.create_sampler(
        handle(1, 1),
        &SamplerDesc {
            compare: None,
            ..probe_sampler()
        },
    );
    let mut never = StreamWriter::new();
    never.create_sampler(
        handle(1, 1),
        &SamplerDesc {
            compare: Some(CompareOp::Never),
            ..probe_sampler()
        },
    );
    assert_ne!(absent.bytes(), never.bytes());

    let absent = decode_stream(absent.bytes()).expect("a stream this crate wrote decodes");
    let never = decode_stream(never.bytes()).expect("a stream this crate wrote decodes");
    match (&absent[0], &never[0]) {
        (
            Command::CreateSampler {
                compare: absent, ..
            },
            Command::CreateSampler { compare: never, .. },
        ) => {
            assert_eq!(*absent, None);
            assert_eq!(*never, Some(CompareOp::Never));
        }
        _ => panic!("expected two CreateSamplers"),
    }

    // The absent one is a byte shorter, which is what says the code is not
    // written when there is nothing to write.
    let mut short = StreamWriter::new();
    short.create_sampler(
        handle(1, 1),
        &SamplerDesc {
            compare: None,
            ..probe_sampler()
        },
    );
    assert_eq!(short.bytes().len(), SAMPLER_COMPARE_AT + 1);
}

/// A filter, address or comparison code no variant claims is an error rather
/// than the neighbour one byte away — the rule `tag::filter_mode_from_code`,
/// `tag::sampler_address_mode_from_code` and `tag::compare_op_from_code` state,
/// seen through a whole command.
#[test]
fn a_sampler_code_no_variant_claims_is_refused_rather_than_folded_into_a_neighbour() {
    let mut stream = StreamWriter::new();
    stream.create_sampler(handle(1, 1), &probe_sampler());
    let whole = stream.bytes().to_vec();

    for (at, field) in [
        (SAMPLER_MAG_AT, "SamplerDesc::mag_filter"),
        (SAMPLER_MAG_AT + 1, "SamplerDesc::min_filter"),
        (SAMPLER_MAG_AT + 2, "SamplerDesc::mip_filter"),
    ] {
        let mut bytes = whole.clone();
        bytes[at] = 0x7F;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum { field, code: 0x7F }),
        );
    }

    // All three address bytes are one field, so all three name it — what the
    // sweep pins is that each is read rather than one being read three times.
    for offset in 0..3 {
        let mut bytes = whole.clone();
        bytes[SAMPLER_ADDRESS_AT + offset] = 0x7F;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum {
                field: "SamplerDesc::address_mode",
                code: 0x7F,
            }),
            "address byte {offset} is not read"
        );
    }

    // The presence byte and the code behind it are separate refusals, and both
    // name the field: a byte that is neither presence value is not a comparison
    // this build has never heard of.
    let mut bytes = whole.clone();
    bytes[SAMPLER_COMPARE_AT] = 2;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "SamplerDesc::compare",
            code: 2,
        })
    );
    let mut bytes = whole.clone();
    bytes[SAMPLER_COMPARE_AT + 1] = 0x7F;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "SamplerDesc::compare",
            code: 0x7F,
        })
    );

    // …and the code one past the last claimed one, which is where an off-by-one
    // in either table lands and where `0x7F` never would.
    let mut bytes = whole.clone();
    bytes[SAMPLER_MAG_AT] = tag::FILTER_MODE_LINEAR + 1;
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "SamplerDesc::mag_filter",
            ..
        })
    ));
    let mut bytes = whole.clone();
    bytes[SAMPLER_ADDRESS_AT] = tag::SAMPLER_ADDRESS_CLAMP_TO_BORDER + 1;
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "SamplerDesc::address_mode",
            ..
        })
    ));
    let mut bytes = whole;
    bytes[SAMPLER_COMPARE_AT + 1] = tag::COMPARE_OP_ALWAYS + 1;
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "SamplerDesc::compare",
            ..
        })
    ));
}

// ── Bind-group layouts ────────────────────────────────────────────────────────

/// A read-only storage buffer visible to the vertex stage — the engine's own
/// vertex-pulling binding, and the entry every test below varies one field of.
fn probe_entry() -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding: 7,
        visibility: ShaderStages::VERTEX,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// Encodes one unlabelled layout holding `entries`.
fn layout_of(entries: &[BindGroupLayoutEntry]) -> StreamWriter {
    let mut stream = StreamWriter::new();
    stream.create_bind_group_layout(
        handle(1, 1),
        &BindGroupLayoutDesc {
            label: None,
            entries,
        },
    );
    stream
}

/// The entries' `u32` count in an unlabelled `create_bind_group_layout` body:
/// the tag, the handle and the absent label's presence byte come first.
const LAYOUT_COUNT_AT: usize = tag::HEADER_BYTES + 1 + 8 + 1;
/// The first entry, behind that count.
const LAYOUT_ENTRY_AT: usize = LAYOUT_COUNT_AT + 4;
/// The first entry's `BindingKind` code, behind its `binding` and `visibility`.
const LAYOUT_KIND_AT: usize = LAYOUT_ENTRY_AT + 4 + 4;

/// **A multi-entry layout survives field for field, and a single-entry one would
/// prove none of it.**
///
/// This is the stream's first counted list of *structs*, and the difference from
/// the lists before it is the stride: a reader out by a byte in `dynamic_offsets`
/// runs off the end, and one out by a byte here decodes the next entry out of the
/// middle of this one and answers a layout that is well-formed and describes
/// different resources.
///
/// **What turns it red.** An entry field read in the wrong order — every value
/// below is distinct in the position a neighbour could be read from. A
/// `BindingKind` payload read once and copied — the two `StorageBuffer`s differ
/// in both bools. A list rebuilt from binding numbers rather than kept in slice
/// order — the third assertion is about exactly that, and
/// `docs/plan/41-webgpu-stream.md` requires it because a `VARIABLE_COUNT` entry
/// must be *last in the slice* and not merely highest-numbered.
#[test]
fn a_bind_group_layout_carries_a_multi_entry_list_in_the_descriptors_own_order() {
    let entries = [
        BindGroupLayoutEntry {
            binding: 3,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::StorageBuffer {
                read_only: false,
                dynamic: true,
            },
            count: 1,
            flags: BindingFlags::empty(),
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::CubeArray,
                sample_type: SampleType::Depth,
            },
            count: 2,
            flags: BindingFlags::PARTIALLY_BOUND,
        },
        BindGroupLayoutEntry {
            binding: 2,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::Sampler { comparison: true },
            count: 1,
            flags: BindingFlags::empty(),
        },
        probe_entry(),
    ];
    let mut stream = StreamWriter::new();
    stream.create_bind_group_layout(
        handle(5, 6),
        &BindGroupLayoutDesc {
            label: Some("frame"),
            entries: &entries,
        },
    );
    stream.destroy_bind_group_layout(handle(7, 8));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateBindGroupLayout {
                layout: handle(5, 6),
                label: Some("frame".into()),
                entries: entries.to_vec(),
            },
            Command::DestroyBindGroupLayout {
                layout: handle(7, 8)
            },
        ])
    );

    let bindings: Vec<u32> = entries.iter().map(|entry| entry.binding).collect();
    assert_ne!(
        bindings,
        {
            let mut sorted = bindings.clone();
            sorted.sort_unstable();
            sorted
        },
        "the binding numbers are already ascending, so the test would not notice \
         a decoder that rebuilt the list from them"
    );
    assert!(
        entries.len() > 1,
        "one entry says nothing about a counted list's stride"
    );
}

/// **Two entries differing in exactly one field are two different layouts**, and
/// every field is swept on its own so no one of them can stand for the others.
///
/// A round trip over a list whose entries differ in several fields at once would
/// pass for an encoder that wrote one field and left the rest at whatever the
/// previous entry held. Each case here changes a single field of a single entry
/// and asserts that both the *bytes* and the decode move — the bytes because a
/// dropped field round-trips through a reader that also drops it.
///
/// **What turns it red.** Any field of `BindGroupLayoutEntry` left off the wire.
/// Any `BindingKind` payload byte not written, which the last four cases sweep —
/// `read_only` and `dynamic` are adjacent presence bytes on one variant, and
/// `view_type` and `sample_type` are adjacent code bytes on another.
#[test]
fn two_entries_differing_in_one_field_are_distinguishable_field_by_field() {
    let base = probe_entry();
    let variants = [
        (
            "binding",
            BindGroupLayoutEntry {
                binding: base.binding + 1,
                ..base
            },
        ),
        (
            "visibility",
            BindGroupLayoutEntry {
                visibility: ShaderStages::FRAGMENT,
                ..base
            },
        ),
        (
            "count",
            BindGroupLayoutEntry {
                count: base.count + 1,
                ..base
            },
        ),
        (
            "flags",
            BindGroupLayoutEntry {
                flags: BindingFlags::UPDATE_AFTER_BIND,
                ..base
            },
        ),
        (
            "kind",
            BindGroupLayoutEntry {
                kind: BindingKind::UniformBuffer { dynamic: false },
                ..base
            },
        ),
        (
            "kind::read_only",
            BindGroupLayoutEntry {
                kind: BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                },
                ..base
            },
        ),
        (
            "kind::dynamic",
            BindGroupLayoutEntry {
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: true,
                },
                ..base
            },
        ),
        (
            "kind::view_type",
            BindGroupLayoutEntry {
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                ..base
            },
        ),
        (
            "kind::sample_type",
            BindGroupLayoutEntry {
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: SampleType::Depth,
                },
                ..base
            },
        ),
        (
            "kind::comparison",
            BindGroupLayoutEntry {
                kind: BindingKind::Sampler { comparison: true },
                ..base
            },
        ),
    ];

    let baseline = layout_of(&[base]);
    for (field, variant) in variants {
        assert_ne!(base, variant, "{field}: the pair under test is one value");
        let changed = layout_of(&[variant]);
        assert_ne!(
            baseline.bytes(),
            changed.bytes(),
            "{field} is not on the wire"
        );
        assert_eq!(
            decode_stream(changed.bytes()),
            Ok(vec![Command::CreateBindGroupLayout {
                layout: handle(1, 1),
                label: None,
                entries: vec![variant],
            }]),
            "{field} does not survive the round trip"
        );
    }

    // The two `SampledImage` cases above differ from each other in one field
    // too, which is the pair the sweep against `base` cannot pin: both differ
    // from a `StorageBuffer` in the code byte alone.
    assert_ne!(
        layout_of(&[variants[7].1]).bytes(),
        layout_of(&[variants[8].1]).bytes(),
        "sample_type is not on the wire beside a view_type that did not change"
    );
}

/// **A `BindingKind` code no variant claims is refused**, and this table's fold
/// costs more than any other on the stream: the variants' payloads are different
/// lengths, so a code read as its neighbour leaves the cursor inside the entry
/// and every field after it decodes out of the wrong bytes.
///
/// **What turns it red.** A catch-all arm in `read_binding_kind`. A table with a
/// row too many, which the code one past the last claimed one lands on and which
/// `0xFF` never would.
#[test]
fn a_binding_kind_code_no_variant_claims_is_refused_rather_than_folded_into_a_neighbour() {
    let whole = layout_of(&[probe_entry()]).bytes().to_vec();
    assert_eq!(
        whole[LAYOUT_KIND_AT],
        tag::BINDING_KIND_STORAGE_BUFFER,
        "the offsets this test corrupts have moved"
    );

    for code in [0x7F, tag::BINDING_KIND_CODES] {
        let mut bytes = whole.clone();
        bytes[LAYOUT_KIND_AT] = code;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum {
                field: "BindGroupLayoutEntry::kind",
                code: code.into(),
            }),
            "code {code:#04x}"
        );
    }

    // …and the payload behind a claimed code is refused on its own terms: a
    // `bool` is a presence byte, so a third value is an error rather than truth,
    // and the two enum bytes of a `SampledImage` name their own fields.
    let mut bytes = whole.clone();
    bytes[LAYOUT_KIND_AT + 1] = 2;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BindingKind::read_only",
            code: 2,
        })
    );

    let sampled = layout_of(&[BindGroupLayoutEntry {
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
        ..probe_entry()
    }]);
    let whole = sampled.bytes().to_vec();
    let mut bytes = whole.clone();
    bytes[LAYOUT_KIND_AT + 1] = tag::IMAGE_VIEW_TYPE_D3 + 1;
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BindingKind::view_type",
            ..
        })
    ));
    let mut bytes = whole;
    bytes[LAYOUT_KIND_AT + 2] = tag::SAMPLE_TYPE_DEPTH + 1;
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BindingKind::sample_type",
            ..
        })
    ));
}

/// **The `u32::MAX` count is a sentinel and the encoder does not resolve it**,
/// and neither is any `BindingFlags` bit dropped on the way.
///
/// `BindGroupLayoutEntry::count`'s docs call `u32::MAX` "as many as this device
/// can", resolved through `resolved_count` against a device's own
/// `max_bindless_descriptors` — which is a number **only the far side has**. So
/// this is `lod_max`'s rule again, and the assertion is on the bytes rather than
/// on the decode: a writer that resolved the sentinel to some plausible ceiling
/// would still round-trip through this crate's own reader.
///
/// **What turns it red.** An encoder that clamped the count. One that dropped
/// the flags word because WebGPU has no bindless model — the refusal belongs to
/// the replayer, which can only refuse what it was told, and a layout silently
/// downgraded to one fixed descriptor is what `BindingFlags`'s own docs call
/// reading garbage at index 4097.
#[test]
fn the_bindless_count_sentinel_and_its_flags_cross_verbatim_rather_than_being_resolved() {
    let flags = BindingFlags::VARIABLE_COUNT
        | BindingFlags::PARTIALLY_BOUND
        | BindingFlags::UPDATE_AFTER_BIND;
    let entry = BindGroupLayoutEntry {
        count: u32::MAX,
        flags,
        ..probe_entry()
    };
    let stream = layout_of(&[entry]);
    let bytes = stream.bytes();

    // The count and the flags are the last eight bytes of the body, behind the
    // `StorageBuffer` code and its two presence bytes.
    let count_at = LAYOUT_KIND_AT + 1 + 2;
    assert_eq!(
        &bytes[count_at..count_at + 4],
        &u32::MAX.to_le_bytes(),
        "the encoder resolved the sentinel instead of carrying it"
    );
    assert_eq!(
        &bytes[count_at + 4..count_at + 8],
        &flags.bits().to_le_bytes(),
        "the encoder dropped flags the replayer is the one that must refuse"
    );
    assert_eq!(bytes.len(), count_at + 8, "the entry has grown a field");

    match &decode_stream(bytes).expect("a stream this crate wrote decodes")[0] {
        Command::CreateBindGroupLayout { entries, .. } => {
            assert_eq!(entries[0].count, u32::MAX);
            assert_eq!(entries[0].flags, flags);
        }
        other => panic!("expected CreateBindGroupLayout, got {}", other.name()),
    }

    // …and an ordinary array count is left alone too, which is what says the
    // sentinel is carried rather than that nothing is.
    let fixed = layout_of(&[BindGroupLayoutEntry {
        count: 64,
        ..probe_entry()
    }]);
    assert_eq!(&fixed.bytes()[count_at..count_at + 4], &64u32.to_le_bytes());
}

/// A `ShaderStages` or `BindingFlags` bit no flag claims is refused rather than
/// truncated away — the rule every bitflags field on this stream follows, met by
/// the two words a layout entry carries.
///
/// **What turns it red.** `from_bits_truncate` in either reader. For the
/// visibility that would hand the replayer a binding narrower than the caller
/// declared, and the shader compiled against it would read whatever the slot
/// held; for the flags it is the bindless downgrade `BindingFlags`'s own docs
/// forbid.
#[test]
fn a_visibility_or_binding_flag_bit_no_flag_claims_is_refused_rather_than_dropped() {
    let whole = layout_of(&[probe_entry()]).bytes().to_vec();

    let visibility_at = LAYOUT_ENTRY_AT + 4;
    let mut bytes = whole.clone();
    bytes[visibility_at..visibility_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BindGroupLayoutEntry::visibility",
            code: u32::MAX.into(),
        })
    );

    // One bit past the last claimed one, which is where a table that stopped a
    // stage short lands and where `u32::MAX` would not distinguish itself.
    let unclaimed = ShaderStages::all().bits() | (ShaderStages::all().bits() + 1);
    let mut bytes = whole.clone();
    bytes[visibility_at..visibility_at + 4].copy_from_slice(&unclaimed.to_le_bytes());
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BindGroupLayoutEntry::visibility",
            ..
        })
    ));

    let flags_at = whole.len() - 4;
    let mut bytes = whole;
    bytes[flags_at..flags_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "BindGroupLayoutEntry::flags",
            code: u32::MAX.into(),
        })
    );
}

/// **An empty entry list is a layout, and a destroy naming nothing is a
/// command.**
///
/// The zero count is the length a reader most easily treats as "read until
/// something stops you", so the command *after* it is what makes the claim
/// checkable: a reader that consumed one entry anyway would eat the destroy and
/// the buffer would decode one command short.
#[test]
fn an_empty_entry_list_consumes_nothing_and_a_destroy_for_an_unknown_id_decodes() {
    let mut stream = StreamWriter::new();
    stream.create_bind_group_layout(
        handle(1, 1),
        &BindGroupLayoutDesc {
            label: Some(""),
            entries: &[],
        },
    );
    stream.destroy_bind_group_layout(handle(9999, 7));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateBindGroupLayout {
                layout: handle(1, 1),
                label: Some(String::new()),
                entries: Vec::new(),
            },
            Command::DestroyBindGroupLayout {
                layout: handle(9999, 7)
            },
        ])
    );
}

// ── Bind groups ─────────────────────────────────────────────────────────────

/// Encodes one unlabelled bind group holding `entries` against layout `handle(9,
/// 9)`, with `variable_count`.
fn group_of(entries: &[BindGroupEntry], variable_count: Option<u32>) -> StreamWriter {
    let mut stream = StreamWriter::new();
    stream.create_bind_group(
        handle(1, 1),
        &BindGroupDesc {
            label: None,
            layout: handle(9, 9),
            entries,
            variable_count,
        },
    );
    stream
}

/// The `u32` entry count in an unlabelled `create_bind_group` body: the tag, the
/// group handle, the absent label's presence byte and the layout handle come
/// first.
const GROUP_COUNT_AT: usize = tag::HEADER_BYTES + 1 + 8 + 1 + 8;
/// The first entry, behind that count.
const GROUP_ENTRY_AT: usize = GROUP_COUNT_AT + 4;
/// The first entry's `BindingResource` discriminant, behind `binding` and
/// `array_index`.
const GROUP_RESOURCE_CODE_AT: usize = GROUP_ENTRY_AT + 4 + 4;

/// A buffer entry, so a `Buffer` resource is what `GROUP_RESOURCE_CODE_AT` points
/// at.
fn buffer_entry() -> BindGroupEntry {
    BindGroupEntry {
        binding: 0,
        array_index: 0,
        resource: BindingResource::Buffer {
            buffer: handle(11, 12),
            offset: 256,
            size: 1024,
        },
    }
}

/// **A multi-entry bind group survives field for field, entries carrying all
/// three resource shapes and `variable_count` both ways.**
///
/// This is the second counted list of structs on the stream, and the entries are
/// deeper than a layout's: each carries a [`BindingResource`] whose variants have
/// different-length bodies, so a stride out by a byte decodes the next entry out
/// of the middle of this one.
///
/// **What turns it red.** An entry field read in the wrong order — every value
/// below is distinct in the position a neighbour could be read from. A resource
/// discriminant folded into a neighbour — the three shapes are all present. A
/// list rebuilt from binding numbers rather than kept in slice order, or
/// `array_index` dropped — the last two entries share binding 5 and differ only
/// in `array_index`. `variable_count` dropped — it is `Some` here and `None` in
/// the byte-distinctness test below.
#[test]
fn a_bind_group_carries_a_multi_entry_list_in_the_descriptors_own_order() {
    let entries = [
        buffer_entry(),
        BindGroupEntry {
            binding: 1,
            array_index: 0,
            resource: BindingResource::ImageView(handle(67, 68)),
        },
        BindGroupEntry {
            binding: 2,
            array_index: 0,
            resource: BindingResource::Sampler(handle(83, 84)),
        },
        // The `WHOLE_BUFFER` sentinel: `u64::MAX`, which crosses verbatim.
        BindGroupEntry {
            binding: 3,
            array_index: 0,
            resource: BindingResource::whole_buffer(handle(13, 14)),
        },
        // Two entries sharing a binding number and differing only in
        // `array_index` — the bindless write path, and the pair a decoder that
        // keyed on binding would collapse.
        BindGroupEntry {
            binding: 5,
            array_index: 0,
            resource: BindingResource::ImageView(handle(69, 70)),
        },
        BindGroupEntry {
            binding: 5,
            array_index: 1,
            resource: BindingResource::ImageView(handle(71, 72)),
        },
    ];
    let mut stream = StreamWriter::new();
    stream.create_bind_group(
        handle(5, 6),
        &BindGroupDesc {
            label: Some("material"),
            layout: handle(93, 94),
            entries: &entries,
            variable_count: Some(2),
        },
    );
    stream.destroy_bind_group(handle(7, 8));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateBindGroup {
                group: handle(5, 6),
                label: Some("material".into()),
                layout: handle(93, 94),
                entries: entries.to_vec(),
                variable_count: Some(2),
            },
            Command::DestroyBindGroup {
                group: handle(7, 8)
            },
        ])
    );

    // The two entries sharing binding 5 differ only in `array_index`, so a body
    // that read it where `binding` goes, or a list keyed on binding, loses one.
    assert_eq!(entries[4].binding, entries[5].binding);
    assert_ne!(entries[4].array_index, entries[5].array_index);
    assert_ne!(entries[4].resource, entries[5].resource);
}

/// **`WHOLE_BUFFER` in a buffer entry's `size` crosses as `u64::MAX` and is not
/// resolved here.**
///
/// It is [`BindingResource::WHOLE_BUFFER`], and the sentinel rule is `lod_max`'s:
/// the encoder never resolves one, because only the replayer has the information
/// to. So this asserts the eight bytes on the wire, not just the decode — a
/// writer that turned the sentinel into WebGPU's absent member would still
/// round-trip through this crate's reader while sending a length no browser can
/// tell from a real one. **It must not cross as `18446744073709551615`**, which
/// is what a `Number` on the far side would round it to; here it is the max
/// integer verbatim, and the replayer resolves it to an absent
/// `GPUBufferBinding.size`.
#[test]
fn whole_buffer_in_a_binding_crosses_as_the_max_integer_rather_than_being_resolved() {
    let entry = BindGroupEntry {
        binding: 0,
        array_index: 0,
        resource: BindingResource::whole_buffer(handle(11, 12)),
    };
    let stream = group_of(&[entry], None);
    let bytes = stream.bytes();

    // The `size` is the last eight bytes of the buffer resource: the code, the
    // handle, and the `offset` come before it.
    let size_at = GROUP_RESOURCE_CODE_AT + 1 + 8 + 8;
    assert_eq!(
        &bytes[size_at..size_at + 8],
        &u64::MAX.to_le_bytes(),
        "the encoder resolved WHOLE_BUFFER instead of carrying it"
    );

    match &decode_stream(bytes).expect("a stream this crate wrote decodes")[0] {
        Command::CreateBindGroup { entries, .. } => {
            let BindingResource::Buffer { size, .. } = entries[0].resource else {
                panic!("expected a buffer resource");
            };
            assert_eq!(size, BindingResource::WHOLE_BUFFER);
        }
        other => panic!("expected CreateBindGroup, got {}", other.name()),
    }
}

/// **A `BindingResource` discriminant no variant claims is refused**, and this
/// table's fold is the most dangerous on the stream: a handle carries no kind, so
/// the discriminant is the only thing saying which resource table an id indexes,
/// and the bodies are different lengths besides.
///
/// **What turns it red.** A catch-all arm in `read_binding_resource`. A table
/// with a row too many, which the code one past the last claimed one lands on and
/// which `0xFF` never would.
#[test]
fn a_binding_resource_code_no_variant_claims_is_refused_rather_than_folded_into_a_neighbour() {
    let whole = group_of(&[buffer_entry()], None).bytes().to_vec();
    assert_eq!(
        whole[GROUP_RESOURCE_CODE_AT],
        tag::BINDING_RESOURCE_BUFFER,
        "the offset this test corrupts has moved"
    );

    for code in [0x7F, tag::BINDING_RESOURCE_CODES] {
        let mut bytes = whole.clone();
        bytes[GROUP_RESOURCE_CODE_AT] = code;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum {
                field: "BindGroupEntry::resource",
                code: code.into(),
            }),
            "code {code:#04x}"
        );
    }
}

/// **A present `variable_count` is not an absent one**, the optional-scalar rule
/// for the field WebGPU cannot express.
///
/// `Some(0)` is the trap: a decoder that read the presence byte as the value
/// would turn "no variable count" into "a variable count of zero", which the
/// replayer would then refuse as a runtime-sized array rather than pass on as
/// the fixed-size layout `None` means.
#[test]
fn an_absent_variable_count_is_distinguishable_from_a_present_one() {
    let absent = group_of(&[buffer_entry()], None);
    let zero = group_of(&[buffer_entry()], Some(0));
    assert_ne!(absent.bytes(), zero.bytes());
    // The absent one is four bytes shorter: the count is not written when there
    // is nothing to write.
    assert_eq!(absent.bytes().len() + 4, zero.bytes().len());

    let absent = decode_stream(absent.bytes()).expect("a stream this crate wrote decodes");
    let zero = decode_stream(zero.bytes()).expect("a stream this crate wrote decodes");
    match (&absent[0], &zero[0]) {
        (
            Command::CreateBindGroup {
                variable_count: absent,
                ..
            },
            Command::CreateBindGroup {
                variable_count: zero,
                ..
            },
        ) => {
            assert_eq!(*absent, None);
            assert_eq!(*zero, Some(0));
        }
        _ => panic!("expected two CreateBindGroups"),
    }

    // A byte that is neither presence value is refused rather than read as
    // truthy. The presence byte is the last byte of an absent-count body.
    let mut wire = group_of(&[buffer_entry()], None).bytes().to_vec();
    let presence_at = wire.len() - 1;
    wire[presence_at] = 2;
    assert_eq!(
        decode_stream(&wire),
        Err(DecodeError::InvalidEnum {
            field: "BindGroupDesc::variable_count",
            code: 2,
        })
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
    // CreateImageViews, four CreateSamplers, six CreateBindGroupLayouts, two
    // CreateBindGroups, three CreateShaderModules, two CreatePipelineLayouts, one
    // CreateComputePipeline and its DestroyComputePipeline, one
    // CreateGraphicsPipeline and its DestroyGraphicsPipeline, two each of
    // BeginRenderPass, BindGroup, BeginComputePass, RequestDevice, Submit and
    // RequestReadback, and one each of the rest of the readback path
    // (CreateCommandEncoder, EndRenderPass, CopyImageToBuffer, Finish,
    // PollReadback, DestroyReadback, DestroyCommandBuffer) and of the compute pass
    // (BindComputePipeline, Dispatch, EndComputePass, CopyBufferToBuffer), the
    // remaining copies and fill (CopyBufferToImage, CopyImageToImage, FillBuffer),
    // and the no-op barrier (PipelineBarrier, twice but one name) — so the
    // distinct-name count is what the writer has methods for.
    assert_eq!(names.len(), 49);
    assert!(names.iter().all(|name| !name.is_empty()));
}

// ── Shader modules ────────────────────────────────────────────────────────────

/// Encodes one unlabelled shader module holding the four artifacts given, and
/// returns its bytes. The label is absent so every field after it sits at a
/// fixed offset.
fn module_of(
    spirv: &[u32],
    wgsl: Option<&str>,
    msl: Option<&str>,
    dxil: &[(&str, &[u8])],
) -> Vec<u8> {
    let mut stream = StreamWriter::new();
    stream.create_shader_module(
        handle(1, 1),
        &ShaderModuleDesc {
            label: None,
            spirv,
            wgsl,
            msl,
            dxil,
        },
    );
    stream.bytes().to_vec()
}

/// The single command a `module_of` buffer decodes to.
fn decode_module(bytes: &[u8]) -> Command {
    decode_stream(bytes)
        .expect("a stream this crate wrote decodes")
        .into_iter()
        .next()
        .expect("one command")
}

/// **Every field of the seam's heaviest descriptor crosses, in the descriptor's
/// order, with all four artifacts non-trivial.**
///
/// The round trip over the corpus says the whole set survives; what this pins is
/// that no artifact can be dropped or read in another's place — every value is
/// distinct, and the `dxil` list's two entries differ in both their string and
/// their container lengths so a decoder reading the wrong length for either leaf
/// answers a different module.
#[test]
fn a_shader_module_carries_every_artifact_in_the_descriptors_order() {
    let module = handle(5, 6);
    let spirv = [0x0723_0203u32, 0x0001_0600, 42, 7, 0];
    let dxil: &[(&str, &[u8])] = &[
        ("vsMain", &[0xDE, 0xAD, 0xBE, 0xEF]),
        ("fragment", &[0x01, 0x02]),
    ];
    let mut stream = StreamWriter::new();
    stream.create_shader_module(
        module,
        &ShaderModuleDesc {
            label: Some("mesh.slang"),
            spirv: &spirv,
            wgsl: Some("@vertex fn vs() {}"),
            msl: Some("vertex void vs() {}"),
            dxil,
        },
    );
    stream.destroy_shader_module(handle(7, 8));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateShaderModule {
                module,
                label: Some("mesh.slang".into()),
                spirv: spirv.to_vec(),
                wgsl: Some("@vertex fn vs() {}".into()),
                msl: Some("vertex void vs() {}".into()),
                dxil: vec![
                    ("vsMain".into(), vec![0xDE, 0xAD, 0xBE, 0xEF]),
                    ("fragment".into(), vec![0x01, 0x02]),
                ],
            },
            Command::DestroyShaderModule {
                module: handle(7, 8),
            },
        ])
    );
}

/// **The four absence conventions each survive, and they are four distinct
/// traps.** `spirv` empty is absent; `wgsl` and `msl` keep `Some("")` — a valid
/// empty module — apart from `None`; and `dxil`'s empty list is absence while a
/// pair whose container is empty is a present, truncated artifact. A decoder
/// that treated `Some("")` as `None`, or an empty container as an absent
/// artifact, goes red here.
#[test]
fn a_shader_module_keeps_each_artifacts_absence_convention_distinct() {
    match decode_module(&module_of(&[], Some(""), Some(""), &[("main", &[])])) {
        Command::CreateShaderModule {
            spirv,
            wgsl,
            msl,
            dxil,
            ..
        } => {
            assert!(spirv.is_empty(), "an empty spirv slice is absent");
            assert_eq!(
                wgsl.as_deref(),
                Some(""),
                "a Some(\"\") wgsl is present and empty, not None"
            );
            assert_eq!(
                msl.as_deref(),
                Some(""),
                "a Some(\"\") msl is present and empty, not None"
            );
            assert_eq!(
                dxil,
                vec![("main".to_string(), Vec::new())],
                "a pair whose container is empty is a present, truncated artifact"
            );
        }
        other => panic!("expected CreateShaderModule, got {}", other.name()),
    }
    match decode_module(&module_of(&[7], None, None, &[])) {
        Command::CreateShaderModule {
            spirv,
            wgsl,
            msl,
            dxil,
            ..
        } => {
            assert_eq!(spirv, vec![7], "a non-empty spirv slice is present");
            assert_eq!(wgsl, None, "an absent wgsl is None, not Some(\"\")");
            assert_eq!(msl, None, "an absent msl is None, not Some(\"\")");
            assert!(dxil.is_empty(), "the empty dxil list is absence");
        }
        other => panic!("expected CreateShaderModule, got {}", other.name()),
    }

    // …and each convention moves the bytes, which is what says the distinction is
    // on the wire rather than only in this crate's own reader.
    assert_ne!(
        module_of(&[], None, None, &[]),
        module_of(&[7], None, None, &[]),
        "spirv empty and non-empty must differ"
    );
    assert_ne!(
        module_of(&[], None, None, &[]),
        module_of(&[], Some(""), None, &[]),
        "wgsl None and Some(\"\") must differ"
    );
    assert_ne!(
        module_of(&[], Some(""), None, &[]),
        module_of(&[], Some("x"), None, &[]),
        "wgsl Some(\"\") and Some(\"x\") must differ"
    );
    assert_ne!(
        module_of(&[], None, None, &[]),
        module_of(&[], None, Some(""), &[]),
        "msl None and Some(\"\") must differ"
    );
    assert_ne!(
        module_of(&[], None, Some(""), &[]),
        module_of(&[], None, Some("x"), &[]),
        "msl Some(\"\") and Some(\"x\") must differ"
    );
    assert_ne!(
        module_of(&[], None, None, &[]),
        module_of(&[], None, None, &[("main", &[])]),
        "an empty dxil list and a pair with an empty container must differ"
    );
}

/// **The `spirv` words survive bit for bit and the reader returns the right
/// count**, carried as a `u32` word count then that many little-endian words.
///
/// A byte-length or decimal encoding, or a reader that returned a wrong count,
/// would not reproduce the bytes: the assertion is on the wire, not just the
/// decode.
#[test]
fn a_shader_modules_spirv_words_survive_bit_for_bit_and_the_reader_returns_the_right_count() {
    let words = [0x0723_0203u32, 0x0001_0600, 0xDEAD_BEEF, 0, u32::MAX];
    let bytes = module_of(&words, None, None, &[]);
    match decode_module(&bytes) {
        Command::CreateShaderModule { spirv, .. } => {
            assert_eq!(spirv.len(), words.len(), "the reader returns every word");
            assert_eq!(spirv, words.to_vec(), "each word survives bit for bit");
        }
        other => panic!("expected CreateShaderModule, got {}", other.name()),
    }

    // The count and the words are the body after the tag, the handle and the
    // absent label's presence byte.
    let count_at = tag::HEADER_BYTES + 1 + 8 + 1;
    assert_eq!(
        &bytes[count_at..count_at + 4],
        &(words.len() as u32).to_le_bytes(),
        "the word count is a u32 prefix"
    );
    for (index, word) in words.iter().enumerate() {
        let at = count_at + 4 + index * 4;
        assert_eq!(
            &bytes[at..at + 4],
            &word.to_le_bytes(),
            "word {index} is little-endian on the wire"
        );
    }
}

/// **The `dxil` pair-list round-trips, two entries with different string and
/// container lengths.** `dxil` is the worst-shaped field on the seam — a counted
/// list whose element is a length-prefixed string *and* a length-prefixed byte
/// slice — so the two entries below differ in both leaf lengths, and a decoder
/// reading the wrong length for either lands the cursor wrong and answers the
/// wrong pairs.
#[test]
fn a_shader_modules_dxil_pair_list_round_trips_with_two_differently_sized_entries() {
    let dxil: &[(&str, &[u8])] = &[("vsMain", &[1, 2, 3, 4]), ("ps", &[9])];
    assert_ne!(
        dxil[0].0.len(),
        dxil[1].0.len(),
        "the two entry-point names are different lengths"
    );
    assert_ne!(
        dxil[0].1.len(),
        dxil[1].1.len(),
        "the two containers are different lengths"
    );

    match decode_module(&module_of(&[], None, None, dxil)) {
        Command::CreateShaderModule { dxil: got, .. } => {
            assert_eq!(
                got,
                vec![
                    ("vsMain".to_string(), vec![1, 2, 3, 4]),
                    ("ps".to_string(), vec![9]),
                ]
            );
        }
        other => panic!("expected CreateShaderModule, got {}", other.name()),
    }
}

// ── Pipeline layouts ──────────────────────────────────────────────────────────

/// Encodes one unlabelled pipeline layout and returns its bytes. The label is
/// absent so every field after it sits at a fixed offset.
fn pipeline_layout_of(
    bind_group_layouts: &[BindGroupLayoutHandle],
    push_constants: Option<PushConstantRange>,
) -> Vec<u8> {
    let mut stream = StreamWriter::new();
    stream.create_pipeline_layout(
        handle(1, 1),
        &PipelineLayoutDesc {
            label: None,
            bind_group_layouts,
            push_constants,
        },
    );
    stream.bytes().to_vec()
}

/// The single command a `pipeline_layout_of` buffer decodes to.
fn decode_one(bytes: &[u8]) -> Command {
    decode_stream(bytes)
        .expect("a stream this crate wrote decodes")
        .into_iter()
        .next()
        .expect("one command")
}

/// **The bind-group layouts cross in set order and the list is not reversed.**
///
/// `bind_group_layouts` is what a shader's `@group(n)` indexes, so its order is
/// part of the value rather than a presentation of it — a decoder that reversed
/// it would bind the wrong set to the wrong slot. Two distinct handles are what
/// makes a reversal visible: a single-element list decodes identically whichever
/// way a reader walks it.
#[test]
fn a_pipeline_layout_carries_its_bind_group_layout_list_in_set_order() {
    let first = handle(93, 94);
    let second = handle(95, 96);
    let forward = pipeline_layout_of(&[first, second], None);
    let reversed = pipeline_layout_of(&[second, first], None);
    assert_ne!(forward, reversed, "set order is not on the wire");

    match decode_one(&forward) {
        Command::CreatePipelineLayout {
            bind_group_layouts,
            push_constants,
            ..
        } => {
            assert_eq!(bind_group_layouts, vec![first, second]);
            assert_eq!(push_constants, None);
        }
        other => panic!("expected CreatePipelineLayout, got {}", other.name()),
    }
    assert_ne!(
        first.to_bits(),
        second.to_bits(),
        "the test would not notice a reversed list otherwise"
    );
}

/// **A `Some` push-constant range round-trips field for field, and an absent one
/// is a distinct, shorter body.**
///
/// WebGPU has no push constants, so a `Some` is refused by the replayer — but it
/// crosses whole so the replayer can refuse it *by name*, which is the writer's
/// "carry what the caller gives" rule. `offset` and `size` are distinct so a swap
/// cannot pass, and `stages` names two bits so it is more than a single-bit value.
#[test]
fn a_pipeline_layouts_push_constant_range_round_trips_and_an_absent_one_differs() {
    let range = PushConstantRange {
        stages: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
        offset: 16,
        size: 128,
    };
    let present = pipeline_layout_of(&[handle(9, 9)], Some(range));
    let absent = pipeline_layout_of(&[handle(9, 9)], None);
    assert_ne!(present, absent);
    // The absent one is twelve bytes shorter: the stages/offset/size trio is not
    // written when there is nothing to write.
    assert_eq!(absent.len() + 12, present.len());

    match decode_one(&present) {
        Command::CreatePipelineLayout {
            push_constants: Some(got),
            ..
        } => {
            assert_eq!(got.stages, range.stages);
            assert_eq!(got.offset, range.offset);
            assert_eq!(got.size, range.size);
            assert_ne!(
                got.offset, got.size,
                "offset and size are equal, so the test would not notice them swapped"
            );
        }
        other => panic!(
            "expected a present push-constant range, got {}",
            other.name()
        ),
    }
    match decode_one(&absent) {
        Command::CreatePipelineLayout { push_constants, .. } => assert_eq!(push_constants, None),
        other => panic!("expected CreatePipelineLayout, got {}", other.name()),
    }
}

/// **The empty pipeline layout is a layout.** No bind-group layouts and no push
/// constants is the empty pipeline layout, which is legal and must build — the
/// counted list at zero, whose end is the push-constant presence byte rather than
/// the next command.
#[test]
fn an_empty_pipeline_layout_is_a_layout() {
    assert_eq!(
        decode_stream(&pipeline_layout_of(&[], None)),
        Ok(vec![Command::CreatePipelineLayout {
            layout: handle(1, 1),
            label: None,
            bind_group_layouts: Vec::new(),
            push_constants: None,
        }])
    );
}

/// A stage bit no `ShaderStages` flag claims in a push-constant range is refused
/// rather than truncated, and a presence byte that is neither canonical value is
/// refused rather than read as truthy.
#[test]
fn a_push_constant_ranges_stage_bit_and_presence_byte_are_both_checked() {
    let range = PushConstantRange {
        stages: ShaderStages::VERTEX,
        offset: 0,
        size: 4,
    };
    let whole = pipeline_layout_of(&[], Some(range));

    // stages, offset and size are the last twelve bytes of the body; the presence
    // byte is the one before them.
    let stages_at = whole.len() - 12;
    let mut bytes = whole.clone();
    bytes[stages_at..stages_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "PushConstantRange::stages",
            code: u32::MAX.into(),
        })
    );

    // One bit past the last claimed stage, where a table that stopped a stage
    // short lands and where `u32::MAX` would not distinguish itself.
    let unclaimed = ShaderStages::all().bits() | (ShaderStages::all().bits() + 1);
    let mut bytes = whole.clone();
    bytes[stages_at..stages_at + 4].copy_from_slice(&unclaimed.to_le_bytes());
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "PushConstantRange::stages",
            ..
        })
    ));

    let presence_at = stages_at - 1;
    let mut bytes = whole;
    bytes[presence_at] = 2;
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "PipelineLayoutDesc::push_constants",
            code: 2,
        })
    );
}

// ── Compute pipelines ─────────────────────────────────────────────────────────

/// **A compute pipeline carries every field of its descriptor in the
/// descriptor's order**, and this is the command where two of those claims are
/// worth the most.
///
/// **The `workgroup_size` is non-uniform on purpose** — `[8, 4, 2]`, three
/// distinct numbers — so a transposition of the three components changes the
/// decode rather than reproducing it. The replayer drops the field (WebGPU reads
/// the real value from the module's `@workgroup_size`), but it round-trips in
/// Rust because Metal reads it from the descriptor, so the wire has to carry it
/// intact.
///
/// **The two handles cross into two *different* tables and are not
/// interchangeable**: `layout` names a pipeline layout and `module` a shader
/// module, and a handle carries no kind, so a body that transposed them would
/// name the wrong object in each. The assertions at the end state that out loud.
#[test]
fn a_compute_pipeline_carries_every_field_of_its_descriptor_in_the_descriptors_order() {
    let pipeline = handle(5, 6);
    let layout = handle(7, 8);
    let module = handle(9, 10);
    let desc = ComputePipelineDesc {
        label: Some("cull"),
        layout,
        compute: ShaderEntry {
            module,
            entry_point: "computeMain",
        },
        workgroup_size: [8, 4, 2],
    };
    let mut stream = StreamWriter::new();
    stream.create_compute_pipeline(pipeline, &desc);
    stream.destroy_compute_pipeline(handle(11, 12));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateComputePipeline {
                pipeline,
                label: Some("cull".into()),
                layout,
                module,
                entry_point: "computeMain".into(),
                workgroup_size: [8, 4, 2],
            },
            Command::DestroyComputePipeline {
                pipeline: handle(11, 12),
            },
        ])
    );

    let [x, y, z] = desc.workgroup_size;
    assert!(
        x != y && y != z && x != z,
        "two workgroup-size components are equal, so the test would not notice \
         them swapped"
    );
    assert_ne!(
        layout.to_bits(),
        module.to_bits(),
        "the two handles must differ, or the test would not notice them swapped"
    );
}

/// **The non-uniform workgroup size is on the wire, little-endian, all three
/// components**, and an absent label puts them at a fixed offset.
///
/// A writer that dropped a component, or resolved the size to what the module
/// declares, would still round-trip through this crate's reader; the assertion is
/// on the bytes.
#[test]
fn a_compute_pipelines_workgroup_size_is_three_u32s_on_the_wire() {
    let desc = ComputePipelineDesc {
        label: None,
        layout: handle(7, 8),
        compute: ShaderEntry {
            module: handle(9, 10),
            entry_point: "cs",
        },
        workgroup_size: [8, 4, 2],
    };
    let mut stream = StreamWriter::new();
    stream.create_compute_pipeline(handle(1, 1), &desc);
    let bytes = stream.bytes();

    // The three components are the last twelve bytes of the body, behind the
    // pipeline handle, the absent label's presence byte, the two descriptor
    // handles and the two-byte `"cs"` entry point with its length prefix.
    let at = tag::HEADER_BYTES + 1 + 8 + 1 + 8 + 8 + 4 + "cs".len();
    for (index, extent) in desc.workgroup_size.iter().enumerate() {
        let component_at = at + index * 4;
        assert_eq!(
            &bytes[component_at..component_at + 4],
            &extent.to_le_bytes(),
            "workgroup-size component {index} is not little-endian on the wire"
        );
    }
    assert_eq!(bytes.len(), at + 12, "the body has grown a field");
}

/// **An absent label is not a present one for a compute pipeline either**, so
/// the `Some("")`/`None` distinction that decides a WGSL truncation elsewhere is
/// held here too.
#[test]
fn a_compute_pipelines_label_keeps_some_empty_apart_from_none() {
    let base = |label| ComputePipelineDesc {
        label,
        layout: handle(7, 8),
        compute: ShaderEntry {
            module: handle(9, 10),
            entry_point: "main",
        },
        workgroup_size: [1, 1, 1],
    };
    let mut empty = StreamWriter::new();
    empty.create_compute_pipeline(handle(1, 1), &base(Some("")));
    let mut absent = StreamWriter::new();
    absent.create_compute_pipeline(handle(1, 1), &base(None));
    assert_ne!(empty.bytes(), absent.bytes());

    let empty = decode_stream(empty.bytes()).expect("a stream this crate wrote decodes");
    let absent = decode_stream(absent.bytes()).expect("a stream this crate wrote decodes");
    match (&empty[0], &absent[0]) {
        (
            Command::CreateComputePipeline { label: empty, .. },
            Command::CreateComputePipeline { label: absent, .. },
        ) => {
            assert_eq!(empty.as_deref(), Some(""));
            assert_eq!(*absent, None);
        }
        _ => panic!("expected two CreateComputePipelines"),
    }
}

// ── Graphics pipelines ────────────────────────────────────────────────────────

/// A rich, non-default graphics pipeline: a `Some` fragment, a `Some`
/// depth-stencil with a `Some` stencil whose `front` and `back` differ in every
/// field, a non-trivial bias, MSAA 4, and two colour targets with distinct
/// formats — one blended, one not. The one every graphics-pipeline test below
/// varies a field of.
fn rich_graphics_pipeline() -> GraphicsPipelineDesc<'static> {
    GraphicsPipelineDesc {
        label: Some("gbuffer"),
        layout: handle(121, 122),
        vertex: ShaderEntry {
            module: handle(113, 114),
            entry_point: "vertexMain",
        },
        fragment: Some(ShaderEntry {
            module: handle(115, 116),
            entry_point: "fragmentMain",
        }),
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleStrip,
            front_face: FrontFace::Cw,
            cull_mode: CullMode::Back,
            polygon_mode: PolygonMode::Fill,
            depth_clamp: false,
        },
        depth_stencil: Some(DepthStencilState {
            format: Format::D32FloatS8Uint,
            depth_write: false,
            depth_compare: CompareOp::GreaterOrEqual,
            stencil: Some(StencilState {
                front: StencilFaceState {
                    compare: CompareOp::Less,
                    fail_op: StencilOp::Keep,
                    depth_fail_op: StencilOp::IncrementWrap,
                    pass_op: StencilOp::Replace,
                },
                back: StencilFaceState {
                    compare: CompareOp::Greater,
                    fail_op: StencilOp::Zero,
                    depth_fail_op: StencilOp::DecrementClamp,
                    pass_op: StencilOp::Invert,
                },
                read_mask: 0x0F,
                write_mask: 0xF0,
                reference: 0x2A,
            }),
            bias: DepthBias {
                constant: -2.0,
                slope_scale: 0.1,
                clamp: 0.25,
            },
        }),
        multisample: MultisampleState {
            samples: 4,
            mask: 0x0000_00FF,
            alpha_to_coverage: true,
        },
        color_targets: &TWO_TARGETS,
    }
}

/// The two colour targets `rich_graphics_pipeline` names, held out so the slice
/// outlives the descriptor a `'static` return needs.
static TWO_TARGETS: [ColorTargetState; 2] = [
    ColorTargetState {
        format: Format::Rgba16Float,
        blend: Some(BlendState {
            color_src: BlendFactor::SrcAlpha,
            color_dst: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
        }),
        write_mask: ColorWrites::ALL,
    },
    ColorTargetState {
        format: Format::Rg16Float,
        blend: None,
        write_mask: ColorWrites::R.union(ColorWrites::G),
    },
];

/// **The whole nested tree survives a round trip field for field.**
///
/// The claims that cost the most, stated as assertions at the end: the stencil's
/// `front` and `back` differ in every field so a swap goes red, the bias floats
/// are bit-exact including one no short decimal names, and the two colour targets
/// stay in order with distinct formats.
#[test]
fn a_graphics_pipeline_carries_its_whole_nested_tree_field_for_field() {
    let desc = rich_graphics_pipeline();
    let mut stream = StreamWriter::new();
    stream.create_graphics_pipeline(handle(131, 132), &desc);
    stream.destroy_graphics_pipeline(handle(133, 134));

    let decoded = decode_stream(stream.bytes()).expect("a stream this crate wrote decodes");
    assert_eq!(decoded.len(), 2);
    let Command::CreateGraphicsPipeline {
        pipeline,
        label,
        layout,
        vertex_module,
        vertex_entry_point,
        fragment,
        primitive,
        depth_stencil,
        multisample,
        color_targets,
    } = &decoded[0]
    else {
        panic!("expected CreateGraphicsPipeline, got {}", decoded[0].name());
    };
    assert_eq!(*pipeline, handle(131, 132));
    assert_eq!(label.as_deref(), Some("gbuffer"));
    assert_eq!(*layout, handle(121, 122));
    assert_eq!(*vertex_module, handle(113, 114));
    assert_eq!(vertex_entry_point, "vertexMain");
    assert_eq!(
        *fragment,
        Some((handle(115, 116), "fragmentMain".to_string()))
    );
    assert_eq!(*primitive, desc.primitive);
    assert_eq!(*depth_stencil, desc.depth_stencil);
    assert_eq!(*multisample, desc.multisample);
    assert_eq!(color_targets.as_slice(), desc.color_targets);
    assert_eq!(
        decoded[1],
        Command::DestroyGraphicsPipeline {
            pipeline: handle(133, 134),
        }
    );

    // The claims worth stating out loud, so the test cannot pass with them
    // quietly untrue.
    let stencil = depth_stencil
        .as_ref()
        .and_then(|ds| ds.stencil.as_ref())
        .expect("the rich pipeline has a stencil");
    assert_ne!(
        stencil.front, stencil.back,
        "front and back are equal, so the test would not notice them swapped"
    );
    assert_ne!(
        stencil.read_mask, stencil.write_mask,
        "the masks are equal, so the test would not notice them swapped"
    );
    let bias = depth_stencil.as_ref().map(|ds| ds.bias).expect("a bias");
    assert_eq!(
        bias.slope_scale, 0.1_f32,
        "the awkward decimal is bit-exact"
    );
    assert_eq!(bias.constant, -2.0_f32);
    assert_eq!(color_targets.len(), 2);
    assert_ne!(
        color_targets[0].format, color_targets[1].format,
        "the two targets share a format, so a reversal would not be noticed"
    );
    assert!(color_targets[0].blend.is_some() && color_targets[1].blend.is_none());
}

/// **An absent fragment is a depth-only pass and a distinct, shorter body** — the
/// `Some("")`/`None` distinction that decides a WGSL truncation elsewhere, applied
/// to the whole fragment stage.
#[test]
fn a_graphics_pipelines_absent_fragment_is_a_shorter_body_than_a_present_one() {
    let present = rich_graphics_pipeline();
    let absent = GraphicsPipelineDesc {
        fragment: None,
        ..rich_graphics_pipeline()
    };
    let mut with = StreamWriter::new();
    with.create_graphics_pipeline(handle(1, 1), &present);
    let mut without = StreamWriter::new();
    without.create_graphics_pipeline(handle(1, 1), &absent);
    assert_ne!(with.bytes(), without.bytes());
    // The absent one is shorter by the fragment module handle and its entry
    // point (its length prefix plus the string), which the present one writes and
    // the absent one does not.
    let fragment_bytes = 8 + 4 + "fragmentMain".len();
    assert_eq!(without.bytes().len() + fragment_bytes, with.bytes().len());

    match decode_stream(without.bytes()).expect("decodes").remove(0) {
        Command::CreateGraphicsPipeline { fragment, .. } => assert_eq!(fragment, None),
        other => panic!("expected CreateGraphicsPipeline, got {}", other.name()),
    }
}

/// **A `None` depth-stencil and an empty colour-target list round-trip** — the
/// two optional/counted fields at their empty extreme, which a reader most easily
/// mistakes for "read until something stops you".
#[test]
fn a_graphics_pipeline_with_no_depth_and_no_targets_round_trips() {
    let desc = GraphicsPipelineDesc {
        depth_stencil: None,
        color_targets: &[],
        ..rich_graphics_pipeline()
    };
    let mut stream = StreamWriter::new();
    stream.create_graphics_pipeline(handle(1, 1), &desc);
    stream.enumerate_adapters();

    let decoded = decode_stream(stream.bytes()).expect("decodes");
    match &decoded[0] {
        Command::CreateGraphicsPipeline {
            depth_stencil,
            color_targets,
            ..
        } => {
            assert_eq!(*depth_stencil, None);
            assert!(color_targets.is_empty());
        }
        other => panic!("expected CreateGraphicsPipeline, got {}", other.name()),
    }
    // The command after it survives: an empty target list ends at the count, not
    // at the next tag.
    assert_eq!(decoded[1], Command::EnumerateAdapters);
}

/// Every nested enum in the tree refuses a code no variant claims, naming the
/// field it belongs to — one red per table — and a `ColorWrites` bit no channel
/// claims is refused rather than truncated.
///
/// The offsets are computed from the wire layout of `rich_graphics_pipeline`
/// behind an absent-then-present body, so each corruption lands on exactly one
/// leaf.
#[test]
fn every_nested_graphics_pipeline_enum_refuses_an_unclaimed_code() {
    let mut stream = StreamWriter::new();
    stream.create_graphics_pipeline(handle(1, 1), &rich_graphics_pipeline());
    let whole = stream.bytes().to_vec();

    // Walk the body to name every leaf's offset. Header, tag, pipeline handle,
    // present label ("gbuffer"), layout handle, vertex module, vertex entry
    // ("vertexMain"), fragment presence + module + entry ("fragmentMain").
    let mut at = tag::HEADER_BYTES + 1; // past the opcode
    at += 8; // pipeline handle
    at += 1 + 4 + "gbuffer".len(); // present label
    at += 8; // layout
    at += 8; // vertex module
    at += 4 + "vertexMain".len(); // vertex entry point
    at += 1; // fragment presence byte (present)
    at += 8; // fragment module
    at += 4 + "fragmentMain".len(); // fragment entry point
    let topology_at = at;
    let front_face_at = topology_at + 1;
    let cull_at = front_face_at + 1;
    let polygon_at = cull_at + 1;
    let depth_clamp_at = polygon_at + 1;
    // depth_stencil: presence, format, depth_write, depth_compare, stencil
    // presence, then front (compare + 3 ops), back (compare + 3 ops).
    let ds_present_at = depth_clamp_at + 1;
    let ds_format_at = ds_present_at + 1;
    let ds_depth_compare_at = ds_format_at + 1 + 1; // + format + depth_write bool
    let stencil_present_at = ds_depth_compare_at + 1;
    let front_compare_at = stencil_present_at + 1;
    let front_fail_at = front_compare_at + 1;
    let back_compare_at = front_compare_at + 4;

    // One leaf per table, each a code no variant claims (0x7F is past every one).
    for (offset, field) in [
        (topology_at, "PrimitiveState::topology"),
        (front_face_at, "PrimitiveState::front_face"),
        (cull_at, "PrimitiveState::cull_mode"),
        (polygon_at, "PrimitiveState::polygon_mode"),
        (ds_format_at, "DepthStencilState::format"),
        (ds_depth_compare_at, "DepthStencilState::depth_compare"),
        (front_compare_at, "StencilFaceState::compare"),
        (front_fail_at, "StencilFaceState::fail_op"),
        (back_compare_at, "StencilFaceState::compare"),
    ] {
        let mut bytes = whole.clone();
        bytes[offset] = 0x7F;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum { field, code: 0x7F }),
            "{field} at offset {offset} was not refused",
        );
    }

    // The depth-clamp and stencil presence bytes are presence fields, so a byte
    // that is neither canonical value is refused naming them.
    for (offset, field) in [
        (depth_clamp_at, "PrimitiveState::depth_clamp"),
        (stencil_present_at, "DepthStencilState::stencil"),
    ] {
        let mut bytes = whole.clone();
        bytes[offset] = 2;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum { field, code: 2 }),
            "{field} presence byte at offset {offset} was not refused",
        );
    }

    // The colour target's blend factors and ops, and its write mask, are past the
    // multisample block and the target count. Find the first target's format by
    // decoding the untouched stream and locating it relative to the end.
    // multisample is samples(4) + mask(4) + alpha_to_coverage(1); then the u32
    // target count; then target 0: format(1), blend presence(1), blend body
    // (6 bytes), write_mask(4); then target 1.
    let stencil_body = 1 + (1 + 3) * 2 + 4 + 4 + 4; // present + two faces + 3 masks
    let bias = 4 + 4 + 4;
    let ms_at = stencil_present_at + stencil_body + bias;
    let target_count_at = ms_at + 4 + 4 + 1;
    let target0_format_at = target_count_at + 4;
    let target0_blend_present_at = target0_format_at + 1;
    let target0_color_src_at = target0_blend_present_at + 1;
    let target0_color_op_at = target0_color_src_at + 2;
    let target0_write_mask_at = target0_color_src_at + 6;

    for (offset, field) in [
        (target0_format_at, "ColorTargetState::format"),
        (target0_color_src_at, "BlendState::color_src"),
        (target0_color_op_at, "BlendState::color_op"),
    ] {
        let mut bytes = whole.clone();
        bytes[offset] = 0x7F;
        assert_eq!(
            decode_stream(&bytes),
            Err(DecodeError::InvalidEnum { field, code: 0x7F }),
            "{field} at offset {offset} was not refused",
        );
    }

    // A ColorWrites bit no channel claims — one past ALL — is refused rather than
    // truncated.
    let unclaimed = ColorWrites::all().bits() + 1;
    let mut bytes = whole.clone();
    bytes[target0_write_mask_at..target0_write_mask_at + 4]
        .copy_from_slice(&unclaimed.to_le_bytes());
    assert_eq!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "ColorTargetState::write_mask",
            code: unclaimed.into(),
        })
    );

    // …and the code one past the last claimed topology, where an off-by-one in
    // the table lands and where 0x7F would not.
    let mut bytes = whole;
    bytes[topology_at] = tag::PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP + 1;
    assert!(matches!(
        decode_stream(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "PrimitiveState::topology",
            ..
        })
    ));
}

/// **The stencil `reference` is carried on the wire and round-trips**, even
/// though it is not a WebGPU pipeline field — it is set per-pass through
/// `setStencilReference`, and the replayer drops it there. A writer that resolved
/// it away would still decode, so this pins it to the byte: the value survives.
#[test]
fn a_graphics_pipelines_stencil_reference_is_carried_rather_than_resolved() {
    let desc = rich_graphics_pipeline();
    let mut stream = StreamWriter::new();
    stream.create_graphics_pipeline(handle(1, 1), &desc);

    match decode_stream(stream.bytes()).expect("decodes").remove(0) {
        Command::CreateGraphicsPipeline { depth_stencil, .. } => {
            let reference = depth_stencil
                .and_then(|ds| ds.stencil)
                .map(|s| s.reference)
                .expect("the rich pipeline has a stencil");
            assert_eq!(reference, 0x2A, "the reference crossed verbatim");
        }
        other => panic!("expected CreateGraphicsPipeline, got {}", other.name()),
    }
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

// ── The readback path (slice 7a) ────────────────────────────────────────────────

#[test]
fn a_copy_to_buffer_carries_every_field_including_a_signed_offset_component() {
    // The copy has more same-typed neighbours than anything else on the stream —
    // a `u64`, two `u32`s, a subresource of three more, a three-`i32` offset and a
    // three-`u32` extent — so every value differs from every other a byte could be
    // read from, and the offset carries a negative so an `i32` read as a `u32`
    // (or vice versa) would not still compare equal.
    let copy = BufferImageCopy {
        buffer: handle(80, 81),
        buffer_offset: 256,
        buffer_row_length: 100,
        buffer_image_height: 200,
        image: handle(82, 83),
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 2,
            base_layer: 3,
            layer_count: 5,
        },
        image_offset: Offset3d { x: -7, y: 9, z: 11 },
        image_extent: Extent3d {
            width: 64,
            height: 48,
            depth_or_layers: 1,
        },
    };
    let mut stream = StreamWriter::new();
    stream.copy_image_to_buffer(&copy);

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![Command::CopyImageToBuffer {
            buffer: copy.buffer,
            buffer_offset: copy.buffer_offset,
            buffer_row_length: copy.buffer_row_length,
            buffer_image_height: copy.buffer_image_height,
            image: copy.image,
            image_subresource: copy.image_subresource,
            image_offset: copy.image_offset,
            image_extent: copy.image_extent,
        }])
    );
    assert_ne!(
        copy.buffer.to_bits(),
        copy.image.to_bits(),
        "the test would not notice the two handles swapped otherwise"
    );
    assert!(
        copy.image_offset.x < 0,
        "a negative component is what makes the i32/u32 confusion visible"
    );
}

#[test]
fn a_submit_carries_its_buffers_waits_and_signals_in_order() {
    // Every handle and every value distinct, so a list read at the wrong stride —
    // or a wait read where a signal belongs — does not still compare equal. The
    // waits and signals are non-empty here though no browser honours them: the
    // encoding must carry them, and the replayer is where they are refused.
    let submit = SubmitInfo {
        command_buffers: &[handle(90, 91), handle(92, 93)],
        waits: &[SemaphoreWait {
            semaphore: handle(94, 95),
            value: 0x0102_0304_0506_0708,
        }],
        signals: &[SemaphoreSignal {
            semaphore: handle(96, 97),
            value: 9,
        }],
    };
    let mut stream = StreamWriter::new();
    stream.submit(&submit);

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![Command::Submit {
            command_buffers: vec![handle(90, 91), handle(92, 93)],
            waits: vec![SemaphoreWait {
                semaphore: handle(94, 95),
                value: 0x0102_0304_0506_0708,
            }],
            signals: vec![SemaphoreSignal {
                semaphore: handle(96, 97),
                value: 9,
            }],
        }])
    );
}

#[test]
fn a_bare_submit_carries_its_empty_lists_rather_than_dropping_them() {
    let buffers = [handle(90, 91)];
    let submit = SubmitInfo::new(&buffers);
    let mut stream = StreamWriter::new();
    stream.submit(&submit);
    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![Command::Submit {
            command_buffers: vec![handle(90, 91)],
            waits: Vec::new(),
            signals: Vec::new(),
        }])
    );
}

#[test]
fn a_readbacks_after_is_distinguishable_present_from_absent() {
    // The presence byte is what keeps `Some(wait)` — a semaphore the replayer
    // refuses — apart from `None`, which is `mapAsync`. A decoder that read the
    // wait unconditionally would consume the handle after the size on a `None`.
    let with_wait = ReadbackDesc {
        label: Some("stats"),
        buffer: handle(100, 101),
        offset: 32,
        size: 64,
        after: Some(SemaphoreWait {
            semaphore: handle(102, 103),
            value: 0x1122_3344_5566_7788,
        }),
    };
    let without = ReadbackDesc {
        after: None,
        ..with_wait
    };
    let mut present = StreamWriter::new();
    present.request_readback(handle(104, 105), &with_wait);
    let mut absent = StreamWriter::new();
    absent.request_readback(handle(104, 105), &without);

    let present = decode_stream(present.bytes()).expect("a stream this crate wrote decodes");
    let absent = decode_stream(absent.bytes()).expect("a stream this crate wrote decodes");
    match (&present[0], &absent[0]) {
        (
            Command::RequestReadback {
                after: Some(wait), ..
            },
            Command::RequestReadback { after: None, .. },
        ) => {
            assert_eq!(wait.value, 0x1122_3344_5566_7788);
            assert_eq!(wait.semaphore, handle(102, 103));
        }
        other => panic!("the two `after` spellings did not survive: {other:?}"),
    }
    assert_ne!(
        present, absent,
        "a decoder that ignored the presence byte would make the two equal"
    );
}

#[test]
fn an_encoder_and_its_finish_round_trip_with_the_queue_that_selects_nothing() {
    let mut stream = StreamWriter::new();
    stream.create_command_encoder(&CommandEncoderDesc {
        label: Some("frame"),
        queue: handle(110, 111),
    });
    stream.end_render_pass();
    stream.finish(handle(112, 113));
    stream.destroy_command_buffer(handle(114, 115));

    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::CreateCommandEncoder {
                label: Some("frame".into()),
                queue: handle(110, 111),
            },
            Command::EndRenderPass,
            Command::Finish {
                command_buffer: handle(112, 113),
            },
            Command::DestroyCommandBuffer {
                command_buffer: handle(114, 115),
            },
        ])
    );
}

#[test]
fn a_poll_and_destroy_readback_carry_only_their_handle() {
    let mut stream = StreamWriter::new();
    stream.poll_readback(handle(120, 121));
    stream.destroy_readback(handle(122, 123));
    assert_eq!(
        decode_stream(stream.bytes()),
        Ok(vec![
            Command::PollReadback {
                readback: handle(120, 121),
            },
            Command::DestroyReadback {
                readback: handle(122, 123),
            },
        ])
    );
}
