//! One decoded command, with every borrowed field owned.
//!
//! The counterpart of [`crcbl_hal::null::Command`], and owned for the same
//! reason: the stream outlives the descriptors that produced it. Two things
//! differ from that type, both deliberate.
//!
//! * **Creation and destruction are commands here.** A stream cannot answer
//!   during the call, so the caller allocates the handle itself and the id
//!   travels with the descriptor — see [`crate::StreamWriter::create_buffer`].
//! * **[`Command::PushConstants`] carries the bytes.** The null backend keeps
//!   only their length, because a test asserting on push-constant contents is
//!   asserting on shader ABI. A replayer needs the bytes.
//!
//! Descriptors are flattened into named fields rather than nested, which is what
//! the null backend's `BeginRenderPass` does and for the same reason: the
//! descriptor's lifetime is gone by the time the command exists.

use core::ops::Range;

use crcbl_hal::{
    AdapterId, BindGroupHandle, BufferHandle, BufferUsage, ColorAttachment, DepthStencilAttachment,
    Features, GraphicsPipelineHandle, MemoryLocation, PipelineLayoutHandle, Rect2d, ShaderStages,
    SurfaceHandle,
};

/// A command decoded out of a stream buffer.
///
/// The variants are a representative subset — see the [crate docs](crate) for
/// which shapes they cover and why the rest are not here yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// [`Device::create_buffer`](crcbl_hal::Device::create_buffer), with the
    /// handle the caller allocated for the object JS is about to create.
    CreateBuffer {
        /// Id the replayer stores the new object at.
        buffer: BufferHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Size in bytes.
        size: u64,
        /// Permitted uses.
        usage: BufferUsage,
        /// Where the memory lives.
        memory: MemoryLocation,
    },
    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface), with
    /// the handle the caller allocated for it.
    ///
    /// **Only the canvas key crosses, not a [`SurfaceTarget`](crcbl_core::SurfaceTarget).**
    /// Four of that enum's six variants carry `NonNull` pointers to platform
    /// objects, and a pointer must never be transmitted as an integer. So the
    /// encoder takes the `u32` that `Web { canvas_id }` carries and the pointer
    /// variants have nothing here to be encoded into at all.
    ///
    /// **`Offscreen` is the other browser-reachable variant and it is not this
    /// command.** It has no canvas key, and a reserved id standing in for one
    /// would be a magic number two decoders had to agree on. It gets its own
    /// command when the parity gate needs to read frames back, because the
    /// replayer's two jobs genuinely differ: this one resolves a canvas out of
    /// the shim's registry and takes its `webgpu` context, and that one has no
    /// canvas to resolve and must allocate a ring of textures nothing presents.
    CreateSurface {
        /// Id the replayer stores the new object at.
        surface: SurfaceHandle,
        /// Registry key the shell's JS shim assigned the canvas — the whole of
        /// [`SurfaceTarget::Web`](crcbl_core::SurfaceTarget::Web), and a number
        /// rather than a name so no string crosses the boundary.
        canvas_id: u32,
    },
    /// [`Device::destroy_buffer`](crcbl_hal::Device::destroy_buffer).
    ///
    /// A destroy naming an id whose slot holds nothing is a **no-op for the
    /// replayer, not an error** — see the [crate docs](crate#destroying-what-was-never-created).
    DestroyBuffer {
        /// Id to release.
        buffer: BufferHandle,
    },
    /// [`Instance::destroy_surface`](crcbl_hal::Instance::destroy_surface).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is.
    DestroySurface {
        /// Id to release.
        surface: SurfaceHandle,
    },
    /// [`begin_debug_label`](crcbl_hal::CommandEncoder::begin_debug_label).
    BeginDebugLabel {
        /// Region name.
        label: String,
    },
    /// [`begin_render_pass`](crcbl_hal::CommandEncoder::begin_render_pass).
    BeginRenderPass {
        /// Pass label, if the caller gave one.
        label: Option<String>,
        /// Colour attachments, in shader output order.
        color_attachments: Vec<ColorAttachment>,
        /// Depth/stencil attachment.
        depth_stencil_attachment: Option<DepthStencilAttachment>,
        /// Region rendered.
        render_area: Rect2d,
    },
    /// [`bind_graphics_pipeline`](crcbl_hal::CommandEncoder::bind_graphics_pipeline).
    BindGraphicsPipeline {
        /// Pipeline bound.
        pipeline: GraphicsPipelineHandle,
    },
    /// [`bind_group`](crcbl_hal::CommandEncoder::bind_group).
    BindGroup {
        /// Set index.
        slot: u32,
        /// Group bound.
        group: BindGroupHandle,
        /// Dynamic offsets supplied, in binding order.
        dynamic_offsets: Vec<u32>,
        /// Layout the binding is against — the last parameter of the HAL call,
        /// and the one most easily dropped when writing an encoder by hand.
        layout: PipelineLayoutHandle,
    },
    /// [`push_constants`](crcbl_hal::CommandEncoder::push_constants).
    PushConstants {
        /// Stages written.
        stages: ShaderStages,
        /// Byte offset within the block.
        offset: u32,
        /// Bytes written.
        data: Vec<u8>,
        /// Layout the write is against; see [`Command::BindGroup::layout`].
        layout: PipelineLayoutHandle,
    },
    /// [`draw`](crcbl_hal::CommandEncoder::draw).
    Draw {
        /// Vertex range.
        vertices: Range<u32>,
        /// Instance range.
        instances: Range<u32>,
    },
    /// [`Instance::adapters`](crcbl_hal::Instance::adapters) — enumerate what
    /// the browser will grant.
    ///
    /// **The only command in this crate whose body is empty**, and the only one
    /// that is answered: the enumeration cannot be handed back during the call,
    /// so the replayer queues a [`Reply::Adapter`](crate::Reply::Adapter) or a
    /// [`Reply::NoAdapter`](crate::Reply::NoAdapter) naming this command's
    /// sequence, and it arrives a frame or more later. See
    /// [`crate::instance`] for the side that waits for it.
    EnumerateAdapters,
    /// [`Instance::request_device`](crcbl_hal::Instance::request_device) — open
    /// the adapter the enumeration granted.
    ///
    /// The whole of [`DeviceDesc`](crcbl_hal::DeviceDesc), flattened as every
    /// other descriptor here is. Answered by a [`Reply::Device`](crate::Reply::Device)
    /// or a [`Reply::DeviceFailed`](crate::Reply::DeviceFailed) naming this
    /// command's sequence; see [`crate::device`] for the side that waits.
    ///
    /// **The feature words cross as [`Features`] bits, not as WebGPU names.**
    /// The replayer owns that vocabulary in both directions — it is the half
    /// that faces WebGPU — so the wire speaks the seam's language here exactly
    /// as it does for load ops and handles.
    RequestDevice {
        /// Which adapter, as [`Instance::adapters`](crcbl_hal::Instance::adapters)
        /// numbered it. Always `0` from a browser: `requestAdapter()` grants one
        /// adapter or none.
        adapter: AdapterId,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Features the caller cannot run without. A bit with no WebGPU name is
        /// unsatisfiable and **fails the request**; it is never quietly dropped.
        required_features: Features,
        /// Features to enable if the adapter has them. Bits with no WebGPU name
        /// are simply not asked for, which is what optional means.
        optional_features: Features,
        /// A surface the device must be able to present to.
        compatible_surface: Option<SurfaceHandle>,
    },
    /// [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps) — what this
    /// surface will accept on this adapter.
    ///
    /// The third command whose answer comes back, and it is answered by a
    /// [`Reply::SurfaceCaps`](crate::Reply::SurfaceCaps) naming this command's
    /// sequence.
    ///
    /// **Two ids, and the pair the HAL call takes in this order.** They are
    /// different widths on the wire — a surface is a handle's eight bytes, an
    /// adapter id is a bare `u32` — so a decoder that read them the other way
    /// round does not merely swap two values, it runs off into the next
    /// command. That is the loud failure; the quiet one is a *second* decoder
    /// choosing the opposite order, which is why the corpus gives the two
    /// values nothing in common.
    SurfaceCaps {
        /// Which surface, as
        /// [`create_surface`](crate::StreamWriter::create_surface) named it.
        surface: SurfaceHandle,
        /// Which adapter, as [`Instance::adapters`](crcbl_hal::Instance::adapters)
        /// numbered it. Always `0` from a browser, for
        /// [`Command::RequestDevice::adapter`]'s reason.
        adapter: AdapterId,
    },
}

impl Command {
    /// A stable variant name.
    ///
    /// What a stream dump prints, and what lets a test assert the *shape* of a
    /// buffer without spelling out every handle and descriptor. Same role as
    /// [`crcbl_hal::null::Command::name`].
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::CreateBuffer { .. } => "CreateBuffer",
            Self::CreateSurface { .. } => "CreateSurface",
            Self::DestroyBuffer { .. } => "DestroyBuffer",
            Self::DestroySurface { .. } => "DestroySurface",
            Self::BeginDebugLabel { .. } => "BeginDebugLabel",
            Self::BeginRenderPass { .. } => "BeginRenderPass",
            Self::BindGraphicsPipeline { .. } => "BindGraphicsPipeline",
            Self::BindGroup { .. } => "BindGroup",
            Self::PushConstants { .. } => "PushConstants",
            Self::Draw { .. } => "Draw",
            Self::EnumerateAdapters => "EnumerateAdapters",
            Self::RequestDevice { .. } => "RequestDevice",
            Self::SurfaceCaps { .. } => "SurfaceCaps",
        }
    }
}
