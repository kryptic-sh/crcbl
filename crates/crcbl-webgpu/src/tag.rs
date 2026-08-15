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
//! `as u8` would encode *declaration order* — and [`Format`]
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

use crcbl_hal::{CompositeAlpha, DeviceType, Format, LoadOp, MemoryLocation, PresentMode, StoreOp};

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

/// Magic bytes at the head of every reply buffer.
///
/// A *different* magic from [`STREAM_MAGIC`], not the same one with a direction
/// flag inside: the two buffers travel opposite ways through the same shim, and
/// a channel wired backwards must fail on the first eight bytes rather than on
/// whichever tag happens to be unclaimed in the other table.
pub const REPLY_MAGIC: &[u8; 8] = b"CRCBLRPL";

/// Current reply format version. Versioned separately from
/// [`STREAM_VERSION`]: the two formats change for different reasons, and a
/// shared number would force one half to be re-blessed for the other's edit.
///
/// `2` since [`Reply::Adapter`](crate::Reply::Adapter) stopped being an id and a
/// name and became the whole of [`AdapterInfo`](crcbl_hal::AdapterInfo). That is
/// what the version word is *for*: the two halves ship as separate artifacts and
/// are cached independently, so a page holding yesterday's JavaScript against
/// today's wasm would otherwise read a name's length prefix as a vendor id.
///
/// # A new tag is not a new version, and the difference is the failure mode
///
/// Neither word moved when the device request and its two replies were added,
/// and that is deliberate. A **changed record** — the edit that took this word
/// to `2` — is invisible to a decoder: the bytes still parse and mean something
/// else, which is the defect only a version can catch. A **new tag** is not: an
/// older decoder meeting one answers [`DecodeError::UnknownTag`] naming the
/// byte, which says more than a header mismatch could and says it about the one
/// record that is new rather than about the whole buffer. Bumping would also
/// refuse every buffer from the older half, including the ones it still decodes
/// perfectly.
///
/// [`DecodeError::UnknownTag`]: crate::DecodeError::UnknownTag
pub const REPLY_VERSION: u16 = 2;

/// Bytes before the first reply: [`REPLY_MAGIC`] and [`REPLY_VERSION`].
///
/// Shorter than [`HEADER_BYTES`] by the base sequence, and deliberately: replies
/// need not arrive in order or at all, so each carries its own sequence and
/// there is no positional base to state. See [`REPLY_SEQUENCE_BYTES`].
pub const REPLY_HEADER_BYTES: usize = 8 + 2;

/// Bytes each reply spends on the sequence it answers, after its tag byte.
///
/// The command stream keeps its sequence numbers *off* the wire because they are
/// positional. Replies cannot: the JS side answers what it can, when it can, and
/// a reply's position in the buffer says nothing about which command it belongs
/// to. So the number is a field here — and a `u64` field, because it names a
/// counter that is 64-bit precisely so it never wraps within a session.
pub const REPLY_SEQUENCE_BYTES: usize = 8;

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

/// The most bytes wasm will let JS write into one reply buffer.
///
/// A bound rather than a limit anyone should reach, in the shape
/// `crcbl-store`'s `MAX_ASSET_BYTES` has — and it earns its place for a reason
/// the two caps above do not have: **this length comes from JS** and drives an
/// allocation wasm makes, so it needs a ceiling that is not "whatever the caller
/// said". Four times [`MAX_FIELD_BYTES`], so a frame carrying one maximal
/// payload still has room for the replies around it.
pub const MAX_REPLY_BYTES: usize = 4 * MAX_FIELD_BYTES;

/// The most sequences that may be waiting for a reply at once.
///
/// A reply that never arrives — JS dropped it, or the page lost its device —
/// leaves its sequence registered for ever, so the waiting set needs a bound for
/// the reason `crcbl-store`'s `MAX_QUEUED_REQUESTS` has one: past it, the engine
/// is told no rather than growing a set nothing is draining.
pub const MAX_WAITING_REPLIES: usize = 1024;

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

/// First tag of the instance family: the [`Instance`](crcbl_hal::Instance)
/// methods that are not creation or destruction — adapter enumeration, the
/// device request and its poll, surface capabilities.
///
/// Appended after the presentation family rather than placed first, where its
/// name would suggest it belongs: the tags above it are already committed to a
/// fixture and to a hand-written JavaScript decoder, and moving them would
/// renumber every one of them for a cosmetic ordering.
pub const FAMILY_INSTANCE: u8 = 0x90;
/// One past the instance family.
pub const FAMILY_INSTANCE_END: u8 = 0xA0;

