//! [`MetalCommandEncoder`]: real recording into an `MTLCommandBuffer` — render
//! passes, compute passes, blit copies, and the encoder boundary a barrier
//! becomes.
//!
//! # A render pass, not a blit clear
//!
//! `docs/plan/09-backends-metal-dx12.md`'s bring-up ladder starts at "clear",
//! and the cheap way to satisfy it would be a blit that fills a texture.
//! [`begin_render_pass`](CommandEncoder::begin_render_pass) instead builds a
//! real `MTLRenderPassDescriptor` and opens a real `MTLRenderCommandEncoder`,
//! so the clear happens because the pass *loads that way* — which is what
//! exercises the attachment table, the load and store actions and the clear
//! value, all of which the triangle rung is then built on top of. A pass opened
//! and immediately closed is a clear, and that is this slice's deliverable.
//!
//! # One `MTLCommandBuffer` per encoder, taken at construction
//!
//! Metal has no record-then-submit split: an `MTLCommandBuffer` comes from the
//! queue already recording, and `commit` *is* the submission. So the encoder
//! takes its command buffer in
//! [`Device::create_command_encoder`](crcbl_hal::Device::create_command_encoder)
//! and [`finish`](CommandEncoder::finish) hands it to the device's table rather
//! than building anything. That has one visible consequence the seam does not
//! otherwise imply: **the command buffer exists from the moment the encoder
//! does**, so a queue with its uncommitted-buffer limit reached blocks in
//! `create_command_encoder` rather than at submit.
//!
//! # Only one Metal encoder may be open at a time
//!
//! Metal raises if a second encoder is created while one is still encoding, and
//! an Objective-C exception crossing into Rust aborts the process. So this type
//! owns exactly one [`Open`] encoder and closes it before opening another:
//! copies open a blit encoder and keep it, a render or compute pass closes the
//! blit encoder first, and [`finish`](CommandEncoder::finish) closes whatever is
//! left. A copy recorded *inside* a pass is refused with
//! [`HalError::InvalidDescriptor`] rather than being allowed to raise — the
//! seam already forbids it ("copies, barriers and query writes are legal only
//! outside any pass"), so this is a caller bug being caught, not a restriction
//! this backend invents.
//!
//! **A pass owns its encoder for exactly its own length.**
//! [`begin_compute_pass`](CommandEncoder::begin_compute_pass) creates one and
//! [`end_compute_pass`](CommandEncoder::end_compute_pass) closes it; a render
//! pass has the same *lifetime* and reaches it the other way round, which the
//! next section is about. Either way pipeline state, argument tables and the
//! seam's scope rules line up with Metal's, since every one of those dies with
//! `endEncoding`.
//!
//! # A render pass is recorded, then encoded at `end_render_pass`
//!
//! [`begin_render_pass`](CommandEncoder::begin_render_pass) builds the
//! `MTLRenderPassDescriptor` — which is where every attachment handle is
//! resolved and every bound checked, so **a bad descriptor still fails there** —
//! and then opens no encoder at all. Each command the pass records becomes a
//! [`RenderCommand`] in a list, and
//! [`end_render_pass`](CommandEncoder::end_render_pass) creates the
//! `MTLRenderCommandEncoder` and replays the list into it in order.
//!
//! **Every other check keeps its original timing too**, because a recorded
//! command carries only values that have already been validated: handles are
//! resolved, ranges are checked and refusals are recorded at the moment the
//! seam call arrives, exactly as they were when the same call encoded straight
//! through. What is deferred is the Objective-C message send and nothing else,
//! so the Metal calls a pass makes — and the order it makes them in — are the
//! ones a straight-through encoder would have made.
//!
//! The one thing that does move is the failure Metal reports by handing back a
//! nil encoder, which is a [`HalError::DeviceLost`] raised at
//! `end_render_pass` instead of at `begin_render_pass`. Both are before
//! [`finish`](CommandEncoder::finish), which is where the seam observes it.
//!
//! **Why defer at all**, since the straight-through form was simpler:
//! [`draw_indirect_count`](CommandEncoder::draw_indirect_count) takes its count
//! from GPU memory, and the only thing on this backend that can read one is a
//! *compute kernel* — which must run **before** the render encoder the seam
//! calls that verb inside was opened, because what it writes is what the pass
//! then draws from. A backend that has already opened the encoder cannot reach
//! backwards to do that; one that has only a list of commands can.
//!
//! That is what [`RenderRecording::prologue`] holds and what
//! [`encode_render_pass`](MetalCommandEncoder::encode_render_pass) encodes
//! first: a compute encoder, opened and ended, and only then the render
//! encoder. `crcbl_mtl::indirect_count` carries the design and the three
//! attempts at an indirect command buffer that lost to it.
//!
//! # A barrier is an encoder boundary
//!
//! See [`pipeline_barrier`](CommandEncoder::pipeline_barrier). Metal tracks
//! hazards between encoders for tracked resources, which is every resource this
//! backend allocates, so the seam's barrier does not translate into a *call* —
//! it translates into the encoder split that makes Metal's tracking apply.
//!
//! # Failures are recorded and reported at `finish`
//!
//! Every recording method returns `()`, so a bad handle or an
//! impossible-to-express region has nowhere to go until
//! [`finish`](CommandEncoder::finish). The first failure is kept and every
//! later command dropped, exactly as `crcbl-vk` does, because a command buffer
//! that submits with commands silently missing is far worse than one that
//! refuses to be built.

use core::ops::Range;
use core::ptr::NonNull;
use std::sync::Arc;

use crcbl_hal::{
    Barriers, BindGroupHandle, BufferCopy, BufferHandle, BufferImageCopy, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePassDesc, ComputePipelineHandle, DrawIndirect,
    DrawIndirectCount, Features, GraphicsPipelineHandle, HalError, ImageCopy,
    ImageSubresourceLayers, ImageType, IndexFormat, PassTimestampWrites, PipelineLayoutHandle,
    QuerySetHandle, Rect2d, RenderPassDesc, ShaderStages, Viewport,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSRange, NSString, NSUInteger};
use objc2_metal::{
    MTLBlitCommandEncoder,
    MTLBuffer,
    MTLCommandBuffer,
    MTLCommandEncoder as _,
    MTLComputeCommandEncoder,
    // `newBufferWithLength:options:` for the buffer a count-limited draw's
    // prologue packs into, which this module allocates because no seam call
    // ever names it.
    MTLDevice as _,
    MTLOrigin,
    MTLRenderCommandEncoder,
    MTLRenderPassDescriptor,
    // `setLabel:` on the buffer a count-limited draw's prologue packs into,
    // which is where a fault report reads a resource's name from.
    MTLResource as _,
    MTLScissorRect,
    MTLTexture,
    MTLViewport,
};

use crate::conv;
use crate::device::{CommandBufferEntry, DeviceInner, ResolvedImage, to_ns};

/// What a command buffer and its encoders are called when the seam gave no
/// label, and the name the copy encoder always carries.
///
/// **This is not cosmetic.** A GPU fault names the encoder that caused it by
/// its label — see `crcbl_mtl::fault` — and an encoder that was never labelled
/// reports an empty string, which turns a report naming the culprit back into a
/// list of blanks. [`RenderPassDesc::label`] is optional and the copy encoder is
/// this backend's own object that no seam call ever names, so these three are
/// the floor beneath every label a caller does supply.
const UNLABELLED_COMMAND_BUFFER: &str = "crcbl command buffer";
/// See [`UNLABELLED_COMMAND_BUFFER`].
const UNLABELLED_RENDER_PASS: &str = "crcbl render pass";
/// See [`UNLABELLED_COMMAND_BUFFER`].
const UNLABELLED_COMPUTE_PASS: &str = "crcbl compute pass";
/// See [`UNLABELLED_COMMAND_BUFFER`].
const COPY_ENCODER: &str = "crcbl copies";
/// The compute encoder a count-limited draw's prologue runs on. See
/// [`UNLABELLED_COMMAND_BUFFER`] for why this backend's own encoders are named
/// at all: a fault report identifies the culprit by its label, and this encoder
/// belongs to no pass the caller ever named.
const PACK_PASS: &str = "crcbl indirect-count args";
/// The buffer that prologue packs into. See [`PACK_PASS`].
const PACK_BUFFER: &str = "crcbl indirect-count packed args";

/// The bound index buffer as the three arguments Metal takes at an indexed
/// draw: the buffer, its index width, and the byte offset of index zero.
///
/// A named alias because it travels between three methods — resolved once by
/// [`MetalCommandEncoder::indirect_index`], carried through
/// [`MetalCommandEncoder::record_indirect_draws`], and recorded into every
/// [`RenderCommand::DrawIndexedIndirect`] the loop emits.
type IndexArguments = (
    Retained<ProtocolObject<dyn MTLBuffer>>,
    objc2_metal::MTLIndexType,
    u64,
);

/// The one Metal encoder that may be open on the command buffer — or, for a
/// render pass, the commands that will become one.
enum Open {
    /// Nothing is encoding; the command buffer will accept a new encoder.
    None,
    /// A blit encoder, kept open across consecutive copies.
    Blit(Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>),
    /// A render pass under record. No Metal encoder exists yet; see the module
    /// docs.
    Render(RenderRecording),
    /// A compute pass.
    Compute(Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>),
}

