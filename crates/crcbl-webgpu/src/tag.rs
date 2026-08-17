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
//! [`BufferUsage`](crcbl_hal::BufferUsage), [`ShaderStages`](crcbl_hal::ShaderStages),
//! [`ImageUsage`](crcbl_hal::ImageUsage) and [`ImageAspect`](crcbl_hal::ImageAspect)
//! declare each bit as an explicit `1 << n`, so `bits()` is already a chosen wire
//! value rather than a position. They cross as `bits()` and come back through
//! `from_bits` rather than `from_bits_truncate`, so a bit no flag claims is an
//! error instead of a silently dropped one.

use crcbl_hal::{
    BindingKind, BindingResource, BlendFactor, BlendOp, CompareOp, CompositeAlpha, CullMode,
    DeviceType, FilterMode, Format, FrontFace, ImageType, ImageViewType, IndexFormat, LoadOp,
    MemoryLocation, PolygonMode, PresentMode, PrimitiveTopology, ResourceState, SampleType,
    SamplerAddressMode, StencilOp, StoreOp,
};

use crate::reply::SurfaceCapsFailure;

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

/// First tag of the device family: the [`Device`](crcbl_hal::Device) methods
/// that make no object and release none — queue submission and the readback
/// poll. Named for the type that declares them, exactly as [`FAMILY_INSTANCE`]
/// is: creation and destruction are grouped by *shape* and go wherever a
/// caller-allocated handle or a released id belongs, and what is left over from
/// each of the two big HAL types earns a family named for the type.
///
/// [`request_readback`](crcbl_hal::Device::request_readback) is deliberately
/// **not** here: it allocates a [`ReadbackHandle`](crcbl_hal::ReadbackHandle)
/// from a [`ReadbackDesc`](crcbl_hal::ReadbackDesc) and so is an ordinary
/// creation ([`REQUEST_READBACK_TAG`]), with
/// [`destroy_readback`](crcbl_hal::Device::destroy_readback) its release
/// ([`DESTROY_READBACK_TAG`]) — the same CREATE/DESTROY split every resource
/// takes. `submit` and `poll_readback` have no descriptor and no handle of their
/// own, which is what leaves them here.
///
/// Appended after the instance family for that family's own reason: the tags
/// below it are committed to a fixture and a hand-written JavaScript decoder,
/// and moving them would renumber every one for a cosmetic ordering.
pub const FAMILY_DEVICE: u8 = 0xA0;
/// One past the device family.
pub const FAMILY_DEVICE_END: u8 = 0xB0;

/// Every family, as `(first, end)` pairs in ascending order.
///
/// The table is what the tests walk, so a family added without a range — or one
/// that overlaps its neighbour — is caught here rather than by two decoders
/// quietly disagreeing.
pub const FAMILIES: [(u8, u8); 10] = [
    (FAMILY_CREATE, FAMILY_CREATE_END),
    (FAMILY_DESTROY, FAMILY_DESTROY_END),
    (FAMILY_ENCODER, FAMILY_ENCODER_END),
    (FAMILY_DRAW, FAMILY_DRAW_END),
    (FAMILY_DISPATCH, FAMILY_DISPATCH_END),
    (FAMILY_COPY, FAMILY_COPY_END),
    (FAMILY_QUERY, FAMILY_QUERY_END),
    (FAMILY_PRESENT, FAMILY_PRESENT_END),
    (FAMILY_INSTANCE, FAMILY_INSTANCE_END),
    (FAMILY_DEVICE, FAMILY_DEVICE_END),
];

/// One past the last claimed tag. Everything above is unassigned.
pub const FAMILIES_END: u8 = FAMILY_DEVICE_END;

// ── Command tags ──────────────────────────────────────────────────────────────
//
// A tag byte comes first so a decoder dispatches rather than trial-decodes: a
// trial decode makes "unknown command" indistinguishable from "malformed known
// command", and silently depends on no two decoders accepting the same bytes.