/// Every family, as `(first, end)` pairs in ascending order.
///
/// The table is what the tests walk, so a family added without a range — or one
/// that overlaps its neighbour — is caught here rather than by two decoders
/// quietly disagreeing.
pub const FAMILIES: [(u8, u8); 9] = [
    (FAMILY_CREATE, FAMILY_CREATE_END),
    (FAMILY_DESTROY, FAMILY_DESTROY_END),
    (FAMILY_ENCODER, FAMILY_ENCODER_END),
    (FAMILY_DRAW, FAMILY_DRAW_END),
    (FAMILY_DISPATCH, FAMILY_DISPATCH_END),
    (FAMILY_COPY, FAMILY_COPY_END),
    (FAMILY_QUERY, FAMILY_QUERY_END),
    (FAMILY_PRESENT, FAMILY_PRESENT_END),
    (FAMILY_INSTANCE, FAMILY_INSTANCE_END),
];

/// One past the last claimed tag. Everything above is unassigned.
pub const FAMILIES_END: u8 = FAMILY_INSTANCE_END;

// ── Command tags ──────────────────────────────────────────────────────────────
//
// A tag byte comes first so a decoder dispatches rather than trial-decodes: a
// trial decode makes "unknown command" indistinguishable from "malformed known
// command", and silently depends on no two decoders accepting the same bytes.

/// [`Command::CreateBuffer`](crate::Command::CreateBuffer).
pub const CREATE_BUFFER_TAG: u8 = 0x00;
/// [`Command::CreateSurface`](crate::Command::CreateSurface).
pub const CREATE_SURFACE_TAG: u8 = 0x01;
/// [`Command::DestroyBuffer`](crate::Command::DestroyBuffer).
pub const DESTROY_BUFFER_TAG: u8 = 0x20;
/// [`Command::DestroySurface`](crate::Command::DestroySurface).
pub const DESTROY_SURFACE_TAG: u8 = 0x21;
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
/// [`Command::EnumerateAdapters`](crate::Command::EnumerateAdapters).
pub const ENUMERATE_ADAPTERS_TAG: u8 = 0x90;
/// [`Command::RequestDevice`](crate::Command::RequestDevice).
pub const REQUEST_DEVICE_TAG: u8 = 0x91;
/// [`Command::SurfaceCaps`](crate::Command::SurfaceCaps).
///
/// Written out rather than as `FAMILY_INSTANCE + 2`, which is the form this
/// constant took once and was reverted from: the table below walks every tag
/// asserting it falls inside its family's range, and a tag *derived* from the
/// family base cannot fail that assertion. The check would still run and would
/// no longer be able to fail.
pub const SURFACE_CAPS_TAG: u8 = 0x92;

// ── Reply families ────────────────────────────────────────────────────────────
//
// The reply table is its own space, not a continuation of the command one. The
// two never meet in a buffer — a reply buffer opens with `REPLY_MAGIC` and
// nothing else does — so overlapping numbers cannot be confused, and keeping
// them separate leaves each free to grow without the other's numbering moving.
//
// Grouped by the family of call being answered, for the command table's reason:
// a corrupt tag then usually lands outside a family rather than inside a
// neighbouring reply.

/// First tag of the instance family: adapter enumeration, device requests.
pub const REPLY_FAMILY_INSTANCE: u8 = 0x00;
/// One past the instance family.
pub const REPLY_FAMILY_INSTANCE_END: u8 = 0x10;

/// First tag of the readback family: [`Device::poll_readback`](crcbl_hal::Device::poll_readback).
pub const REPLY_FAMILY_READBACK: u8 = 0x10;
/// One past the readback family.
pub const REPLY_FAMILY_READBACK_END: u8 = 0x18;

/// First tag of the query family: [`Device::query_results`](crcbl_hal::Device::query_results).
pub const REPLY_FAMILY_QUERY: u8 = 0x18;
/// One past the query family.
pub const REPLY_FAMILY_QUERY_END: u8 = 0x20;

/// Every reply family, as `(first, end)` pairs in ascending order. Walked by the
/// same test that walks [`FAMILIES`].
pub const REPLY_FAMILIES: [(u8, u8); 3] = [
    (REPLY_FAMILY_INSTANCE, REPLY_FAMILY_INSTANCE_END),
    (REPLY_FAMILY_READBACK, REPLY_FAMILY_READBACK_END),
    (REPLY_FAMILY_QUERY, REPLY_FAMILY_QUERY_END),
];