impl Open {
    /// The open encoder as the base protocol, for the calls they all share.
    ///
    /// `None` for a recording render pass as well as for nothing at all: there
    /// is no Metal object to send to yet, and a caller that has something to
    /// say to the pass records it instead. Both callers below check for
    /// [`Open::Render`] first, and this returning `None` for it is what keeps a
    /// missed check from quietly reaching the wrong encoder.
    fn as_encoder(&self) -> Option<&ProtocolObject<dyn objc2_metal::MTLCommandEncoder>> {
        match self {
            Self::None | Self::Render(_) => None,
            Self::Blit(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
            Self::Compute(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
        }
    }
}

/// Which pass a command with no bind point of its own belongs to.
///
/// See [`MetalCommandEncoder::pass_target`]. The render arm carries nothing
/// because a recording render pass has no Metal object yet; the compute arm
/// carries its encoder, which is what the call is made on.
enum Target {
    Render,
    Compute(Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>),
}

/// A render pass being recorded: everything `end_render_pass` needs to open an
/// `MTLRenderCommandEncoder` and drive it.
///
/// The descriptor is built — and every attachment handle resolved — when the
/// pass begins, so it is an already-validated Objective-C object held here
/// rather than a plan to be re-checked at replay.
struct RenderRecording {
    /// The dispatches that must run **before** the render encoder exists, in
    /// the order the seam recorded the draws that need them.
    ///
    /// Empty for every pass but one that called
    /// [`draw_indirect_count`](CommandEncoder::draw_indirect_count) or its
    /// indexed sibling — this is the whole reason the pass is deferred, and the
    /// module docs above say why. Each entry packs the argument structures one
    /// count-limited draw will read; `crcbl_mtl::indirect_count` is the design.
    prologue: Vec<PackDispatch>,
    descriptor: Retained<MTLRenderPassDescriptor>,
    /// `setLabel:`, which the fault reporter reads a faulting encoder back out
    /// by; see [`UNLABELLED_RENDER_PASS`].
    label: Retained<NSString>,
    /// The seam's render area as a scissor rectangle, or `None` when Metal
    /// should be told nothing. See [`crate::pass::plan_scissor`].
    scissor: Option<MTLScissorRect>,
    /// The pass's commands, in the order the seam recorded them.
    commands: Vec<RenderCommand>,
}

/// One count-limited draw's packing dispatch, held until the render pass is
/// encoded.
///
/// Retains what it references for [`RenderCommand`]'s reason, and one more
/// besides: the buffer the kernel writes is created here rather than handed
/// over by the seam, so nothing else is keeping it alive between the seam call
/// and the replay.
struct PackDispatch {
    /// `crcbl_mtl::indirect_count`'s kernel, compiled once per device.
    pipeline: Retained<ProtocolObject<dyn objc2_metal::MTLComputePipelineState>>,
    /// The caller's count buffer, read at a word index the block carries.
    count: Retained<ProtocolObject<dyn MTLBuffer>>,
    /// The caller's argument buffer, **read and never written**; see
    /// `crcbl_mtl::indirect_count` for why the kernel copies rather than
    /// patching in place.
    source: Retained<ProtocolObject<dyn MTLBuffer>>,
    /// The packed copy the draws read, owned by this backend.
    packed: Retained<ProtocolObject<dyn MTLBuffer>>,
    /// The kernel's uniform block, sent inline with `setBytes:length:atIndex:`.
    params: [u8; crate::indirect_count::PARAMS_SIZE],
    /// Threadgroups to dispatch, which is one for every bound
    /// `crcbl_mtl::indirect_count`'s ceiling permits.
    groups: NSUInteger,
}

/// One render-pass command, holding everything its Metal call will need.
///
/// **Every variant owns or retains what it references**, which is the whole
/// obligation deferral creates: a `Retained` keeps the Objective-C object alive
/// from the moment the seam call arrived until the replay, so a buffer or
/// pipeline the caller destroys in between is still there to be encoded
/// against. Nothing here borrows, and nothing here is a handle to be looked up
/// a second time — a lookup at replay could fail, and its failure would arrive
/// at the wrong call.
enum RenderCommand {
    /// `pushDebugGroup:` on the encoder.
    PushDebugGroup(Retained<NSString>),
    /// `popDebugGroup` on the encoder.
    PopDebugGroup,
    /// `insertDebugSignpost:`.
    InsertDebugSignpost(Retained<NSString>),
    SetViewport(MTLViewport),
    SetScissor(MTLScissorRect),
    SetStencilReference(u32),
    /// The pipeline state **and** the rasteriser state Metal keeps on the
    /// encoder rather than in the pipeline object; see
    /// [`bind_graphics_pipeline`](CommandEncoder::bind_graphics_pipeline).
    BindPipeline(crate::pipeline::BoundPipeline),
    /// One resolved bind group, with every argument-table index already
    /// absolute and every dynamic offset already folded in.
    BindGroup(Vec<crate::binding::BoundBinding>),
    /// The whole push-constant block as it stood when the write arrived.
    ///
    /// **A copy, not a borrow of the shadow.** `setBytes:length:atIndex:`
    /// copies the bytes it is given, so encoding straight through could point
    /// at the shadow itself; a recorded command cannot, because a later write
    /// splices the shadow in place and the two draws either side of it are
    /// meant to see different blocks.
    PushConstants {
        index: NSUInteger,
        bytes: Vec<u8>,
        /// Whether the pipeline layout's range declares the vertex stage. The
        /// caller's `stages` are not consulted; see
        /// [`push_constants`](CommandEncoder::push_constants).
        vertex: bool,
        /// As `vertex`, for the fragment stage.
        fragment: bool,
    },
    Draw {
        primitive: objc2_metal::MTLPrimitiveType,
        vertex_start: NSUInteger,
        vertex_count: NSUInteger,
        instance_count: NSUInteger,
        base_instance: NSUInteger,
    },
    DrawIndexed {
        primitive: objc2_metal::MTLPrimitiveType,
        index_count: NSUInteger,
        index_type: objc2_metal::MTLIndexType,
        index_buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
        index_offset: NSUInteger,
        instance_count: NSUInteger,
        base_vertex: isize,
        base_instance: NSUInteger,
    },
    /// One argument structure of an indirect draw. The multi-draw loop records
    /// one of these per structure, because that is one Metal call per
    /// structure; see `crcbl_mtl::draw`.
    DrawIndirect {
        primitive: objc2_metal::MTLPrimitiveType,
        args: Retained<ProtocolObject<dyn MTLBuffer>>,
        offset: NSUInteger,
    },
    /// [`DrawIndirect`](Self::DrawIndirect) with the bound index buffer, which
    /// Metal takes as an argument of the draw.
    DrawIndexedIndirect {
        primitive: objc2_metal::MTLPrimitiveType,
        index_type: objc2_metal::MTLIndexType,
        index_buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
        index_offset: NSUInteger,
        args: Retained<ProtocolObject<dyn MTLBuffer>>,
        offset: NSUInteger,
    },
}

/// Dispatches one count-limited draw's packing kernel.
///
/// The argument-table indices are the ones the compiled MSL declares —
/// `[[buffer(0)]]` through `[[buffer(3)]]`, in the order
/// `indirect_count_args.slang` declares its bindings — and **not** anything
/// `crcbl_mtl::binding` computed: this pipeline has no seam-side layout, no
/// bind group and no push-constant block, so the flattening rule that module
/// owns has nothing to say about it.
///
/// Both caller-owned buffers are bound at offset **zero**. Every offset the
/// seam passed travels in the uniform block as a word index instead, which is
/// what keeps this free of any question about what offset alignment a `device`
/// buffer binding requires on the device it is running on.
fn encode_pack(encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>, dispatch: &PackDispatch) {
    encoder.setComputePipelineState(&dispatch.pipeline);
    // SAFETY: `objc2` marks these unsafe because Metal bounds-checks neither
    // the argument-table index nor the offset. The indices are the ones the
    // compiled MSL declares — `crcbl_mtl::indirect_count`'s embedded artifact
    // is the record of them, and its
    // `the_embedded_kernel_is_the_committed_artifact` is what keeps that record
    // true — and all three are far below the buffer table's capacity. The
    // offset is zero on all three by construction: every offset the seam passed
    // travels in the uniform block as a word index instead, and
    // `plan_indirect_count` bounded each of those against the buffer it indexes.
    // Every buffer is kept alive by the `Retained` this dispatch holds.
    unsafe {
        encoder.setBuffer_offset_atIndex(Some(&dispatch.count), 0, 1);
        encoder.setBuffer_offset_atIndex(Some(&dispatch.source), 0, 2);
        encoder.setBuffer_offset_atIndex(Some(&dispatch.packed), 0, 3);
    }
    let length = to_ns(dispatch.params.len() as u64);
    let source = NonNull::from(&dispatch.params).cast::<core::ffi::c_void>();
    // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither the
    // pointer nor the argument-table index. `source` points at `length`
    // initialised bytes of an array this command owns and nothing mutates for
    // the duration of the call, and Metal copies them before returning. Index
    // `0` is the slot the compiled MSL puts the uniform block at, which
    // `crcbl_mtl::indirect_count`'s embedded artifact is the record of.
    unsafe { encoder.setBytes_length_atIndex(source, length, 0) };
    // `groups` is at least one: `plan_indirect_count` answers `None` for a draw
    // of nothing, so a recorded dispatch always has a structure to pack. Metal
    // raises on a zero threadgroup count, and a raise aborts the process.
    encoder.dispatchThreadgroups_threadsPerThreadgroup(
        objc2_metal::MTLSize {
            width: dispatch.groups,
            height: 1,
            depth: 1,
        },
        objc2_metal::MTLSize {
            width: to_ns(u64::from(crate::indirect_count::WORKGROUP_SIZE)),
            height: 1,
            depth: 1,
        },
    );
}

/// Makes one recorded command's Metal call.
///
/// Every value here was checked when the seam call arrived — a handle resolved,
/// a range bounded, a stage mask read off the pipeline layout — so this makes
/// calls and decides nothing. That is deliberate: a check moved down here would
/// report its failure at `end_render_pass` rather than at the call the caller
/// made, and `crcbl_hal`'s own recorder tests assert on which call failed.
fn replay(encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>, command: &RenderCommand) {
    match command {
        RenderCommand::PushDebugGroup(name) => encoder.pushDebugGroup(name),
        RenderCommand::PopDebugGroup => encoder.popDebugGroup(),
        RenderCommand::InsertDebugSignpost(name) => encoder.insertDebugSignpost(name),
        RenderCommand::SetViewport(viewport) => encoder.setViewport(*viewport),
        RenderCommand::SetScissor(rect) => encoder.setScissorRect(*rect),
        RenderCommand::SetStencilReference(reference) => {
            encoder.setStencilReferenceValue(*reference);
        }
        RenderCommand::BindPipeline(bound) => {
            encoder.setRenderPipelineState(&bound.raw);
            encoder.setCullMode(bound.raster.cull);
            encoder.setFrontFacingWinding(bound.raster.winding);
            encoder.setTriangleFillMode(bound.raster.fill);
            encoder.setDepthClipMode(bound.raster.clip);
            let [constant, slope_scale, clamp] = bound.raster.bias;
            encoder.setDepthBias_slopeScale_clamp(constant, slope_scale, clamp);
            // Always an object, never nil: a pipeline with no depth/stencil
            // state carries the device's always-pass one instead, because nil
            // hangs Apple's paravirtual GPU. `crcbl_mtl::pipeline`'s
            // `default_depth_stencil_state` has the bisect and the no-op
            // argument.
            encoder.setDepthStencilState(Some(&bound.depth_stencil));
            if let Some(reference) = bound.raster.stencil_reference {
                encoder.setStencilReferenceValue(reference);
            }
        }
        RenderCommand::BindGroup(bindings) => crate::binding::apply(bindings, encoder),
        RenderCommand::PushConstants {
            index,
            bytes,
            vertex,
            fragment,
        } => {
            let length = to_ns(bytes.len() as u64);
            let source = NonNull::from(&**bytes).cast::<core::ffi::c_void>();
            // SAFETY: `objc2` marks these unsafe because Metal bounds-checks
            // neither the pointer nor the argument-table index. `source` points
            // at `length` initialised bytes of a `Vec` this command owns and
            // nothing mutates for the duration of the call, and Metal copies
            // them before returning — that is what "inlined buffer contents"
            // means. `length` is the recorded block's own length, which
            // `crate::argument::plan` bounded by `Limits::max_push_constant_size`,
            // and `index` is the buffer-table entry after the last binding,
            // which the same call bounded by `BUFFER_TABLE_ENTRIES`.
            if *vertex {
                unsafe { encoder.setVertexBytes_length_atIndex(source, length, *index) };
            }
            // SAFETY: as above, on the fragment stage's table.
            if *fragment {
                unsafe { encoder.setFragmentBytes_length_atIndex(source, length, *index) };
            }
        }
        RenderCommand::Draw {
            primitive,
            vertex_start,
            vertex_count,
            instance_count,
            base_instance,
        } => {
            // SAFETY: `objc2` marks this unsafe because Metal bounds-checks
            // neither count. Neither is a bound into an object here — a draw's
            // vertex count indexes whatever the vertex shader chooses to read,
            // not a buffer this call names — and `draw` recorded this only
            // after checking both ranges non-empty, which is the one value
            // (`0`) Metal's own validation layer objects to.
            unsafe {
                encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                    *primitive,
                    *vertex_start,
                    *vertex_count,
                    *instance_count,
                    *base_instance,
                );
            }
        }
        RenderCommand::DrawIndexed {
            primitive,
            index_count,
            index_type,
            index_buffer,
            index_offset,
            instance_count,
            base_vertex,
            base_instance,
        } => {
            // SAFETY: `objc2` marks this unsafe because Metal bounds-checks
            // neither the index count nor the buffer offset. `draw_indexed`'s
            // `draw_offset` checked the whole index range against the bound
            // region's own length, which `bind_index_buffer` computed from the
            // allocation's, and the counts were checked non-empty there. The
            // buffer is kept alive by the `Retained` this command holds.
            unsafe {
                encoder.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(
                    *primitive,
                    *index_count,
                    *index_type,
                    index_buffer,
                    *index_offset,
                    *instance_count,
                    *base_vertex,
                    *base_instance,
                );
            }
        }
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks the
        // indirect offset. `crate::draw::plan_indirect` checked every structure
        // recorded against the argument buffer's own length, and the buffer is
        // kept alive by the `Retained` this command holds.
        //
        // What Metal cannot check, and neither can this, is the *contents* of
        // the argument structures — an index count larger than the bound index
        // buffer is a GPU-side fault either way, which is the whole nature of
        // an indirect draw.
        RenderCommand::DrawIndirect {
            primitive,
            args,
            offset,
        } => unsafe {
            encoder.drawPrimitives_indirectBuffer_indirectBufferOffset(*primitive, args, *offset);
        },
        // SAFETY: as above, plus the index buffer's offset — which
        // `bind_index_buffer` checked against its allocation.
        RenderCommand::DrawIndexedIndirect {
            primitive,
            index_type,
            index_buffer,
            index_offset,
            args,
            offset,
        } => unsafe {
            encoder.drawIndexedPrimitives_indexType_indexBuffer_indexBufferOffset_indirectBuffer_indirectBufferOffset(
                *primitive,
                *index_type,
                index_buffer,
                *index_offset,
                args,
                *offset,
            );
        },
    }
}

/// Records into one `MTLCommandBuffer`.
pub(crate) struct MetalCommandEncoder {
    device: Arc<DeviceInner>,
    /// `None` only when `MTLCommandQueue::commandBuffer` returned nil, which is
    /// recorded in `failed` at the same moment.
    raw: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    open: Open,
    /// Bumped every time an encoder is opened, so a debug group pushed on one
    /// encoder is never popped on the next.
    epoch: u64,
    /// The first failure, reported by `finish`. Every later command is dropped.
    failed: Option<HalError>,
    /// Whether the seam thinks a render or compute pass is open.
    ///
    /// Both track a scope that [`Open`] also has an encoder for; they are
    /// separate because the seam's rules are about the *pass* — a copy inside
    /// one is refused whether or not an encoder happens to be open, and a pass
    /// left open at `finish` is an error rather than something to close
    /// quietly.
    in_render_pass: bool,
    in_compute_pass: bool,
    /// Open debug groups, innermost last, and which of Metal's two stacks each
    /// went on. See [`crate::pass::Labels`].
    labels: crate::pass::Labels,
    /// Whether the open render pass pushed a label of its own.
    render_pass_label: bool,
    /// Whether the open compute pass pushed a label of its own.
    compute_pass_label: bool,
    /// The topology of the bound graphics pipeline, which Metal takes at the
    /// draw call rather than in the pipeline object.
    ///
    /// `None` means nothing is bound. It is cleared by [`Self::close_open`]
    /// along with the encoder, because a render encoder's pipeline state does
    /// not survive `endEncoding` — keeping it would let a draw in the *next*
    /// pass proceed with no pipeline set and let Metal raise.
    bound_primitive: Option<objc2_metal::MTLPrimitiveType>,
    /// The threadgroup size of the bound compute pipeline, which Metal takes at
    /// the dispatch call rather than in the pipeline object.
    ///
    /// The compute half of [`Self::bound_primitive`], and cleared by
    /// [`Self::close_open`] for the same reason: an `MTLComputeCommandEncoder`
    /// loses its pipeline state at `endEncoding`, so a dispatch in the next
    /// pass must not inherit the previous pass's number.
    bound_threads: Option<objc2_metal::MTLSize>,
    /// The index buffer an indexed draw will read, which Metal takes at the
    /// draw call rather than as encoder state.
    ///
    /// Unlike [`Self::bound_primitive`] this **survives** [`Self::close_open`],
    /// and `crcbl_mtl::draw` argues why: there is no Metal state to lose, so
    /// keeping it is what makes the binding behave the way Vulkan's does.
    index: Option<crate::draw::IndexBinding>,
    /// The push-constant block as it stands on the open encoder.
    ///
    /// `setBytes:length:atIndex:` replaces the whole argument, so a seam write
    /// of part of the block has to be re-sent with the rest of it;
    /// `crcbl_mtl::argument` holds the splice and argues the shape.
    ///
    /// Cleared by [`Self::close_open`] alongside [`Self::bound_primitive`] and
    /// for the same reason: an encoder's argument tables do not survive
    /// `endEncoding`, so carrying the shadow into the next pass would let a
    /// dispatch there proceed with a block Metal no longer holds.
    push_constants: Option<crate::argument::Shadow>,
}

impl core::fmt::Debug for MetalCommandEncoder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MetalCommandEncoder")
            .field("failed", &self.failed.is_some())
            .field("in_render_pass", &self.in_render_pass)
            .finish_non_exhaustive()
    }
}

