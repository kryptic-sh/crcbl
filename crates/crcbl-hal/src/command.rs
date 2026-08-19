//! Command recording: passes, barriers, copies, draws, dispatches, queries.
//!
//! # Flat scopes, not nested encoders
//!
//! [`CommandEncoder::begin_render_pass`] / [`end_render_pass`](CommandEncoder::end_render_pass)
//! are flat calls on one encoder rather than a `begin` that returns a borrowed
//! `RenderPassEncoder` sub-object. The sub-object form is more type-safe and is
//! what `wgpu` does, but it does not survive `dyn`: returning
//! `Box<dyn RenderPassEncoder + '_>` costs an allocation per pass, and the
//! borrow relationship is awkward to express through a trait object.
//!
//! The safety this gives up is bought back elsewhere. The *only* caller of this
//! trait is `crcbl-render`'s render graph, which emits passes from a compiled
//! list and cannot mis-nest them by construction; the backends' validation
//! layers catch it if it somehow does; and [`crate::null`] tracks scope
//! explicitly and records a validation error, so the graph's own unit suite
//! fails loudly on a mis-nested pass without a GPU in the room.
//!
//! # Barriers: resource states, not stage/access masks
//!
//! [`ResourceState`] is a small closed set of "what is this resource being used
//! for right now" values. The backend expands one into a Vulkan `sync2`
//! stage+access+layout triple, a DX12 enhanced-barrier state, a Metal fence, or
//! nothing at all on WebGPU.
//!
//! Exposing raw stage/access masks was rejected: Metal cannot represent them,
//! WebGPU has no barriers to give them to, and the render graph does not have
//! finer information than the state anyway — a pass declares "I sample this",
//! not "I sample this in the fragment stage with `SHADER_STORAGE_READ`". The
//! cost is some over-synchronisation on Vulkan where a state maps to a broader
//! stage mask than strictly needed. That is the *known* place this seam gives up
//! performance, and refining it is a P1-or-later change confined to
//! [`ResourceState`] and its backend translation.
//!
//! # Indirect is the steady state
//!
//! [`CommandEncoder::draw_indexed_indirect_count`] is the normal path (topic 03:
//! a compute pass writes the arguments and the count, the CPU never learns how
//! many draws happen). [`CommandEncoder::draw`] exists for full-screen triangles
//! and bring-up. A device with no count buffer uses
//! [`CommandEncoder::draw_indexed_indirect`] once per bucket — hence
//! [`DrawIndirect::draw_count`], so even that is one call per bucket rather than
//! one per draw.

use core::ops::Range;

use crcbl_core::Handle;

use crate::{
    BindGroupHandle, BufferHandle, ComputePipelineHandle, Extent3d, GraphicsPipelineHandle,
    HalError, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageViewHandle,
    IndexFormat, Offset3d, PipelineLayoutHandle, QuerySetHandle, Rect2d, ShaderStages, Viewport,
    device::QueueHandle,
};

/// Marker type for command-buffer handles. Uninhabited.
#[derive(Debug)]
pub enum CommandBuffer {}

/// A finished command buffer, ready to submit.
pub type CommandBufferHandle = Handle<CommandBuffer>;

/// Reversed-Z depth constants.
///
/// The engine's depth convention is **1.0 at the near plane, 0.0 at infinity,
/// with an infinite far plane** (`docs/plan/02-vulkan-backend.md`, locked). A
/// sector-tiled world with 300 m+ sightlines z-fights immediately on a
/// conventional `0..1` buffer, and retrofitting the convention after P1
/// invalidates every blessed golden frame.
///
/// These constants exist so no call site writes a bare `1.0` whose meaning
/// depends on a convention documented elsewhere.
///
/// ```
/// use crcbl_hal::{ClearValue, CompareOp, DepthStencilState, depth};
///
/// // Clear to the far plane, test with Greater: nearer fragments win.
/// assert_eq!(depth::CLEAR, 0.0);
/// assert_eq!(DepthStencilState::default().depth_compare, CompareOp::Greater);
/// assert_eq!(ClearValue::default().depth, depth::CLEAR);
/// ```
pub mod depth {
    /// Depth value at the near plane: **1.0**.
    pub const NEAR: f32 = 1.0;

    /// Depth value at infinity: **0.0**. The far plane is infinite, so this is
    /// a limit rather than a distance.
    pub const FAR: f32 = 0.0;

    /// The value a depth attachment is cleared to — [`FAR`], so that any real
    /// geometry passes a [`CompareOp::Greater`](crate::CompareOp) test.
    ///
    /// Clearing to `1.0` (the conventional-Z value) with the engine's default
    /// compare op renders nothing at all, which is a confusing enough symptom to
    /// be worth a named constant.
    pub const CLEAR: f32 = FAR;
}