/// One past the last claimed reply tag. Everything above is unassigned.
pub const REPLY_FAMILIES_END: u8 = REPLY_FAMILY_QUERY_END;

// ── Reply tags ────────────────────────────────────────────────────────────────
//
// **A partial set**, one per reply *shape* rather than one per HAL method that
// needs an answer — see `crate::reply` for what is missing and why that is a
// deliberate stopping point rather than an oversight.

/// [`Reply::Adapter`](crate::Reply::Adapter).
pub const ADAPTER_REPLY_TAG: u8 = 0x00;
/// [`Reply::NoAdapter`](crate::Reply::NoAdapter).
pub const NO_ADAPTER_REPLY_TAG: u8 = 0x01;
/// [`Reply::Device`](crate::Reply::Device).
pub const DEVICE_REPLY_TAG: u8 = 0x02;
/// [`Reply::DeviceFailed`](crate::Reply::DeviceFailed).
pub const DEVICE_FAILED_REPLY_TAG: u8 = 0x03;
/// [`Reply::SurfaceCaps`](crate::Reply::SurfaceCaps).
///
/// In the instance family because
/// [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps) is the call it
/// answers, not because a surface is involved: the reply table is grouped by
/// the family of *call*, exactly as the command table is.
pub const SURFACE_CAPS_REPLY_TAG: u8 = 0x04;
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending).
pub const READBACK_PENDING_REPLY_TAG: u8 = 0x10;
/// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady).
pub const READBACK_READY_REPLY_TAG: u8 = 0x11;
/// [`Reply::QueryResults`](crate::Reply::QueryResults).
pub const QUERY_RESULTS_REPLY_TAG: u8 = 0x18;

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

// ── DeviceType ────────────────────────────────────────────────────────────────
//
// Carried by [`Reply::Adapter`](crate::Reply::Adapter). **A browser never sends
// anything but [`DEVICE_TYPE_OTHER`]** — WebGPU does not say whether an adapter
// is discrete or integrated, and `crcbl-webgpu`'s replayer writes the value that
// means "the backend declined to say" rather than guessing. The other four codes
// exist because the field is a [`DeviceType`] and a wire form for it must be
// total; see [`crate::reply`] for the whole list of fields the browser cannot
// supply.

/// [`DeviceType::Cpu`].
pub const DEVICE_TYPE_CPU: u8 = 0x00;
/// [`DeviceType::Integrated`].
pub const DEVICE_TYPE_INTEGRATED: u8 = 0x01;
/// [`DeviceType::Discrete`].
pub const DEVICE_TYPE_DISCRETE: u8 = 0x02;
/// [`DeviceType::Virtual`].
pub const DEVICE_TYPE_VIRTUAL: u8 = 0x03;
/// [`DeviceType::Other`] — and the only one a browser ever produces.
pub const DEVICE_TYPE_OTHER: u8 = 0x04;

/// The wire code for a [`DeviceType`].
#[must_use]
pub const fn device_type_code(kind: DeviceType) -> u8 {
    match kind {
        DeviceType::Cpu => DEVICE_TYPE_CPU,
        DeviceType::Integrated => DEVICE_TYPE_INTEGRATED,
        DeviceType::Discrete => DEVICE_TYPE_DISCRETE,
        DeviceType::Virtual => DEVICE_TYPE_VIRTUAL,
        DeviceType::Other => DEVICE_TYPE_OTHER,
    }
}

/// The [`DeviceType`] a wire code names, or `None` if it names none.
#[must_use]
pub const fn device_type_from_code(code: u8) -> Option<DeviceType> {
    match code {
        DEVICE_TYPE_CPU => Some(DeviceType::Cpu),
        DEVICE_TYPE_INTEGRATED => Some(DeviceType::Integrated),
        DEVICE_TYPE_DISCRETE => Some(DeviceType::Discrete),
        DEVICE_TYPE_VIRTUAL => Some(DeviceType::Virtual),
        DEVICE_TYPE_OTHER => Some(DeviceType::Other),
        _ => None,
    }
}

// ── Format ────────────────────────────────────────────────────────────────────
//
// The largest code table on this seam, and the one this module's header is
// written about: [`Format`] is deliberately not `#[non_exhaustive]`, so a
// variant may be inserted *in the middle*, and `as u8` would renumber every code
// after the insertion point without anything failing to compile. So every code
// is written out and [`format_code`] is an exhaustive `match` — a variant added
// to `crcbl-hal` stops this file compiling, which is the moment the number
// beside it is impossible to miss.
//
// The order below is declaration order and the codes are contiguous, which is a
// convenience for a reader and nothing more: nothing derives one from the other,
// and a variant inserted in the middle tomorrow takes the *next free* code
// rather than displacing the ones below it.

