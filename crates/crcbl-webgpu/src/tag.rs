//! The opcode table, the enum code tables, and the caps the reader enforces.
//!
//! Everything the two halves of the stream have to agree on byte-for-byte lives
//! here, so adding a command touches one file — which is the condition
//! `docs/plan/41-webgpu-stream.md` sets for leaving the numbers with the encoder
//! rather than in the document.
//!
//! # The tags are ours, not the compiler's
//!
//! None of the HAL enums carries `#[repr(u8)]` or explicit discriminants, so
//! `as u8` would encode *declaration order* — and [`Format`](crcbl_hal::Format)
//! is deliberately not `#[non_exhaustive]`, so a variant may be inserted in the
//! middle. That silently renumbers every code after the insertion point, and the
//! failure lands in a decoder on the other side of a language boundary where
//! nothing connects it back to the edit that caused it.
//!
//! So every code below is written out, and every encoder is an exhaustive
//! `match`: a variant added to a HAL enum stops *this* file compiling, which is
//! the moment the number beside it is impossible to miss.
//!
//! Bitflags are the exception, and only because they are not enums:
//! [`BufferUsage`](crcbl_hal::BufferUsage) and
//! [`ShaderStages`](crcbl_hal::ShaderStages) declare each bit as an explicit
//! `1 << n`, so `bits()` is already a chosen wire value rather than a position.

use crcbl_hal::{LoadOp, MemoryLocation, StoreOp};

// ── Header ────────────────────────────────────────────────────────────────────

/// Magic bytes at the head of every stream buffer.
///
/// The 8-byte ASCII magic plus a `u16` version is `crcbl-store`'s replay and
/// save header shape. It earns its place here for a reason those files do not
/// have: the Rust and JS halves ship as separate artifacts and are cached
/// independently, so a decoder meeting a stream from a different build is
/// reachable in a browser in a way it is not in a single binary.
pub const STREAM_MAGIC: &[u8; 8] = b"CRCBLGPU";

/// Current stream format version.
pub const STREAM_VERSION: u16 = 1;

/// Bytes before the first command: [`STREAM_MAGIC`], [`STREAM_VERSION`], and the
/// sequence number of the first command in the buffer.
pub const HEADER_BYTES: usize = 8 + 2 + 8;

// ── Caps ──────────────────────────────────────────────────────────────────────

/// Largest single length-prefixed byte field — a label, a push-constant block.
///
/// The buffer is process-internal, so this is not the network's number; the
/// defect it prevents is the same one either way, a corrupt length driving an
/// allocation the stream has no bytes for. The writer asserts against it too, so
/// nothing this crate encodes can be something it refuses to decode.
pub const MAX_FIELD_BYTES: usize = 1 << 20;

/// Largest element count in a length-prefixed array — dynamic offsets, colour
/// attachments. See [`MAX_FIELD_BYTES`] for why the cap exists.
pub const MAX_ELEMENT_COUNT: usize = 1 << 16;

// ── Command families ──────────────────────────────────────────────────────────
//
// Tags are grouped into contiguous ranges by family, as `crcbl-net`'s codec
// groups messages by direction. A corrupt tag then usually lands outside a
// family rather than inside a neighbouring command, which is the difference
// between an `UnknownTag` and a plausible-looking wrong decode.
//
// **The ranges are sized to the seam, not to a nibble.** An earlier draft of
// `docs/plan/41-webgpu-stream.md` gave each family one nibble, and that never
// fitted: `crcbl-hal`'s `Device` declares seventeen `create_*` methods and
// sixteen `destroy_*`, and `CommandEncoder`'s state commands come to sixteen
// again. Creation was over capacity before a single command was written. The
// sizes below leave every family room for the methods it must eventually carry.

/// First tag of the creation family — object creation, carrying the handle the
/// caller allocated for the object.
pub const FAMILY_CREATE: u8 = 0x00;
/// One past the creation family.
pub const FAMILY_CREATE_END: u8 = 0x20;

/// First tag of the destruction family.
pub const FAMILY_DESTROY: u8 = 0x20;
/// One past the destruction family.
pub const FAMILY_DESTROY_END: u8 = 0x40;

