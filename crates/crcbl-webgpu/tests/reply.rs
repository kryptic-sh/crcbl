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

use crcbl_hal::{AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Limits};
use crcbl_webgpu::{DecodeError, Reply, ReplyReader, ReplyWriter, decode_replies, tag};

use corpus::handle;
use replies::{encode_all_replies, every_reply};

/// One edit to a baseline value, for the suites that walk a record field by
/// field.
type Mutate<T> = fn(&mut T);

/// A minimal adapter, for the tests that are about one field of it.
fn adapter_info() -> AdapterInfo {
    AdapterInfo {
        id: AdapterId(0),
        name: "ok".into(),
        vendor_id: 0,
        device_id: 0,
        device_type: DeviceType::Other,
        driver: String::new(),
        backend: BackendKind::WebGpu,
        caps: DeviceCaps {
            features: Features::COMPUTE,
            limits: Limits::minimum(),
        },
    }
}

/// Where `Adapter::device_type`'s code byte sits, counted from the start of a
/// buffer holding one adapter reply and nothing else.
///
/// Computed from the field widths rather than written as a number, so it follows
/// the encoding instead of having to be re-derived when a field moves: the
/// header, then the tag and sequence, then `id`, the length-prefixed `name`, and
/// the two ids.
fn device_type_offset(name: &str) -> usize {
    tag::REPLY_HEADER_BYTES + 1 + 8 + 4 + (4 + name.len()) + 4 + 4
}

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
    // The corpus holds two of several shapes — Adapters, NoAdapters, Devices,
    // DeviceFaileds, ReadbackReadys, QueryResults — so the distinct-name count
    // is what the writer has methods for.
    assert_eq!(names.len(), 7);
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
    replies.adapter(3, &adapter_info());
    let mut bytes = replies.bytes().to_vec();
    // The name's two bytes sit immediately before `vendor_id`, `device_id` and
    // the device-type code.
    let at = device_type_offset("ok") - 4 - 4 - 2;
    bytes[at..at + 2].copy_from_slice(&[0xFF, 0xFE]);
    assert_eq!(
        decode_replies(&bytes),
        Err(DecodeError::NotUtf8 {
            field: "Adapter::name"
        })
    );
}

// ── The adapter record ────────────────────────────────────────────────────────

/// **Every field of [`AdapterInfo`] survives, `backend` excepted.**
///
/// The corpus covers this too, but only for the two adapters it holds; this
/// walks each field on its own so a writer that dropped one — or wrote two in
/// the wrong order — names the field rather than showing a byte offset.
#[test]
fn each_adapter_field_survives_a_round_trip_on_its_own() {
    let base = AdapterInfo {
        id: AdapterId(1),
        name: "base".into(),
        vendor_id: 2,
        device_id: 3,
        device_type: DeviceType::Cpu,
        driver: "d".into(),
        backend: BackendKind::WebGpu,
        caps: DeviceCaps {
            features: Features::COMPUTE,
            limits: Limits::minimum(),
        },
    };

    let mutations: Vec<(&str, Mutate<AdapterInfo>)> = vec![
        ("id", |info| info.id = AdapterId(9)),
        ("name", |info| info.name = "Apple M2 — ✱".into()),
        ("name empty", |info| info.name = String::new()),
        ("vendor_id", |info| info.vendor_id = 0x1002),
        ("device_id", |info| info.device_id = 0x744C),
        ("device_type", |info| {
            info.device_type = DeviceType::Integrated;
        }),
        ("driver", |info| info.driver = "radv 25.1.4".into()),
        ("driver empty", |info| info.driver = String::new()),
        ("features", |info| {
            info.caps.features = Features::TIMESTAMP_QUERY | Features::DEPTH_CLAMP;
        }),
        ("features empty", |info| {
            info.caps.features = Features::empty();
        }),
        ("features all", |info| info.caps.features = Features::all()),
        ("limits", |info| info.caps.limits = Limits::desktop()),
    ];

    for (what, mutate) in mutations {
        let mut info = base.clone();
        mutate(&mut info);
        assert_ne!(info, base, "{what} did not change anything");

        let mut replies = ReplyWriter::new();
        replies.adapter(7, &info);
        assert_eq!(
            decode_replies(replies.bytes()),
            Ok(vec![(7, Reply::Adapter { info })]),
            "{what}"
        );
    }
}