/// [`Format::R8Unorm`].
pub const FORMAT_R8_UNORM: u8 = 0x00;
/// [`Format::Rg8Unorm`].
pub const FORMAT_RG8_UNORM: u8 = 0x01;
/// [`Format::Rgba8Unorm`].
pub const FORMAT_RGBA8_UNORM: u8 = 0x02;
/// [`Format::Rgba8UnormSrgb`].
pub const FORMAT_RGBA8_UNORM_SRGB: u8 = 0x03;
/// [`Format::Bgra8Unorm`].
pub const FORMAT_BGRA8_UNORM: u8 = 0x04;
/// [`Format::Bgra8UnormSrgb`].
pub const FORMAT_BGRA8_UNORM_SRGB: u8 = 0x05;
/// [`Format::Rgb10a2Unorm`].
pub const FORMAT_RGB10A2_UNORM: u8 = 0x06;
/// [`Format::R11g11b10Float`].
pub const FORMAT_R11G11B10_FLOAT: u8 = 0x07;
/// [`Format::R16Float`].
pub const FORMAT_R16_FLOAT: u8 = 0x08;
/// [`Format::Rg16Float`].
pub const FORMAT_RG16_FLOAT: u8 = 0x09;
/// [`Format::Rgba16Float`].
pub const FORMAT_RGBA16_FLOAT: u8 = 0x0A;
/// [`Format::R32Float`].
pub const FORMAT_R32_FLOAT: u8 = 0x0B;
/// [`Format::Rg32Float`].
pub const FORMAT_RG32_FLOAT: u8 = 0x0C;
/// [`Format::Rgba32Float`].
pub const FORMAT_RGBA32_FLOAT: u8 = 0x0D;
/// [`Format::R32Uint`].
pub const FORMAT_R32_UINT: u8 = 0x0E;
/// [`Format::Rg32Uint`].
pub const FORMAT_RG32_UINT: u8 = 0x0F;
/// [`Format::D32Float`].
pub const FORMAT_D32_FLOAT: u8 = 0x10;
/// [`Format::D32FloatS8Uint`].
pub const FORMAT_D32_FLOAT_S8_UINT: u8 = 0x11;
/// [`Format::D24UnormS8Uint`].
pub const FORMAT_D24_UNORM_S8_UINT: u8 = 0x12;
/// [`Format::D16Unorm`].
pub const FORMAT_D16_UNORM: u8 = 0x13;
/// [`Format::Bc1RgbaUnorm`].
pub const FORMAT_BC1_RGBA_UNORM: u8 = 0x14;
/// [`Format::Bc1RgbaUnormSrgb`].
pub const FORMAT_BC1_RGBA_UNORM_SRGB: u8 = 0x15;
/// [`Format::Bc3RgbaUnorm`].
pub const FORMAT_BC3_RGBA_UNORM: u8 = 0x16;
/// [`Format::Bc3RgbaUnormSrgb`].
pub const FORMAT_BC3_RGBA_UNORM_SRGB: u8 = 0x17;
/// [`Format::Bc4RUnorm`].
pub const FORMAT_BC4_R_UNORM: u8 = 0x18;
/// [`Format::Bc5RgUnorm`].
pub const FORMAT_BC5_RG_UNORM: u8 = 0x19;
/// [`Format::Bc6hRgbUfloat`].
pub const FORMAT_BC6H_RGB_UFLOAT: u8 = 0x1A;
/// [`Format::Bc7RgbaUnorm`].
pub const FORMAT_BC7_RGBA_UNORM: u8 = 0x1B;
/// [`Format::Bc7RgbaUnormSrgb`].
pub const FORMAT_BC7_RGBA_UNORM_SRGB: u8 = 0x1C;