// SAFETY: `MTLCommandBuffer` and the encoder protocols are the objects
// `objc2-metal` deliberately leaves unmarked — Apple documents a command buffer
// and its encoders as usable from one thread at a time and *not* safe to encode
// into concurrently — and the seam requires this type to be `Send + Sync`
// through `HalThreadSafe`. Both halves are discharged by the trait's own shape
// rather than by a promise:
//
// * **Every recording method takes `&mut self`.** Two threads cannot hold `&mut`
//   to one encoder, so "one thread at a time" is enforced by the borrow checker,
//   which is exactly the discharge `crcbl_hal::command` describes for Vulkan's
//   identical external-synchronisation rule.
// * **`&self` can do nothing to the Metal objects.** The only method taking
//   `&self` is `Debug::fmt`, which reads two Rust-side flags and touches no
//   Objective-C object at all, so sharing a reference across threads exposes no
//   Metal call.
// * Retain and release are atomic in the Objective-C runtime, so moving the
//   `Retained` pointers between threads and dropping them on another is sound
//   on its own. That covers the recorded [`RenderCommand`]s as well: they hold
//   `Retained` pointers and nothing else that is not plain data, and they are
//   only ever read back through the same `&mut self`.
unsafe impl Send for MetalCommandEncoder {}
// SAFETY: as above.
unsafe impl Sync for MetalCommandEncoder {}

impl MetalCommandEncoder {
    pub(crate) fn new(device: Arc<DeviceInner>, desc: &CommandEncoderDesc<'_>) -> Self {
        let mut encoder = Self {
            device,
            raw: None,
            open: Open::None,
            epoch: 0,
            failed: None,
            in_render_pass: false,
            in_compute_pass: false,
            labels: crate::pass::Labels::default(),
            render_pass_label: false,
            compute_pass_label: false,
            bound_primitive: None,
            bound_threads: None,
            index: None,
            push_constants: None,
        };
        // The queue is checked before the command buffer is taken, so a handle
        // belonging to another device is reported as the crossing it is rather
        // than being answered with a working encoder on the wrong device.
        if let Err(error) = encoder.device.check_queue(desc.queue) {
            encoder.failed = Some(error);
            return encoder;
        }
        // Through `crcbl_mtl::fault`, which is what asks Metal for per-encoder
        // status on a fault. Every command buffer this backend creates goes
        // through it, because the one that skipped it is the one that faults.
        let Some(raw) = crate::fault::command_buffer(
            &encoder.device.queue,
            desc.label.unwrap_or(UNLABELLED_COMMAND_BUFFER),
        ) else {
            encoder.failed = Some(HalError::DeviceLost(
                "MTLCommandQueue::commandBufferWithDescriptor: returned nil".to_string(),
            ));
            return encoder;
        };
        encoder.raw = Some(raw);
        encoder
    }

    /// Whether *new* commands may be recorded.
    fn ok(&self) -> bool {
        self.failed.is_none() && self.raw.is_some()
    }

    /// Records the first failure and drops every later command.
    fn fail(&mut self, error: HalError) {
        if self.failed.is_none() {
            crcbl_core::log::error!("crcbl-mtl: command recording failed: {error}");
            self.failed = Some(error);
        }
    }

    /// Refuses a pass that asked for timestamps, and says whether it did.
    ///
    /// **A refusal rather than an accepted no-op**, because there is no set
    /// these could name that a timestamp could go in: this backend reports no
    /// [`Features::TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY) and
    /// `Device::create_query_set` refuses the timestamp kind on every Mac, so
    /// the handle is either dead or an occlusion pool. Dropping it silently
    /// would leave the caller reading zeros out of a pool nothing wrote and
    /// reporting them as timings, which is the shape the whole capability model
    /// exists to prevent. `crcbl_render::PassTimers` never reaches here: it
    /// gates on the feature bit and builds nothing without it.
    fn refuse_timestamps(
        &mut self,
        what: &'static str,
        writes: Option<PassTimestampWrites>,
    ) -> bool {
        if writes.is_none() {
            return false;
        }
        self.fail(crate::device::timestamp_writes_unsupported(what));
        true
    }

    /// Closes whatever encoder is open, leaving the command buffer ready to
    /// accept another — and, for a render pass, *encodes* it first.
    ///
    /// Called before every encoder change *and* by `finish`, because
    /// `MTLCommandBuffer::commit` with an encoder still encoding raises.
    ///
    /// A recording render pass is replayed here rather than only in
    /// `end_render_pass`, so that every path which used to end a render encoder
    /// still produces one: `finish` with a pass left open, and `drop` on an
    /// encoder that was never finished. The second makes no difference to
    /// anything observable — a command buffer that is never committed executes
    /// nothing whatever is in it — and it is done anyway, because "sometimes we
    /// encode the pass and sometimes we do not" is a second rule to keep in
    /// step with the first.
    fn close_open(&mut self) {
        match core::mem::replace(&mut self.open, Open::None) {
            Open::None => {}
            Open::Blit(encoder) => encoder.endEncoding(),
            Open::Compute(encoder) => encoder.endEncoding(),
            Open::Render(recording) => self.encode_render_pass(&recording),
        }
        // Pipeline state and argument tables belong to the encoder that is
        // going away; see `bound_primitive`, `bound_threads` and
        // `push_constants`.
        self.bound_primitive = None;
        self.bound_threads = None;
        self.push_constants = None;
    }

    /// Opens the render encoder a recorded pass has been waiting for, replays
    /// the pass into it, and ends it — after running the pass's prologue.
    ///
    /// The order is the one a straight-through encoder produced: the label and
    /// the scissor rectangle `begin_render_pass` would have set on the fresh
    /// encoder, then every command in the order the seam recorded it, then
    /// `endEncoding`.
    ///
    /// **What a straight-through encoder could not have produced is the
    /// prologue**, and that is the whole reason for deferring. Every
    /// [`PackDispatch`] runs on a compute encoder of its own, opened and ended
    /// before the render encoder exists, so the arguments the pass draws from
    /// are written before the pass is recorded onto the command buffer at all.
    /// The hazard between the two is Metal's own: every buffer this backend
    /// allocates is a tracked resource, and Metal orders a write on one encoder
    /// against a read on the next without being told to.
    ///
    /// This runs whether or not recording has failed, for the reason
    /// `end_render_pass` gives: the encoder must exist and be ended before
    /// `commit`, and a failed encoder's command buffer is thrown away
    /// regardless.
    fn encode_render_pass(&mut self, recording: &RenderRecording) {
        // Cloned rather than borrowed — one retain — because `fail` below needs
        // `&mut self` and a borrow of `self.raw` held across it is a second one.
        let Some(raw) = self.raw.clone() else {
            return;
        };
        if !recording.prologue.is_empty() {
            let Some(encoder) = raw.computeCommandEncoder() else {
                self.fail(HalError::DeviceLost(
                    "MTLCommandBuffer::computeCommandEncoder returned nil for a count-limited \
                     draw's packing pass"
                        .to_string(),
                ));
                return;
            };
            encoder.setLabel(Some(&NSString::from_str(PACK_PASS)));
            for dispatch in &recording.prologue {
                encode_pack(&encoder, dispatch);
            }
            encoder.endEncoding();
        }
        let encoder = raw.renderCommandEncoderWithDescriptor(&recording.descriptor);
        let Some(encoder) = encoder else {
            self.fail(HalError::DeviceLost(
                "MTLCommandBuffer::renderCommandEncoderWithDescriptor: returned nil".to_string(),
            ));
            return;
        };
        encoder.setLabel(Some(&recording.label));
        if let Some(scissor) = recording.scissor {
            encoder.setScissorRect(scissor);
        }
        for command in &recording.commands {
            replay(&encoder, command);
        }
        encoder.endEncoding();
    }

    /// Whether a render pass is recording.
    const fn recording(&self) -> bool {
        matches!(self.open, Open::Render(_))
    }

    /// Appends a command to the open render pass.
    ///
    /// Every caller has already established that one is open — the seam's
    /// scope rules make "outside a render pass" a refusal with its own sentence
    /// rather than a silent drop, so the check belongs at the call site where
    /// that sentence is written. Nothing is recorded if there is no pass, which
    /// is what that refusal already reported.
    fn record(&mut self, command: RenderCommand) {
        if let Open::Render(recording) = &mut self.open {
            recording.commands.push(command);
        }
    }