/// Stencil constants the seam guarantees rather than inherits.
///
/// ```
/// use crcbl_hal::stencil;
///
/// assert_eq!(stencil::INITIAL_REFERENCE, 0);
/// ```
pub mod stencil {
    /// The reference value a render pass starts with, before any
    /// [`set_stencil_reference`](crate::CommandEncoder::set_stencil_reference).
    ///
    /// **Zero, and stated here because it is a promise rather than an
    /// observation.** WebGPU and Metal both start an encoder at zero on their
    /// own; D3D12's `OMSetStencilRef` is command-list state that outlives a
    /// pass, and Vulkan's reference is *undefined* until
    /// `vkCmdSetStencilReference` is called, since `crcbl-vk` declares
    /// `VK_DYNAMIC_STATE_STENCIL_REFERENCE` on every pipeline. Those two
    /// backends set this value as their passes open so that all four agree.
    pub const INITIAL_REFERENCE: u32 = 0;
}

/// What to do with an attachment's existing contents at the start of a pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LoadOp {
    /// Preserve them. The most expensive option on tile-based GPUs; use only
    /// when the pass genuinely reads what was there.
    Load,
    /// Replace them with the attachment's clear value.
    Clear,
    /// Leave them undefined. Correct whenever the pass writes every pixel — and
    /// the right default for a swapchain image, whose previous contents are
    /// never meaningful.
    #[default]
    DontCare,
}

/// What to do with an attachment's contents at the end of a pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StoreOp {
    /// Write them to memory.
    #[default]
    Store,
    /// Discard them. For depth buffers nothing reads afterwards, and for MSAA
    /// targets that were resolved.
    Discard,
}

/// Clear values for a render pass attachment.
///
/// One struct rather than a colour/depth union: an attachment is colour *or*
/// depth-stencil, and its own descriptor already says which, so a union would
/// only add a mismatch to check.
///
/// [`Default`] clears to transparent black and to [`depth::CLEAR`] — the
/// reversed-Z far plane, **not** `1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClearValue {
    /// RGBA clear colour, in the attachment's own space (linear for
    /// [`Rgba16Float`](crate::Format::Rgba16Float), display-referred for an
    /// sRGB swapchain).
    pub color: [f32; 4],
    /// Depth clear value. Defaults to [`depth::CLEAR`].
    pub depth: f32,
    /// Stencil clear value.
    pub stencil: u32,
}

impl Default for ClearValue {
    fn default() -> Self {
        Self {
            color: [0.0; 4],
            depth: depth::CLEAR,
            stencil: 0,
        }
    }
}

impl ClearValue {
    /// A colour clear, with the reversed-Z depth default left in place.
    #[must_use]
    pub const fn color(rgba: [f32; 4]) -> Self {
        Self {
            color: rgba,
            depth: depth::CLEAR,
            stencil: 0,
        }
    }
}

/// One colour attachment of a render pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorAttachment {
    /// View rendered into. Must already be in
    /// [`ResourceState::ColorAttachment`]; the HAL does not transition it.
    pub view: ImageViewHandle,
    /// MSAA resolve destination, if any.
    ///
    /// Like [`view`](Self::view), and for the same reason it says so: this must
    /// already be in [`ResourceState::ColorAttachment`]. Every backend reads it
    /// that way — `crcbl-vk` names `COLOR_ATTACHMENT_OPTIMAL` as the resolve
    /// image's layout and `crcbl-dx12` transitions out of `RENDER_TARGET` and
    /// back — so the seam should say it rather than leave three backends
    /// agreeing by coincidence..
    pub resolve: Option<ImageViewHandle>,
    /// Start-of-pass behaviour.
    pub load: LoadOp,
    /// End-of-pass behaviour.
    pub store: StoreOp,
    /// Used when `load` is [`LoadOp::Clear`].
    pub clear: ClearValue,
}

/// The depth/stencil attachment of a render pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthStencilAttachment {
    /// View rendered into. Must already be in
    /// [`ResourceState::DepthStencilWrite`] or
    /// [`ResourceState::DepthStencilRead`] — see
    /// [`read_only`](Self::read_only), which is how the backend is told which.
    pub view: ImageViewHandle,
    /// Whether the pass only *tests* depth/stencil — the depth-prepass-read
    /// shape.
    ///
    /// **Which of the two states the view is already in**, and the backend has
    /// no other way to know. Vulkan and DX12 both give a read-only depth
    /// attachment a different image layout from a written one
    /// (`DEPTH_STENCIL_READ_ONLY_OPTIMAL` vs `DEPTH_STENCIL_ATTACHMENT_OPTIMAL`),
    /// and `vkCmdBeginRendering` requires the layout named here to be the one
    /// the image is actually in. Guessing from the store op is not equivalent:
    /// a pass may read depth and store nothing, or write depth and discard it.
    ///
    /// `true` pairs with [`ResourceState::DepthStencilRead`], `false` with
    /// [`ResourceState::DepthStencilWrite`].
    pub read_only: bool,
    /// Depth start-of-pass behaviour.
    pub depth_load: LoadOp,
    /// Depth end-of-pass behaviour.
    pub depth_store: StoreOp,
    /// Stencil start-of-pass behaviour.
    pub stencil_load: LoadOp,
    /// Stencil end-of-pass behaviour.
    pub stencil_store: StoreOp,
    /// Used when either load op is [`LoadOp::Clear`]. Defaults to
    /// [`depth::CLEAR`] — see [`ClearValue`].
    pub clear: ClearValue,
}