/// The wire code for a [`Format`].
#[must_use]
pub const fn format_code(format: Format) -> u8 {
    match format {
        Format::R8Unorm => FORMAT_R8_UNORM,
        Format::Rg8Unorm => FORMAT_RG8_UNORM,
        Format::Rgba8Unorm => FORMAT_RGBA8_UNORM,
        Format::Rgba8UnormSrgb => FORMAT_RGBA8_UNORM_SRGB,
        Format::Bgra8Unorm => FORMAT_BGRA8_UNORM,
        Format::Bgra8UnormSrgb => FORMAT_BGRA8_UNORM_SRGB,
        Format::Rgb10a2Unorm => FORMAT_RGB10A2_UNORM,
        Format::R11g11b10Float => FORMAT_R11G11B10_FLOAT,
        Format::R16Float => FORMAT_R16_FLOAT,
        Format::Rg16Float => FORMAT_RG16_FLOAT,
        Format::Rgba16Float => FORMAT_RGBA16_FLOAT,
        Format::R32Float => FORMAT_R32_FLOAT,
        Format::Rg32Float => FORMAT_RG32_FLOAT,
        Format::Rgba32Float => FORMAT_RGBA32_FLOAT,
        Format::R32Uint => FORMAT_R32_UINT,
        Format::Rg32Uint => FORMAT_RG32_UINT,
        Format::D32Float => FORMAT_D32_FLOAT,
        Format::D32FloatS8Uint => FORMAT_D32_FLOAT_S8_UINT,
        Format::D24UnormS8Uint => FORMAT_D24_UNORM_S8_UINT,
        Format::D16Unorm => FORMAT_D16_UNORM,
        Format::Bc1RgbaUnorm => FORMAT_BC1_RGBA_UNORM,
        Format::Bc1RgbaUnormSrgb => FORMAT_BC1_RGBA_UNORM_SRGB,
        Format::Bc3RgbaUnorm => FORMAT_BC3_RGBA_UNORM,
        Format::Bc3RgbaUnormSrgb => FORMAT_BC3_RGBA_UNORM_SRGB,
        Format::Bc4RUnorm => FORMAT_BC4_R_UNORM,
        Format::Bc5RgUnorm => FORMAT_BC5_RG_UNORM,
        Format::Bc6hRgbUfloat => FORMAT_BC6H_RGB_UFLOAT,
        Format::Bc7RgbaUnorm => FORMAT_BC7_RGBA_UNORM,
        Format::Bc7RgbaUnormSrgb => FORMAT_BC7_RGBA_UNORM_SRGB,
    }
}

/// The [`Format`] a wire code names, or `None` if it names none.
///
/// **`None` rather than a nearby variant**, and that is the whole reason this
/// table exists: a code this build does not claim comes from a build that knows
/// a format this one does not, and answering with its neighbour would report a
/// surface as offering `Bgra8Unorm` where the other half said `Bgra8UnormSrgb` —
/// a colour-space bug three layers down from anything that could name it.
#[must_use]
pub const fn format_from_code(code: u8) -> Option<Format> {
    match code {
        FORMAT_R8_UNORM => Some(Format::R8Unorm),
        FORMAT_RG8_UNORM => Some(Format::Rg8Unorm),
        FORMAT_RGBA8_UNORM => Some(Format::Rgba8Unorm),
        FORMAT_RGBA8_UNORM_SRGB => Some(Format::Rgba8UnormSrgb),
        FORMAT_BGRA8_UNORM => Some(Format::Bgra8Unorm),
        FORMAT_BGRA8_UNORM_SRGB => Some(Format::Bgra8UnormSrgb),
        FORMAT_RGB10A2_UNORM => Some(Format::Rgb10a2Unorm),
        FORMAT_R11G11B10_FLOAT => Some(Format::R11g11b10Float),
        FORMAT_R16_FLOAT => Some(Format::R16Float),
        FORMAT_RG16_FLOAT => Some(Format::Rg16Float),
        FORMAT_RGBA16_FLOAT => Some(Format::Rgba16Float),
        FORMAT_R32_FLOAT => Some(Format::R32Float),
        FORMAT_RG32_FLOAT => Some(Format::Rg32Float),
        FORMAT_RGBA32_FLOAT => Some(Format::Rgba32Float),
        FORMAT_R32_UINT => Some(Format::R32Uint),
        FORMAT_RG32_UINT => Some(Format::Rg32Uint),
        FORMAT_D32_FLOAT => Some(Format::D32Float),
        FORMAT_D32_FLOAT_S8_UINT => Some(Format::D32FloatS8Uint),
        FORMAT_D24_UNORM_S8_UINT => Some(Format::D24UnormS8Uint),
        FORMAT_D16_UNORM => Some(Format::D16Unorm),
        FORMAT_BC1_RGBA_UNORM => Some(Format::Bc1RgbaUnorm),
        FORMAT_BC1_RGBA_UNORM_SRGB => Some(Format::Bc1RgbaUnormSrgb),
        FORMAT_BC3_RGBA_UNORM => Some(Format::Bc3RgbaUnorm),
        FORMAT_BC3_RGBA_UNORM_SRGB => Some(Format::Bc3RgbaUnormSrgb),
        FORMAT_BC4_R_UNORM => Some(Format::Bc4RUnorm),
        FORMAT_BC5_RG_UNORM => Some(Format::Bc5RgUnorm),
        FORMAT_BC6H_RGB_UFLOAT => Some(Format::Bc6hRgbUfloat),
        FORMAT_BC7_RGBA_UNORM => Some(Format::Bc7RgbaUnorm),
        FORMAT_BC7_RGBA_UNORM_SRGB => Some(Format::Bc7RgbaUnormSrgb),
        _ => None,
    }
}

