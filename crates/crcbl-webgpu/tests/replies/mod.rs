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

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, DeviceCaps, DeviceType, Features, Format,
    Limits, PresentMode, SurfaceCaps,
};
use crcbl_webgpu::reply::{SurfaceCapsFailure, TRUNCATION_MARKER};
use crcbl_webgpu::{Reply, ReplyWriter, tag};

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
        // **The device's own capabilities, and deliberately not the adapter's.**
        // Every field below is a value `every_adapter_field` does not have —
        // fewer features, a smaller `max_image_2d`, no timestamp period — so a
        // replayer that answered a device request by copying its adapter record
        // would produce different bytes here rather than the same ones.
        (
            7,
            Reply::Device {
                caps: DeviceCaps {
                    features: DEVICE_GRANTED,
                    limits: Limits {
                        max_image_2d: 8192,
                        timestamp_period_ns: 0.0,
                        ..every_limit()
                    },
                },
            },
        ),
        // A device that opened with nothing beyond the floor: the other end of
        // the same shape, and the case where the feature word is empty rather
        // than merely small.
        (
            19,
            Reply::Device {
                caps: DeviceCaps {
                    features: Features::empty(),
                    limits: Limits::minimum(),
                },
            },
        ),
        // The refusal, with the gap that caused it. `TIMELINE_SEMAPHORE` is the
        // flag `DeviceDesc::for_adapter` requires and WebGPU has no answer for,
        // so it is the refusal a browser actually produces.
        (
            3,
            Reply::DeviceFailed {
                reason: "no WebGPU feature satisfies Features(TIMELINE_SEMAPHORE) — ✱".into(),
                unsupported: Features::TIMELINE_SEMAPHORE,
            },
        ),
        // …and a failure that is not about features at all: `requestDevice`
        // rejected, so the reason is the browser's and the word is empty. The
        // two halves of this reply are independent and this is what says so.
        (
            29,
            Reply::DeviceFailed {
                reason: String::new(),
                unsupported: Features::empty(),
            },
        ),
        // **The three lists are three different lengths**, which is the whole
        // job of this entry: a decoder that read one list's count and then
        // walked another's would still decode cleanly if they matched, and
        // three equal counts is exactly the shape a browser's answer has. The
        // present modes are all four, so the top of that table is pinned, and
        // the formats end on the highest code the table claims — the byte an
        // off-by-one at the end of `format_from_code` gets wrong first.
        //
        // **Not a browser's answer**, for the reason the second `Adapter` is
        // not one: the decoder decodes what the wire says.
        (
            37,
            Reply::SurfaceCaps {
                caps: SurfaceCaps {
                    formats: vec![
                        Format::Bgra8UnormSrgb,
                        Format::Rgba16Float,
                        Format::Bc7RgbaUnormSrgb,
                    ],
                    present_modes: vec![
                        PresentMode::Fifo,
                        PresentMode::FifoRelaxed,
                        PresentMode::Mailbox,
                        PresentMode::Immediate,
                    ],
                    composite_alpha: vec![CompositeAlpha::Opaque, CompositeAlpha::Inherit],
                    min_image_count: 2,
                    max_image_count: 8,
                    current_extent: Some((1920, 1080)),
                },
            },
        ),
        // What a WebGPU canvas actually reports, and the **absent** extent: a
        // browser has no `currentExtent` query at all, so `None` is not a
        // corner case here but the ordinary answer.
        (
            41,
            Reply::SurfaceCaps {
                caps: SurfaceCaps {
                    formats: vec![Format::Bgra8Unorm],
                    present_modes: vec![PresentMode::Fifo],
                    composite_alpha: vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
                    min_image_count: 2,
                    max_image_count: 2,
                    current_extent: None,
                },
            },
        ),
        // **A zero extent that is present**, and the reason absence is a
        // presence byte rather than a sentinel: an unconfigured or minimised
        // window has size `(0, 0)`, which the seam's own docs call a caller
        // problem rather than a missing answer. These bytes differ from the
        // entry above by more than their two zeroes — the presence byte itself
        // differs — so a writer on either side that collapsed the two would
        // produce a shorter buffer here rather than the same one.
        (
            43,
            Reply::SurfaceCaps {
                caps: SurfaceCaps {
                    formats: vec![Format::R8Unorm],
                    present_modes: vec![PresentMode::Fifo],
                    composite_alpha: vec![CompositeAlpha::PostMultiplied],
                    min_image_count: 0,
                    max_image_count: 0,
                    current_extent: Some((0, 0)),
                },
            },
        ),
        // **One entry per cause, and the query has one.** Every other consumer
        // of the failure-code table agrees with itself by construction —
        // `tag.rs`'s round trip reads its own constants, and the replayer's
        // checks read the same JavaScript table the writer does — so this corpus
        // is the only thing that pins the number *across* the two languages. A
        // `SURFACE_CAPS_FAILURE.BACKEND` still spelled `0x03` in `gpu-reply.js`,
        // as it was while the wire carried four causes, is a byte difference
        // here and nowhere else.
        //
        // The reason and the cause are independent halves, as
        // `Reply::DeviceFailed`'s are, so the second of these carries no reason
        // at all.
        (
            47,
            Reply::SurfaceCapsFailed {
                reason: "getPreferredCanvasFormat() answered \"rgba32float\" — ✱".into(),
                cause: SurfaceCapsFailure::Backend,
            },
        ),
        (
            53,
            Reply::SurfaceCapsFailed {
                reason: String::new(),
                cause: SurfaceCapsFailure::Backend,
            },
        ),
        // The out-of-band errors, and **two of them in one reply**, which is the
        // whole reason this reply carries a list: `take_error` hands back one
        // message per call, and a device that is failing produces them faster
        // than one round trip a frame can carry. A decoder that read the count
        // and then one message would leave the second where the next reply's tag
        // should be, which is what a two-element list catches and a one-element
        // list cannot.
        //
        // The wording is a real browser's, down to the nested "While validating"
        // context and the newline: this is the shape the cap and the truncation
        // below exist for, and a message with a `\n` and a `"` in it is the one a
        // writer that went through JSON would mangle.
        (
            59,
            Reply::DeviceErrors {
                messages: vec![
                    "Buffer usage (MapRead|Storage) is invalid.\n - While validating \
                     [Buffer \"hud ✱ staging\"]"
                        .into(),
                    "vkAllocateMemory failed with VK_ERROR_OUT_OF_DEVICE_MEMORY".into(),
                ],
            },
        ),
        // **The ordinary answer on a page that is working**, and the count's zero
        // boundary. It is not the absence of a reply: the command is answered
        // every time it is asked, because an unanswered sequence waits for ever.
        (
            61,
            Reply::DeviceErrors {
                messages: Vec::new(),
            },
        ),
        // **A message that did not fit, as the wire carries it** — the cut
        // prefix and the marker, not the original.
        //
        // This is the one entry in either corpus whose JavaScript counterpart is
        // handed something *different* to what is written here:
        // `reply-encode.mjs` gives its writer the whole untruncated message and
        // has to arrive at exactly these bytes. That is what pins the cut across
        // the two languages — the cap, the marker, and the walk back to a `char`
        // boundary, which here lands one byte short of the budget because every
        // character is three bytes wide. The Rust writer's own cut is checked in
        // `tests/reply.rs`, which is where the untruncated input lives on this
        // side.
        (
            67,
            Reply::DeviceErrors {
                messages: vec![truncated_message()],
            },
        ),
    ]
}