/// The two timestamps that bracket a pass — **the seam's only timestamp verb**.
///
/// A timestamp is named by the pass it brackets rather than by a point in the
/// command stream, because a point in the command stream is not something every
/// API can express. Vulkan's `vkCmdWriteTimestamp2` and D3D12's `EndQuery` on a
/// timestamp heap take one anywhere; Metal samples only where a device's
/// `MTLCounterSamplingPoint` values allow, which is stage boundaries; and
/// WebGPU takes them *only* through `GPURenderPassDescriptor.timestampWrites`
/// and `GPUComputePassDescriptor.timestampWrites`, whose
/// `beginningOfPassWriteIndex`/`endOfPassWriteIndex` this struct is. Half the
/// backends cannot honour an arbitrary point at all, so the pass boundary is the
/// intersection — and putting it in the descriptor makes it a *shape* rather
/// than a rule a caller has to remember, because there is no free-standing call
/// left to record in the wrong place.
///
/// Consequently a pass is the smallest thing this seam can time. A copy or a
/// barrier is not inside one, so what a profiler attributes to a pass is the
/// pass alone, and the transitions the caller emitted around it are attributed
/// to nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassTimestampWrites {
    /// Set the two queries are written into. Must hold
    /// [`QueryKind::Timestamp`](crate::QueryKind::Timestamp) queries; a backend
    /// fails the encoder rather than writing a timestamp into a pool of another
    /// kind.
    pub set: QuerySetHandle,
    /// Query written when the pass begins.
    pub beginning_of_pass: u32,
    /// Query written when the pass ends.
    ///
    /// **Distinct from [`beginning_of_pass`](Self::beginning_of_pass)**: WebGPU
    /// requires the two indices to differ, and a pass that wrote both ends into
    /// one query would report the second over the first on every other backend,
    /// so a backend refuses the pair rather than measuring nothing.
    pub end_of_pass: u32,
}

/// A render pass, in dynamic-rendering form: attachments are named inline and
/// there is no `RenderPass` or `Framebuffer` object to keep compatible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPassDesc<'a> {
    /// Pass name. Shows up in RenderDoc/PIX capture trees, in the render
    /// graph's text dump, and in the null backend's recorded stream — which is
    /// what makes "this frame emitted these passes in this order" assertable.
    pub label: Option<&'a str>,
    /// Colour attachments, in shader output order.
    pub color_attachments: &'a [ColorAttachment],
    /// Depth/stencil attachment, if any.
    pub depth_stencil_attachment: Option<DepthStencilAttachment>,
    /// Region rendered. Usually the full attachment size.
    pub render_area: Rect2d,
    /// Timestamps bracketing this pass, if it is being timed. See
    /// [`PassTimestampWrites`].
    pub timestamp_writes: Option<PassTimestampWrites>,
}

/// A compute pass.
///
/// Compute has no attachments, so this exists purely to give the pass a name
/// and a timestamp boundary — which is exactly what the per-pass GPU timer
/// report needs (topic 03 §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputePassDesc<'a> {
    /// Pass name; see [`RenderPassDesc::label`].
    pub label: Option<&'a str>,
    /// Timestamps bracketing this pass; see
    /// [`RenderPassDesc::timestamp_writes`].
    pub timestamp_writes: Option<PassTimestampWrites>,
}

// --- barriers --------------------------------------------------------------

/// What a resource is being used for.
///
/// The seam's entire synchronisation vocabulary. A backend expands one of these
/// into whatever its API needs; see the module docs for why raw stage/access
/// masks are not exposed.
///
/// States are *uses*, not permissions: a resource is in exactly one state at a
/// time from the graph's point of view, and moving between two states is a
/// barrier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ResourceState {
    /// Contents are undefined. The only legal *source* state for a freshly
    /// created image, and for a swapchain image just acquired. Transitioning
    /// *from* `Undefined` discards contents, which is free; transitioning *to*
    /// it is meaningless.
    #[default]
    Undefined,
    /// Read as a sampled texture or a read-only storage buffer/image in any
    /// shader stage.
    ShaderRead,
    /// Written by a shader (storage buffer or storage image).
    ShaderWrite,
    /// Both read and written by a shader in the same pass.
    ShaderReadWrite,
    /// A colour attachment being rendered into.
    ColorAttachment,
    /// A depth/stencil attachment being written.
    DepthStencilWrite,
    /// A depth/stencil attachment being tested but not written — the state for
    /// a pass drawing against an existing depth prepass.
    DepthStencilRead,
    /// Source of a copy.
    TransferSrc,
    /// Destination of a copy, clear or fill.
    TransferDst,
    /// Read as indirect draw/dispatch arguments or an indirect count. The state
    /// a culling pass's output must reach before the draws that consume it —
    /// the single most important barrier in a GPU-driven frame, and the one
    /// whose absence produces "sometimes nothing draws".
    IndirectArgument,
    /// Read as an index buffer.
    IndexBuffer,
    /// Mapped and read by the CPU.
    HostRead,
    /// Ready to be handed to the compositor.
    Present,
}