/// [`Command::CreateBuffer`](crate::Command::CreateBuffer).
pub const CREATE_BUFFER_TAG: u8 = 0x00;
/// [`Command::CreateSurface`](crate::Command::CreateSurface).
pub const CREATE_SURFACE_TAG: u8 = 0x01;
/// [`Command::CreateImage`](crate::Command::CreateImage).
pub const CREATE_IMAGE_TAG: u8 = 0x02;
/// [`Command::CreateImageView`](crate::Command::CreateImageView).
pub const CREATE_IMAGE_VIEW_TAG: u8 = 0x03;
/// [`Command::CreateSampler`](crate::Command::CreateSampler).
///
/// Written out rather than as `FAMILY_CREATE + 4`, for the reason
/// [`SURFACE_CAPS_TAG`] spells out: a tag derived from its family's base cannot
/// fail the range walk below, so the check would still run and would no longer
/// be able to fail.
pub const CREATE_SAMPLER_TAG: u8 = 0x04;
/// [`Command::CreateBindGroupLayout`](crate::Command::CreateBindGroupLayout).
///
/// Written out rather than as `FAMILY_CREATE + 5`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const CREATE_BIND_GROUP_LAYOUT_TAG: u8 = 0x05;
/// [`Command::CreateBindGroup`](crate::Command::CreateBindGroup).
///
/// Written out rather than as `FAMILY_CREATE + 6`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const CREATE_BIND_GROUP_TAG: u8 = 0x06;
/// [`Command::CreateShaderModule`](crate::Command::CreateShaderModule).
///
/// Written out rather than as `FAMILY_CREATE + 7`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const CREATE_SHADER_MODULE_TAG: u8 = 0x07;
/// [`Command::CreatePipelineLayout`](crate::Command::CreatePipelineLayout).
///
/// Written out rather than as `FAMILY_CREATE + 8`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const CREATE_PIPELINE_LAYOUT_TAG: u8 = 0x08;
/// [`Command::CreateComputePipeline`](crate::Command::CreateComputePipeline).
///
/// Written out rather than as `FAMILY_CREATE + 9`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const CREATE_COMPUTE_PIPELINE_TAG: u8 = 0x09;
/// [`Command::CreateGraphicsPipeline`](crate::Command::CreateGraphicsPipeline).
///
/// Written out rather than as `FAMILY_CREATE + 10`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const CREATE_GRAPHICS_PIPELINE_TAG: u8 = 0x0A;
/// [`Command::RequestReadback`](crate::Command::RequestReadback).
///
/// In the creation family, not the device one:
/// [`request_readback`](crcbl_hal::Device::request_readback) allocates a
/// [`ReadbackHandle`](crcbl_hal::ReadbackHandle) the caller writes into the
/// stream, exactly as every `create_*` does. See [`FAMILY_DEVICE`] for the
/// split. Written out rather than as `FAMILY_CREATE + 11`, for
/// [`CREATE_SAMPLER_TAG`]'s reason.
pub const REQUEST_READBACK_TAG: u8 = 0x0B;
/// [`Command::CreateOffscreenSurface`](crate::Command::CreateOffscreenSurface).
///
/// The offscreen counterpart of [`CREATE_SURFACE_TAG`], and in the creation
/// family beside it because it too carries the handle the caller allocated for a
/// surface. Written out rather than as `FAMILY_CREATE + 12`, for
/// [`CREATE_SAMPLER_TAG`]'s reason. It carries *only* that handle: unlike
/// [`CREATE_SURFACE_TAG`] there is no canvas key to resolve, and unlike a canvas
/// there is no extent or format to carry either — a surface is created before
/// any device exists and before any swapchain is described, so the ring's size
/// and format arrive later, with [`CREATE_SWAPCHAIN_TAG`]. See
/// [`Command::CreateOffscreenSurface`](crate::Command::CreateOffscreenSurface).
pub const CREATE_OFFSCREEN_SURFACE_TAG: u8 = 0x0C;
/// [`Command::DestroyBuffer`](crate::Command::DestroyBuffer).
pub const DESTROY_BUFFER_TAG: u8 = 0x20;
/// [`Command::DestroySurface`](crate::Command::DestroySurface).
pub const DESTROY_SURFACE_TAG: u8 = 0x21;
/// [`Command::DestroyImage`](crate::Command::DestroyImage).
pub const DESTROY_IMAGE_TAG: u8 = 0x22;
/// [`Command::DestroyImageView`](crate::Command::DestroyImageView).
pub const DESTROY_IMAGE_VIEW_TAG: u8 = 0x23;
/// [`Command::DestroySampler`](crate::Command::DestroySampler).
///
/// Written out rather than as `FAMILY_DESTROY + 4`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_SAMPLER_TAG: u8 = 0x24;
/// [`Command::DestroyBindGroupLayout`](crate::Command::DestroyBindGroupLayout).
///
/// Written out rather than as `FAMILY_DESTROY + 5`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_BIND_GROUP_LAYOUT_TAG: u8 = 0x25;
/// [`Command::DestroyBindGroup`](crate::Command::DestroyBindGroup).
///
/// Written out rather than as `FAMILY_DESTROY + 6`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_BIND_GROUP_TAG: u8 = 0x26;
/// [`Command::DestroyShaderModule`](crate::Command::DestroyShaderModule).
///
/// Written out rather than as `FAMILY_DESTROY + 7`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_SHADER_MODULE_TAG: u8 = 0x27;
/// [`Command::DestroyPipelineLayout`](crate::Command::DestroyPipelineLayout).
///
/// Written out rather than as `FAMILY_DESTROY + 8`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_PIPELINE_LAYOUT_TAG: u8 = 0x28;
/// [`Command::DestroyComputePipeline`](crate::Command::DestroyComputePipeline).
///
/// Written out rather than as `FAMILY_DESTROY + 9`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_COMPUTE_PIPELINE_TAG: u8 = 0x29;
/// [`Command::DestroyGraphicsPipeline`](crate::Command::DestroyGraphicsPipeline).
///
/// Written out rather than as `FAMILY_DESTROY + 10`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_GRAPHICS_PIPELINE_TAG: u8 = 0x2A;
/// [`Command::DestroyCommandBuffer`](crate::Command::DestroyCommandBuffer).
///
/// Written out rather than as `FAMILY_DESTROY + 11`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_COMMAND_BUFFER_TAG: u8 = 0x2B;
/// [`Command::DestroyReadback`](crate::Command::DestroyReadback).
///
/// Written out rather than as `FAMILY_DESTROY + 12`, for [`CREATE_SAMPLER_TAG`]'s
/// reason.
pub const DESTROY_READBACK_TAG: u8 = 0x2C;
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
/// [`Command::CreateCommandEncoder`](crate::Command::CreateCommandEncoder).
///
/// In the encoder family rather than the creation one: it names no handle — the
/// encoder it opens is the replayer's implicit-current one, exactly as
/// `crcbl-hal`'s recording methods assume — so there is nothing for a caller to
/// allocate. Its terminal op [`Finish`](crate::Command::Finish) is what yields a
/// [`CommandBufferHandle`](crcbl_hal::CommandBufferHandle), and it too is an
/// encoder state transition rather than a descriptor-driven creation.
pub const CREATE_COMMAND_ENCODER_TAG: u8 = 0x45;
/// [`Command::EndRenderPass`](crate::Command::EndRenderPass).
pub const END_RENDER_PASS_TAG: u8 = 0x46;
/// [`Command::Finish`](crate::Command::Finish) — the implicit-current encoder's
/// terminal op, which produces the [`CommandBufferHandle`](crcbl_hal::CommandBufferHandle)
/// the caller allocated. In the encoder family for
/// [`CREATE_COMMAND_ENCODER_TAG`]'s reason.
pub const FINISH_TAG: u8 = 0x47;
/// [`Command::BeginComputePass`](crate::Command::BeginComputePass).
pub const BEGIN_COMPUTE_PASS_TAG: u8 = 0x48;
/// [`Command::EndComputePass`](crate::Command::EndComputePass).
pub const END_COMPUTE_PASS_TAG: u8 = 0x49;
/// [`Command::BindComputePipeline`](crate::Command::BindComputePipeline).
pub const BIND_COMPUTE_PIPELINE_TAG: u8 = 0x4A;
/// [`Command::PipelineBarrier`](crate::Command::PipelineBarrier) — the
/// documented no-op. It carries the barrier lists for wire fidelity, but WebGPU
/// tracks resource state itself so the replayer records nothing.
pub const PIPELINE_BARRIER_TAG: u8 = 0x4B;
/// [`Command::SetViewport`](crate::Command::SetViewport) — dynamic viewport
/// state on the open render pass, which becomes `setViewport(x, y, w, h,
/// minDepth, maxDepth)`.
pub const SET_VIEWPORT_TAG: u8 = 0x4C;
/// [`Command::SetScissor`](crate::Command::SetScissor) — dynamic scissor state
/// on the open render pass, which becomes `setScissorRect(x, y, w, h)`.
pub const SET_SCISSOR_TAG: u8 = 0x4D;
/// [`Command::BindIndexBuffer`](crate::Command::BindIndexBuffer) — binds the
/// index buffer for subsequent indexed draws, which becomes
/// `setIndexBuffer(buffer, format, offset)`.
pub const BIND_INDEX_BUFFER_TAG: u8 = 0x4E;
/// [`Command::Draw`](crate::Command::Draw).
pub const DRAW_TAG: u8 = 0x60;
/// [`Command::DrawIndexed`](crate::Command::DrawIndexed) — an indexed draw,
/// which becomes `drawIndexed(indexCount, instanceCount, firstIndex, baseVertex,
/// firstInstance)`.
pub const DRAW_INDEXED_TAG: u8 = 0x61;
/// [`Command::DrawIndirect`](crate::Command::DrawIndirect) — an indirect
/// non-indexed draw with a CPU-known count. WebGPU has only a single-draw
/// `drawIndirect(indirectBuffer, indirectOffset)`, so the replayer unrolls a
/// `draw_count > 1` into that many calls, one per argument structure the
/// `stride` steps over.
pub const DRAW_INDIRECT_TAG: u8 = 0x62;
/// [`Command::DrawIndexedIndirect`](crate::Command::DrawIndexedIndirect) — an
/// indirect indexed draw with a CPU-known count,
/// [`GeometryPath::IndirectPerBatch`](crcbl_hal::GeometryPath::IndirectPerBatch)'s
/// draw path. WebGPU has only a single-draw `drawIndexedIndirect(indirectBuffer,
/// indirectOffset)`, so the replayer unrolls a `draw_count > 1` into that many
/// calls, one per argument structure the `stride` steps over.
pub const DRAW_INDEXED_INDIRECT_TAG: u8 = 0x63;
/// [`Command::Dispatch`](crate::Command::Dispatch) — the first tag of the
/// dispatch family, [`FAMILY_DISPATCH`].
pub const DISPATCH_TAG: u8 = 0x70;
/// [`Command::CopyImageToBuffer`](crate::Command::CopyImageToBuffer) — the
/// readback path's image→buffer copy.
pub const COPY_IMAGE_TO_BUFFER_TAG: u8 = 0x78;
/// [`Command::CopyBufferToBuffer`](crate::Command::CopyBufferToBuffer) — the
/// buffer→buffer copy that carries a dispatch's storage-buffer output back to a
/// host-readable buffer.
pub const COPY_BUFFER_TO_BUFFER_TAG: u8 = 0x79;
/// [`Command::CopyBufferToImage`](crate::Command::CopyBufferToImage) — the
/// buffer→image copy, the upload counterpart of [`COPY_IMAGE_TO_BUFFER_TAG`].
pub const COPY_BUFFER_TO_IMAGE_TAG: u8 = 0x7A;
/// [`Command::CopyImageToImage`](crate::Command::CopyImageToImage) — the
/// image→image copy.
pub const COPY_IMAGE_TO_IMAGE_TAG: u8 = 0x7B;
/// [`Command::FillBuffer`](crate::Command::FillBuffer) — a buffer fill, which
/// WebGPU can perform only for the value zero (`clearBuffer`); any other value
/// is refused by the replayer.
pub const FILL_BUFFER_TAG: u8 = 0x7C;
/// [`Command::WriteBuffer`](crate::Command::WriteBuffer) — a host→buffer upload,
/// which becomes WebGPU's `queue.writeBuffer(buffer, offset, data)`.
///
/// In the copy-and-fill family rather than the device one: it is a queue-side
/// data transfer, the upload counterpart of the copies above, even though the
/// HAL declares it on [`Device`](crcbl_hal::Device) rather than on the encoder.
/// It makes no object and is not recorded on the implicit-current encoder — the
/// replayer submits it straight to the queue.
pub const WRITE_BUFFER_TAG: u8 = 0x7D;
/// [`Command::CreateSwapchain`](crate::Command::CreateSwapchain) — the first tag
/// of the presentation family, [`FAMILY_PRESENT`]. It configures a canvas's
/// context; see the command for why `image_count` and `present_mode` are carried
/// and dropped by the replayer.
pub const CREATE_SWAPCHAIN_TAG: u8 = 0x88;
/// [`Command::AcquireNextFrame`](crate::Command::AcquireNextFrame) — binds the
/// frame `getCurrentTexture()` hands back under the caller-allocated image and
/// view handles.
pub const ACQUIRE_NEXT_FRAME_TAG: u8 = 0x89;
/// [`Command::Present`](crate::Command::Present) — the documented no-op. The
/// browser composites the configured canvas on its own `requestAnimationFrame`;
/// a non-empty `waits` is refused because WebGPU has no semaphores.
pub const PRESENT_TAG: u8 = 0x8A;
/// [`Command::DestroySwapchain`](crate::Command::DestroySwapchain) — unconfigures
/// the canvas context and releases the handle.
pub const DESTROY_SWAPCHAIN_TAG: u8 = 0x8B;
/// [`Command::ReconfigureSwapchain`](crate::Command::ReconfigureSwapchain) — the
/// same descriptor [`CREATE_SWAPCHAIN_TAG`] carries, naming an already-configured
/// swapchain to re-`configure` in place. The handle stays valid across it.
pub const RECONFIGURE_SWAPCHAIN_TAG: u8 = 0x8C;
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
/// [`Command::Submit`](crate::Command::Submit) — the first command of the device
/// family; see [`FAMILY_DEVICE`] for why submission and the readback poll sit
/// together and apart from the create/destroy pairs.
pub const SUBMIT_TAG: u8 = 0xA0;
/// [`Command::PollReadback`](crate::Command::PollReadback) — the one device-family
/// command that is *answered*, by a [`Reply::ReadbackReady`](crate::Reply::ReadbackReady)
/// or [`Reply::ReadbackPending`](crate::Reply::ReadbackPending) naming its sequence.
pub const POLL_READBACK_TAG: u8 = 0xA1;

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
/// [`Reply::SurfaceCapsFailed`](crate::Reply::SurfaceCapsFailed).
///
/// A *new tag* rather than a new [`REPLY_VERSION`], which is the distinction
/// that constant's own docs draw: an older decoder meeting this byte answers
/// [`DecodeError::UnknownTag`](crate::DecodeError::UnknownTag) naming it, while
/// every reply it does know still decodes.
pub const SURFACE_CAPS_FAILED_REPLY_TAG: u8 = 0x05;
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

// ── ResourceState ─────────────────────────────────────────────────────────────
//
// The barrier `from`/`to` states. Codes `0x00..=0x0C` follow the enum's
// declaration order. The whole vocabulary crosses even though the replayer never
// acts on it — a [`Command::PipelineBarrier`](crate::Command::PipelineBarrier) is
// a no-op on WebGPU — so a state folded into a neighbour here would be a wire
// infidelity nothing downstream could catch.

/// [`ResourceState::Undefined`].
pub const RESOURCE_STATE_UNDEFINED: u8 = 0x00;
/// [`ResourceState::ShaderRead`].
pub const RESOURCE_STATE_SHADER_READ: u8 = 0x01;
/// [`ResourceState::ShaderWrite`].
pub const RESOURCE_STATE_SHADER_WRITE: u8 = 0x02;
/// [`ResourceState::ShaderReadWrite`].
pub const RESOURCE_STATE_SHADER_READ_WRITE: u8 = 0x03;
/// [`ResourceState::ColorAttachment`].
pub const RESOURCE_STATE_COLOR_ATTACHMENT: u8 = 0x04;
/// [`ResourceState::DepthStencilWrite`].
pub const RESOURCE_STATE_DEPTH_STENCIL_WRITE: u8 = 0x05;
/// [`ResourceState::DepthStencilRead`].
pub const RESOURCE_STATE_DEPTH_STENCIL_READ: u8 = 0x06;
/// [`ResourceState::TransferSrc`].
pub const RESOURCE_STATE_TRANSFER_SRC: u8 = 0x07;
/// [`ResourceState::TransferDst`].
pub const RESOURCE_STATE_TRANSFER_DST: u8 = 0x08;
/// [`ResourceState::IndirectArgument`].
pub const RESOURCE_STATE_INDIRECT_ARGUMENT: u8 = 0x09;
/// [`ResourceState::IndexBuffer`].
pub const RESOURCE_STATE_INDEX_BUFFER: u8 = 0x0A;
/// [`ResourceState::HostRead`].
pub const RESOURCE_STATE_HOST_READ: u8 = 0x0B;
/// [`ResourceState::Present`].
pub const RESOURCE_STATE_PRESENT: u8 = 0x0C;