// ── PresentMode ───────────────────────────────────────────────────────────────
//
// **A browser only ever offers [`PRESENT_MODE_FIFO`]** — WebGPU has no present
// mode at all and its canvas presents at the `requestAnimationFrame` boundary,
// which is what `Fifo` describes. The other three codes exist because the field
// is a [`PresentMode`] and a wire form for it must be total: a surface reply
// written by something that *can* offer them decodes as offering them.

/// [`PresentMode::Fifo`] — and the only one a browser ever produces.
pub const PRESENT_MODE_FIFO: u8 = 0x00;
/// [`PresentMode::FifoRelaxed`].
pub const PRESENT_MODE_FIFO_RELAXED: u8 = 0x01;
/// [`PresentMode::Mailbox`].
pub const PRESENT_MODE_MAILBOX: u8 = 0x02;
/// [`PresentMode::Immediate`].
pub const PRESENT_MODE_IMMEDIATE: u8 = 0x03;

/// The wire code for a [`PresentMode`].
#[must_use]
pub const fn present_mode_code(mode: PresentMode) -> u8 {
    match mode {
        PresentMode::Fifo => PRESENT_MODE_FIFO,
        PresentMode::FifoRelaxed => PRESENT_MODE_FIFO_RELAXED,
        PresentMode::Mailbox => PRESENT_MODE_MAILBOX,
        PresentMode::Immediate => PRESENT_MODE_IMMEDIATE,
    }
}

/// The [`PresentMode`] a wire code names, or `None` if it names none.
///
/// **Never [`PresentMode::Fifo`] as a fallback**, tempting though it is: `Fifo`
/// is the mode every surface is promised to have, so a drifted table answering
/// with it would be indistinguishable from a surface that genuinely offers only
/// that — which is what a browser reports.
#[must_use]
pub const fn present_mode_from_code(code: u8) -> Option<PresentMode> {
    match code {
        PRESENT_MODE_FIFO => Some(PresentMode::Fifo),
        PRESENT_MODE_FIFO_RELAXED => Some(PresentMode::FifoRelaxed),
        PRESENT_MODE_MAILBOX => Some(PresentMode::Mailbox),
        PRESENT_MODE_IMMEDIATE => Some(PresentMode::Immediate),
        _ => None,
    }
}

// ── CompositeAlpha ────────────────────────────────────────────────────────────

/// [`CompositeAlpha::Opaque`].
pub const COMPOSITE_ALPHA_OPAQUE: u8 = 0x00;
/// [`CompositeAlpha::PreMultiplied`].
pub const COMPOSITE_ALPHA_PRE_MULTIPLIED: u8 = 0x01;
/// [`CompositeAlpha::PostMultiplied`].
pub const COMPOSITE_ALPHA_POST_MULTIPLIED: u8 = 0x02;
/// [`CompositeAlpha::Inherit`].
pub const COMPOSITE_ALPHA_INHERIT: u8 = 0x03;

/// The wire code for a [`CompositeAlpha`].
#[must_use]
pub const fn composite_alpha_code(alpha: CompositeAlpha) -> u8 {
    match alpha {
        CompositeAlpha::Opaque => COMPOSITE_ALPHA_OPAQUE,
        CompositeAlpha::PreMultiplied => COMPOSITE_ALPHA_PRE_MULTIPLIED,
        CompositeAlpha::PostMultiplied => COMPOSITE_ALPHA_POST_MULTIPLIED,
        CompositeAlpha::Inherit => COMPOSITE_ALPHA_INHERIT,
    }
}