impl ResourceState {
    /// Whether this state writes the resource.
    ///
    /// Read→read transitions need no barrier at all, so a graph asks this to
    /// skip emitting one.
    #[must_use]
    pub const fn is_write(self) -> bool {
        matches!(
            self,
            Self::ShaderWrite
                | Self::ShaderReadWrite
                | Self::ColorAttachment
                | Self::DepthStencilWrite
                | Self::TransferDst
        )
    }

    /// Whether a transition between two states requires a barrier.
    ///
    /// `false` only when both states are reads *and* they are the same state —
    /// two different read states still differ in image layout on Vulkan and
    /// DX12, so they need a transition even though no memory hazard exists.
    #[must_use]
    pub const fn needs_barrier(from: Self, to: Self) -> bool {
        if from.is_write() || to.is_write() {
            return true;
        }
        // Both reads: only a layout change remains.
        from as u8 != to as u8
    }
}

/// Queue-family ownership transfer attached to a barrier.
///
/// MVP uploads share the graphics+compute queue, so this is `None` everywhere
/// today. It exists from P0 because `docs/plan/02-vulkan-backend.md` requires
/// that "the render graph's barrier model must nonetheless represent
/// queue-family acquire/release from the start so a dedicated transfer queue is
/// additive later rather than a barrier-model rewrite".
///
/// A transfer is expressed as **two** barriers with identical fields: a release
/// recorded on `from`'s queue and an acquire recorded on `to`'s. The HAL relays
/// both; deciding to emit them is the graph's job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueueTransfer {
    /// Queue releasing ownership.
    pub from: QueueHandle,
    /// Queue acquiring ownership.
    pub to: QueueHandle,
}

/// A buffer state transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferBarrier {
    /// Buffer affected.
    pub buffer: BufferHandle,
    /// State being left.
    pub from: ResourceState,
    /// State being entered.
    pub to: ResourceState,
    /// Ownership transfer, if any.
    pub queue_transfer: Option<QueueTransfer>,
}

impl BufferBarrier {
    /// A same-queue transition.
    #[must_use]
    pub const fn new(buffer: BufferHandle, from: ResourceState, to: ResourceState) -> Self {
        Self {
            buffer,
            from,
            to,
            queue_transfer: None,
        }
    }
}

/// An image state (and, on Vulkan/DX12, layout) transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageBarrier {
    /// Image affected.
    pub image: ImageHandle,
    /// Subrange affected.
    pub range: ImageSubresourceRange,
    /// State being left. [`ResourceState::Undefined`] discards contents.
    pub from: ResourceState,
    /// State being entered.
    pub to: ResourceState,
    /// Ownership transfer, if any.
    pub queue_transfer: Option<QueueTransfer>,
}

impl ImageBarrier {
    /// A same-queue transition of a whole image.
    #[must_use]
    pub const fn new(
        image: ImageHandle,
        range: ImageSubresourceRange,
        from: ResourceState,
        to: ResourceState,
    ) -> Self {
        Self {
            image,
            range,
            from,
            to,
            queue_transfer: None,
        }
    }
}

/// A batch of barriers recorded as one call.
///
/// Batched rather than one-at-a-time because every backend pays a fixed cost per
/// barrier *call*, and a graph that computed six transitions between two passes
/// should emit one call, not six. An empty `Barriers` is legal and does nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Barriers<'a> {
    /// Buffer transitions.
    pub buffers: &'a [BufferBarrier],
    /// Image transitions.
    pub images: &'a [ImageBarrier],
    /// Global execution+memory barrier with no specific resource, for the cases
    /// where a graph knows a dependency exists but not which resource carries
    /// it. Prefer the typed lists; this is the sledgehammer.
    pub global: bool,
}

// --- copies ----------------------------------------------------------------

/// A buffer-to-buffer copy region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferCopy {
    /// Source buffer.
    pub src: BufferHandle,
    /// Byte offset in the source.
    pub src_offset: u64,
    /// Destination buffer.
    pub dst: BufferHandle,
    /// Byte offset in the destination.
    pub dst_offset: u64,
    /// Bytes copied.
    pub size: u64,
}

/// A buffer↔image copy region.
///
/// One struct for both directions; which is the source is determined by the
/// method called, not by a field, so the two can never disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferImageCopy {
    /// Buffer side.
    pub buffer: BufferHandle,
    /// Byte offset in the buffer.
    pub buffer_offset: u64,
    /// Texels per row in the buffer; `0` means tightly packed. Must satisfy the
    /// backend's row-pitch alignment when non-zero.
    pub buffer_row_length: u32,
    /// Rows per layer in the buffer; `0` means tightly packed.
    pub buffer_image_height: u32,
    /// Image side.
    pub image: ImageHandle,
    /// Mip level and layers touched.
    pub image_subresource: ImageSubresourceLayers,
    /// Texel offset within the image.
    pub image_offset: Offset3d,
    /// Region size in texels.
    pub image_extent: Extent3d,
}