/// First tag of the encoder-state family: debug labels, passes, bindings, push
/// constants.
pub const FAMILY_ENCODER: u8 = 0x40;
/// One past the encoder-state family.
pub const FAMILY_ENCODER_END: u8 = 0x60;

/// First tag of the draw family.
pub const FAMILY_DRAW: u8 = 0x60;
/// One past the draw family.
pub const FAMILY_DRAW_END: u8 = 0x70;

/// First tag of the dispatch family.
pub const FAMILY_DISPATCH: u8 = 0x70;
/// One past the dispatch family.
pub const FAMILY_DISPATCH_END: u8 = 0x78;

/// First tag of the copy-and-fill family.
pub const FAMILY_COPY: u8 = 0x78;
/// One past the copy-and-fill family.
pub const FAMILY_COPY_END: u8 = 0x80;

/// First tag of the query family: query sets, timestamps, resolves.
pub const FAMILY_QUERY: u8 = 0x80;
/// One past the query family.
pub const FAMILY_QUERY_END: u8 = 0x88;

/// First tag of the presentation family: swapchain acquire and present.
pub const FAMILY_PRESENT: u8 = 0x88;
/// One past the presentation family.
pub const FAMILY_PRESENT_END: u8 = 0x90;

/// Every family, as `(first, end)` pairs in ascending order.
///
/// The table is what the tests walk, so a family added without a range — or one
/// that overlaps its neighbour — is caught here rather than by two decoders
/// quietly disagreeing.
pub const FAMILIES: [(u8, u8); 8] = [
    (FAMILY_CREATE, FAMILY_CREATE_END),
    (FAMILY_DESTROY, FAMILY_DESTROY_END),
    (FAMILY_ENCODER, FAMILY_ENCODER_END),
    (FAMILY_DRAW, FAMILY_DRAW_END),
    (FAMILY_DISPATCH, FAMILY_DISPATCH_END),
    (FAMILY_COPY, FAMILY_COPY_END),
    (FAMILY_QUERY, FAMILY_QUERY_END),
    (FAMILY_PRESENT, FAMILY_PRESENT_END),
];

/// One past the last claimed tag. Everything above is unassigned.
pub const FAMILIES_END: u8 = FAMILY_PRESENT_END;

// ── Command tags ──────────────────────────────────────────────────────────────
//
// A tag byte comes first so a decoder dispatches rather than trial-decodes: a
// trial decode makes "unknown command" indistinguishable from "malformed known
// command", and silently depends on no two decoders accepting the same bytes.

/// [`Command::CreateBuffer`](crate::Command::CreateBuffer).
pub const CREATE_BUFFER_TAG: u8 = 0x00;
/// [`Command::DestroyBuffer`](crate::Command::DestroyBuffer).
pub const DESTROY_BUFFER_TAG: u8 = 0x20;
/// [`Command::BeginDebugLabel`](crate::Command::BeginDebugLabel).
pub const BEGIN_DEBUG_LABEL_TAG: u8 = 0x40;
/// [`Command::BeginRenderPass`](crate::Command::BeginRenderPass).
pub const BEGIN_RENDER_PASS_TAG: u8 = 0x41;
/// [`Command::BindGraphicsPipeline`](crate::Command::BindGraphicsPipeline).
pub const BIND_GRAPHICS_PIPELINE_TAG: u8 = 0x42;
/// [`Command::BindGroup`](crate::Command::BindGroup).
pub const BIND_GROUP_TAG: u8 = 0x43;
/// [`Command::PushConstants`](crate::Command::PushConstants).
pub const PUSH_CONSTANTS_TAG: u8 = 0x44;
/// [`Command::Draw`](crate::Command::Draw).
pub const DRAW_TAG: u8 = 0x60;

// ── Optional fields ───────────────────────────────────────────────────────────
//
// An `Option<Handle>` needs none of this: `Handle::to_bits` is documented as
// never zero, so zero is `None` and no presence byte is written. Everything else
// optional gets one, which is the shape `crcbl-net`'s `Hello` uses for its
// optional resume token — and, like that one, a value other than these two is
// refused rather than treated as truthy.