    /// The blit encoder, opening one if the command buffer has none.
    ///
    /// `None` means the caller has already been told why — either recording had
    /// failed, or a pass is open and the seam forbids a copy there.
    fn blit(&mut self) -> Option<Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>> {
        if !self.ok() {
            return None;
        }
        if self.in_render_pass || self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "a copy inside a pass: the seam places copies between passes, and opening a blit \
                 encoder here would end the pass's own encoder — taking its pipeline state and \
                 argument tables with it"
                    .to_string(),
            ));
            return None;
        }
        if let Open::Blit(encoder) = &self.open {
            return Some(encoder.clone());
        }
        self.close_open();
        let raw = self.raw.as_ref()?;
        let Some(encoder) = raw.blitCommandEncoder() else {
            self.fail(HalError::DeviceLost(
                "MTLCommandBuffer::blitCommandEncoder returned nil".to_string(),
            ));
            return None;
        };
        encoder.setLabel(Some(&NSString::from_str(COPY_ENCODER)));
        self.epoch += 1;
        self.open = Open::Blit(encoder.clone());
        Some(encoder)
    }

    /// Whether a command the seam places *between* passes may be recorded here,
    /// recording the failure if it may not.
    ///
    /// [`Self::blit`] refuses the same case, and this exists so the two query
    /// commands say which call was refused rather than borrowing the copy's
    /// sentence. The rule is the seam's rather than Metal's —
    /// `crcbl_hal::null`'s recorder puts `ResetQuerySet` and `ResolveQuerySet`
    /// through the same `need_outside` check copies and barriers get — but Metal
    /// is where it bites: opening a blit encoder inside a pass ends that pass's
    /// own encoder, taking its pipeline state and argument tables with it.
    fn outside_a_pass(&mut self, what: &'static str) -> bool {
        if self.in_render_pass || self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} inside a pass: the seam records it between passes, and serving it here \
                 would end the pass's own encoder"
            )));
            return false;
        }
        true
    }

    /// Resolves a buffer handle, recording the failure if it does not.
    fn buffer(&mut self, handle: BufferHandle) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        let resolved = self.device.buffer_raw(handle);
        match resolved {
            Ok(raw) => Some(raw),
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Resolves an image handle to its texture, seam format and dimensionality.
    fn image(&mut self, handle: crcbl_hal::ImageHandle) -> Option<ResolvedImage> {
        match self.device.image_raw(handle) {
            Ok(resolved) => Some(resolved),
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Checks a copy's subresource against the texture it names, and returns
    /// the slice range to loop over.
    ///
    /// Metal's buffer↔texture and texture↔texture copies each move **one
    /// slice**, where the seam's [`ImageSubresourceLayers`] may name several, so
    /// every such copy is a loop and this is what bounds it. Metal does not
    /// bounds-check the slice or level, so an out-of-range one raises; refusing
    /// here keeps it an error the caller can catch.
    fn slices(
        &mut self,
        texture: &ProtocolObject<dyn MTLTexture>,
        subresource: ImageSubresourceLayers,
        what: &str,
    ) -> Option<Range<NSUInteger>> {
        let levels = texture.mipmapLevelCount();
        let slices = texture.arrayLength();
        let base = to_ns(u64::from(subresource.base_layer));
        let count = to_ns(u64::from(subresource.layer_count));
        if to_ns(u64::from(subresource.mip)) >= levels {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} names mip {} of an image with {levels}",
                subresource.mip
            )));
            return None;
        }
        if count == 0 || base.checked_add(count).is_none_or(|end| end > slices) {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} names layers {base}..{} of an image with {slices}",
                base.saturating_add(count)
            )));
            return None;
        }
        Some(base..base + count)
    }
}

impl CommandEncoder for MetalCommandEncoder {
    // --- debug ---

    /// Opens a named region, on whichever object is currently encoding.
    ///
    /// Metal has two debug-group stacks rather than one:
    /// `MTLCommandBuffer::pushDebugGroup:` for the space between encoders and
    /// `MTLCommandEncoder::pushDebugGroup:` for the space inside one. A group
    /// pushed on an encoder ends when that encoder does, whatever this trait's
    /// caller intended, so [`crate::pass::Labels`] records which stack each push
    /// went on and `end_debug_label` pops it there — never on the encoder that
    /// happens to be open by then.
    ///
    /// A group opened **inside a render pass** is one of the encoder's, and
    /// becomes a recorded command like every other call the pass makes.
    fn begin_debug_label(&mut self, label: &str) {
        if !self.ok() {
            return;
        }
        let epoch = self.epoch;
        let name = NSString::from_str(label);
        let placement = if self.recording() {
            self.record(RenderCommand::PushDebugGroup(name));
            crate::pass::Placement::Encoder
        } else if let Some(encoder) = self.open.as_encoder() {
            encoder.pushDebugGroup(&name);
            crate::pass::Placement::Encoder
        } else {
            let Some(raw) = self.raw.as_ref() else {
                return;
            };
            raw.pushDebugGroup(&name);
            crate::pass::Placement::CommandBuffer
        };
        self.labels.push(placement, epoch);
    }

    fn end_debug_label(&mut self) {
        // Popped whether or not recording has failed: `finish` drains this
        // stack, and a push that is never matched leaves the capture tool's
        // tree open around every later command.
        let epoch = self.epoch;
        let Some(placement) = self.labels.pop(epoch) else {
            return;
        };
        match placement {
            crate::pass::Placement::CommandBuffer => {
                if let Some(raw) = self.raw.as_ref() {
                    raw.popDebugGroup();
                }
            }
            // The epoch matched, so no *new* encoder has been opened since the
            // push — but the one it went on may have been closed without a
            // successor, and then there is nothing to pop it off.
            crate::pass::Placement::Encoder => {
                if self.recording() {
                    self.record(RenderCommand::PopDebugGroup);
                } else if let Some(encoder) = self.open.as_encoder() {
                    encoder.popDebugGroup();
                }
            }
        }
    }

    /// Inserts a zero-width marker.
    ///
    /// Metal puts `insertDebugSignpost:` on the *encoder* and gives the command
    /// buffer no equivalent, so a marker recorded between encoders has nowhere
    /// to go and is dropped — which is what the seam permits for debug
    /// instrumentation ("accepted and dropped … degrades, it does not break").
    fn insert_debug_marker(&mut self, label: &str) {
        if !self.ok() {
            return;
        }
        let name = NSString::from_str(label);
        if self.recording() {
            self.record(RenderCommand::InsertDebugSignpost(name));
        } else if let Some(encoder) = self.open.as_encoder() {
            encoder.insertDebugSignpost(&name);
        }
    }

    // --- sync ---

    /// Ends the open encoder, which is what a barrier *is* on Metal.
    ///
    /// # The decision, and what would make it wrong
    ///
    /// Metal has no `vkCmdPipelineBarrier`. What it has is **automatic hazard
    /// tracking between encoders**: for a resource whose
    /// `MTLResource::hazardTrackingMode` is
    /// [`Tracked`](objc2_metal::MTLHazardTrackingMode::Tracked), the driver
    /// inserts the dependency itself when one encoder writes what a later
    /// encoder reads. Every resource this backend allocates is tracked —
    /// `newBufferWithLength:options:` and `newTextureWithDescriptor:` default to
    /// it, and `conv::resource_options` never sets
    /// `MTLResourceHazardTrackingModeUntracked`, which
    /// `every_resource_is_hazard_tracked` asserts on real objects rather than on
    /// paper.
    ///
    /// So the translation of [`Barriers`] is not a call. It is the **encoder
    /// boundary** that makes the tracking apply, and this closes the open blit
    /// encoder to create one. Leaving the encoder open instead would put two
    /// copies with a hazard between them inside a single blit encoder, which is
    /// precisely the case the inter-encoder tracking does not cover.
    ///
    /// The states themselves are read and dropped, and that is correct rather
    /// than lazy: Metal has no image layouts, so [`ResourceState`] has no
    /// per-resource expansion to make, and the seam's own module docs say a
    /// backend expands a state into "a Vulkan `sync2` triple, a DX12 state, **a
    /// Metal fence, or nothing at all**".
    ///
    /// **Three things would break this**, and all three are additions rather
    /// than accidents:
    ///
    /// 1. **Sub-allocating from an `MTLHeap`, or reaching resources through an
    ///    argument buffer.** Both are `MTLHazardTrackingModeUntracked` or
    ///    invisible to the encoder, and Metal inserts nothing for them — which
    ///    is what `useResource:usage:` and `MTLFence` exist for. The binding
    ///    slice deliberately introduced neither: `crcbl_mtl::binding` binds
    ///    argument-table slots directly, which Metal both makes resident and
    ///    hazard-tracks, so this argument survives it intact. The slice that
    ///    adds argument buffers is the one that must turn this into
    ///    `updateFence:`/`waitForFence:` pairs.
    /// 2. **`MTLParallelRenderCommandEncoder`.** Its sub-encoders are not
    ///    ordered against one another, so "between encoders" stops meaning
    ///    "before and after".
    /// 3. **A barrier inside a pass.** Metal's within-encoder ordering needs
    ///    `memoryBarrierWithScope:` instead, and this method cannot reach for it
    ///    because it does not know it is inside one. The seam forbids the case
    ///    outright, [`crcbl_hal::null`] fails the render graph's own suite on
    ///    it, and this backend leaves the render encoder alone rather than
    ///    guessing.
    ///
    /// [`ResourceState`]: crcbl_hal::ResourceState
    fn pipeline_barrier(&mut self, _barriers: &Barriers<'_>) {
        if !self.ok() || self.in_render_pass || self.in_compute_pass {
            return;
        }
        self.close_open();
    }