/// An image-to-image copy region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageCopy {
    /// Source image.
    pub src: ImageHandle,
    /// Source mip level and layers.
    pub src_subresource: ImageSubresourceLayers,
    /// Source texel offset.
    pub src_offset: Offset3d,
    /// Destination image.
    pub dst: ImageHandle,
    /// Destination mip level and layers.
    pub dst_subresource: ImageSubresourceLayers,
    /// Destination texel offset.
    pub dst_offset: Offset3d,
    /// Region size in texels.
    pub extent: Extent3d,
}

// --- indirect --------------------------------------------------------------

/// Arguments for an indirect draw whose *count* is known on the CPU.
///
/// The argument buffer's contents are still GPU-written; only how many argument
/// structures to read is fixed. This is
/// [`GeometryPath::IndirectPerBatch`](crate::GeometryPath::IndirectPerBatch)'s
/// draw path: one call per bucket with `draw_count` set to the bucket's
/// capacity.
///
/// `draw_count > 1` requires
/// [`Features::MULTI_DRAW_INDIRECT`](crate::Features::MULTI_DRAW_INDIRECT).
///
/// **Which structure the words are is the call's, not this type's.**
/// [`draw_indirect`](CommandEncoder::draw_indirect) reads four words,
/// [`draw_indexed_indirect`](CommandEncoder::draw_indexed_indirect) five and
/// [`draw_mesh_tasks_indirect`](CommandEncoder::draw_mesh_tasks_indirect)
/// three; each method's docs name the API structure it is. `stride` is what
/// keeps them apart in a buffer, so it is spelled at the call site rather than
/// derived here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawIndirect {
    /// Buffer holding the argument structures. Must be in
    /// [`ResourceState::IndirectArgument`].
    pub args: BufferHandle,
    /// Byte offset of the first argument structure.
    pub offset: u64,
    /// Number of argument structures to read.
    pub draw_count: u32,
    /// Bytes between consecutive argument structures.
    pub stride: u32,
}

/// Arguments for an indirect draw whose count is *also* read from GPU memory.
///
/// [`GeometryPath::IndirectCount`](crate::GeometryPath::IndirectCount)'s
/// steady-state draw call: a compute pass writes both the arguments and the
/// count, and the CPU never learns how many draws happen. Requires
/// [`Features::DRAW_INDIRECT_COUNT`](crate::Features::DRAW_INDIRECT_COUNT).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawIndirectCount {
    /// Buffer holding the argument structures.
    pub args: BufferHandle,
    /// Byte offset of the first argument structure.
    pub args_offset: u64,
    /// Buffer holding a `u32` draw count.
    pub count_buffer: BufferHandle,
    /// Byte offset of the count.
    pub count_offset: u64,
    /// Upper bound on draws, regardless of what the count buffer says. Bounded
    /// by [`Limits::max_draw_indirect_count`](crate::Limits::max_draw_indirect_count).
    pub max_draw_count: u32,
    /// Bytes between consecutive argument structures.
    pub stride: u32,
}

// --- the encoder -----------------------------------------------------------

/// Records commands into a command buffer.
///
/// Obtained from [`Device::create_command_encoder`](crate::Device::create_command_encoder)
/// and consumed by [`finish`](CommandEncoder::finish). One encoder produces one
/// command buffer.
///
/// # Scope rules
///
/// * Draw commands, [`set_viewport`](CommandEncoder::set_viewport),
///   [`set_scissor`](CommandEncoder::set_scissor) and
///   [`bind_graphics_pipeline`](CommandEncoder::bind_graphics_pipeline) are legal
///   only between `begin_render_pass` and `end_render_pass`.
/// * [`dispatch`](CommandEncoder::dispatch) and
///   [`bind_compute_pipeline`](CommandEncoder::bind_compute_pipeline) are legal
///   only inside a compute pass.
/// * Copies, barriers and query resets/resolves are legal only **outside** any
///   pass — Vulkan forbids them inside dynamic rendering, and the render graph
///   places them between passes anyway. Timestamps are the exception that
///   proves the rule: they are not a call at all, but
///   [`PassTimestampWrites`] on the pass descriptor.
/// * Passes do not nest.
///
/// A backend may assume these hold. [`crate::null`] checks them and records a
/// validation error, which is how the graph's unit suite catches a violation
/// without a GPU.
///
/// # Thread safety
///
/// `Send + Sync` on native, through
/// [`HalThreadSafe`](crate::threading::HalThreadSafe) — an encoder may be moved
/// to a worker thread and recorded there, which the P8 job system will do.
///
/// Vulkan's external-synchronisation rule for command buffers ("two threads may
/// not record into one") is discharged by `&mut self` on every recording
/// method, not by dropping `Sync`: two threads cannot hold `&mut` to the same
/// encoder, and `&self` alone can record nothing. An earlier version of this
/// doc claimed `Send` but not `Sync`, which contradicted the trait's own bound
/// — every implementor has always had to be `Sync`.
///
/// On `wasm32` the bound is vacuous, because wgpu's web types are `!Send` and
/// the browser is single-threaded.
pub trait CommandEncoder: core::fmt::Debug + crate::threading::HalThreadSafe {
    // --- debug ---