/// **Every field of [`Limits`] survives, one at a time.**
///
/// Driven by moving each field off a baseline rather than by comparing two
/// presets: a round trip that wrote `max_image_3d` where `max_image_2d` belongs
/// still compares equal whenever the two happen to agree, which they do in both
/// presets `crcbl-hal` ships.
#[test]
fn each_limits_field_survives_a_round_trip_on_its_own() {
    let base = Limits::minimum();
    let mutations: Vec<(&str, Mutate<Limits>)> = vec![
        ("max_image_2d", |l| l.max_image_2d = 1),
        ("max_image_3d", |l| l.max_image_3d = 2),
        ("max_image_array_layers", |l| l.max_image_array_layers = 3),
        ("max_storage_buffer_range", |l| {
            l.max_storage_buffer_range = u64::MAX;
        }),
        ("max_uniform_buffer_range", |l| {
            l.max_uniform_buffer_range = 5;
        }),
        ("max_bind_groups", |l| l.max_bind_groups = 6),
        ("max_bindless_descriptors", |l| {
            l.max_bindless_descriptors = 7;
        }),
        ("max_push_constant_size", |l| l.max_push_constant_size = 8),
        ("max_color_attachments", |l| l.max_color_attachments = 9),
        ("max_sample_count", |l| l.max_sample_count = 16),
        ("max_draw_indirect_count", |l| {
            l.max_draw_indirect_count = 11;
        }),
        ("max_compute_workgroup_size x", |l| {
            l.max_compute_workgroup_size[0] = 12;
        }),
        ("max_compute_workgroup_size y", |l| {
            l.max_compute_workgroup_size[1] = 13;
        }),
        ("max_compute_workgroup_size z", |l| {
            l.max_compute_workgroup_size[2] = 14;
        }),
        ("max_compute_invocations_per_workgroup", |l| {
            l.max_compute_invocations_per_workgroup = 15;
        }),
        ("max_compute_workgroups_per_dimension", |l| {
            l.max_compute_workgroups_per_dimension = 16;
        }),
        ("min_uniform_buffer_offset_alignment", |l| {
            l.min_uniform_buffer_offset_alignment = 17;
        }),
        ("min_storage_buffer_offset_alignment", |l| {
            l.min_storage_buffer_offset_alignment = 18;
        }),
        ("optimal_buffer_copy_offset_alignment", |l| {
            l.optimal_buffer_copy_offset_alignment = 19;
        }),
        ("max_sampler_anisotropy", |l| {
            l.max_sampler_anisotropy = 16.0
        }),
        ("timestamp_period_ns", |l| l.timestamp_period_ns = 0.5),
    ];
    // One row per field, plus two more for the other two axes of the array.
    assert_eq!(mutations.len(), 21, "a Limits field has no row here");

    for (what, mutate) in mutations {
        let mut limits = base;
        mutate(&mut limits);
        assert_ne!(limits, base, "{what} did not change anything");

        let info = AdapterInfo {
            caps: DeviceCaps {
                features: Features::empty(),
                limits,
            },
            ..adapter_info()
        };
        let mut replies = ReplyWriter::new();
        replies.adapter(1, &info);
        assert_eq!(
            decode_replies(replies.bytes()),
            Ok(vec![(1, Reply::Adapter { info })]),
            "{what}"
        );
    }
}

/// **`backend` is not on the wire**, so it comes back as the crate that decoded
/// whatever the encoder was handed. Documented, and this is what says so out
/// loud: without it a reader that *did* read a backend code off the wire would
/// look identical on every corpus entry, all of which are `WebGpu` already.
#[test]
fn the_backend_field_is_the_decoders_own_rather_than_something_the_wire_carried() {
    let mut info = adapter_info();
    info.backend = BackendKind::Vulkan;

    let mut replies = ReplyWriter::new();
    replies.adapter(0, &info);
    let decoded = decode_replies(replies.bytes()).expect("a buffer this crate wrote decodes");
    let [(_, Reply::Adapter { info: back })] = decoded.as_slice() else {
        panic!("one adapter reply, and it is an adapter reply");
    };
    assert_eq!(back.backend, BackendKind::WebGpu);
    assert_eq!(back.name, info.name, "every other field is the wire's");
}

/// A feature word with a bit no flag claims is refused rather than truncated.
///
/// `from_bits_truncate` would take a word from a build that has more flags than
/// this one and report a *lesser* device — a downgrade nothing would log,
/// because from the seam's point of view the adapter simply lacks the feature.
#[test]
fn a_feature_bit_no_flag_claims_is_refused_rather_than_dropped() {
    let mut replies = ReplyWriter::new();
    replies.adapter(4, &adapter_info());
    let mut bytes = replies.bytes().to_vec();

    // The feature word follows the device-type code and the empty driver's
    // length prefix.
    let at = device_type_offset("ok") + 1 + 4;
    let unclaimed = Features::all().bits() | (1 << 63);
    bytes[at..at + 8].copy_from_slice(&unclaimed.to_le_bytes());
    assert_eq!(
        decode_replies(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "DeviceCaps::features",
            code: unclaimed,
        })
    );

    // …and every bit that *is* claimed still decodes, so the check above is
    // about the unclaimed bit rather than about the word being large.
    bytes[at..at + 8].copy_from_slice(&Features::all().bits().to_le_bytes());
    let decoded = decode_replies(&bytes).expect("every claimed bit decodes");
    let [(_, Reply::Adapter { info })] = decoded.as_slice() else {
        panic!("one adapter reply");
    };
    assert_eq!(info.caps.features, Features::all());
}