/// The [`CompositeAlpha`] a wire code names, or `None` if it names none.
///
/// The two multiplied modes are adjacent codes and mean opposite things about
/// the colour channels, so folding an unknown code into either is a surface that
/// composites wrongly and never says why.
#[must_use]
pub const fn composite_alpha_from_code(code: u8) -> Option<CompositeAlpha> {
    match code {
        COMPOSITE_ALPHA_OPAQUE => Some(CompositeAlpha::Opaque),
        COMPOSITE_ALPHA_PRE_MULTIPLIED => Some(CompositeAlpha::PreMultiplied),
        COMPOSITE_ALPHA_POST_MULTIPLIED => Some(CompositeAlpha::PostMultiplied),
        COMPOSITE_ALPHA_INHERIT => Some(CompositeAlpha::Inherit),
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
    const TAGS: [(&str, u8, u8); 13] = [
        ("CreateBuffer", CREATE_BUFFER_TAG, FAMILY_CREATE),
        ("CreateSurface", CREATE_SURFACE_TAG, FAMILY_CREATE),
        ("DestroyBuffer", DESTROY_BUFFER_TAG, FAMILY_DESTROY),
        ("DestroySurface", DESTROY_SURFACE_TAG, FAMILY_DESTROY),
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
        ("EnumerateAdapters", ENUMERATE_ADAPTERS_TAG, FAMILY_INSTANCE),
        ("RequestDevice", REQUEST_DEVICE_TAG, FAMILY_INSTANCE),
        ("SurfaceCaps", SURFACE_CAPS_TAG, FAMILY_INSTANCE),
    ];

    /// Every reply tag this slice defines, with the family its name claims.
    /// Spelled out for the reason [`TAGS`] is.
    const REPLY_TAGS: [(&str, u8, u8); 8] = [
        ("Adapter", ADAPTER_REPLY_TAG, REPLY_FAMILY_INSTANCE),
        ("NoAdapter", NO_ADAPTER_REPLY_TAG, REPLY_FAMILY_INSTANCE),
        ("Device", DEVICE_REPLY_TAG, REPLY_FAMILY_INSTANCE),
        (
            "DeviceFailed",
            DEVICE_FAILED_REPLY_TAG,
            REPLY_FAMILY_INSTANCE,
        ),
        ("SurfaceCaps", SURFACE_CAPS_REPLY_TAG, REPLY_FAMILY_INSTANCE),
        (
            "ReadbackPending",
            READBACK_PENDING_REPLY_TAG,
            REPLY_FAMILY_READBACK,
        ),
        (
            "ReadbackReady",
            READBACK_READY_REPLY_TAG,
            REPLY_FAMILY_READBACK,
        ),
        ("QueryResults", QUERY_RESULTS_REPLY_TAG, REPLY_FAMILY_QUERY),
    ];

    /// Asserts every tag in `tags` is distinct and inside the range `families`
    /// gives the family its row names.
    fn every_tag_sits_in_its_family(tags: &[(&str, u8, u8)], families: &[(u8, u8)]) {
        let mut seen: Vec<u8> = tags.iter().map(|(_, tag, _)| *tag).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two rows share a tag");

        for (name, tag, family) in tags {
            let (first, end) = families
                .iter()
                .find(|(first, _)| first == family)
                .expect("every family in the table has a range");
            assert!(
                (*first..*end).contains(tag),
                "{name} has tag {tag:#04x}, outside its family's {first:#04x}..{end:#04x}"
            );
        }
    }

    #[test]
    fn every_tag_is_unique_and_sits_in_the_family_its_name_claims() {
        every_tag_sits_in_its_family(&TAGS, &FAMILIES);
    }

    /// The reply table is a separate space with the same discipline, so it gets
    /// the same walk. The two tables deliberately reuse numbers — `0x00` is both
    /// `CreateBuffer` and `Adapter` — which is safe only because a reply buffer
    /// opens with [`REPLY_MAGIC`] and a command buffer never does.
    #[test]
    fn every_reply_tag_is_unique_and_sits_in_the_family_its_name_claims() {
        every_tag_sits_in_its_family(&REPLY_TAGS, &REPLY_FAMILIES);
        assert_ne!(
            STREAM_MAGIC, REPLY_MAGIC,
            "the two tag spaces overlap, so the magics are what keeps them apart"
        );
    }

    /// The ranges must tile without overlapping, and each must be big enough for
    /// the methods its family will eventually carry. The counts below are read
    /// off `crcbl-hal` and are the reason the families are not nibbles:
    /// `Device`'s `create_*` methods alone outnumber what a nibble holds, and
    /// `Instance`'s `create_surface` lands in the same family on top of them.
    #[test]
    fn the_family_ranges_tile_and_hold_what_the_hal_will_put_in_them() {
        let tile = |families: &[(u8, u8)], end_of_all: u8| {
            let mut previous_end = 0u8;
            for (first, end) in families {
                assert_eq!(
                    *first, previous_end,
                    "family {first:#04x} leaves a gap or overlaps its neighbour"
                );
                assert!(first < end, "family {first:#04x} is empty");
                previous_end = *end;
            }
            assert_eq!(previous_end, end_of_all);
        };
        tile(&FAMILIES, FAMILIES_END);
        tile(&REPLY_FAMILIES, REPLY_FAMILIES_END);

        let room = |first: u8, end: u8| usize::from(end - first);
        assert!(
            room(FAMILY_CREATE, FAMILY_CREATE_END) >= 18,
            "create_* methods"
        );
        assert!(room(FAMILY_DESTROY, FAMILY_DESTROY_END) >= 17, "destroy_*");
        assert!(
            room(FAMILY_ENCODER, FAMILY_ENCODER_END) >= 16,
            "encoder state"
        );
        assert!(room(FAMILY_DRAW, FAMILY_DRAW_END) >= 8, "draw calls");
        // `Instance` declares seven methods; `create_surface` and
        // `destroy_surface` belong to the two families above and `create_device`
        // is provided in terms of `request_device`, which leaves `backend`,
        // `adapters`, `surface_caps` and `request_device` — and
        // `PendingDevice::poll`, which is an instance-level call in everything
        // but its receiver.
        assert!(
            room(FAMILY_INSTANCE, FAMILY_INSTANCE_END) >= 5,
            "instance-level calls"
        );
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

        let device_type = [
            DeviceType::Cpu,
            DeviceType::Integrated,
            DeviceType::Discrete,
            DeviceType::Virtual,
            DeviceType::Other,
        ];
        let codes: Vec<u8> = device_type.iter().map(|k| device_type_code(*k)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two DeviceTypes share a code"
        );
        for kind in device_type {
            assert_eq!(device_type_from_code(device_type_code(kind)), Some(kind));
        }

        // Driven off `Format::ALL` rather than a list written out again here:
        // `crcbl-hal` keeps that list because its own backends need it, and a
        // second copy in this crate would be a second place to forget a variant.
        // What forces a *code* to exist is `format_code`'s exhaustive `match`;
        // what this checks is that no two of them collide and that the reverse
        // table agrees.
        let codes: Vec<u8> = Format::ALL.iter().map(|f| format_code(*f)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two Formats share a code");
        for format in Format::ALL {
            assert_eq!(format_from_code(format_code(*format)), Some(*format));
        }

        let present = [
            PresentMode::Fifo,
            PresentMode::FifoRelaxed,
            PresentMode::Mailbox,
            PresentMode::Immediate,
        ];
        let codes: Vec<u8> = present.iter().map(|m| present_mode_code(*m)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two PresentModes share a code"
        );
        for mode in present {
            assert_eq!(present_mode_from_code(present_mode_code(mode)), Some(mode));
        }

        let alpha = [
            CompositeAlpha::Opaque,
            CompositeAlpha::PreMultiplied,
            CompositeAlpha::PostMultiplied,
            CompositeAlpha::Inherit,
        ];
        let codes: Vec<u8> = alpha.iter().map(|a| composite_alpha_code(*a)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two CompositeAlphas share a code"
        );
        for mode in alpha {
            assert_eq!(
                composite_alpha_from_code(composite_alpha_code(mode)),
                Some(mode)
            );
        }
    }

    #[test]
    fn a_code_no_variant_claims_decodes_to_nothing() {
        assert_eq!(load_op_from_code(0xFF), None);
        assert_eq!(store_op_from_code(0xFF), None);
        assert_eq!(memory_location_from_code(0xFF), None);
        assert_eq!(device_type_from_code(0xFF), None);
        assert_eq!(format_from_code(0xFF), None);
        assert_eq!(present_mode_from_code(0xFF), None);
        assert_eq!(composite_alpha_from_code(0xFF), None);
        // The one directly above the last claimed code, which is where an
        // off-by-one in either table lands and where `0xFF` never would.
        assert_eq!(device_type_from_code(DEVICE_TYPE_OTHER + 1), None);
        assert_eq!(format_from_code(FORMAT_BC7_RGBA_UNORM_SRGB + 1), None);
        assert_eq!(present_mode_from_code(PRESENT_MODE_IMMEDIATE + 1), None);
        assert_eq!(composite_alpha_from_code(COMPOSITE_ALPHA_INHERIT + 1), None);
    }

    fn distinct(codes: &[u8]) -> usize {
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    }
}
