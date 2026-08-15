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
    BindGroupHandle, BufferHandle, BufferUsage, ColorAttachment, DepthStencilAttachment,
    GraphicsPipelineHandle, MemoryLocation, PipelineLayoutHandle, Rect2d, ShaderStages,
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
    /// [`Device::destroy_buffer`](crcbl_hal::Device::destroy_buffer).
    ///
    /// A destroy naming an id whose slot holds nothing is a **no-op for the
    /// replayer, not an error** — see the [crate docs](crate#destroying-what-was-never-created).
    DestroyBuffer {
        /// Id to release.
        buffer: BufferHandle,
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
            Self::DestroyBuffer { .. } => "DestroyBuffer",
            Self::BeginDebugLabel { .. } => "BeginDebugLabel",
            Self::BeginRenderPass { .. } => "BeginRenderPass",
            Self::BindGraphicsPipeline { .. } => "BindGraphicsPipeline",
            Self::BindGroup { .. } => "BindGroup",
            Self::PushConstants { .. } => "PushConstants",
            Self::Draw { .. } => "Draw",
        }
    }
}