    // --- copies ---

    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) {
        let (Some(src), Some(dst)) = (self.buffer(copy.src), self.buffer(copy.dst)) else {
            return;
        };
        if copy.size == 0 {
            return;
        }
        // Metal does not bounds-check either end, so an overrun raises instead
        // of returning an error. Both are checked against the allocation's own
        // length, read off the object.
        for (buffer, offset, what) in [
            (&src, copy.src_offset, "source"),
            (&dst, copy.dst_offset, "destination"),
        ] {
            let end = offset.checked_add(copy.size);
            if end.is_none_or(|end| end > buffer.length() as u64) {
                self.fail(HalError::InvalidDescriptor(format!(
                    "copy_buffer_to_buffer {what} range {offset}..{} exceeds the buffer's {} bytes",
                    offset.saturating_add(copy.size),
                    buffer.length()
                )));
                return;
            }
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // offset nor size. Both ranges were just checked against the two
        // buffers' own `length()` immediately above, and both objects are kept
        // alive by the `Retained` held across the call.
        unsafe {
            encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                &src,
                to_ns(copy.src_offset),
                &dst,
                to_ns(copy.dst_offset),
                to_ns(copy.size),
            );
        }
    }

    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) {
        let Some(plan) = self.plan_buffer_image_copy(copy, "copy_buffer_to_image") else {
            return;
        };
        let Some(encoder) = self.blit() else {
            return;
        };
        for (index, slice) in plan.slices.clone().enumerate() {
            let offset = plan.buffer_offset + index as u64 * plan.footprint.bytes_per_image;
            // SAFETY: `objc2` marks this unsafe because Metal bounds-checks
            // neither the buffer range nor the region. `plan_buffer_image_copy`
            // checked the region against the texture's own extent at this mip
            // level, the slice against its `arrayLength`, and the whole buffer
            // span against the allocation's `length()`; both objects are kept
            // alive by the `Retained`s held across the call.
            unsafe {
                encoder
                    .copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                        &plan.buffer,
                        to_ns(offset),
                        to_ns(plan.footprint.bytes_per_row),
                        to_ns(plan.slice_stride),
                        plan.size,
                        &plan.texture,
                        slice,
                        to_ns(u64::from(copy.image_subresource.mip)),
                        plan.origin,
                    );
            }
        }
    }

    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) {
        let Some(plan) = self.plan_buffer_image_copy(copy, "copy_image_to_buffer") else {
            return;
        };
        let Some(encoder) = self.blit() else {
            return;
        };
        for (index, slice) in plan.slices.clone().enumerate() {
            let offset = plan.buffer_offset + index as u64 * plan.footprint.bytes_per_image;
            // SAFETY: as `copy_buffer_to_image`, with the two ends swapped —
            // the same `plan_buffer_image_copy` checked the same region, slice
            // range and buffer span.
            unsafe {
                encoder
                    .copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                        &plan.texture,
                        slice,
                        to_ns(u64::from(copy.image_subresource.mip)),
                        plan.origin,
                        plan.size,
                        &plan.buffer,
                        to_ns(offset),
                        to_ns(plan.footprint.bytes_per_row),
                        to_ns(plan.slice_stride),
                    );
            }
        }
    }

    fn copy_image_to_image(&mut self, copy: &ImageCopy) {
        let (Some(src), Some(dst)) = (self.image(copy.src), self.image(copy.dst)) else {
            return;
        };
        let (src, dst, src_type) = (src.raw, dst.raw, src.image_type);
        let (Some(src_origin), Some(dst_origin)) =
            (conv::origin(copy.src_offset), conv::origin(copy.dst_offset))
        else {
            self.fail(HalError::InvalidDescriptor(format!(
                "copy_image_to_image has a negative texel offset: {:?} → {:?}",
                copy.src_offset, copy.dst_offset
            )));
            return;
        };
        let size = conv::copy_size(copy.extent, matches!(src_type, ImageType::D3));
        let (Some(src_slices), Some(dst_slices)) = (
            self.slices(&src, copy.src_subresource, "copy_image_to_image source"),
            self.slices(
                &dst,
                copy.dst_subresource,
                "copy_image_to_image destination",
            ),
        ) else {
            return;
        };
        if src_slices.len() != dst_slices.len() {
            self.fail(HalError::InvalidDescriptor(format!(
                "copy_image_to_image moves {} source layers into {} destination layers",
                src_slices.len(),
                dst_slices.len()
            )));
            return;
        }
        if !self.region_fits(&src, copy.src_subresource.mip, src_origin, size, "source")
            || !self.region_fits(
                &dst,
                copy.dst_subresource.mip,
                dst_origin,
                size,
                "destination",
            )
        {
            return;
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        for (src_slice, dst_slice) in src_slices.zip(dst_slices) {
            // SAFETY: `objc2` marks this unsafe because Metal bounds-checks
            // neither the region nor the slice. `region_fits` checked the region
            // against each texture's own extent at the named mip level and
            // `slices` checked both slice ranges against their `arrayLength`;
            // both textures are kept alive by the `Retained`s held here.
            unsafe {
                encoder
                    .copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                        &src,
                        src_slice,
                        to_ns(u64::from(copy.src_subresource.mip)),
                        src_origin,
                        size,
                        &dst,
                        dst_slice,
                        to_ns(u64::from(copy.dst_subresource.mip)),
                        dst_origin,
                    );
            }
        }
    }

    /// Fills a buffer range with a repeating 32-bit value.
    ///
    /// **Metal's `fillBuffer:range:value:` takes a byte, not a word.** So a
    /// `u32` whose four bytes are not identical has no Metal encoding at all,
    /// and this refuses it by name rather than filling with the low byte and
    /// leaving a caller to find out from a corrupt indirect count. Every value
    /// the seam's own reason for existing needs — "the idiomatic way to zero an
    /// indirect count buffer" — is a repeated byte, and `0` most of all.
    ///
    /// # Which refusal, and why they differ
    ///
    /// The two failures below are the two halves of the seam's own split, and
    /// they are deliberately different variants:
    ///
    /// * A word whose bytes differ is
    ///   [`Capability::BufferFillWord`](crcbl_hal::Capability::BufferFillWord) —
    ///   a thing Metal's API has not got, on every device — so it is
    ///   [`HalError::Unsupported`], which is the variant a caller matches on to
    ///   take the clear-dispatch fallback `crcbl-render` already has.
    /// * A range running past the end of the buffer is the caller's arithmetic,
    ///   so it stays [`HalError::InvalidDescriptor`] naming the range: no
    ///   fallback would help, and the same call with a smaller `size` works.
    fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32) {
        let bytes = value.to_ne_bytes();
        if bytes.iter().any(|byte| *byte != bytes[0]) {
            // The word itself cannot ride along: `Unsupported::what` is
            // `&'static str` by design. `fail` logs the error, and the value is
            // the caller's own, so what is worth carrying is the obstacle.
            self.fail(crate::MetalInstance::unsupported(
                "fill_buffer with a u32 whose four bytes differ: \
                 MTLBlitCommandEncoder::fillBuffer:range:value: repeats a single byte, so only a \
                 value whose four bytes are equal has an encoding",
            ));
            return;
        }
        let Some(raw) = self.buffer(buffer) else {
            return;
        };
        if size == 0 {
            return;
        }
        if offset
            .checked_add(size)
            .is_none_or(|end| end > raw.length() as u64)
        {
            self.fail(HalError::InvalidDescriptor(format!(
                "fill_buffer range {offset}..{} exceeds the buffer's {} bytes",
                offset.saturating_add(size),
                raw.length()
            )));
            return;
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        encoder.fillBuffer_range_value(&raw, NSRange::new(to_ns(offset), to_ns(size)), bytes[0]);
    }

    // --- render scope ---

    /// Opens a render pass, which on Metal is a descriptor plus an encoder.
    ///
    /// # `render_area` becomes a scissor, and that is a real difference
    ///
    /// Metal's `MTLRenderPassDescriptor` has **no render-area rectangle**.
    /// `renderTargetWidth`/`renderTargetHeight` are an origin-anchored *size
    /// limit* on the whole pass rather than Vulkan's `renderArea`, and they
    /// cannot express an offset at all. So the seam's
    /// [`RenderPassDesc::render_area`] is applied as the render encoder's
    /// scissor rectangle, which bounds draws exactly as Vulkan's render area
    /// does.
    ///
    /// What that leaves different, stated rather than hidden: **a
    /// [`LoadOp::Clear`](crcbl_hal::LoadOp::Clear) on Metal clears the whole
    /// attachment**, where Vulkan clears only the render area. The seam
    /// documents `render_area` as "usually the full attachment size" and the
    /// render graph always passes exactly that, so nothing above the seam
    /// depends on the difference today; a caller that wanted a partial clear
    /// would need one this backend cannot give it.
    fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>) {
        if !self.ok() {
            return;
        }
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_render_pass inside a render pass; passes do not nest".to_string(),
            ));
            return;
        }
        if self.refuse_timestamps("begin_render_pass", desc.timestamp_writes) {
            return;
        }
        if desc.color_attachments.is_empty() && desc.depth_stencil_attachment.is_none() {
            // Metal raises on a descriptor with no attachment at all, and the
            // pass would render nowhere in any case.
            self.fail(HalError::InvalidDescriptor(
                "a render pass with no colour and no depth/stencil attachment".to_string(),
            ));
            return;
        }

        let descriptor = MTLRenderPassDescriptor::new();
        // The attachment extent every scissor is clamped against, taken from
        // the first attachment. Metal derives the render target size from the
        // attachments the same way.
        let mut target: Option<(NSUInteger, NSUInteger)> = None;
        for (index, attachment) in desc.color_attachments.iter().enumerate() {
            let view = match self.device.view_raw(attachment.view) {
                Ok((view, _)) => view,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            };
            target.get_or_insert((view.width(), view.height()));
            // `MTLRenderPassDescriptor`'s colour array has a fixed number of
            // slots and subscripting past them raises, so the index is bounded
            // first. This backend leaves `max_color_attachments` at the seam's
            // floor, which is below Metal's array length — a ceiling lower than
            // the truth is always safe, and `crcbl_mtl::adapter` says why every
            // unqueried limit stays at the floor.
            let ceiling = self.device.caps.limits.max_color_attachments as usize;
            if index >= ceiling {
                self.fail(HalError::InvalidDescriptor(format!(
                    "{} colour attachments exceed this device's limit of {ceiling}",
                    desc.color_attachments.len()
                )));
                return;
            }
            // SAFETY: `objc2` marks the subscript unsafe because Metal does not
            // bounds-check the attachment index. It was just checked against
            // `ceiling`, which is at or below the array's own length.
            let slot = unsafe {
                descriptor
                    .colorAttachments()
                    .objectAtIndexedSubscript(index)
            };
            slot.setTexture(Some(&view));
            slot.setLoadAction(conv::load_action(attachment.load));
            slot.setClearColor(conv::clear_color(attachment.clear.color));
            match attachment.resolve {
                Some(handle) => {
                    let resolve = match self.device.view_raw(handle) {
                        Ok((resolve, _)) => resolve,
                        Err(error) => {
                            self.fail(error);
                            return;
                        }
                    };
                    slot.setResolveTexture(Some(&resolve));
                    slot.setStoreAction(conv::resolve_store_action(attachment.store));
                }
                None => slot.setStoreAction(conv::store_action(attachment.store)),
            }
        }

        if let Some(attachment) = desc.depth_stencil_attachment {
            let (view, format) = match self.device.view_raw(attachment.view) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            };
            target.get_or_insert((view.width(), view.height()));
            let depth = descriptor.depthAttachment();
            depth.setTexture(Some(&view));
            depth.setLoadAction(conv::load_action(attachment.depth_load));
            depth.setStoreAction(conv::store_action(attachment.depth_store));
            // Reversed-Z: the seam's default is `depth::CLEAR`, which is 0.0,
            // and this backend widens it and passes it on. Clearing to 1.0 with
            // the engine's `Greater` depth test renders nothing at all.
            depth.setClearDepth(f64::from(attachment.clear.depth));
            // Keyed off the **format**, not the ops: a `D32Float` attachment has
            // no stencil plane to load or store however the caller filled
            // `stencil_load`/`stencil_store`, and giving Metal a stencil
            // attachment whose texture has no stencil raises.
            if format.has_stencil() {
                let stencil = descriptor.stencilAttachment();
                stencil.setTexture(Some(&view));
                stencil.setLoadAction(conv::load_action(attachment.stencil_load));
                stencil.setStoreAction(conv::store_action(attachment.stencil_store));
                stencil.setClearStencil(attachment.clear.stencil);
            }
            // `DepthStencilAttachment::read_only` selects an image *layout* on
            // Vulkan and DX12 and has no Metal counterpart — Metal has no
            // layouts, and whether the pass writes depth is already carried by
            // the store action and by the pipeline's depth-write flag. So it is
            // read and deliberately not acted on here.
        }

        // The blit encoder a preceding upload left open, which Metal will not
        // let the render encoder coexist with — and which is also this
        // backend's barrier. Closed when the pass *begins* rather than when it
        // is encoded, so the command buffer's encoder order is the one a
        // straight-through backend produced.
        self.close_open();
        if self.raw.is_none() {
            return;
        }
        // Set only for a genuine sub-rectangle, and clamped to the attachment;
        // `crate::pass::plan_scissor` holds the rule and its tests.
        let scissor = target
            .and_then(|(width, height)| {
                crate::pass::plan_scissor(desc.render_area, width as u64, height as u64)
            })
            .map(|scissor| MTLScissorRect {
                x: to_ns(scissor.x),
                y: to_ns(scissor.y),
                width: to_ns(scissor.width),
                height: to_ns(scissor.height),
            });
        self.epoch += 1;
        self.open = Open::Render(RenderRecording {
            prologue: Vec::new(),
            descriptor,
            label: NSString::from_str(desc.label.unwrap_or(UNLABELLED_RENDER_PASS)),
            scissor,
            commands: Vec::new(),
        });
        self.in_render_pass = true;
        if let Some(label) = desc.label {
            self.begin_debug_label(label);
            self.render_pass_label = true;
        }
    }

    fn end_render_pass(&mut self) {
        if !self.in_render_pass {
            return;
        }
        if core::mem::take(&mut self.render_pass_label) {
            self.end_debug_label();
        }
        // Not gated on `ok`: an already-failed encoder still has to encode and
        // end its render encoder, because `commit` with one still encoding
        // raises — and the recorded list is what the straight-through form had
        // already put on the encoder by this point, so replaying it is what
        // keeps the two identical.
        self.close_open();
        self.in_render_pass = false;
    }

    fn set_viewport(&mut self, viewport: &Viewport) {
        if !self.recording() {
            return;
        }
        // No Y flip and no depth inversion. Metal's clip space already matches
        // the seam's convention — unlike Vulkan, whose inverted Y is why
        // `crcbl-vk` passes a negative height — and reversed-Z comes from the
        // projection matrix, so touching `znear`/`zfar` here would apply it
        // twice.
        self.record(RenderCommand::SetViewport(MTLViewport {
            originX: f64::from(viewport.x),
            originY: f64::from(viewport.y),
            width: f64::from(viewport.width),
            height: f64::from(viewport.height),
            znear: f64::from(viewport.depth_min),
            zfar: f64::from(viewport.depth_max),
        }));
    }

    fn set_scissor(&mut self, rect: &Rect2d) {
        if !self.recording() {
            return;
        }
        self.record(RenderCommand::SetScissor(MTLScissorRect {
            x: to_ns(rect.x.max(0) as u64),
            y: to_ns(rect.y.max(0) as u64),
            width: to_ns(u64::from(rect.width)),
            height: to_ns(u64::from(rect.height)),
        }));
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        if !self.recording() {
            return;
        }
        self.record(RenderCommand::SetStencilReference(reference));
    }

    /// Sets the pipeline state, **and the rasteriser state Metal keeps on the
    /// encoder rather than in the pipeline object**.
    ///
    /// Cull mode, winding, fill mode, depth clip, depth bias, the depth/stencil
    /// test and the stencil reference are all encoder calls in Metal and all
    /// fields of [`GraphicsPipelineDesc`](crcbl_hal::GraphicsPipelineDesc)
    /// here, so binding replays every one of them from
    /// `crcbl_mtl::pipeline`'s `RasterState`. Leaving any of them out would
    /// make a pipeline draw with whatever the *previous* pipeline set, which is
    /// the class of bug that shows up as one pass rendering correctly and the
    /// next one inheriting its culling.
    ///
    /// Outside a render pass this is a caller error rather than a no-op: there
    /// is no Metal object to hold the state, so silently dropping it would mean
    /// the following draw used none of it.
    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        if !self.ok() {
            return;
        }
        if !self.recording() {
            self.fail(HalError::InvalidDescriptor(
                "bind_graphics_pipeline outside a render pass: Metal keeps pipeline state on the \
                 render encoder, so there is nowhere to record it"
                    .to_string(),
            ));
            return;
        }
        let bound = match self.device.graphics_pipeline_raw(pipeline) {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        // The topology is read at the *draw* rather than sent to Metal, so it
        // stays encoder state here; `replay` makes every call the rest of this
        // binding becomes.
        self.bound_primitive = Some(bound.raster.primitive);
        self.record(RenderCommand::BindPipeline(bound));
    }

    /// Records which buffer a later indexed draw reads, because Metal takes it
    /// at the draw rather than as encoder state.
    ///
    /// `drawIndexedPrimitives:` takes the index buffer, its offset and its type
    /// as **arguments of the draw itself**, so there is no encoder call to make
    /// here and the backend carries the binding across to
    /// [`draw_indexed`](CommandEncoder::draw_indexed) itself. Two consequences,
    /// both argued in `crcbl_mtl::draw`: this is legal outside a render pass,
    /// and the binding survives a pass boundary the way Vulkan's does.
    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat) {
        if !self.ok() {
            return;
        }
        let Some(raw) = self.buffer(buffer) else {
            return;
        };
        let capacity = match crate::draw::plan_index_binding(offset, format, raw.length() as u64) {
            Ok(capacity) => capacity,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        self.index = Some(crate::draw::IndexBinding {
            raw,
            offset,
            format,
            capacity,
        });
    }

    /// Sets one bind group's resources on the open encoder, whichever kind of
    /// pass it belongs to.
    ///
    /// The indices come from `crcbl_mtl::binding`'s flattening of
    /// `(set, binding)` onto Metal's three per-stage argument tables; the
    /// pipeline layout is what says where this slot's tables start, which is
    /// exactly why the seam passes one. **The open pass is what decides the
    /// bind point** — the seam has no compute-vs-graphics parameter here, and
    /// the scope is the only signal a backend gets.
    ///
    /// Outside a pass this is a caller error rather than a no-op, for the
    /// reason [`bind_graphics_pipeline`](CommandEncoder::bind_graphics_pipeline)
    /// gives: Metal keeps argument tables on the encoder, so there is nowhere
    /// to record the binding and the following draw or dispatch would use none
    /// of it.
    fn bind_group(
        &mut self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    ) {
        if !self.ok() {
            return;
        }
        let target = match self.pass_target(
            "bind_group outside a render or compute pass: Metal keeps its argument tables on the \
             encoder, so there is nowhere to record it",
        ) {
            Some(target) => target,
            None => return,
        };
        let limits = self.device.caps.limits;
        match self
            .device
            .bind_group_raw(slot, group, dynamic_offsets, layout, &limits)
        {
            Ok(bindings) => match target {
                Target::Render => self.record(RenderCommand::BindGroup(bindings)),
                Target::Compute(encoder) => crate::binding::apply_compute(&bindings, &encoder),
            },
            Err(error) => self.fail(error),
        }
    }

    /// Sends the push-constant block through `setBytes:length:atIndex:`, at the
    /// buffer index the pipeline layout placed it at.
    ///
    /// **The caller's `stages` are not consulted; the layout's are.** Metal's
    /// argument tables are per stage and each has its own selector — vertex,
    /// fragment, and the compute encoder's unqualified one — so which of them a
    /// write reaches has to be decided by something, and the range the layout
    /// declared is the only side that also decided the index. `crcbl-vk` passes
    /// the layout's mask rather than the caller's for a related reason:
    /// `vkCmdPushConstants` demands the declared one. A layout naming only
    /// [`ShaderStages::COMPUTE`] therefore writes nothing in a render pass,
    /// which is exactly what `crcbl_mtl::binding`'s `apply` does with a
    /// compute-only binding.
    ///
    /// **The whole block goes every time**, spliced into the shadow this encoder
    /// keeps: `setBytes:length:atIndex:` replaces the argument and has no
    /// destination offset, so sending only the caller's `data` would bind a
    /// block starting at the caller's `offset` and every member behind it would
    /// be read from the wrong bytes. `crcbl_mtl::argument` holds that arithmetic
    /// and its tests.
    ///
    /// **Outside a pass this is a caller error rather than a no-op**, for the
    /// reason [`bind_group`](CommandEncoder::bind_group) gives: Metal keeps its
    /// argument tables on the encoder, so there is nowhere to record the write
    /// and the following draw or dispatch would not see it.
    fn push_constants(
        &mut self,
        _stages: ShaderStages,
        offset: u32,
        data: &[u8],
        layout: PipelineLayoutHandle,
    ) {
        if !self.ok() {
            return;
        }
        let target = match self.pass_target(
            "push_constants outside a render or compute pass: Metal has no push constants and this \
             backend sends the block with setBytes:length:atIndex:, which writes the open \
             encoder's argument table",
        ) {
            Some(target) => target,
            None => return,
        };
        // The layout is resolved before the emptiness check, so an empty write
        // through a layout that declares no range is still the refusal the seam
        // asks for rather than a quiet no-op.
        let block = match self.device.push_constant_block(layout) {
            Ok(block) => block,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        // A shadow planned from another layout has another index and another
        // length, so it is replaced rather than spliced into.
        let mut shadow = match self.push_constants.take() {
            Some(shadow) if shadow.matches(layout) => shadow,
            _ => crate::argument::Shadow::new(layout, block),
        };
        if let Err(error) = shadow.splice(offset, data) {
            self.push_constants = Some(shadow);
            self.fail(error);
            return;
        }
        if data.is_empty() {
            // Nothing was asked for, and the layout was still checked above.
            self.push_constants = Some(shadow);
            return;
        }

        let index = to_ns(u64::from(block.index));
        match target {
            Target::Render => {
                let vertex = block.stages.contains(ShaderStages::VERTEX);
                let fragment = block.stages.contains(ShaderStages::FRAGMENT);
                // A layout naming neither raster stage writes nothing here, and
                // recording a command that would make no call is not worth the
                // copy of the block that carrying it costs.
                if vertex || fragment {
                    // A **copy** of the shadow rather than a pointer into it,
                    // which is the one thing deferral changes about this call:
                    // a later `push_constants` splices the same shadow in
                    // place, and the two draws either side of it are meant to
                    // see different blocks.
                    let bytes = shadow.bytes().to_vec();
                    self.record(RenderCommand::PushConstants {
                        index,
                        bytes,
                        vertex,
                        fragment,
                    });
                }
            }
            Target::Compute(encoder) => {
                if block.stages.contains(ShaderStages::COMPUTE) {
                    let bytes = shadow.bytes();
                    let length = to_ns(bytes.len() as u64);
                    let source = NonNull::from(bytes).cast::<core::ffi::c_void>();
                    // SAFETY: `objc2` marks this unsafe because Metal
                    // bounds-checks neither the pointer nor the argument-table
                    // index. `source` points at `length` initialised bytes of a
                    // slice this encoder owns and does not touch for the
                    // duration of the call, and Metal copies them before
                    // returning — that is what "inlined buffer contents" means.
                    // `length` is the shadow's own length, which
                    // `crate::argument::plan` bounded by
                    // `Limits::max_push_constant_size`, and `index` is the
                    // buffer-table entry after the last binding, which the same
                    // call bounded by `BUFFER_TABLE_ENTRIES`. The encoder is
                    // kept alive by the `Retained` held across the call.
                    unsafe { encoder.setBytes_length_atIndex(source, length, index) };
                }
            }
        }
        self.push_constants = Some(shadow);
    }

    // --- draws ---

    /// `drawPrimitives:vertexStart:vertexCount:instanceCount:baseInstance:`.
    ///
    /// The five-argument form always, not the three-argument one: the seam
    /// hands over an instance *range*, and the short form has no base instance
    /// to put its start in. Passing `0..1` through the long form costs nothing
    /// and keeps one code path.
    ///
    /// # The topology comes from the pipeline, and there must be one
    ///
    /// Metal takes the primitive type as a **draw** argument while the seam
    /// declares it on the pipeline, so this reads the value
    /// `bind_graphics_pipeline` recorded. With nothing bound there is no
    /// topology and no pipeline state, and Metal answers that by raising —
    /// which aborts the process — so it is refused here while it is still an
    /// error a caller can catch.
    ///
    /// An empty vertex or instance range draws nothing and is not an error: the
    /// seam's ranges are half-open and `0..0` is a legitimate "no work this
    /// frame" a culling pass produces.
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        let Some(primitive) = self.draw_target("draw") else {
            return;
        };
        if vertices.is_empty() || instances.is_empty() {
            return;
        }
        self.record(RenderCommand::Draw {
            primitive,
            vertex_start: to_ns(u64::from(vertices.start)),
            vertex_count: to_ns(u64::from(vertices.end - vertices.start)),
            instance_count: to_ns(u64::from(instances.end - instances.start)),
            base_instance: to_ns(u64::from(instances.start)),
        });
    }

    /// `drawIndexedPrimitives:…:baseVertex:baseInstance:`, with the index
    /// buffer [`bind_index_buffer`](CommandEncoder::bind_index_buffer)
    /// recorded.
    ///
    /// The eight-argument form always, for the reason [`draw`](CommandEncoder::draw)
    /// takes the five-argument one: the seam hands over a base vertex and an
    /// instance *range*, and the shorter forms have nowhere to put either.
    ///
    /// An indexed draw with no index buffer bound is refused rather than left
    /// to Metal, which has no nil to pass — `indexBuffer` is a non-optional
    /// argument of the selector, so there is no call to make at all.
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        let Some(primitive) = self.draw_target("draw_indexed") else {
            return;
        };
        if indices.is_empty() || instances.is_empty() {
            return;
        }
        let Some(index) = self.index.as_ref() else {
            self.fail(HalError::InvalidDescriptor(
                "draw_indexed with no index buffer bound: Metal takes the buffer as an argument \
                 of the draw and has no nil to pass for it"
                    .to_string(),
            ));
            return;
        };
        let count = indices.end - indices.start;
        let (raw, index_type) = (index.raw.clone(), index.index_type());
        let offset = match index.draw_offset(indices.start, count) {
            Ok(offset) => offset,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        self.record(RenderCommand::DrawIndexed {
            primitive,
            index_count: to_ns(u64::from(count)),
            index_type,
            index_buffer: raw,
            index_offset: to_ns(offset),
            instance_count: to_ns(u64::from(instances.end - instances.start)),
            base_vertex: base_vertex as isize,
            base_instance: to_ns(u64::from(instances.start)),
        });
    }

    /// One `drawPrimitives:indirectBuffer:indirectBufferOffset:` per argument
    /// structure. See `crcbl_mtl::draw` for why a loop *is* multi-draw-indirect
    /// here rather than an approximation of it.
    fn draw_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, false);
    }

    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, true);
    }

    /// The multi-draw loop above, with a **prologue** that reads the count.
    ///
    /// `crcbl_mtl::indirect_count` carries the design; the short of it is that
    /// a compute kernel packs the argument structures this loop will read and
    /// leaves every structure past the count with no instances, so the draws
    /// past the count render nothing.
    fn draw_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.indirect_count(draw, false);
    }

    fn draw_indexed_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.indirect_count(draw, true);
    }

    /// Refused for the same reason `create_mesh_pipeline` is: there is no mesh
    /// pipeline for this to have been recorded against.
    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        self.fail(crate::MetalInstance::not_yet(
            "drawMeshThreadgroups: (the Metal mesh slice)",
        ));
    }

    /// Refused with [`draw_mesh_tasks`](Self::draw_mesh_tasks). Metal's own
    /// form is `drawMeshThreadgroupsWithIndirectBuffer:`, whose buffer holds
    /// the `MTLDispatchThreadgroupsIndirectArguments` the seam documents —
    /// there is simply no mesh pipeline here for either to be recorded
    /// against.
    fn draw_mesh_tasks_indirect(&mut self, _draw: &DrawIndirect) {
        self.fail(crate::MetalInstance::not_yet(
            "drawMeshThreadgroupsWithIndirectBuffer: (the Metal mesh slice)",
        ));
    }

    // --- compute scope ---

    /// Opens a compute pass, **and the `MTLComputeCommandEncoder` that is its
    /// Metal object**.
    ///
    /// The encoder's lifetime is the pass's, which is the one shape that keeps
    /// the seam's scope and Metal's agreeing: everything a compute pass does —
    /// the pipeline state, the argument tables a `bind_group` writes, the
    /// dispatches — lives on that encoder and dies with `endEncoding`. Opening
    /// it at the first dispatch instead would leave `bind_group` and
    /// `bind_compute_pipeline` with nowhere to record, which is precisely the
    /// state this backend was in before compute could dispatch at all.
    fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>) {
        if !self.ok() {
            return;
        }
        if self.in_compute_pass || self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_compute_pass inside a pass; passes do not nest".to_string(),
            ));
            return;
        }
        if self.refuse_timestamps("begin_compute_pass", desc.timestamp_writes) {
            return;
        }
        // Metal raises on a second encoder, so the copy encoder a preceding
        // upload left open goes first — which is also this backend's barrier.
        self.close_open();
        let Some(raw) = self.raw.as_ref() else {
            return;
        };
        let Some(encoder) = raw.computeCommandEncoder() else {
            self.fail(HalError::DeviceLost(
                "MTLCommandBuffer::computeCommandEncoder returned nil".to_string(),
            ));
            return;
        };
        self.epoch += 1;
        encoder.setLabel(Some(&NSString::from_str(
            desc.label.unwrap_or(UNLABELLED_COMPUTE_PASS),
        )));
        self.open = Open::Compute(encoder);
        self.in_compute_pass = true;
        if let Some(label) = desc.label {
            self.begin_debug_label(label);
            self.compute_pass_label = true;
        }
    }

    fn end_compute_pass(&mut self) {
        if !self.in_compute_pass {
            return;
        }
        if core::mem::take(&mut self.compute_pass_label) {
            self.end_debug_label();
        }
        // Not gated on `ok`, as `end_render_pass`: an already-failed encoder
        // still has to close its compute encoder, because `commit` with one
        // still encoding raises.
        self.close_open();
        self.in_compute_pass = false;
    }

    /// Sets the pipeline state, and records the threadgroup size the dispatch
    /// after it will need.
    ///
    /// Metal takes threads-per-threadgroup at
    /// `dispatchThreadgroups:threadsPerThreadgroup:` while every other backend
    /// reads it out of the module, which is why
    /// [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size)
    /// exists — see `crcbl_mtl::pipeline`, where the size is checked and stored
    /// on the pipeline entry. This is where it comes back off.
    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        if !self.ok() {
            return;
        }
        let Open::Compute(encoder) = &self.open else {
            self.fail(HalError::InvalidDescriptor(
                "bind_compute_pipeline outside a compute pass: Metal keeps pipeline state on the \
                 compute encoder, so there is nowhere to record it"
                    .to_string(),
            ));
            return;
        };
        let encoder = encoder.clone();
        let (raw, threads) = match self.device.compute_pipeline_raw(pipeline) {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        encoder.setComputePipelineState(&raw);
        self.bound_threads = Some(threads);
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        let Some((encoder, threads)) = self.dispatch_target("dispatch") else {
            return;
        };
        // A dispatch of nothing, which `vkCmdDispatch` accepts as a no-op — and
        // which a caller computing `count.div_ceil(WORKGROUP_SIZE)` writes
        // whenever `count` is zero. Metal's header documents every threadgroup
        // count as at least one, and its validation layer raises on a zero,
        // which aborts the process; the no-op is performed here instead.
        if x == 0 || y == 0 || z == 0 {
            return;
        }
        encoder.dispatchThreadgroups_threadsPerThreadgroup(
            objc2_metal::MTLSize {
                width: to_ns(u64::from(x)),
                height: to_ns(u64::from(y)),
                depth: to_ns(u64::from(z)),
            },
            threads,
        );
    }

    /// `dispatchThreadgroupsWithIndirectBuffer:indirectBufferOffset:threadsPerThreadgroup:`.
    ///
    /// The argument structure is `MTLDispatchThreadgroups­IndirectArguments` —
    /// three `uint32_t`s, field for field what Vulkan calls
    /// `VkDispatchIndirectCommand`, which is what lets one compute pass write
    /// arguments both backends read. The **threadgroup size is still a CPU
    /// value**: only the counts come from the buffer.
    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64) {
        let Some((encoder, threads)) = self.dispatch_target("dispatch_indirect") else {
            return;
        };
        let Some(raw) = self.buffer(args) else {
            return;
        };
        // Metal reads three `uint32_t`s and bounds-checks neither the offset
        // nor the span, so a short buffer is a GPU fault rather than an error.
        // Refused here while it is still one a caller can catch.
        let length = raw.length() as u64;
        if offset
            .checked_add(DISPATCH_ARGUMENTS)
            .is_none_or(|end| end > length)
        {
            self.fail(HalError::InvalidDescriptor(format!(
                "dispatch_indirect reads {DISPATCH_ARGUMENTS} bytes at offset {offset} of a \
                 {length}-byte buffer"
            )));
            return;
        }
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // the offset nor the argument structure it reads. The span was checked
        // against this buffer's own length just above, the buffer is kept alive
        // by the `Retained` held here, and the encoder is the open one.
        unsafe {
            encoder
                .dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(
                    &raw,
                    to_ns(offset),
                    threads,
                );
        }
    }

    // --- queries ---

    /// Zeroes the range, which is what a reset means for a buffer.
    ///
    /// Metal has no `vkCmdResetQueryPool`: an occlusion pool here is a plain
    /// `MTLBuffer` (`crate::query` says why), and a fresh allocation's contents
    /// are not promised to be anything. So "reset so it can be written again" is
    /// a `fillBuffer:range:value:` of zero — the same call
    /// [`fill_buffer`](CommandEncoder::fill_buffer) makes — and it is what puts a
    /// defined value under every [`Device::query_results`](crcbl_hal::Device::query_results)
    /// of a query nothing has counted into. Zero is the right one: it is what a
    /// query that passed no samples reports.
    ///
    /// Recorded outside a pass, like every other command that needs the blit
    /// encoder; `crcbl_hal::null`'s recorder puts `ResetQuerySet` through the
    /// same `need_outside` check, so that is the seam's rule rather than Metal's.
    fn reset_query_set(&mut self, set: QuerySetHandle, range: Range<u32>) {
        if !self.ok() {
            return;
        }
        let (raw, count) = match self.device.query_set_raw(set) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        if range.end > count {
            self.fail(HalError::InvalidDescriptor(format!(
                "reset_query_set range {}..{} exceeds the set's {count} queries",
                range.start, range.end
            )));
            return;
        }
        if range.start >= range.end {
            return;
        }
        if !self.outside_a_pass("reset_query_set") {
            return;
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        let queries = u64::from(range.end - range.start);
        encoder.fillBuffer_range_value(
            &raw,
            NSRange::new(
                to_ns(crate::query::span_bytes(u64::from(range.start))),
                to_ns(crate::query::span_bytes(queries)),
            ),
            0,
        );
    }

    /// Copies the counts into a caller's buffer, which on Metal is an ordinary
    /// blit.
    ///
    /// There is no `ResolveQueryData` to call and no stride to negotiate: the
    /// pool is already one `u64` per query, which is the layout the seam's two
    /// read paths assume, so this is `copyFromBuffer:…:size:` at
    /// `crate::query`'s offsets. Both ends are bounds-checked here because Metal
    /// checks neither and an overrun raises rather than failing.
    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        range: Range<u32>,
        dst: BufferHandle,
        dst_offset: u64,
    ) {
        if !self.ok() {
            return;
        }
        let (raw, count) = match self.device.query_set_raw(set) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(destination) = self.buffer(dst) else {
            return;
        };
        if range.end > count {
            self.fail(HalError::InvalidDescriptor(format!(
                "resolve_query_set range {}..{} exceeds the set's {count} queries",
                range.start, range.end
            )));
            return;
        }
        let queries = u64::from(range.end.saturating_sub(range.start));
        if let Err(error) =
            crate::query::check_destination(dst_offset, queries, destination.length() as u64)
        {
            self.fail(error);
            return;
        }
        if queries == 0 {
            return;
        }
        if !self.outside_a_pass("resolve_query_set") {
            return;
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // offset nor size. The source span was bounded by the set's own query
        // count and the destination span by the destination's own `length()`
        // immediately above, and both objects are kept alive by the `Retained`
        // held across the call.
        unsafe {
            encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                &raw,
                to_ns(crate::query::span_bytes(u64::from(range.start))),
                &destination,
                to_ns(dst_offset),
                to_ns(crate::query::span_bytes(queries)),
            );
        }
    }

    // --- finish ---

    fn finish(mut self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a render pass still open".to_string(),
            ));
            self.end_render_pass();
        }
        if self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a compute pass still open".to_string(),
            ));
            self.end_compute_pass();
        }
        while !self.labels.is_empty() {
            self.end_debug_label();
        }
        // Always, and before anything else can fail: `commit` with an encoder
        // still encoding raises, and a raise aborts the process.
        self.close_open();
        if let Some(error) = self.failed.take() {
            return Err(error);
        }
        let Some(raw) = self.raw.take() else {
            // Unreachable: `raw` is `None` only when construction recorded a
            // failure, which the `take` above would have returned.
            return Err(HalError::DeviceLost(
                "this encoder never had a command buffer".to_string(),
            ));
        };
        let device = Arc::clone(&self.device);
        let handle = device.state().command_buffers.insert(CommandBufferEntry {
            owner: device.id,
            raw,
            committed: false,
        });
        Ok(device.stamp(handle))
    }
}