/// The wire code for a [`ResourceState`].
#[must_use]
pub const fn resource_state_code(state: ResourceState) -> u8 {
    match state {
        ResourceState::Undefined => RESOURCE_STATE_UNDEFINED,
        ResourceState::ShaderRead => RESOURCE_STATE_SHADER_READ,
        ResourceState::ShaderWrite => RESOURCE_STATE_SHADER_WRITE,
        ResourceState::ShaderReadWrite => RESOURCE_STATE_SHADER_READ_WRITE,
        ResourceState::ColorAttachment => RESOURCE_STATE_COLOR_ATTACHMENT,
        ResourceState::DepthStencilWrite => RESOURCE_STATE_DEPTH_STENCIL_WRITE,
        ResourceState::DepthStencilRead => RESOURCE_STATE_DEPTH_STENCIL_READ,
        ResourceState::TransferSrc => RESOURCE_STATE_TRANSFER_SRC,
        ResourceState::TransferDst => RESOURCE_STATE_TRANSFER_DST,
        ResourceState::IndirectArgument => RESOURCE_STATE_INDIRECT_ARGUMENT,
        ResourceState::IndexBuffer => RESOURCE_STATE_INDEX_BUFFER,
        ResourceState::HostRead => RESOURCE_STATE_HOST_READ,
        ResourceState::Present => RESOURCE_STATE_PRESENT,
    }
}