/// The character the truncation case is built from, three bytes wide in UTF-8 so
/// that no cut at a byte budget divisible by two or four lands on a boundary.
///
/// The message it fills is written by whoever needs one past the cap —
/// `tests/reply.rs` on this side, `reply-encode.mjs` on the other — because what
/// is shared between the two binaries here is the *expectation*, and an input
/// nothing in `fixture.rs` uses would be dead code in half of them.
pub const WIDE_CHAR: char = '✱';

/// What a message past [`tag::MAX_DEVICE_ERROR_BYTES`] made of [`WIDE_CHAR`]
/// becomes on the wire.
///
/// Spelled from the rule rather than from the writer: the budget is the cap less
/// the marker, and the last whole character that fits inside it is the last one
/// carried. A writer that cut at the budget itself would split the character
/// straddling it and produce a field that is not UTF-8; one that forgot the
/// marker would produce a longer prefix. Both are a byte difference here.
pub fn truncated_message() -> String {
    let budget = tag::MAX_DEVICE_ERROR_BYTES - TRUNCATION_MARKER.len();
    format!(
        "{}{TRUNCATION_MARKER}",
        WIDE_CHAR.to_string().repeat(budget / WIDE_CHAR.len_utf8())
    )
}

/// The features a device opened by `web/engine/gpu-replay.js` comes back with
/// when nothing optional was asked for: the four core WebGPU grants outright.
///
/// A **subset** of [`WEBGPU_REACHABLE`] and that is the point — the adapter can
/// have all eight, the device gets what the request named. `reply-encode.mjs`
/// derives the same word from the production mapping over a device stub with no
/// features at all.
const DEVICE_GRANTED: Features = Features::COMPUTE
    .union(Features::OCCLUSION_QUERY)
    .union(Features::DEPTH_BIAS_CLAMP)
    .union(Features::DEBUG_MARKERS);

/// Encodes `reply` through the writer method it came from.
///
/// Exhaustive, so a variant added to [`Reply`] stops this file compiling — which
/// is the point at which the suites that use it are impossible to leave
/// un-extended.
pub fn encode_reply(replies: &mut ReplyWriter, sequence: u64, reply: &Reply) {
    match reply {
        Reply::Adapter { info } => replies.adapter(sequence, info),
        Reply::NoAdapter { reason } => replies.no_adapter(sequence, reason),
        Reply::Device { caps } => replies.device(sequence, caps),
        Reply::DeviceFailed {
            reason,
            unsupported,
        } => replies.device_failed(sequence, reason, *unsupported),
        Reply::SurfaceCaps { caps } => replies.surface_caps(sequence, caps),
        Reply::SurfaceCapsFailed { reason, cause } => {
            replies.surface_caps_failed(sequence, reason, *cause);
        }
        Reply::DeviceErrors { messages } => {
            // The count it wrote is dropped here and asserted where it means
            // something: this corpus is under the cap by construction, so a
            // check of it would be a check on the corpus rather than on the
            // writer. `tests/reply.rs` is where a list past the cap is handed
            // over and the return value is what says how much of it crossed.
            let _ = replies.device_errors(sequence, messages);
        }
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