impl MetalCommandEncoder {
    /// The open render encoder and the topology a draw needs, or the failure
    /// that says which is missing.
    ///
    /// Metal takes the primitive type as a **draw** argument while the seam
    /// declares it on the pipeline, so every draw reads the value
    /// `bind_graphics_pipeline` recorded. With nothing bound there is no
    /// topology and no pipeline state, and Metal answers that by raising —
    /// which aborts the process — so it is refused here while it is still an
    /// error a caller can catch.
    fn draw_target(&mut self, what: &str) -> Option<objc2_metal::MTLPrimitiveType> {
        if !self.ok() {
            return None;
        }
        if !self.recording() {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} outside a render pass; the seam places every draw inside one"
            )));
            return None;
        }
        let Some(primitive) = self.bound_primitive else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} with no graphics pipeline bound: Metal raises rather than reporting it, \
                 and a raise aborts the process"
            )));
            return None;
        };
        Some(primitive)
    }

    /// Which kind of pass is open, or the failure that says none is.
    ///
    /// The two commands the seam gives no bind point — `bind_group` and
    /// `push_constants` — read the open pass to decide where they go, and each
    /// has its own sentence for the case where there is none. A render pass
    /// carries no encoder here (see [`Open::Render`]), so this answers *which*
    /// and the caller records or encodes accordingly.
    fn pass_target(&mut self, refusal: &str) -> Option<Target> {
        match &self.open {
            Open::Render(_) => Some(Target::Render),
            Open::Compute(encoder) => Some(Target::Compute(encoder.clone())),
            _ => {
                self.fail(HalError::InvalidDescriptor(refusal.to_string()));
                None
            }
        }
    }

    /// The open compute encoder and the threadgroup size a dispatch needs, or
    /// the failure that says which is missing.
    ///
    /// The compute twin of [`Self::draw_target`], and refused for the same
    /// reason: Metal answers a dispatch with no pipeline bound by raising,
    /// which aborts the process, so it is caught here while it is still an
    /// error a caller can handle.
    fn dispatch_target(
        &mut self,
        what: &str,
    ) -> Option<(
        Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>,
        objc2_metal::MTLSize,
    )> {
        if !self.ok() {
            return None;
        }
        let Open::Compute(encoder) = &self.open else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} outside a compute pass; the seam places every dispatch inside one"
            )));
            return None;
        };
        let encoder = encoder.clone();
        let Some(threads) = self.bound_threads else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} with no compute pipeline bound: Metal takes the threadgroup size at the \
                 dispatch and raises rather than reporting it, and a raise aborts the process"
            )));
            return None;
        };
        Some((encoder, threads))
    }

    /// The shared body of the two indirect draws.
    ///
    /// One Metal call per argument structure; `crcbl_mtl::draw`'s
    /// [`plan_indirect`](crate::draw::plan_indirect) is what bounds the span
    /// they read first.
    fn indirect(&mut self, draw: &DrawIndirect, indexed: bool) {
        let Some(primitive) = self.draw_target(if indexed {
            "draw_indexed_indirect"
        } else {
            "draw_indirect"
        }) else {
            return;
        };
        let Some(args) = self.buffer(draw.args) else {
            return;
        };
        let plan = match crate::draw::plan_indirect(draw, indexed, args.length() as u64) {
            Ok(Some(plan)) => plan,
            // A draw of nothing, which the seam permits and which reads no
            // argument structure at all.
            Ok(None) => return,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let index = self.indirect_index("draw_indexed_indirect", indexed);
        if !self.ok() {
            return;
        }
        self.record_indirect_draws(
            primitive,
            &args,
            index.as_ref(),
            (0..plan.count).map(|structure| plan.offset(structure)),
        );
    }

    /// The shared body of the two **count-limited** indirect draws.
    ///
    /// The same loop as [`indirect`](Self::indirect) with a prologue in front
    /// of it: a compute dispatch that reads the count and packs the argument
    /// structures the loop will draw from, leaving every structure at or past
    /// the count with no instances. `crcbl_mtl::indirect_count` holds the
    /// design, the ceiling and the reason the kernel copies rather than
    /// patching the caller's arguments in place.
    ///
    /// **The refusal comes first and is the device's**, matching how
    /// `crcbl-vk`'s own `indirect_count` refuses when
    /// [`Features::DRAW_INDIRECT_COUNT`] is absent: `crcbl_mtl::adapter` is
    /// where the flag is decided, and switching it off there is what turns this
    /// whole slice off.
    fn indirect_count(&mut self, draw: &DrawIndirectCount, indexed: bool) {
        let what = if indexed {
            "draw_indexed_indirect_count"
        } else {
            "draw_indirect_count"
        };
        if !self.ok() {
            return;
        }
        if !self
            .device
            .caps
            .features
            .contains(Features::DRAW_INDIRECT_COUNT)
        {
            self.fail(crate::MetalInstance::unsupported(
                crcbl_hal::METAL_NO_DRAW_INDIRECT_COUNT,
            ));
            return;
        }
        let Some(primitive) = self.draw_target(what) else {
            return;
        };
        let Some(args) = self.buffer(draw.args) else {
            return;
        };
        let Some(count) = self.buffer(draw.count_buffer) else {
            return;
        };
        let plan = match crate::indirect_count::plan_indirect_count(
            draw,
            indexed,
            args.length() as u64,
            count.length() as u64,
        ) {
            Ok(Some(plan)) => plan,
            // A `max_draw_count` of zero: no structure to pack and no draw to
            // issue, so the prologue is not recorded either.
            Ok(None) => return,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let index = self.indirect_index(what, indexed);
        if !self.ok() {
            return;
        }
        let pipeline = match self.device.pack_pipeline() {
            Ok(pipeline) => pipeline,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let Some(packed) = self.pack_buffer(plan.packed_bytes) else {
            return;
        };
        self.record_pack(PackDispatch {
            pipeline,
            count,
            source: args,
            packed: packed.clone(),
            params: plan.params,
            groups: to_ns(u64::from(plan.groups)),
        });
        // **The draws read `packed`, not the caller's buffer.** Reading the
        // caller's would draw the instance counts the kernel was there to
        // override.
        self.record_indirect_draws(
            primitive,
            &packed,
            index.as_ref(),
            (0..plan.draws).map(|structure| plan.offset(structure)),
        );
    }

    /// The bound index buffer's three draw arguments, resolved once for a whole
    /// multi-draw rather than once per structure.
    ///
    /// `None` is two things, and [`ok`](Self::ok) is what tells them apart — the
    /// same arrangement every recording method here uses, because a failure has
    /// nowhere to go until `finish`. Either the draw is not indexed, or it is
    /// and nothing is bound, in which case the caller has been told: Metal takes
    /// the index buffer as an argument of the draw and has no nil to pass for
    /// it.
    fn indirect_index(&mut self, what: &str, indexed: bool) -> Option<IndexArguments> {
        if !indexed {
            return None;
        }
        let Some(index) = self.index.as_ref() else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} with no index buffer bound: Metal takes the buffer as an argument of the \
                 draw and has no nil to pass for it"
            )));
            return None;
        };
        Some((index.raw.clone(), index.index_type(), index.offset))
    }

    /// Records one indirect draw per argument structure, which is one Metal
    /// call per structure.
    ///
    /// **The loop *is* the multi-draw** — see `crcbl_mtl::draw` — so deferring
    /// it must not fold it into anything smaller. It is shared by the plain and
    /// the count-limited forms because the two differ only in which buffer the
    /// structures are in and how their number was arrived at; a second copy is a
    /// second place for the indexed arm to drift.
    fn record_indirect_draws(
        &mut self,
        primitive: objc2_metal::MTLPrimitiveType,
        args: &Retained<ProtocolObject<dyn MTLBuffer>>,
        index: Option<&IndexArguments>,
        offsets: impl Iterator<Item = u64>,
    ) {
        for offset in offsets {
            let offset = to_ns(offset);
            let command = match index {
                Some((raw, index_type, index_offset)) => RenderCommand::DrawIndexedIndirect {
                    primitive,
                    index_type: *index_type,
                    index_buffer: raw.clone(),
                    index_offset: to_ns(*index_offset),
                    args: args.clone(),
                    offset,
                },
                None => RenderCommand::DrawIndirect {
                    primitive,
                    args: args.clone(),
                    offset,
                },
            };
            self.record(command);
        }
    }

    /// Appends a packing dispatch to the open render pass's prologue.
    ///
    /// [`record`](Self::record)'s sibling, and silent for the same reason: every
    /// caller has already established that a pass is open, because
    /// [`draw_target`](Self::draw_target) said so with its own sentence.
    fn record_pack(&mut self, dispatch: PackDispatch) {
        if let Open::Render(recording) = &mut self.open {
            recording.prologue.push(dispatch);
        }
    }

    /// Allocates the buffer one count-limited draw's kernel packs into.
    ///
    /// **Private, and one per call.** Private because the CPU never reads it;
    /// one per call because a buffer shared between two draws — or between two
    /// frames — is a buffer one of them is still reading while the other packs
    /// it, and this backend has no allocator to hand back a region that is
    /// provably idle. `crcbl_mtl::indirect_count`'s ceiling is what keeps the
    /// allocation trivially small: at most eight argument structures, which is
    /// 160 bytes.
    ///
    /// It is released when the recording is dropped, and Metal keeps it alive
    /// from there: `crcbl_mtl::fault`'s command buffer is created with
    /// `retainedReferences` at its default of true, so the encoders that
    /// reference a resource retain it until the buffer completes. Every
    /// [`RenderCommand`] already depends on that.
    fn pack_buffer(&mut self, bytes: u64) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        let Some(raw) = self.device.raw.newBufferWithLength_options(
            to_ns(bytes),
            conv::resource_options(crcbl_hal::MemoryLocation::DeviceLocal),
        ) else {
            // As `create_buffer`: nil is the allocation failing, and it is the
            // one failure the seam has a name for here.
            self.fail(HalError::OutOfDeviceMemory);
            return None;
        };
        raw.setLabel(Some(&NSString::from_str(PACK_BUFFER)));
        Some(raw)
    }

    /// Everything a buffer↔image copy needs, once every bound has been checked.
    fn plan_buffer_image_copy(&mut self, copy: &BufferImageCopy, what: &str) -> Option<CopyPlan> {
        let buffer = self.buffer(copy.buffer)?;
        let image = self.image(copy.image)?;
        let (texture, format, image_type) = (image.raw, image.format, image.image_type);
        let Some(footprint) = conv::copy_footprint(format, copy.image_subresource.aspect, copy)
        else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} names aspect {:?} of a {format:?} image, which is not one plane",
                copy.image_subresource.aspect
            )));
            return None;
        };
        let Some(origin) = conv::origin(copy.image_offset) else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} has a negative texel offset: {:?}",
                copy.image_offset
            )));
            return None;
        };
        let is_3d = matches!(image_type, ImageType::D3);
        let size = conv::copy_size(copy.image_extent, is_3d);
        let slices = self.slices(&texture, copy.image_subresource, what)?;
        if !self.region_fits(&texture, copy.image_subresource.mip, origin, size, what) {
            return None;
        }
        // Metal wants the *source*/*destination* image stride only where there
        // is more than one image to stride between — a 3D copy, or a slice loop
        // whose regions really are `bytes_per_image` apart. For a single 2D
        // slice the parameter is unused and Metal's own header asks for 0.
        let slice_stride = if is_3d || slices.len() > 1 {
            footprint.bytes_per_image
        } else {
            0
        };
        // How many 2D images the buffer side spans: one per array slice, or one
        // per depth step for a volume. The product is right for either, because
        // whichever is not in play is 1.
        let images = (slices.len() as u64) * (size.depth as u64);
        let span = footprint
            .bytes_per_image
            .checked_mul(images)
            .and_then(|span| copy.buffer_offset.checked_add(span));
        if span.is_none_or(|end| end > buffer.length() as u64) {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} reads or writes {images} images of {} bytes from offset {} of a {}-byte \
                 buffer",
                footprint.bytes_per_image,
                copy.buffer_offset,
                buffer.length()
            )));
            return None;
        }
        Some(CopyPlan {
            buffer,
            buffer_offset: copy.buffer_offset,
            texture,
            footprint,
            slice_stride,
            origin,
            size,
            slices,
        })
    }

    /// Whether a copy region lies inside a texture's extent at `mip`.
    ///
    /// Metal bounds-checks none of this and raises on an overrun, so it is
    /// checked against the object's own dimensions — halved per mip level the
    /// way the chain is defined, with a floor of one texel.
    fn region_fits(
        &mut self,
        texture: &ProtocolObject<dyn MTLTexture>,
        mip: u32,
        origin: MTLOrigin,
        size: objc2_metal::MTLSize,
        what: &str,
    ) -> bool {
        let shift = u32::min(mip, NSUInteger::BITS - 1);
        let level = |extent: NSUInteger| (extent >> shift).max(1);
        let (width, height, depth) = (
            level(texture.width()),
            level(texture.height()),
            level(texture.depth()),
        );
        let fits = origin.x.saturating_add(size.width) <= width
            && origin.y.saturating_add(size.height) <= height
            && origin.z.saturating_add(size.depth) <= depth;
        if !fits {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} region at {origin:?} of {size:?} does not fit mip {mip}, which is \
                 {width}x{height}x{depth}"
            )));
        }
        fits
    }
}

/// A buffer↔image copy with every bound already checked.
struct CopyPlan {
    buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    buffer_offset: u64,
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
    footprint: conv::CopyFootprint,
    /// What Metal is told the image stride is; `0` for a single 2D slice, which
    /// is what its header asks for.
    slice_stride: u64,
    origin: MTLOrigin,
    size: objc2_metal::MTLSize,
    slices: Range<NSUInteger>,
}

/// Bytes an indirect dispatch reads: `MTLDispatchThreadgroupsIndirectArguments`
/// is three `uint32_t`s, fixed by Metal's own header.
const DISPATCH_ARGUMENTS: u64 = 12;

impl Drop for MetalCommandEncoder {
    fn drop(&mut self) {
        // Only reached when `finish` was never called or failed. The command
        // buffer is released without ever being committed, which Metal permits
        // — but an encoder still encoding on it is not, so close it first. A
        // render pass still recording is encoded and ended here too; see
        // `close_open` for why that is unconditional rather than skipped on a
        // buffer nothing will run.
        self.close_open();
    }
}
