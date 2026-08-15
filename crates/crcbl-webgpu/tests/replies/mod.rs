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

use crcbl_hal::{AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Limits};
use crcbl_webgpu::{Reply, ReplyWriter};

use crate::corpus::handle;

/// Every [`Features`] flag `web/engine/gpu-replay.js` can ever set, which is the
/// whole of what a browser can grant.
///
/// Spelled out here and **derived from the production mapping table on the
/// JavaScript side**, which is what makes the fixture a check on the mapping
/// rather than only on the byte writer: `reply-encode.mjs` runs
/// `halFeaturesFor` over a stub adapter holding every WebGPU feature name that
/// maps, and the bits it produces have to be these. Four flags come from a
/// `GPUFeatureName` and four from core WebGPU, which needs no name at all —
/// `crate::reply`'s docs and `gpu-replay.js`'s header carry the mapping in both
/// directions, including what is dropped each way.
const WEBGPU_REACHABLE: Features = Features::COMPUTE
    .union(Features::OCCLUSION_QUERY)
    .union(Features::DEPTH_BIAS_CLAMP)
    .union(Features::DEBUG_MARKERS)
    .union(Features::INDIRECT_FIRST_INSTANCE)
    .union(Features::TIMESTAMP_QUERY)
    .union(Features::DEPTH_CLAMP)
    .union(Features::TEXTURE_COMPRESSION_BC);

/// The limits a browser's `GPUSupportedLimits` maps onto, with the four values
/// WebGPU has no limit for at the numbers `gpu-replay.js` writes.
///
/// Not [`Limits::minimum`]: a preset both halves could reach by name would let
/// a field written in the wrong order still compare equal, and this corpus
/// exists to make that a byte difference. Every value below is distinct, and the
/// two `f32`s are exact in `f32` so the JavaScript writer — whose numbers are
/// `f64` until they reach the wire — produces the same bytes.
fn every_limit() -> Limits {
    Limits {
        max_image_2d: 16384,
        max_image_3d: 2048,
        max_image_array_layers: 256,
        max_storage_buffer_range: 134_217_728,
        max_uniform_buffer_range: 65536,
        max_bind_groups: 4,
        // `0` is the value `Limits` documents for a device without bindless,
        // and a browser is always such a device.
        max_bindless_descriptors: 0,
        max_push_constant_size: 0,
        max_color_attachments: 8,
        max_sample_count: 4,
        max_draw_indirect_count: 1,
        max_compute_workgroup_size: [256, 254, 64],
        max_compute_invocations_per_workgroup: 256,
        max_compute_workgroups_per_dimension: 65535,
        min_uniform_buffer_offset_alignment: 256,
        min_storage_buffer_offset_alignment: 128,
        optimal_buffer_copy_offset_alignment: 512,
        max_sampler_anisotropy: 1.0,
        timestamp_period_ns: 1.0,
    }
}

/// The [`AdapterInfo`] a browser that granted everything it could would produce.
///
/// `vendor_id`, `device_id`, `device_type` and `driver` are the documented
/// absences rather than plausible values — WebGPU reports no numeric ids, no
/// device class and no driver at all. `backend` is
/// [`BackendKind::WebGpu`] because the field never crosses the wire: the decoder
/// names the crate that decoded, so this is the only value a round trip can
/// produce.
fn every_adapter_field() -> AdapterInfo {
    AdapterInfo {
        id: AdapterId(3),
        name: "Apple M2 — ✱".into(),
        vendor_id: 0,
        device_id: 0,
        device_type: DeviceType::Other,
        driver: String::new(),
        backend: BackendKind::WebGpu,
        caps: DeviceCaps {
            features: WEBGPU_REACHABLE,
            limits: every_limit(),
        },
    }
}

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
                info: every_adapter_field(),
            },
        ),
        // The other end of the same shape, and **not a browser's answer**: a
        // name the adapter declined to give, alongside the four fields WebGPU
        // can never fill carrying real values. The decoder must not know that a
        // browser cannot fill them — it decodes what the wire says — and this is
        // what would catch a reader that hard-coded the absences instead.
        (
            2,
            Reply::Adapter {
                info: AdapterInfo {
                    id: AdapterId(0),
                    name: String::new(),
                    vendor_id: 0x1002,
                    device_id: 0x744C,
                    device_type: DeviceType::Discrete,
                    driver: "radv 25.1.4".into(),
                    backend: BackendKind::WebGpu,
                    caps: DeviceCaps {
                        features: Features::empty(),
                        limits: Limits::minimum(),
                    },
                },
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
        Reply::Adapter { info } => replies.adapter(sequence, info),
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