/// Presence byte for an absent optional field.
pub const ABSENT: u8 = 0;
/// Presence byte for a present optional field.
pub const PRESENT: u8 = 1;

// ── LoadOp ────────────────────────────────────────────────────────────────────

/// [`LoadOp::Load`].
pub const LOAD_OP_LOAD: u8 = 0x00;
/// [`LoadOp::Clear`].
pub const LOAD_OP_CLEAR: u8 = 0x01;
/// [`LoadOp::DontCare`].
pub const LOAD_OP_DONT_CARE: u8 = 0x02;

/// The wire code for a [`LoadOp`].
#[must_use]
pub const fn load_op_code(op: LoadOp) -> u8 {
    match op {
        LoadOp::Load => LOAD_OP_LOAD,
        LoadOp::Clear => LOAD_OP_CLEAR,
        LoadOp::DontCare => LOAD_OP_DONT_CARE,
    }
}

/// The [`LoadOp`] a wire code names, or `None` if it names none.
#[must_use]
pub const fn load_op_from_code(code: u8) -> Option<LoadOp> {
    match code {
        LOAD_OP_LOAD => Some(LoadOp::Load),
        LOAD_OP_CLEAR => Some(LoadOp::Clear),
        LOAD_OP_DONT_CARE => Some(LoadOp::DontCare),
        _ => None,
    }
}

// ── StoreOp ───────────────────────────────────────────────────────────────────

/// [`StoreOp::Store`].
pub const STORE_OP_STORE: u8 = 0x00;
/// [`StoreOp::Discard`].
pub const STORE_OP_DISCARD: u8 = 0x01;

/// The wire code for a [`StoreOp`].
#[must_use]
pub const fn store_op_code(op: StoreOp) -> u8 {
    match op {
        StoreOp::Store => STORE_OP_STORE,
        StoreOp::Discard => STORE_OP_DISCARD,
    }
}

/// The [`StoreOp`] a wire code names, or `None` if it names none.
#[must_use]
pub const fn store_op_from_code(code: u8) -> Option<StoreOp> {
    match code {
        STORE_OP_STORE => Some(StoreOp::Store),
        STORE_OP_DISCARD => Some(StoreOp::Discard),
        _ => None,
    }
}

// ── MemoryLocation ────────────────────────────────────────────────────────────

/// [`MemoryLocation::DeviceLocal`].
pub const MEMORY_DEVICE_LOCAL: u8 = 0x00;
/// [`MemoryLocation::HostUpload`].
pub const MEMORY_HOST_UPLOAD: u8 = 0x01;
/// [`MemoryLocation::HostReadback`].
pub const MEMORY_HOST_READBACK: u8 = 0x02;

/// The wire code for a [`MemoryLocation`].
#[must_use]
pub const fn memory_location_code(memory: MemoryLocation) -> u8 {
    match memory {
        MemoryLocation::DeviceLocal => MEMORY_DEVICE_LOCAL,
        MemoryLocation::HostUpload => MEMORY_HOST_UPLOAD,
        MemoryLocation::HostReadback => MEMORY_HOST_READBACK,
    }
}