/// The [`ResourceState`] a wire code names, or `None` if it names none.
#[must_use]
pub const fn resource_state_from_code(code: u8) -> Option<ResourceState> {
    match code {
        RESOURCE_STATE_UNDEFINED => Some(ResourceState::Undefined),
        RESOURCE_STATE_SHADER_READ => Some(ResourceState::ShaderRead),
        RESOURCE_STATE_SHADER_WRITE => Some(ResourceState::ShaderWrite),
        RESOURCE_STATE_SHADER_READ_WRITE => Some(ResourceState::ShaderReadWrite),
        RESOURCE_STATE_COLOR_ATTACHMENT => Some(ResourceState::ColorAttachment),
        RESOURCE_STATE_DEPTH_STENCIL_WRITE => Some(ResourceState::DepthStencilWrite),
        RESOURCE_STATE_DEPTH_STENCIL_READ => Some(ResourceState::DepthStencilRead),
        RESOURCE_STATE_TRANSFER_SRC => Some(ResourceState::TransferSrc),
        RESOURCE_STATE_TRANSFER_DST => Some(ResourceState::TransferDst),
        RESOURCE_STATE_INDIRECT_ARGUMENT => Some(ResourceState::IndirectArgument),
        RESOURCE_STATE_INDEX_BUFFER => Some(ResourceState::IndexBuffer),
        RESOURCE_STATE_HOST_READ => Some(ResourceState::HostRead),
        RESOURCE_STATE_PRESENT => Some(ResourceState::Present),
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

// ── IndexFormat ───────────────────────────────────────────────────────────────
//
// Carried by [`Command::BindIndexBuffer`](crate::Command::BindIndexBuffer) and
// written out for the module header's reason. The two variants are the same
// bandwidth trade Vulkan and WebGPU both draw, and a fold between them is not a
// refusal but a misread index buffer — every index read at twice or half its
// stride — so this table answers `None` for a code it does not claim.

/// [`IndexFormat::Uint16`].
pub const INDEX_FORMAT_UINT16: u8 = 0x00;
/// [`IndexFormat::Uint32`].
pub const INDEX_FORMAT_UINT32: u8 = 0x01;

/// The wire code for an [`IndexFormat`].
#[must_use]
pub const fn index_format_code(format: IndexFormat) -> u8 {
    match format {
        IndexFormat::Uint16 => INDEX_FORMAT_UINT16,
        IndexFormat::Uint32 => INDEX_FORMAT_UINT32,
    }
}

/// The [`IndexFormat`] a wire code names, or `None` if it names none.
#[must_use]
pub const fn index_format_from_code(code: u8) -> Option<IndexFormat> {
    match code {
        INDEX_FORMAT_UINT16 => Some(IndexFormat::Uint16),
        INDEX_FORMAT_UINT32 => Some(IndexFormat::Uint32),
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

// ── ImageType ─────────────────────────────────────────────────────────────────

/// [`ImageType::D1`].
pub const IMAGE_TYPE_D1: u8 = 0x00;
/// [`ImageType::D2`].
pub const IMAGE_TYPE_D2: u8 = 0x01;
/// [`ImageType::D3`].
pub const IMAGE_TYPE_D3: u8 = 0x02;

/// The wire code for an [`ImageType`].
#[must_use]
pub const fn image_type_code(image_type: ImageType) -> u8 {
    match image_type {
        ImageType::D1 => IMAGE_TYPE_D1,
        ImageType::D2 => IMAGE_TYPE_D2,
        ImageType::D3 => IMAGE_TYPE_D3,
    }
}

/// The [`ImageType`] a wire code names, or `None` if it names none.
///
/// **Never a neighbouring dimensionality**, and this is the table where a
/// neighbour costs the most: [`Extent3d::depth_or_layers`](crcbl_hal::Extent3d::depth_or_layers)
/// is *the depth* for [`ImageType::D3`] and *the array-layer count* for every
/// other variant, so the same three numbers describe two different images
/// depending only on this byte. Folding [`IMAGE_TYPE_D3`] into
/// [`IMAGE_TYPE_D2`] turns a 64-deep volume into 64 flat slices — and
/// [`Extent3d::full_mip_levels`](crcbl_hal::Extent3d::full_mip_levels)'s own
/// docs give that pair its mip counts, seven against three. Nothing further
/// down can tell them apart, because the bytes are identical.
#[must_use]
pub const fn image_type_from_code(code: u8) -> Option<ImageType> {
    match code {
        IMAGE_TYPE_D1 => Some(ImageType::D1),
        IMAGE_TYPE_D2 => Some(ImageType::D2),
        IMAGE_TYPE_D3 => Some(ImageType::D3),
        _ => None,
    }
}

// ── ImageViewType ─────────────────────────────────────────────────────────────
//
// A separate table from [`ImageType`] and not a superset of it, because the HAL
// keeps them separate: a view reinterprets its image's dimensionality, so the
// two fields of a create pair legitimately disagree. Sharing one table would
// make `D2Array`, `Cube` and `CubeArray` spellable where an image's own
// dimensionality is asked for.

/// [`ImageViewType::D1`].
pub const IMAGE_VIEW_TYPE_D1: u8 = 0x00;
/// [`ImageViewType::D2`].
pub const IMAGE_VIEW_TYPE_D2: u8 = 0x01;
/// [`ImageViewType::D2Array`].
pub const IMAGE_VIEW_TYPE_D2_ARRAY: u8 = 0x02;
/// [`ImageViewType::Cube`].
pub const IMAGE_VIEW_TYPE_CUBE: u8 = 0x03;
/// [`ImageViewType::CubeArray`].
pub const IMAGE_VIEW_TYPE_CUBE_ARRAY: u8 = 0x04;
/// [`ImageViewType::D3`].
pub const IMAGE_VIEW_TYPE_D3: u8 = 0x05;

/// The wire code for an [`ImageViewType`].
#[must_use]
pub const fn image_view_type_code(view_type: ImageViewType) -> u8 {
    match view_type {
        ImageViewType::D1 => IMAGE_VIEW_TYPE_D1,
        ImageViewType::D2 => IMAGE_VIEW_TYPE_D2,
        ImageViewType::D2Array => IMAGE_VIEW_TYPE_D2_ARRAY,
        ImageViewType::Cube => IMAGE_VIEW_TYPE_CUBE,
        ImageViewType::CubeArray => IMAGE_VIEW_TYPE_CUBE_ARRAY,
        ImageViewType::D3 => IMAGE_VIEW_TYPE_D3,
    }
}

/// The [`ImageViewType`] a wire code names, or `None` if it names none.
///
/// **Never a neighbouring dimensionality**, and here the neighbours are the two
/// pairs that read the *same* layer range two ways:
/// [`IMAGE_VIEW_TYPE_D2`]/[`IMAGE_VIEW_TYPE_D2_ARRAY`] and
/// [`IMAGE_VIEW_TYPE_CUBE`]/[`IMAGE_VIEW_TYPE_CUBE_ARRAY`] are adjacent codes,
/// and each member of a pair accepts exactly the
/// [`base_layer`](crcbl_hal::ImageSubresourceRange::base_layer) and
/// [`layer_count`](crcbl_hal::ImageSubresourceRange::layer_count) the other
/// does. So a code folded into its neighbour builds a view rather than refusing
/// one: a cube map's six faces become six unrelated slices, or a shadow
/// atlas's cascades become one cube. The subresource range cannot contradict
/// it, because this byte is the only field that says how those layers are to be
/// read.
#[must_use]
pub const fn image_view_type_from_code(code: u8) -> Option<ImageViewType> {
    match code {
        IMAGE_VIEW_TYPE_D1 => Some(ImageViewType::D1),
        IMAGE_VIEW_TYPE_D2 => Some(ImageViewType::D2),
        IMAGE_VIEW_TYPE_D2_ARRAY => Some(ImageViewType::D2Array),
        IMAGE_VIEW_TYPE_CUBE => Some(ImageViewType::Cube),
        IMAGE_VIEW_TYPE_CUBE_ARRAY => Some(ImageViewType::CubeArray),
        IMAGE_VIEW_TYPE_D3 => Some(ImageViewType::D3),
        _ => None,
    }
}

// ── FilterMode ────────────────────────────────────────────────────────────────
//
// Two variants, and a [`SamplerDesc`](crcbl_hal::SamplerDesc) carries **three**
// of them back to back — `mag_filter`, `min_filter`, `mip_filter`. That is what
// makes this small table worth writing out: no code here can be folded into a
// neighbour without landing on the other variant, so the failure a wrong table
// produces is not a refusal but a sampler that filters correctly in one
// direction and not the other.

/// [`FilterMode::Nearest`].
pub const FILTER_MODE_NEAREST: u8 = 0x00;
/// [`FilterMode::Linear`].
pub const FILTER_MODE_LINEAR: u8 = 0x01;

/// The wire code for a [`FilterMode`].
#[must_use]
pub const fn filter_mode_code(mode: FilterMode) -> u8 {
    match mode {
        FilterMode::Nearest => FILTER_MODE_NEAREST,
        FilterMode::Linear => FILTER_MODE_LINEAR,
    }
}

/// The [`FilterMode`] a wire code names, or `None` if it names none.
///
/// **`None` rather than [`FilterMode::Linear`] as a default**, tempting though
/// it is for a two-variant enum where one variant is what every sampler in the
/// engine actually wants: a code this build does not claim comes from a build
/// that knows a filter this one does not, and answering with `Linear` would turn
/// it into trilinear filtering nobody asked for — visible only as softness, and
/// attributable to nothing.
#[must_use]
pub const fn filter_mode_from_code(code: u8) -> Option<FilterMode> {
    match code {
        FILTER_MODE_NEAREST => Some(FilterMode::Nearest),
        FILTER_MODE_LINEAR => Some(FilterMode::Linear),
        _ => None,
    }
}

// ── SamplerAddressMode ────────────────────────────────────────────────────────
//
// Also three in a row on the wire — `address_mode` is a `[SamplerAddressMode; 3]`
// for U, V and W — so this table has [`FilterMode`]'s hazard with four variants
// instead of two.

/// [`SamplerAddressMode::Repeat`].
pub const SAMPLER_ADDRESS_REPEAT: u8 = 0x00;
/// [`SamplerAddressMode::MirrorRepeat`].
pub const SAMPLER_ADDRESS_MIRROR_REPEAT: u8 = 0x01;
/// [`SamplerAddressMode::ClampToEdge`].
pub const SAMPLER_ADDRESS_CLAMP_TO_EDGE: u8 = 0x02;
/// [`SamplerAddressMode::ClampToBorder`].
pub const SAMPLER_ADDRESS_CLAMP_TO_BORDER: u8 = 0x03;

/// The wire code for a [`SamplerAddressMode`].
#[must_use]
pub const fn sampler_address_mode_code(mode: SamplerAddressMode) -> u8 {
    match mode {
        SamplerAddressMode::Repeat => SAMPLER_ADDRESS_REPEAT,
        SamplerAddressMode::MirrorRepeat => SAMPLER_ADDRESS_MIRROR_REPEAT,
        SamplerAddressMode::ClampToEdge => SAMPLER_ADDRESS_CLAMP_TO_EDGE,
        SamplerAddressMode::ClampToBorder => SAMPLER_ADDRESS_CLAMP_TO_BORDER,
    }
}

/// The [`SamplerAddressMode`] a wire code names, or `None` if it names none.
///
/// **The adjacent pair that costs the most is
/// [`SAMPLER_ADDRESS_CLAMP_TO_EDGE`] and [`SAMPLER_ADDRESS_CLAMP_TO_BORDER`]**,
/// because they agree everywhere a test would look and differ exactly at the
/// edge texel: one repeats the border texel outwards and the other fetches
/// transparent black. An atlas sampled with the wrong one of the two bleeds its
/// neighbour's colour into every seam, and there is no error anywhere — the
/// sampler is valid, the draw succeeds, and the difference is a fringe a person
/// has to see. The two also part company at the *backend*: WebGPU has no border
/// colour at all, so one of them is a refusal there and the other is not.
#[must_use]
pub const fn sampler_address_mode_from_code(code: u8) -> Option<SamplerAddressMode> {
    match code {
        SAMPLER_ADDRESS_REPEAT => Some(SamplerAddressMode::Repeat),
        SAMPLER_ADDRESS_MIRROR_REPEAT => Some(SamplerAddressMode::MirrorRepeat),
        SAMPLER_ADDRESS_CLAMP_TO_EDGE => Some(SamplerAddressMode::ClampToEdge),
        SAMPLER_ADDRESS_CLAMP_TO_BORDER => Some(SamplerAddressMode::ClampToBorder),
        _ => None,
    }
}

// ── CompareOp ─────────────────────────────────────────────────────────────────
//
// Carried by [`SamplerDesc::compare`](crcbl_hal::SamplerDesc::compare) — the
// first optional *enum* on this stream, so it rides a presence byte and this
// table is read only when that byte says [`PRESENT`].

/// [`CompareOp::Never`].
pub const COMPARE_OP_NEVER: u8 = 0x00;
/// [`CompareOp::Less`].
pub const COMPARE_OP_LESS: u8 = 0x01;
/// [`CompareOp::Equal`].
pub const COMPARE_OP_EQUAL: u8 = 0x02;
/// [`CompareOp::LessOrEqual`].
pub const COMPARE_OP_LESS_OR_EQUAL: u8 = 0x03;
/// [`CompareOp::Greater`] — the engine's depth test under reversed-Z.
pub const COMPARE_OP_GREATER: u8 = 0x04;
/// [`CompareOp::NotEqual`].
pub const COMPARE_OP_NOT_EQUAL: u8 = 0x05;
/// [`CompareOp::GreaterOrEqual`].
pub const COMPARE_OP_GREATER_OR_EQUAL: u8 = 0x06;
/// [`CompareOp::Always`].
pub const COMPARE_OP_ALWAYS: u8 = 0x07;

/// The wire code for a [`CompareOp`].
#[must_use]
pub const fn compare_op_code(op: CompareOp) -> u8 {
    match op {
        CompareOp::Never => COMPARE_OP_NEVER,
        CompareOp::Less => COMPARE_OP_LESS,
        CompareOp::Equal => COMPARE_OP_EQUAL,
        CompareOp::LessOrEqual => COMPARE_OP_LESS_OR_EQUAL,
        CompareOp::Greater => COMPARE_OP_GREATER,
        CompareOp::NotEqual => COMPARE_OP_NOT_EQUAL,
        CompareOp::GreaterOrEqual => COMPARE_OP_GREATER_OR_EQUAL,
        CompareOp::Always => COMPARE_OP_ALWAYS,
    }
}

/// The [`CompareOp`] a wire code names, or `None` if it names none.
///
/// **[`COMPARE_OP_GREATER`] and [`COMPARE_OP_LESS`] are the pair a fold must
/// never make**, and they are the most dangerous pair in this module.
/// [`CompareOp`]'s own docs say why: the name is the comparison performed, never
/// what it means for visibility, and under this engine's reversed-Z it is
/// `Greater` that asks "is the fragment closer than the stored caster?" A
/// hardware-PCF shadow sampler that got `Less` instead is not a sampler that
/// fails — it is one that lights exactly the surfaces that should be in shadow
/// and shadows the rest, on every frame, with nothing anywhere reporting an
/// error. Nothing downstream can tell the two apart either, because a
/// comparison sampler reports nothing about the comparison it was built with —
/// this table answering `None` for a code it does not claim is the only place
/// the difference is visible at all.
#[must_use]
pub const fn compare_op_from_code(code: u8) -> Option<CompareOp> {
    match code {
        COMPARE_OP_NEVER => Some(CompareOp::Never),
        COMPARE_OP_LESS => Some(CompareOp::Less),
        COMPARE_OP_EQUAL => Some(CompareOp::Equal),
        COMPARE_OP_LESS_OR_EQUAL => Some(CompareOp::LessOrEqual),
        COMPARE_OP_GREATER => Some(CompareOp::Greater),
        COMPARE_OP_NOT_EQUAL => Some(CompareOp::NotEqual),
        COMPARE_OP_GREATER_OR_EQUAL => Some(CompareOp::GreaterOrEqual),
        COMPARE_OP_ALWAYS => Some(CompareOp::Always),
        _ => None,
    }
}

// ── PrimitiveTopology ─────────────────────────────────────────────────────────
//
// The first of the graphics-pipeline enums, all of them reached only through
// [`Command::CreateGraphicsPipeline`](crate::Command::CreateGraphicsPipeline)'s
// [`PrimitiveState`](crcbl_hal::PrimitiveState). Written out for the module
// header's reason — declaration order is not a wire contract — and read on the
// far side into `GPUPrimitiveState.topology`.

/// [`PrimitiveTopology::PointList`].
pub const PRIMITIVE_TOPOLOGY_POINT_LIST: u8 = 0x00;
/// [`PrimitiveTopology::LineList`].
pub const PRIMITIVE_TOPOLOGY_LINE_LIST: u8 = 0x01;
/// [`PrimitiveTopology::LineStrip`].
pub const PRIMITIVE_TOPOLOGY_LINE_STRIP: u8 = 0x02;
/// [`PrimitiveTopology::TriangleList`] — the engine's default under vertex
/// pulling.
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_LIST: u8 = 0x03;
/// [`PrimitiveTopology::TriangleStrip`].
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP: u8 = 0x04;

/// The wire code for a [`PrimitiveTopology`].
#[must_use]
pub const fn primitive_topology_code(topology: PrimitiveTopology) -> u8 {
    match topology {
        PrimitiveTopology::PointList => PRIMITIVE_TOPOLOGY_POINT_LIST,
        PrimitiveTopology::LineList => PRIMITIVE_TOPOLOGY_LINE_LIST,
        PrimitiveTopology::LineStrip => PRIMITIVE_TOPOLOGY_LINE_STRIP,
        PrimitiveTopology::TriangleList => PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
        PrimitiveTopology::TriangleStrip => PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP,
    }
}

/// The [`PrimitiveTopology`] a wire code names, or `None` if it names none.
///
/// **Never a neighbour**, and here a neighbour costs a *strip* read as a *list*
/// or the reverse: `LineStrip` folded into `LineList` connects segments that
/// were meant to be independent, or breaks a connected outline into disjoint
/// pieces, and the primitive assembles either way — nothing downstream refuses
/// it, so this table answering `None` for an unclaimed code is the only place
/// the difference is caught.
#[must_use]
pub const fn primitive_topology_from_code(code: u8) -> Option<PrimitiveTopology> {
    match code {
        PRIMITIVE_TOPOLOGY_POINT_LIST => Some(PrimitiveTopology::PointList),
        PRIMITIVE_TOPOLOGY_LINE_LIST => Some(PrimitiveTopology::LineList),
        PRIMITIVE_TOPOLOGY_LINE_STRIP => Some(PrimitiveTopology::LineStrip),
        PRIMITIVE_TOPOLOGY_TRIANGLE_LIST => Some(PrimitiveTopology::TriangleList),
        PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP => Some(PrimitiveTopology::TriangleStrip),
        _ => None,
    }
}

// ── FrontFace ─────────────────────────────────────────────────────────────────

/// [`FrontFace::Ccw`].
pub const FRONT_FACE_CCW: u8 = 0x00;
/// [`FrontFace::Cw`].
pub const FRONT_FACE_CW: u8 = 0x01;

/// The wire code for a [`FrontFace`].
#[must_use]
pub const fn front_face_code(front_face: FrontFace) -> u8 {
    match front_face {
        FrontFace::Ccw => FRONT_FACE_CCW,
        FrontFace::Cw => FRONT_FACE_CW,
    }
}

/// The [`FrontFace`] a wire code names, or `None` if it names none.
///
/// **`None` rather than a default winding**, tempting though it is for a
/// two-variant enum: `front_face` and [`cull_mode`](cull_mode_from_code) decide
/// together which triangles survive, so a winding read as its opposite culls
/// exactly the faces that should have been kept — a mesh inside-out, with the
/// draw succeeding and nothing reporting it.
#[must_use]
pub const fn front_face_from_code(code: u8) -> Option<FrontFace> {
    match code {
        FRONT_FACE_CCW => Some(FrontFace::Ccw),
        FRONT_FACE_CW => Some(FrontFace::Cw),
        _ => None,
    }
}

// ── CullMode ──────────────────────────────────────────────────────────────────

/// [`CullMode::None`].
pub const CULL_MODE_NONE: u8 = 0x00;
/// [`CullMode::Front`].
pub const CULL_MODE_FRONT: u8 = 0x01;
/// [`CullMode::Back`].
pub const CULL_MODE_BACK: u8 = 0x02;

/// The wire code for a [`CullMode`].
#[must_use]
pub const fn cull_mode_code(cull_mode: CullMode) -> u8 {
    match cull_mode {
        CullMode::None => CULL_MODE_NONE,
        CullMode::Front => CULL_MODE_FRONT,
        CullMode::Back => CULL_MODE_BACK,
    }
}

/// The [`CullMode`] a wire code names, or `None` if it names none.
///
/// **The pair that costs the most is [`CULL_MODE_FRONT`] and [`CULL_MODE_BACK`]**:
/// they agree on how much is discarded and disagree only on *which* half, so a
/// fold shows the far side of every object and hides the near one, which reads
/// as geometry turned inside out rather than as an error. `None` folded into
/// either instead drops culling entirely, doubling the work with no visible
/// wrong — so the unclaimed code answers `None` and nothing folds.
#[must_use]
pub const fn cull_mode_from_code(code: u8) -> Option<CullMode> {
    match code {
        CULL_MODE_NONE => Some(CullMode::None),
        CULL_MODE_FRONT => Some(CullMode::Front),
        CULL_MODE_BACK => Some(CullMode::Back),
        _ => None,
    }
}

// ── PolygonMode ───────────────────────────────────────────────────────────────
//
// Two variants, and [`PolygonMode::Line`] is the one WebGPU cannot express at
// all — wireframe is `Features::POLYGON_MODE_LINE`, native-only — so, like a
// mesh stage or a push-constant range, it crosses verbatim and the replayer
// refuses it. The code table is still written out: a `Line` read as `Fill`
// would silently fill a wireframe pass the caller meant to see through.

/// [`PolygonMode::Fill`].
pub const POLYGON_MODE_FILL: u8 = 0x00;
/// [`PolygonMode::Line`] — wireframe, which WebGPU has no core expression for.
pub const POLYGON_MODE_LINE: u8 = 0x01;

/// The wire code for a [`PolygonMode`].
#[must_use]
pub const fn polygon_mode_code(polygon_mode: PolygonMode) -> u8 {
    match polygon_mode {
        PolygonMode::Fill => POLYGON_MODE_FILL,
        PolygonMode::Line => POLYGON_MODE_LINE,
    }
}

/// The [`PolygonMode`] a wire code names, or `None` if it names none.
///
/// **`None` rather than [`PolygonMode::Fill`] as a default**: `Line` is what the
/// replayer refuses (WebGPU has no wireframe fill mode), so folding an unclaimed
/// code into `Fill` would turn a wireframe pass into a solid one silently — the
/// opposite of the loud refusal the seam owes it.
#[must_use]
pub const fn polygon_mode_from_code(code: u8) -> Option<PolygonMode> {
    match code {
        POLYGON_MODE_FILL => Some(PolygonMode::Fill),
        POLYGON_MODE_LINE => Some(PolygonMode::Line),
        _ => None,
    }
}

// ── StencilOp ─────────────────────────────────────────────────────────────────
//
// Carried four at a time on the wire — a [`StencilState`](crcbl_hal::StencilState)
// holds two [`StencilFaceState`](crcbl_hal::StencilFaceState)s, each with three
// `StencilOp`s (`fail_op`, `depth_fail_op`, `pass_op`) — so this table has
// [`FilterMode`]'s hazard with eight variants and six reads back to back: an op
// read out of position lands on a real op rather than an error, and a stencil
// buffer written the wrong way is visible only as the second pass it drives.

/// [`StencilOp::Keep`].
pub const STENCIL_OP_KEEP: u8 = 0x00;
/// [`StencilOp::Zero`].
pub const STENCIL_OP_ZERO: u8 = 0x01;
/// [`StencilOp::Replace`].
pub const STENCIL_OP_REPLACE: u8 = 0x02;
/// [`StencilOp::Invert`].
pub const STENCIL_OP_INVERT: u8 = 0x03;
/// [`StencilOp::IncrementClamp`].
pub const STENCIL_OP_INCREMENT_CLAMP: u8 = 0x04;
/// [`StencilOp::DecrementClamp`].
pub const STENCIL_OP_DECREMENT_CLAMP: u8 = 0x05;
/// [`StencilOp::IncrementWrap`].
pub const STENCIL_OP_INCREMENT_WRAP: u8 = 0x06;
/// [`StencilOp::DecrementWrap`].
pub const STENCIL_OP_DECREMENT_WRAP: u8 = 0x07;

/// The wire code for a [`StencilOp`].
#[must_use]
pub const fn stencil_op_code(op: StencilOp) -> u8 {
    match op {
        StencilOp::Keep => STENCIL_OP_KEEP,
        StencilOp::Zero => STENCIL_OP_ZERO,
        StencilOp::Replace => STENCIL_OP_REPLACE,
        StencilOp::Invert => STENCIL_OP_INVERT,
        StencilOp::IncrementClamp => STENCIL_OP_INCREMENT_CLAMP,
        StencilOp::DecrementClamp => STENCIL_OP_DECREMENT_CLAMP,
        StencilOp::IncrementWrap => STENCIL_OP_INCREMENT_WRAP,
        StencilOp::DecrementWrap => STENCIL_OP_DECREMENT_WRAP,
    }
}

/// The [`StencilOp`] a wire code names, or `None` if it names none.
///
/// **The clamp/wrap pairs are the adjacent ones a fold costs the most**:
/// `IncrementClamp` and `IncrementWrap` differ only at the value's extreme, so
/// a stencil counter written with the wrong one agrees everywhere until it
/// saturates and then silently disagrees — exactly the kind of difference no
/// draw reports.
#[must_use]
pub const fn stencil_op_from_code(code: u8) -> Option<StencilOp> {
    match code {
        STENCIL_OP_KEEP => Some(StencilOp::Keep),
        STENCIL_OP_ZERO => Some(StencilOp::Zero),
        STENCIL_OP_REPLACE => Some(StencilOp::Replace),
        STENCIL_OP_INVERT => Some(StencilOp::Invert),
        STENCIL_OP_INCREMENT_CLAMP => Some(StencilOp::IncrementClamp),
        STENCIL_OP_DECREMENT_CLAMP => Some(StencilOp::DecrementClamp),
        STENCIL_OP_INCREMENT_WRAP => Some(StencilOp::IncrementWrap),
        STENCIL_OP_DECREMENT_WRAP => Some(StencilOp::DecrementWrap),
        _ => None,
    }
}

// ── BlendFactor ───────────────────────────────────────────────────────────────
//
// Carried four at a time per colour target — a [`BlendState`](crcbl_hal::BlendState)
// has `color_src`, `color_dst`, `alpha_src`, `alpha_dst` — with two [`BlendOp`]s
// interleaved between them, so, like the sampler's filters, a factor read out of
// position decodes to another factor rather than to an error and a target blends
// with the wrong equation, visible only as the wrong colour.

/// [`BlendFactor::Zero`].
pub const BLEND_FACTOR_ZERO: u8 = 0x00;
/// [`BlendFactor::One`].
pub const BLEND_FACTOR_ONE: u8 = 0x01;
/// [`BlendFactor::Src`].
pub const BLEND_FACTOR_SRC: u8 = 0x02;
/// [`BlendFactor::OneMinusSrc`].
pub const BLEND_FACTOR_ONE_MINUS_SRC: u8 = 0x03;
/// [`BlendFactor::SrcAlpha`].
pub const BLEND_FACTOR_SRC_ALPHA: u8 = 0x04;
/// [`BlendFactor::OneMinusSrcAlpha`].
pub const BLEND_FACTOR_ONE_MINUS_SRC_ALPHA: u8 = 0x05;
/// [`BlendFactor::Dst`].
pub const BLEND_FACTOR_DST: u8 = 0x06;
/// [`BlendFactor::OneMinusDst`].
pub const BLEND_FACTOR_ONE_MINUS_DST: u8 = 0x07;
/// [`BlendFactor::DstAlpha`].
pub const BLEND_FACTOR_DST_ALPHA: u8 = 0x08;
/// [`BlendFactor::OneMinusDstAlpha`].
pub const BLEND_FACTOR_ONE_MINUS_DST_ALPHA: u8 = 0x09;

/// The wire code for a [`BlendFactor`].
#[must_use]
pub const fn blend_factor_code(factor: BlendFactor) -> u8 {
    match factor {
        BlendFactor::Zero => BLEND_FACTOR_ZERO,
        BlendFactor::One => BLEND_FACTOR_ONE,
        BlendFactor::Src => BLEND_FACTOR_SRC,
        BlendFactor::OneMinusSrc => BLEND_FACTOR_ONE_MINUS_SRC,
        BlendFactor::SrcAlpha => BLEND_FACTOR_SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
        BlendFactor::Dst => BLEND_FACTOR_DST,
        BlendFactor::OneMinusDst => BLEND_FACTOR_ONE_MINUS_DST,
        BlendFactor::DstAlpha => BLEND_FACTOR_DST_ALPHA,
        BlendFactor::OneMinusDstAlpha => BLEND_FACTOR_ONE_MINUS_DST_ALPHA,
    }
}

/// The [`BlendFactor`] a wire code names, or `None` if it names none.
///
/// **Each factor and its `OneMinus` complement are the adjacent pair a fold
/// costs the most**: `Src` against `OneMinusSrc` composites in exactly the
/// opposite direction, so a fold inverts the blend and leaves a valid pipeline
/// the browser accepts and no draw reports.
#[must_use]
pub const fn blend_factor_from_code(code: u8) -> Option<BlendFactor> {
    match code {
        BLEND_FACTOR_ZERO => Some(BlendFactor::Zero),
        BLEND_FACTOR_ONE => Some(BlendFactor::One),
        BLEND_FACTOR_SRC => Some(BlendFactor::Src),
        BLEND_FACTOR_ONE_MINUS_SRC => Some(BlendFactor::OneMinusSrc),
        BLEND_FACTOR_SRC_ALPHA => Some(BlendFactor::SrcAlpha),
        BLEND_FACTOR_ONE_MINUS_SRC_ALPHA => Some(BlendFactor::OneMinusSrcAlpha),
        BLEND_FACTOR_DST => Some(BlendFactor::Dst),
        BLEND_FACTOR_ONE_MINUS_DST => Some(BlendFactor::OneMinusDst),
        BLEND_FACTOR_DST_ALPHA => Some(BlendFactor::DstAlpha),
        BLEND_FACTOR_ONE_MINUS_DST_ALPHA => Some(BlendFactor::OneMinusDstAlpha),
        _ => None,
    }
}

// ── BlendOp ───────────────────────────────────────────────────────────────────
//
// Two per colour target — `color_op` and `alpha_op` — and the reason `Min`/`Max`
// ignore their factors while the arithmetic ops do not, so a fold between the two
// groups also changes whether the factors above even matter.

/// [`BlendOp::Add`].
pub const BLEND_OP_ADD: u8 = 0x00;
/// [`BlendOp::Subtract`].
pub const BLEND_OP_SUBTRACT: u8 = 0x01;
/// [`BlendOp::ReverseSubtract`].
pub const BLEND_OP_REVERSE_SUBTRACT: u8 = 0x02;
/// [`BlendOp::Min`].
pub const BLEND_OP_MIN: u8 = 0x03;
/// [`BlendOp::Max`].
pub const BLEND_OP_MAX: u8 = 0x04;

/// The wire code for a [`BlendOp`].
#[must_use]
pub const fn blend_op_code(op: BlendOp) -> u8 {
    match op {
        BlendOp::Add => BLEND_OP_ADD,
        BlendOp::Subtract => BLEND_OP_SUBTRACT,
        BlendOp::ReverseSubtract => BLEND_OP_REVERSE_SUBTRACT,
        BlendOp::Min => BLEND_OP_MIN,
        BlendOp::Max => BLEND_OP_MAX,
    }
}

/// The [`BlendOp`] a wire code names, or `None` if it names none.
///
/// **`Subtract` and `ReverseSubtract` are the adjacent pair a fold costs the
/// most**: they swap which operand is subtracted from which, so a target reads
/// its own colour where it meant the destination's, and the pipeline is valid
/// either way.
#[must_use]
pub const fn blend_op_from_code(code: u8) -> Option<BlendOp> {
    match code {
        BLEND_OP_ADD => Some(BlendOp::Add),
        BLEND_OP_SUBTRACT => Some(BlendOp::Subtract),
        BLEND_OP_REVERSE_SUBTRACT => Some(BlendOp::ReverseSubtract),
        BLEND_OP_MIN => Some(BlendOp::Min),
        BLEND_OP_MAX => Some(BlendOp::Max),
        _ => None,
    }
}

// ── SampleType ────────────────────────────────────────────────────────────────
//
// Carried by [`BindingKind::SampledImage`], and the smallest table here with the
// loudest failure. `crcbl-hal` says why the field exists at all: **WebGPU puts
// it in the layout**, as `GPUTextureBindingLayout.sampleType`, and refuses a
// depth-format view bound through a slot that says `float`. So the two codes are
// not two spellings of a preference — they are two different bind-group layouts,
// and a stream that folded one into the other builds a layout the browser then
// refuses every bind group against.

/// [`SampleType::Float`] — ordinary filterable colour texels.
pub const SAMPLE_TYPE_FLOAT: u8 = 0x00;
/// [`SampleType::Depth`] — a depth texture read through a comparison sampler.
pub const SAMPLE_TYPE_DEPTH: u8 = 0x01;

/// The wire code for a [`SampleType`].
#[must_use]
pub const fn sample_type_code(sample_type: SampleType) -> u8 {
    match sample_type {
        SampleType::Float => SAMPLE_TYPE_FLOAT,
        SampleType::Depth => SAMPLE_TYPE_DEPTH,
    }
}

/// The [`SampleType`] a wire code names, or `None` if it names none.
///
/// **`None` rather than [`SampleType::Float`] as a default**, tempting though it
/// is for a two-variant enum whose first variant is what nearly every binding
/// wants: a `Depth` slot arriving as `Float` is a layout WebGPU accepts and then
/// refuses every depth view against, with the error naming the *bind group*
/// rather than the layout that was wrong.
#[must_use]
pub const fn sample_type_from_code(code: u8) -> Option<SampleType> {
    match code {
        SAMPLE_TYPE_FLOAT => Some(SampleType::Float),
        SAMPLE_TYPE_DEPTH => Some(SampleType::Depth),
        _ => None,
    }
}

// ── BindingKind ───────────────────────────────────────────────────────────────
//
// **The one code table here whose enum carries data**, so it is a code plus a
// body rather than a code alone — and the shape is the reason there is no
// `binding_kind_from_code`: a code names a variant, and the variant's fields have
// to be read before there is a [`BindingKind`] to answer with. So this half is
// the writer's table, [`crate::StreamReader`] holds the dispatch, and the arm
// that refuses an unclaimed code lives there with the reads it guards.
//
// **What each code costs on the far side is not symmetrical**, which is why the
// codes are spread across the variants rather than grouped by "buffer-ish" and
// "image-ish": WebGPU does not have a binding *type* at all.
// `GPUBindGroupLayoutEntry` carries **one of** `buffer`, `sampler`, `texture`,
// `storageTexture` or `externalTexture` as a *member*, each an object with its
// own fields, so this byte decides which member exists. A code folded into a
// neighbour therefore does not mis-set a field — it builds a layout describing a
// different kind of resource, and every bind group made against it is refused by
// the browser naming the group. `web/engine/gpu-replay.js` is where that mapping
// is written out, including the variant WebGPU cannot express.

/// [`BindingKind::UniformBuffer`]; a `bool` `dynamic` follows.
pub const BINDING_KIND_UNIFORM_BUFFER: u8 = 0x00;
/// [`BindingKind::StorageBuffer`]; `read_only` then `dynamic`, both `bool`.
pub const BINDING_KIND_STORAGE_BUFFER: u8 = 0x01;
/// [`BindingKind::SampledImage`]; an [`ImageViewType`] code then a
/// [`SampleType`] code.
pub const BINDING_KIND_SAMPLED_IMAGE: u8 = 0x02;
/// [`BindingKind::StorageImage`]; a `bool` `read_only` follows.
pub const BINDING_KIND_STORAGE_IMAGE: u8 = 0x03;
/// [`BindingKind::Sampler`]; a `bool` `comparison` follows.
pub const BINDING_KIND_SAMPLER: u8 = 0x04;

/// The wire code for a [`BindingKind`], which is followed by that variant's own
/// fields.
///
/// Exhaustive, so a variant added to `crcbl-hal` stops this file compiling —
/// which is the moment the number beside it, and the body the reader has to
/// grow, are both impossible to miss.
#[must_use]
pub const fn binding_kind_code(kind: BindingKind) -> u8 {
    match kind {
        BindingKind::UniformBuffer { .. } => BINDING_KIND_UNIFORM_BUFFER,
        BindingKind::StorageBuffer { .. } => BINDING_KIND_STORAGE_BUFFER,
        BindingKind::SampledImage { .. } => BINDING_KIND_SAMPLED_IMAGE,
        BindingKind::StorageImage { .. } => BINDING_KIND_STORAGE_IMAGE,
        BindingKind::Sampler { .. } => BINDING_KIND_SAMPLER,
    }
}

/// One past the last claimed [`BindingKind`] code.
///
/// What the reader's refusing arm is checked against: `0xFF` never lands on an
/// off-by-one in the table, and this byte does.
pub const BINDING_KIND_CODES: u8 = 0x05;

// ── BindingResource ─────────────────────────────────────────────────────────────
//
// [`BindGroupEntry`](crcbl_hal::BindGroupEntry)'s payload, and the second code
// table on this seam whose enum carries data — so, like [`BindingKind`], it is a
// code plus a body rather than a code alone, there is no `binding_resource_from_code`,
// and the refusing arm lives in [`crate::StreamReader`] with the reads it guards.
//
// **What a neighbour costs here is the most dangerous confusion on the seam.** A
// [`Handle`](crcbl_core::Handle) carries no kind, so a buffer, a view and a
// sampler may hold identical bits, and this byte is the *only* thing that says
// which of the replayer's three resource tables a handle indexes. Folding a
// [`BindingResource::Sampler`](crcbl_hal::BindingResource::Sampler) into a
// [`BindingResource::ImageView`](crcbl_hal::BindingResource::ImageView) does not
// mis-set a field — it resolves the same bits against the wrong table and binds a
// sampler where a texture belongs, which the browser refuses naming the *bind
// group* rather than the entry that was wrong. The codes are also not
// symmetrical in *length*:
// [`Buffer`](crcbl_hal::BindingResource::Buffer) carries a handle and two `u64`s
// while the other two carry a bare handle, so a code read as its neighbour lands
// the cursor sixteen bytes off and every entry after it decodes out of noise.
// `web/engine/gpu-replay.js` is where the routing to each table is written out.

/// [`BindingResource::Buffer`]; a handle then two `u64`s, `offset` and `size`.
pub const BINDING_RESOURCE_BUFFER: u8 = 0x00;
/// [`BindingResource::ImageView`]; a bare handle.
pub const BINDING_RESOURCE_IMAGE_VIEW: u8 = 0x01;
/// [`BindingResource::Sampler`]; a bare handle.
pub const BINDING_RESOURCE_SAMPLER: u8 = 0x02;

/// The wire code for a [`BindingResource`], which is followed by that variant's
/// own fields.
///
/// Exhaustive, so a variant added to `crcbl-hal` stops this file compiling —
/// which is the moment the number beside it, and the body the reader has to grow,
/// are both impossible to miss.
#[must_use]
pub const fn binding_resource_code(resource: BindingResource) -> u8 {
    match resource {
        BindingResource::Buffer { .. } => BINDING_RESOURCE_BUFFER,
        BindingResource::ImageView(_) => BINDING_RESOURCE_IMAGE_VIEW,
        BindingResource::Sampler(_) => BINDING_RESOURCE_SAMPLER,
    }
}

/// One past the last claimed [`BindingResource`] code. What the reader's refusing
/// arm is checked against, for [`BINDING_KIND_CODES`]'s reason.
pub const BINDING_RESOURCE_CODES: u8 = 0x03;

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

// ── SurfaceCapsFailure ────────────────────────────────────────────────────────
//
// The one code table on this seam whose enum is **this crate's own** rather than
// `crcbl-hal`'s, and it is not a shortcut: a [`HalError`] carries data — a
// handle's bits, an adapter index — and the query has no arguments to carry
// back. What crosses is which error to build.
//
// **One cause, and the table is written out anyway** for the reason every other
// table here is: a variant added to [`SurfaceCapsFailure`] stops
// [`surface_caps_failure_code`] compiling, which is the moment the number beside
// it is impossible to miss.
//
// The table has no holes, so the code above the last claimed one is `0x01` — the
// byte a JavaScript half that kept an older, wider table would send, and the
// byte [`surface_caps_failure_from_code`] answers `None` for.

/// [`SurfaceCapsFailure::Backend`] — the only cause the query can answer with.
pub const SURFACE_CAPS_FAILURE_BACKEND: u8 = 0x00;

/// The wire code for a [`SurfaceCapsFailure`].
#[must_use]
pub const fn surface_caps_failure_code(cause: SurfaceCapsFailure) -> u8 {
    match cause {
        SurfaceCapsFailure::Backend => SURFACE_CAPS_FAILURE_BACKEND,
    }
}

/// The [`SurfaceCapsFailure`] a wire code names, or `None` if it names none.
///
/// **Never [`SurfaceCapsFailure::Backend`] as a catch-all**, tempting though a
/// "something went wrong" arm is now that it is the only arm there is: the
/// codes a sender can spell and the codes this table claims are the two halves
/// of a hand-written format, and an arm that took every byte would make them
/// agree by construction. A JavaScript half still writing the retired causes
/// must be an error here, which is the only place that drift is visible.
#[must_use]
pub const fn surface_caps_failure_from_code(code: u8) -> Option<SurfaceCapsFailure> {
    match code {
        SURFACE_CAPS_FAILURE_BACKEND => Some(SurfaceCapsFailure::Backend),
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
    const TAGS: [(&str, u8, u8); 62] = [
        ("CreateBuffer", CREATE_BUFFER_TAG, FAMILY_CREATE),
        ("CreateSurface", CREATE_SURFACE_TAG, FAMILY_CREATE),
        (
            "CreateOffscreenSurface",
            CREATE_OFFSCREEN_SURFACE_TAG,
            FAMILY_CREATE,
        ),
        ("CreateImage", CREATE_IMAGE_TAG, FAMILY_CREATE),
        ("CreateImageView", CREATE_IMAGE_VIEW_TAG, FAMILY_CREATE),
        ("CreateSampler", CREATE_SAMPLER_TAG, FAMILY_CREATE),
        (
            "CreateBindGroupLayout",
            CREATE_BIND_GROUP_LAYOUT_TAG,
            FAMILY_CREATE,
        ),
        ("CreateBindGroup", CREATE_BIND_GROUP_TAG, FAMILY_CREATE),
        (
            "CreateShaderModule",
            CREATE_SHADER_MODULE_TAG,
            FAMILY_CREATE,
        ),
        (
            "CreatePipelineLayout",
            CREATE_PIPELINE_LAYOUT_TAG,
            FAMILY_CREATE,
        ),
        (
            "CreateComputePipeline",
            CREATE_COMPUTE_PIPELINE_TAG,
            FAMILY_CREATE,
        ),
        (
            "CreateGraphicsPipeline",
            CREATE_GRAPHICS_PIPELINE_TAG,
            FAMILY_CREATE,
        ),
        ("RequestReadback", REQUEST_READBACK_TAG, FAMILY_CREATE),
        (
            "DestroyComputePipeline",
            DESTROY_COMPUTE_PIPELINE_TAG,
            FAMILY_DESTROY,
        ),
        (
            "DestroyGraphicsPipeline",
            DESTROY_GRAPHICS_PIPELINE_TAG,
            FAMILY_DESTROY,
        ),
        (
            "DestroyPipelineLayout",
            DESTROY_PIPELINE_LAYOUT_TAG,
            FAMILY_DESTROY,
        ),
        (
            "DestroyBindGroupLayout",
            DESTROY_BIND_GROUP_LAYOUT_TAG,
            FAMILY_DESTROY,
        ),
        ("DestroyBindGroup", DESTROY_BIND_GROUP_TAG, FAMILY_DESTROY),
        (
            "DestroyShaderModule",
            DESTROY_SHADER_MODULE_TAG,
            FAMILY_DESTROY,
        ),
        ("DestroyBuffer", DESTROY_BUFFER_TAG, FAMILY_DESTROY),
        ("DestroySurface", DESTROY_SURFACE_TAG, FAMILY_DESTROY),
        ("DestroyImage", DESTROY_IMAGE_TAG, FAMILY_DESTROY),
        ("DestroyImageView", DESTROY_IMAGE_VIEW_TAG, FAMILY_DESTROY),
        ("DestroySampler", DESTROY_SAMPLER_TAG, FAMILY_DESTROY),
        (
            "DestroyCommandBuffer",
            DESTROY_COMMAND_BUFFER_TAG,
            FAMILY_DESTROY,
        ),
        ("DestroyReadback", DESTROY_READBACK_TAG, FAMILY_DESTROY),
        ("BeginDebugLabel", BEGIN_DEBUG_LABEL_TAG, FAMILY_ENCODER),
        ("BeginRenderPass", BEGIN_RENDER_PASS_TAG, FAMILY_ENCODER),
        (
            "BindGraphicsPipeline",
            BIND_GRAPHICS_PIPELINE_TAG,
            FAMILY_ENCODER,
        ),
        ("BindGroup", BIND_GROUP_TAG, FAMILY_ENCODER),
        ("PushConstants", PUSH_CONSTANTS_TAG, FAMILY_ENCODER),
        (
            "CreateCommandEncoder",
            CREATE_COMMAND_ENCODER_TAG,
            FAMILY_ENCODER,
        ),
        ("EndRenderPass", END_RENDER_PASS_TAG, FAMILY_ENCODER),
        ("Finish", FINISH_TAG, FAMILY_ENCODER),
        ("BeginComputePass", BEGIN_COMPUTE_PASS_TAG, FAMILY_ENCODER),
        ("EndComputePass", END_COMPUTE_PASS_TAG, FAMILY_ENCODER),
        (
            "BindComputePipeline",
            BIND_COMPUTE_PIPELINE_TAG,
            FAMILY_ENCODER,
        ),
        ("PipelineBarrier", PIPELINE_BARRIER_TAG, FAMILY_ENCODER),
        ("SetViewport", SET_VIEWPORT_TAG, FAMILY_ENCODER),
        ("SetScissor", SET_SCISSOR_TAG, FAMILY_ENCODER),
        ("BindIndexBuffer", BIND_INDEX_BUFFER_TAG, FAMILY_ENCODER),
        ("Draw", DRAW_TAG, FAMILY_DRAW),
        ("DrawIndexed", DRAW_INDEXED_TAG, FAMILY_DRAW),
        ("DrawIndirect", DRAW_INDIRECT_TAG, FAMILY_DRAW),
        (
            "DrawIndexedIndirect",
            DRAW_INDEXED_INDIRECT_TAG,
            FAMILY_DRAW,
        ),
        ("Dispatch", DISPATCH_TAG, FAMILY_DISPATCH),
        ("CopyImageToBuffer", COPY_IMAGE_TO_BUFFER_TAG, FAMILY_COPY),
        ("CopyBufferToBuffer", COPY_BUFFER_TO_BUFFER_TAG, FAMILY_COPY),
        ("CopyBufferToImage", COPY_BUFFER_TO_IMAGE_TAG, FAMILY_COPY),
        ("CopyImageToImage", COPY_IMAGE_TO_IMAGE_TAG, FAMILY_COPY),
        ("FillBuffer", FILL_BUFFER_TAG, FAMILY_COPY),
        ("WriteBuffer", WRITE_BUFFER_TAG, FAMILY_COPY),
        ("CreateSwapchain", CREATE_SWAPCHAIN_TAG, FAMILY_PRESENT),
        ("AcquireNextFrame", ACQUIRE_NEXT_FRAME_TAG, FAMILY_PRESENT),
        ("Present", PRESENT_TAG, FAMILY_PRESENT),
        ("DestroySwapchain", DESTROY_SWAPCHAIN_TAG, FAMILY_PRESENT),
        (
            "ReconfigureSwapchain",
            RECONFIGURE_SWAPCHAIN_TAG,
            FAMILY_PRESENT,
        ),
        ("EnumerateAdapters", ENUMERATE_ADAPTERS_TAG, FAMILY_INSTANCE),
        ("RequestDevice", REQUEST_DEVICE_TAG, FAMILY_INSTANCE),
        ("SurfaceCaps", SURFACE_CAPS_TAG, FAMILY_INSTANCE),
        ("Submit", SUBMIT_TAG, FAMILY_DEVICE),
        ("PollReadback", POLL_READBACK_TAG, FAMILY_DEVICE),
    ];

    /// Every reply tag this slice defines, with the family its name claims.
    /// Spelled out for the reason [`TAGS`] is.
    const REPLY_TAGS: [(&str, u8, u8); 9] = [
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
            "SurfaceCapsFailed",
            SURFACE_CAPS_FAILED_REPLY_TAG,
            REPLY_FAMILY_INSTANCE,
        ),
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

        let image_type = [ImageType::D1, ImageType::D2, ImageType::D3];
        let codes: Vec<u8> = image_type.iter().map(|t| image_type_code(*t)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two ImageTypes share a code");
        for kind in image_type {
            assert_eq!(image_type_from_code(image_type_code(kind)), Some(kind));
        }

        let view_type = [
            ImageViewType::D1,
            ImageViewType::D2,
            ImageViewType::D2Array,
            ImageViewType::Cube,
            ImageViewType::CubeArray,
            ImageViewType::D3,
        ];
        let codes: Vec<u8> = view_type.iter().map(|t| image_view_type_code(*t)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two ImageViewTypes share a code"
        );
        for kind in view_type {
            assert_eq!(
                image_view_type_from_code(image_view_type_code(kind)),
                Some(kind)
            );
        }

        let filter = [FilterMode::Nearest, FilterMode::Linear];
        let codes: Vec<u8> = filter.iter().map(|m| filter_mode_code(*m)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two FilterModes share a code"
        );
        for mode in filter {
            assert_eq!(filter_mode_from_code(filter_mode_code(mode)), Some(mode));
        }

        let address = [
            SamplerAddressMode::Repeat,
            SamplerAddressMode::MirrorRepeat,
            SamplerAddressMode::ClampToEdge,
            SamplerAddressMode::ClampToBorder,
        ];
        let codes: Vec<u8> = address
            .iter()
            .map(|m| sampler_address_mode_code(*m))
            .collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two SamplerAddressModes share a code"
        );
        for mode in address {
            assert_eq!(
                sampler_address_mode_from_code(sampler_address_mode_code(mode)),
                Some(mode)
            );
        }

        let compare = [
            CompareOp::Never,
            CompareOp::Less,
            CompareOp::Equal,
            CompareOp::LessOrEqual,
            CompareOp::Greater,
            CompareOp::NotEqual,
            CompareOp::GreaterOrEqual,
            CompareOp::Always,
        ];
        let codes: Vec<u8> = compare.iter().map(|op| compare_op_code(*op)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two CompareOps share a code");
        for op in compare {
            assert_eq!(compare_op_from_code(compare_op_code(op)), Some(op));
        }
        // The pair whose fold is silent, stated on its own: `Greater` is the
        // engine's shadow test under reversed-Z and `Less` is its opposite, and
        // a sampler built with either reports nothing about which it got.
        assert_ne!(
            compare_op_code(CompareOp::Greater),
            compare_op_code(CompareOp::Less)
        );

        let topology = [
            PrimitiveTopology::PointList,
            PrimitiveTopology::LineList,
            PrimitiveTopology::LineStrip,
            PrimitiveTopology::TriangleList,
            PrimitiveTopology::TriangleStrip,
        ];
        let codes: Vec<u8> = topology
            .iter()
            .map(|t| primitive_topology_code(*t))
            .collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two PrimitiveTopologies share a code"
        );
        for kind in topology {
            assert_eq!(
                primitive_topology_from_code(primitive_topology_code(kind)),
                Some(kind)
            );
        }

        let front_face = [FrontFace::Ccw, FrontFace::Cw];
        let codes: Vec<u8> = front_face.iter().map(|f| front_face_code(*f)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two FrontFaces share a code");
        for face in front_face {
            assert_eq!(front_face_from_code(front_face_code(face)), Some(face));
        }

        let cull = [CullMode::None, CullMode::Front, CullMode::Back];
        let codes: Vec<u8> = cull.iter().map(|c| cull_mode_code(*c)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two CullModes share a code");
        for mode in cull {
            assert_eq!(cull_mode_from_code(cull_mode_code(mode)), Some(mode));
        }

        let polygon = [PolygonMode::Fill, PolygonMode::Line];
        let codes: Vec<u8> = polygon.iter().map(|p| polygon_mode_code(*p)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two PolygonModes share a code"
        );
        for mode in polygon {
            assert_eq!(polygon_mode_from_code(polygon_mode_code(mode)), Some(mode));
        }

        let stencil = [
            StencilOp::Keep,
            StencilOp::Zero,
            StencilOp::Replace,
            StencilOp::Invert,
            StencilOp::IncrementClamp,
            StencilOp::DecrementClamp,
            StencilOp::IncrementWrap,
            StencilOp::DecrementWrap,
        ];
        let codes: Vec<u8> = stencil.iter().map(|s| stencil_op_code(*s)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two StencilOps share a code");
        for op in stencil {
            assert_eq!(stencil_op_from_code(stencil_op_code(op)), Some(op));
        }

        let blend_factor = [
            BlendFactor::Zero,
            BlendFactor::One,
            BlendFactor::Src,
            BlendFactor::OneMinusSrc,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::Dst,
            BlendFactor::OneMinusDst,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
        ];
        let codes: Vec<u8> = blend_factor.iter().map(|f| blend_factor_code(*f)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two BlendFactors share a code"
        );
        for factor in blend_factor {
            assert_eq!(
                blend_factor_from_code(blend_factor_code(factor)),
                Some(factor)
            );
        }

        let blend_op = [
            BlendOp::Add,
            BlendOp::Subtract,
            BlendOp::ReverseSubtract,
            BlendOp::Min,
            BlendOp::Max,
        ];
        let codes: Vec<u8> = blend_op.iter().map(|o| blend_op_code(*o)).collect();
        assert_eq!(distinct(&codes), codes.len(), "two BlendOps share a code");
        for op in blend_op {
            assert_eq!(blend_op_from_code(blend_op_code(op)), Some(op));
        }

        let sample_type = [SampleType::Float, SampleType::Depth];
        let codes: Vec<u8> = sample_type.iter().map(|t| sample_type_code(*t)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two SampleTypes share a code"
        );
        for kind in sample_type {
            assert_eq!(sample_type_from_code(sample_type_code(kind)), Some(kind));
        }

        // `BindingKind` has no `from_code` — a code names a variant and the
        // variant's fields have to be read before there is one to answer with —
        // so what can be checked here is that the five codes are distinct and
        // that they stop where the reader's refusing arm believes they do. The
        // round trip is a whole command, in `tests/stream.rs`.
        let binding_kind = [
            BindingKind::UniformBuffer { dynamic: false },
            BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            BindingKind::StorageImage { read_only: false },
            BindingKind::Sampler { comparison: false },
        ];
        let codes: Vec<u8> = binding_kind.iter().map(|k| binding_kind_code(*k)).collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two BindingKinds share a code"
        );
        assert_eq!(
            codes.iter().copied().max().map(|top| top + 1),
            Some(BINDING_KIND_CODES),
            "BINDING_KIND_CODES is not one past the last claimed code, so the \
             reader's refusing arm is checked against the wrong byte"
        );
        // The payload does not change the code, which is what makes the byte a
        // *kind* rather than a summary of the variant's fields.
        assert_eq!(
            binding_kind_code(BindingKind::StorageBuffer {
                read_only: false,
                dynamic: true
            }),
            BINDING_KIND_STORAGE_BUFFER
        );

        // `BindingResource` has no `from_code` for `BindingKind`'s reason — a
        // code names a variant whose fields have to be read before there is one
        // to answer with — so what is checked here is that its three codes are
        // distinct and stop where the reader's refusing arm believes they do. The
        // round trip is a whole command, in `tests/stream.rs`.
        let buffer = crcbl_hal::BufferHandle::from_bits(1 << 32).expect("generation 1");
        let view = crcbl_hal::ImageViewHandle::from_bits(1 << 32).expect("generation 1");
        let sampler = crcbl_hal::SamplerHandle::from_bits(1 << 32).expect("generation 1");
        let binding_resource = [
            BindingResource::whole_buffer(buffer),
            BindingResource::ImageView(view),
            BindingResource::Sampler(sampler),
        ];
        let codes: Vec<u8> = binding_resource
            .iter()
            .map(|r| binding_resource_code(*r))
            .collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two BindingResources share a code"
        );
        assert_eq!(
            codes.iter().copied().max().map(|top| top + 1),
            Some(BINDING_RESOURCE_CODES),
            "BINDING_RESOURCE_CODES is not one past the last claimed code, so the \
             reader's refusing arm is checked against the wrong byte"
        );
        // The buffer's offset and size do not change the code, which is what makes
        // the byte a *variant* rather than a summary of its fields.
        assert_eq!(
            binding_resource_code(BindingResource::Buffer {
                buffer,
                offset: 42,
                size: 99,
            }),
            BINDING_RESOURCE_BUFFER
        );

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

        let resource_state = [
            ResourceState::Undefined,
            ResourceState::ShaderRead,
            ResourceState::ShaderWrite,
            ResourceState::ShaderReadWrite,
            ResourceState::ColorAttachment,
            ResourceState::DepthStencilWrite,
            ResourceState::DepthStencilRead,
            ResourceState::TransferSrc,
            ResourceState::TransferDst,
            ResourceState::IndirectArgument,
            ResourceState::IndexBuffer,
            ResourceState::HostRead,
            ResourceState::Present,
        ];
        let codes: Vec<u8> = resource_state
            .iter()
            .map(|s| resource_state_code(*s))
            .collect();
        assert_eq!(
            distinct(&codes),
            codes.len(),
            "two ResourceStates share a code"
        );
        for state in resource_state {
            assert_eq!(
                resource_state_from_code(resource_state_code(state)),
                Some(state)
            );
        }

        // No distinctness assertion beside this one, unlike every table above:
        // `SurfaceCapsFailure` has a single variant, so a "no two share a code"
        // check over it would hold however the table were written and could
        // never go red. The round trip still can — and a variant added tomorrow
        // stops `surface_caps_failure_code` compiling, which is what brings
        // whoever adds it back to this line.
        let cause = SurfaceCapsFailure::Backend;
        assert_eq!(
            surface_caps_failure_from_code(surface_caps_failure_code(cause)),
            Some(cause)
        );
    }

    #[test]
    fn a_code_no_variant_claims_decodes_to_nothing() {
        assert_eq!(load_op_from_code(0xFF), None);
        assert_eq!(resource_state_from_code(0xFF), None);
        assert_eq!(store_op_from_code(0xFF), None);
        assert_eq!(memory_location_from_code(0xFF), None);
        assert_eq!(image_type_from_code(0xFF), None);
        assert_eq!(image_view_type_from_code(0xFF), None);
        assert_eq!(filter_mode_from_code(0xFF), None);
        assert_eq!(sampler_address_mode_from_code(0xFF), None);
        assert_eq!(compare_op_from_code(0xFF), None);
        assert_eq!(primitive_topology_from_code(0xFF), None);
        assert_eq!(front_face_from_code(0xFF), None);
        assert_eq!(cull_mode_from_code(0xFF), None);
        assert_eq!(polygon_mode_from_code(0xFF), None);
        assert_eq!(stencil_op_from_code(0xFF), None);
        assert_eq!(blend_factor_from_code(0xFF), None);
        assert_eq!(blend_op_from_code(0xFF), None);
        assert_eq!(sample_type_from_code(0xFF), None);
        assert_eq!(device_type_from_code(0xFF), None);
        assert_eq!(format_from_code(0xFF), None);
        assert_eq!(present_mode_from_code(0xFF), None);
        assert_eq!(composite_alpha_from_code(0xFF), None);
        assert_eq!(surface_caps_failure_from_code(0xFF), None);
        // The one directly above the last claimed code, which is where an
        // off-by-one in either table lands and where `0xFF` never would.
        assert_eq!(image_type_from_code(IMAGE_TYPE_D3 + 1), None);
        assert_eq!(resource_state_from_code(RESOURCE_STATE_PRESENT + 1), None);
        assert_eq!(image_view_type_from_code(IMAGE_VIEW_TYPE_D3 + 1), None);
        assert_eq!(filter_mode_from_code(FILTER_MODE_LINEAR + 1), None);
        assert_eq!(
            sampler_address_mode_from_code(SAMPLER_ADDRESS_CLAMP_TO_BORDER + 1),
            None
        );
        assert_eq!(compare_op_from_code(COMPARE_OP_ALWAYS + 1), None);
        assert_eq!(
            primitive_topology_from_code(PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP + 1),
            None
        );
        assert_eq!(front_face_from_code(FRONT_FACE_CW + 1), None);
        assert_eq!(cull_mode_from_code(CULL_MODE_BACK + 1), None);
        assert_eq!(polygon_mode_from_code(POLYGON_MODE_LINE + 1), None);
        assert_eq!(stencil_op_from_code(STENCIL_OP_DECREMENT_WRAP + 1), None);
        assert_eq!(
            blend_factor_from_code(BLEND_FACTOR_ONE_MINUS_DST_ALPHA + 1),
            None
        );
        assert_eq!(blend_op_from_code(BLEND_OP_MAX + 1), None);
        assert_eq!(sample_type_from_code(SAMPLE_TYPE_DEPTH + 1), None);
        assert_eq!(device_type_from_code(DEVICE_TYPE_OTHER + 1), None);
        assert_eq!(format_from_code(FORMAT_BC7_RGBA_UNORM_SRGB + 1), None);
        assert_eq!(present_mode_from_code(PRESENT_MODE_IMMEDIATE + 1), None);
        assert_eq!(composite_alpha_from_code(COMPOSITE_ALPHA_INHERIT + 1), None);
        assert_eq!(
            surface_caps_failure_from_code(SURFACE_CAPS_FAILURE_BACKEND + 1),
            None
        );
    }

    fn distinct(codes: &[u8]) -> usize {
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    }
}