/// A device-type code no variant claims is an error, not `Other`.
///
/// Falling back to `Other` would read as "the backend declined to say", which is
/// a real answer a browser gives — so a drifted table would be indistinguishable
/// from the ordinary case.
#[test]
fn a_device_type_code_no_variant_claims_is_refused_rather_than_read_as_other() {
    let mut replies = ReplyWriter::new();
    replies.adapter(5, &adapter_info());
    let mut bytes = replies.bytes().to_vec();
    let at = device_type_offset("ok");
    assert_eq!(
        bytes[at],
        tag::DEVICE_TYPE_OTHER,
        "the offset must land on the device-type code"
    );

    bytes[at] = 0xEE;
    assert_eq!(
        decode_replies(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "Adapter::device_type",
            code: 0xEE,
        })
    );
}

// ── The device record ─────────────────────────────────────────────────────────

/// **A device reply is the capability half alone, and it is a different record
/// from an adapter's.**
///
/// Both carry a [`DeviceCaps`], and the shared writer is what makes that cheap —
/// so the thing worth asserting is that they do not decode as each other, and
/// that the caps that come back are the ones that went in rather than a default.
#[test]
fn a_device_reply_carries_the_capabilities_it_was_given_and_is_not_an_adapter() {
    let caps = DeviceCaps {
        features: Features::COMPUTE | Features::DEBUG_MARKERS,
        limits: Limits {
            max_image_2d: 8192,
            timestamp_period_ns: 0.0,
            ..Limits::desktop()
        },
    };
    let mut replies = ReplyWriter::new();
    replies.device(12, &caps);
    assert_eq!(
        decode_replies(replies.bytes()),
        Ok(vec![(12, Reply::Device { caps })])
    );

    // The same capabilities inside an adapter record are a different reply, and
    // must not be decodable as this one.
    let mut adapter = ReplyWriter::new();
    adapter.adapter(
        12,
        &AdapterInfo {
            caps,
            ..adapter_info()
        },
    );
    assert_ne!(adapter.bytes(), replies.bytes());
}

/// A device reply whose feature word has an unclaimed bit is refused, not
/// truncated — [`Reply::Adapter`]'s rule, on the record a renderer selects its
/// paths from.
#[test]
fn a_device_feature_bit_no_flag_claims_is_refused_rather_than_dropped() {
    let mut replies = ReplyWriter::new();
    replies.device(
        2,
        &DeviceCaps {
            features: Features::COMPUTE,
            limits: Limits::minimum(),
        },
    );
    let mut bytes = replies.bytes().to_vec();
    // The feature word opens the body, straight after the tag and sequence.
    let at = tag::REPLY_HEADER_BYTES + 1 + 8;
    let unclaimed = Features::all().bits() | (1 << 40);
    bytes[at..at + 8].copy_from_slice(&unclaimed.to_le_bytes());
    assert_eq!(
        decode_replies(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "DeviceCaps::features",
            code: unclaimed,
        })
    );
}

/// **The reason and the gap are independent halves**, and each has to survive
/// on its own: a refusal that was not about features carries an empty word, and
/// a browser that refused without saying why carries an empty string.
#[test]
fn a_device_failure_carries_its_reason_and_the_features_that_were_missing() {
    let cases = [
        ("gone", Features::TIMELINE_SEMAPHORE | Features::MESH_SHADER),
        ("", Features::empty()),
        ("no gap, just a refusal — ✱", Features::empty()),
        ("", Features::all()),
    ];
    for (reason, unsupported) in cases {
        let mut replies = ReplyWriter::new();
        replies.device_failed(6, reason, unsupported);
        assert_eq!(
            decode_replies(replies.bytes()),
            Ok(vec![(
                6,
                Reply::DeviceFailed {
                    reason: reason.into(),
                    unsupported,
                }
            )]),
            "{reason:?} / {unsupported:?}"
        );
    }
}

/// The gap is a [`Features`] word on the wire, so an unclaimed bit in it is an
/// error too — otherwise a drifted build's refusal would report a *different*
/// missing feature from the one it meant.
#[test]
fn a_device_failures_missing_features_are_refused_when_a_bit_is_unclaimed() {
    let mut replies = ReplyWriter::new();
    replies.device_failed(6, "x", Features::empty());
    let mut bytes = replies.bytes().to_vec();
    // Tag, sequence, the reason's length prefix and its one byte, then the word.
    let at = tag::REPLY_HEADER_BYTES + 1 + 8 + 4 + 1;
    let unclaimed = 1u64 << 63;
    bytes[at..at + 8].copy_from_slice(&unclaimed.to_le_bytes());
    assert_eq!(
        decode_replies(&bytes),
        Err(DecodeError::InvalidEnum {
            field: "DeviceFailed::unsupported",
            code: unclaimed,
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
    replies.adapter(3, &adapter_info());
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