/// The [`MemoryLocation`] a wire code names, or `None` if it names none.
#[must_use]
pub const fn memory_location_from_code(code: u8) -> Option<MemoryLocation> {
    match code {
        MEMORY_DEVICE_LOCAL => Some(MemoryLocation::DeviceLocal),
        MEMORY_HOST_UPLOAD => Some(MemoryLocation::HostUpload),
        MEMORY_HOST_READBACK => Some(MemoryLocation::HostReadback),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tag this slice defines, with the family its name claims.
    ///
    /// Spelled out rather than derived from the constants: a table that builds
    /// each tag out of `FAMILY_X | n` cannot disagree with itself, so it would
    /// check nothing. This one can, and that is the point.
    const TAGS: [(&str, u8, u8); 8] = [
        ("CreateBuffer", CREATE_BUFFER_TAG, FAMILY_CREATE),
        ("DestroyBuffer", DESTROY_BUFFER_TAG, FAMILY_DESTROY),
        ("BeginDebugLabel", BEGIN_DEBUG_LABEL_TAG, FAMILY_ENCODER),
        ("BeginRenderPass", BEGIN_RENDER_PASS_TAG, FAMILY_ENCODER),
        (
            "BindGraphicsPipeline",
            BIND_GRAPHICS_PIPELINE_TAG,
            FAMILY_ENCODER,
        ),
        ("BindGroup", BIND_GROUP_TAG, FAMILY_ENCODER),
        ("PushConstants", PUSH_CONSTANTS_TAG, FAMILY_ENCODER),
        ("Draw", DRAW_TAG, FAMILY_DRAW),
    ];

    #[test]
    fn every_tag_is_unique_and_sits_in_the_family_its_name_claims() {
        let mut seen: Vec<u8> = TAGS.iter().map(|(_, tag, _)| *tag).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two commands share a tag");

        for (name, tag, family) in TAGS {
            let (first, end) = FAMILIES
                .into_iter()
                .find(|(first, _)| *first == family)
                .expect("every family in the table has a range");
            assert!(
                (first..end).contains(&tag),
                "{name} has tag {tag:#04x}, outside its family's {first:#04x}..{end:#04x}"
            );
        }
    }

    /// The ranges must tile without overlapping, and each must be big enough for
    /// the methods its family will eventually carry. The counts below are read
    /// off `crcbl-hal` and are the reason the families are not nibbles: `Device`
    /// declares seventeen `create_*` methods, which a nibble cannot hold.
    #[test]
    fn the_family_ranges_tile_and_hold_what_the_hal_will_put_in_them() {
        let mut previous_end = 0u8;
        for (first, end) in FAMILIES {
            assert_eq!(
                first, previous_end,
                "family {first:#04x} leaves a gap or overlaps its neighbour"
            );
            assert!(first < end, "family {first:#04x} is empty");
            previous_end = end;
        }
        assert_eq!(previous_end, FAMILIES_END);

        let room = |first: u8, end: u8| usize::from(end - first);
        assert!(
            room(FAMILY_CREATE, FAMILY_CREATE_END) >= 17,
            "create_* methods"
        );
        assert!(room(FAMILY_DESTROY, FAMILY_DESTROY_END) >= 16, "destroy_*");
        assert!(
            room(FAMILY_ENCODER, FAMILY_ENCODER_END) >= 16,
            "encoder state"
        );
        assert!(room(FAMILY_DRAW, FAMILY_DRAW_END) >= 8, "draw calls");
    }

    /// The `match`es these wrap are exhaustive, so a HAL variant added tomorrow
    /// fails to compile here. What that cannot catch is a code written twice, or
    /// a decoder that disagrees with its encoder.
    #[test]
    fn every_enum_code_is_distinct_and_decodes_back_to_what_encoded_it() {
        let load = [LoadOp::Load, LoadOp::Clear, LoadOp::DontCare];
        let store = [StoreOp::Store, StoreOp::Discard];
        let memory = [
            MemoryLocation::DeviceLocal,
            MemoryLocation::HostUpload,
            MemoryLocation::HostReadback,
        ];

        let codes: Vec<u8> = load.iter().map(|op| load_op_code(*op)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two LoadOps share a code");
        for op in load {
            assert_eq!(load_op_from_code(load_op_code(op)), Some(op));
        }

        let codes: Vec<u8> = store.iter().map(|op| store_op_code(*op)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two StoreOps share a code");
        for op in store {
            assert_eq!(store_op_from_code(store_op_code(op)), Some(op));
        }

        let codes: Vec<u8> = memory.iter().map(|m| memory_location_code(*m)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two MemoryLocations share a code"
        );
        for m in memory {
            assert_eq!(memory_location_from_code(memory_location_code(m)), Some(m));
        }
    }

    #[test]
    fn a_code_no_variant_claims_decodes_to_nothing() {
        assert_eq!(load_op_from_code(0xFF), None);
        assert_eq!(store_op_from_code(0xFF), None);
        assert_eq!(memory_location_from_code(0xFF), None);
    }

    fn distinct(codes: &[u8]) -> usize {
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    }
}