    /// Opens a named region in capture tools and profilers.
    ///
    /// Accepted and dropped when
    /// [`Features::DEBUG_MARKERS`](crate::Features::DEBUG_MARKERS) is absent —
    /// debug instrumentation degrades, it does not break the build (topic 10's
    /// stated policy).
    fn begin_debug_label(&mut self, label: &str);

    /// Closes the most recent [`begin_debug_label`](CommandEncoder::begin_debug_label).
    fn end_debug_label(&mut self);

    /// Inserts a zero-width marker.
    fn insert_debug_marker(&mut self, label: &str);

    // --- sync ---

    /// Records a batch of state transitions.
    ///
    /// The only way to synchronise inside a command buffer. See the module docs
    /// for the division of labour: this seam expresses barriers, the render
    /// graph decides where they go.
    fn pipeline_barrier(&mut self, barriers: &Barriers<'_>);

    // --- copies ---

    /// Buffer-to-buffer copy.
    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy);

    /// Buffer-to-image copy — the texture upload path.
    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy);

    /// Image-to-buffer copy — the readback path (`crcbl screenshot`, the
    /// delayed debug ring).
    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy);

    /// Image-to-image copy.
    fn copy_image_to_image(&mut self, copy: &ImageCopy);

    /// Fills a buffer range with a repeating 32-bit value.
    ///
    /// The idiomatic way to zero an indirect count buffer at the start of a
    /// frame, which is otherwise a whole compute dispatch for four bytes.
    fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32);

    // --- render scope ---

    /// Opens a render pass. Attachments must already be in the right state.
    fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>);

    /// Closes the current render pass.
    fn end_render_pass(&mut self);

    /// Sets the viewport transform. See [`Viewport`] on the depth range and
    /// reversed-Z.
    fn set_viewport(&mut self, viewport: &Viewport);

    /// Sets the scissor rectangle.
    fn set_scissor(&mut self, rect: &Rect2d);

    /// Sets the stencil reference value for subsequent draws — the value each
    /// [`StencilFaceState::compare`](crate::StencilFaceState::compare) tests
    /// against and [`StencilOp::Replace`](crate::StencilOp::Replace) writes.
    ///
    /// **The only channel there is.** [`StencilState`](crate::StencilState)
    /// carries no reference of its own, because two of the APIs behind this
    /// seam have no pipeline field to put one in. The rule every backend
    /// implements, and which
    /// [`Capability::StencilReference`](crate::Capability::StencilReference)
    /// stands for:
    ///
    /// * The reference is **pass state**: a pass opens holding
    ///   [`stencil::INITIAL_REFERENCE`] and
    ///   keeps whatever this call last set until it ends.
    /// * **Binding a graphics pipeline does not disturb it.** A call made
    ///   before a bind is still in force for the draws after it.
    fn set_stencil_reference(&mut self, reference: u32);

    /// Binds a graphics pipeline.
    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle);

    /// Binds an index buffer.
    ///
    /// There is no `bind_vertex_buffer`: geometry is pulled from storage
    /// buffers. See [`crate::pipeline`].
    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat);

    // --- bindings, shared by both scopes ---

    /// Binds a bind group at `slot`, with dynamic offsets for any
    /// dynamic-offset bindings in its layout, in binding order.
    ///
    /// `dynamic_offsets` is the substitute for push constants on a device
    /// without them — see
    /// [`BindingKind::UniformBuffer`](crate::BindingKind::UniformBuffer).
    fn bind_group(
        &mut self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    );

    /// Writes push constants.
    ///
    /// Requires [`Features::PUSH_CONSTANTS`](crate::Features::PUSH_CONSTANTS).
    /// A backend without it must fail loudly at pipeline-layout creation rather
    /// than silently dropping the write here.
    fn push_constants(
        &mut self,
        stages: ShaderStages,
        offset: u32,
        data: &[u8],
        layout: PipelineLayoutHandle,
    );

    // --- draws ---

    /// Non-indexed draw. For full-screen triangles and bring-up; the geometry
    /// path is indirect.
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>);

    /// Indexed draw.
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>);

    /// Indirect non-indexed draw with a CPU-known draw count.
    fn draw_indirect(&mut self, draw: &DrawIndirect);

    /// Indirect indexed draw with a CPU-known draw count.
    /// **[`GeometryPath::IndirectPerBatch`](crate::GeometryPath::IndirectPerBatch)'s
    /// draw path**, one call per bucket.
    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect);

    /// Indirect non-indexed draw with a GPU-read draw count.
    fn draw_indirect_count(&mut self, draw: &DrawIndirectCount);

    /// Indirect indexed draw with a GPU-read draw count.
    /// **[`GeometryPath::IndirectCount`](crate::GeometryPath::IndirectCount)'s
    /// steady-state draw call** — one per pass, regardless of scene size.
    fn draw_indexed_indirect_count(&mut self, draw: &DrawIndirectCount);

    /// Launches a mesh-shading workload, in workgroups —
    /// **[`GeometryPath::MeshShader`](crate::GeometryPath::MeshShader)'s draw
    /// call**.
    ///
    /// `x`, `y` and `z` count workgroups of the *first* stage the bound
    /// pipeline has: the task stage when
    /// [`MeshPipelineDesc::task`](crate::MeshPipelineDesc::task) was `Some`,
    /// and the mesh stage otherwise. That is Vulkan's rule for
    /// `vkCmdDrawMeshTasksEXT`, D3D12's for `DispatchMesh`, and Metal's for
    /// `drawMeshThreadgroups:`, so it is the seam's rule too.
    ///
    /// This is a *draw*, not a dispatch: it belongs inside a render pass with a
    /// mesh pipeline bound, and its output is fragments rather than buffer
    /// writes. The three-dimensional count is why it does not reuse
    /// [`draw`](CommandEncoder::draw)'s vertex/instance ranges — there are no
    /// vertices to range over.
    ///
    /// Requires [`Features::MESH_SHADER`](crate::Features::MESH_SHADER), which
    /// [`create_mesh_pipeline`](crate::Device::create_mesh_pipeline) has
    /// already refused without.
    fn draw_mesh_tasks(&mut self, x: u32, y: u32, z: u32);

    /// [`draw_mesh_tasks`](CommandEncoder::draw_mesh_tasks) with the workgroup
    /// counts read from GPU memory — **the shape a culling pass's output needs
    /// to reduce work rather than only output**.
    ///
    /// # The argument layout
    ///
    /// One argument structure is **three consecutive 32-bit words**: the x, y
    /// and z workgroup counts, in that order, of the same stage
    /// [`draw_mesh_tasks`](CommandEncoder::draw_mesh_tasks) counts groups of.
    /// The layout is not this seam's to choose — it is
    /// `VkDrawMeshTasksIndirectCommandEXT`, `D3D12_DISPATCH_MESH_ARGUMENTS` and
    /// Metal's `MTLDispatchThreadgroupsIndirectArguments`, which are the same
    /// three words in the same order, because a driver reads these bytes
    /// directly.
    ///
    /// [`DrawIndirect::stride`] is therefore 12 for a tightly packed buffer, and
    /// [`DrawIndirect::offset`] must be a multiple of 4.
    /// [`draw_count`](DrawIndirect::draw_count) `> 1` requires
    /// [`Features::MULTI_DRAW_INDIRECT`](crate::Features::MULTI_DRAW_INDIRECT),
    /// exactly as it does for the indexed form.
    ///
    /// [`DrawIndirect::args`] must be in
    /// [`ResourceState::IndirectArgument`] — a buffer the same pass also reads
    /// as a storage buffer is in one state or the other, never both, so a
    /// caller that needs both wants two buffers.
    ///
    /// # Capability
    ///
    /// Requires [`Features::MESH_SHADER`](crate::Features::MESH_SHADER) and
    /// nothing more. There is no separate flag because none of the three APIs
    /// has one: `VK_EXT_mesh_shader` defines `vkCmdDrawMeshTasksIndirectEXT`
    /// alongside `vkCmdDrawMeshTasksEXT`, D3D12 mesh shader tier 1 admits
    /// `D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH_MESH` in a command signature, and
    /// a Metal device with mesh functions has
    /// `drawMeshThreadgroupsWithIndirectBuffer:`. A device that can launch this
    /// stage at all can launch it from memory.
    fn draw_mesh_tasks_indirect(&mut self, draw: &DrawIndirect);

    // --- compute scope ---

    /// Opens a compute pass.
    fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>);

    /// Closes the current compute pass.
    fn end_compute_pass(&mut self);

    /// Binds a compute pipeline.
    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle);

    /// Dispatches a compute workload, in workgroups.
    fn dispatch(&mut self, x: u32, y: u32, z: u32);

    /// Dispatches with the workgroup counts read from GPU memory — the shape a
    /// variable-size culling or compaction pass needs.
    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64);

    // --- queries ---

    /// Resets a query range so it can be written again.
    ///
    /// Required on Vulkan before every write; a no-op on backends that reset
    /// implicitly. Callers always call it, so the Vulkan path is never the
    /// special case.
    ///
    /// **What is written between a reset and a resolve is
    /// [`RenderPassDesc::timestamp_writes`]**, not a call of its own: see
    /// [`PassTimestampWrites`] for why the seam has no free-standing timestamp.
    fn reset_query_set(&mut self, set: QuerySetHandle, range: Range<u32>);

    /// Copies query results into a buffer, **as the device wrote them**.
    ///
    /// The counterpart of [`Device::query_results`](crate::Device::query_results)
    /// and the one place the two read paths differ: that one reports a
    /// [`QueryKind::Timestamp`](crate::QueryKind::Timestamp) in nanoseconds, and
    /// this one cannot. It records a GPU-side copy — `vkCmdCopyQueryPoolResults`,
    /// `ResolveQueryData`, `GPUCommandEncoder.resolveQuerySet` — so the values
    /// never pass through the CPU and there is nowhere to apply a scale. A
    /// caller reading the destination gets the device's own clock units, and
    /// what those are worth is a question only the backend can answer.
    ///
    /// So this is for feeding a delta back to the GPU — a shader that compares
    /// two readings, which needs no unit — and
    /// [`Device::query_results`](crate::Device::query_results) is for a number a
    /// human or a HUD is going to read.
    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        range: Range<u32>,
        dst: BufferHandle,
        dst_offset: u64,
    );

    // --- finish ---

    /// Ends recording and produces a submittable command buffer.
    ///
    /// Takes `Box<Self>` rather than `self` so the method stays object-safe:
    /// `Box<dyn CommandEncoder>` can call it, a bare `self` receiver could not.
    ///
    /// # Errors
    ///
    /// Returns [`HalError`] if the backend rejected the recorded stream — an
    /// unclosed pass, an out-of-range value, or an out-of-memory condition in
    /// the command pool.
    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError>;
}

/// Creation parameters for a command encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandEncoderDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Queue the resulting command buffer will be submitted to.
    ///
    /// Named at creation because Vulkan command pools are per-queue-family and
    /// DX12 command allocators are per-type; discovering it at submit time
    /// would force a copy.
    pub queue: QueueHandle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_defaults_to_the_reversed_z_far_plane() {
        let clear = ClearValue::default();
        assert_eq!(clear.depth, depth::CLEAR);
        assert_eq!(clear.depth, 0.0);
        assert_eq!(clear.color, [0.0; 4]);
        // A colour clear must not quietly reintroduce conventional-Z depth.
        assert_eq!(ClearValue::color([1.0, 0.0, 0.0, 1.0]).depth, depth::CLEAR);
    }

    #[test]
    fn depth_constants_bracket_the_range_in_reversed_order() {
        assert_eq!(depth::NEAR, 1.0);
        assert_eq!(depth::FAR, 0.0);
        // A const block: the relationship is a compile-time invariant of the
        // depth convention, not a runtime property.
        const {
            assert!(
                depth::NEAR > depth::FAR,
                "reversed-Z: the near plane holds the larger value"
            );
        }
    }

    #[test]
    fn attachment_load_defaults_to_dont_care() {
        // Preserving contents must be asked for: `Load` is the expensive option
        // on tile-based GPUs and is wrong for a swapchain image.
        assert_eq!(LoadOp::default(), LoadOp::DontCare);
        assert_eq!(StoreOp::default(), StoreOp::Store);
    }

    #[test]
    fn write_states_are_classified() {
        assert!(ResourceState::ColorAttachment.is_write());
        assert!(ResourceState::TransferDst.is_write());
        assert!(ResourceState::ShaderReadWrite.is_write());
        assert!(!ResourceState::ShaderRead.is_write());
        assert!(!ResourceState::IndirectArgument.is_write());
        assert!(!ResourceState::Undefined.is_write());
    }

    #[test]
    fn read_to_same_read_needs_no_barrier_but_read_to_other_read_does() {
        use ResourceState as S;
        assert!(!S::needs_barrier(S::ShaderRead, S::ShaderRead));
        // Different reads still differ in image layout on vk/dx12.
        assert!(S::needs_barrier(S::ShaderRead, S::IndirectArgument));
        assert!(S::needs_barrier(S::ShaderRead, S::TransferSrc));
        // Anything involving a write always needs one.
        assert!(S::needs_barrier(S::ShaderWrite, S::ShaderWrite));
        assert!(S::needs_barrier(S::ShaderWrite, S::IndirectArgument));
        assert!(S::needs_barrier(S::Undefined, S::ColorAttachment));
    }

    #[test]
    fn barriers_default_to_empty_and_non_global() {
        let barriers = Barriers::default();
        assert!(barriers.buffers.is_empty());
        assert!(barriers.images.is_empty());
        assert!(!barriers.global);
    }

    #[test]
    fn same_queue_barrier_constructors_leave_ownership_alone() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let buffer: BufferHandle = pool.insert(0).cast();
        let barrier = BufferBarrier::new(
            buffer,
            ResourceState::ShaderWrite,
            ResourceState::IndirectArgument,
        );
        assert!(barrier.queue_transfer.is_none());
        assert_eq!(barrier.to, ResourceState::IndirectArgument);
    }

    #[test]
    fn default_resource_state_is_undefined() {
        // A freshly created resource has no meaningful contents, and the graph's
        // first transition must therefore be free to discard them.
        assert_eq!(ResourceState::default(), ResourceState::Undefined);
    }
}
