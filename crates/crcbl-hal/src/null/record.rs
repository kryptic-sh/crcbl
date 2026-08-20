//! The recorded stream: what [`NullBackend`](super) saw, in order.
//!
//! This is the half of the null backend that makes it a *test tool* rather than
//! a compile check. A no-op backend proves the seam has no leaks; a recording
//! one lets a test assert "this frame emitted these passes in this order, with
//! these barriers between them, and leaked nothing" — which is precisely the
//! graph-compile suite `docs/plan/12-testing.md` schedules against NullBackend.
//!
//! # Getting at it
//!
//! A [`Recorder`] is a cheap `Arc` handle. Make one, hand a clone to
//! [`NullInstance::with_recorder`](super::NullInstance::with_recorder), then run
//! the frame entirely through `dyn Instance` / `dyn Device` and read the stream
//! back afterwards. The test never names a backend type after construction,
//! which is the property the integration suite exists to check.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use core::fmt;
use core::ops::Range;

use crcbl_core::{Handle, Pool};

use crate::{
    BindGroupHandle, BindGroupLayoutEntry, BindGroupLayoutHandle, BufferBarrier, BufferCopy,
    BufferHandle, BufferImageCopy, CommandBufferHandle, ComputePipelineHandle,
    DepthStencilAttachment, DrawIndirect, DrawIndirectCount, GraphicsPipelineHandle, ImageBarrier,
    ImageCopy, ImageHandle, IndexFormat, MemoryLocation, QuerySetHandle, QueueHandle, Rect2d,
    SemaphoreSignal, SemaphoreWait, ShaderSources, ShaderStages, SwapchainHandle, Viewport,
};

/// Every kind of object the null backend tracks.
///
/// The tag half of a handle: `Handle<T>` distinguishes kinds at compile time,
/// and this distinguishes them at lookup time so two kinds can reuse the same
/// slot index without colliding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectKind {
    /// A buffer.
    Buffer,
    /// An image.
    Image,
    /// An image view.
    ImageView,
    /// A sampler.
    Sampler,
    /// A shader module.
    ShaderModule,
    /// A bind-group layout.
    BindGroupLayout,
    /// A bind group.
    BindGroup,
    /// A pipeline layout.
    PipelineLayout,
    /// A graphics pipeline.
    GraphicsPipeline,
    /// A compute pipeline.
    ComputePipeline,
    /// A query set.
    QuerySet,
    /// A semaphore.
    Semaphore,
    /// A finished command buffer.
    CommandBuffer,
    /// A presentation surface.
    Surface,
    /// A swapchain.
    Swapchain,
    /// An in-flight readback request.
    Readback,
}

impl ObjectKind {
    /// Every kind, in declaration order. Indexing this array by `kind as usize`
    /// is how the backend's pool table is addressed.
    pub const ALL: [Self; 16] = [
        Self::Buffer,
        Self::Image,
        Self::ImageView,
        Self::Sampler,
        Self::ShaderModule,
        Self::BindGroupLayout,
        Self::BindGroup,
        Self::PipelineLayout,
        Self::GraphicsPipeline,
        Self::ComputePipeline,
        Self::QuerySet,
        Self::Semaphore,
        Self::CommandBuffer,
        Self::Surface,
        Self::Swapchain,
        Self::Readback,
    ];

    /// A short lowercase name, used in errors and in [`fmt::Display`].
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Buffer => "buffer",
            Self::Image => "image",
            Self::ImageView => "image view",
            Self::Sampler => "sampler",
            Self::ShaderModule => "shader module",
            Self::BindGroupLayout => "bind group layout",
            Self::BindGroup => "bind group",
            Self::PipelineLayout => "pipeline layout",
            Self::GraphicsPipeline => "graphics pipeline",
            Self::ComputePipeline => "compute pipeline",
            Self::QuerySet => "query set",
            Self::Semaphore => "semaphore",
            Self::CommandBuffer => "command buffer",
            Self::Surface => "surface",
            Self::Swapchain => "swapchain",
            Self::Readback => "readback",
        }
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One recorded command, with every borrowed field owned.
///
/// Mirrors [`CommandEncoder`](crate::CommandEncoder) one-to-one. Slices and
/// `&str`s become `Vec`s and `String`s so the stream outlives the descriptors
/// that produced it; `push_constants` keeps only the byte count, because a test
/// asserting on push-constant *contents* is asserting on shader ABI, not on the
/// command stream.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// [`begin_debug_label`](crate::CommandEncoder::begin_debug_label).
    BeginDebugLabel(String),
    /// [`end_debug_label`](crate::CommandEncoder::end_debug_label).
    EndDebugLabel,
    /// [`insert_debug_marker`](crate::CommandEncoder::insert_debug_marker).
    DebugMarker(String),
    /// [`pipeline_barrier`](crate::CommandEncoder::pipeline_barrier).
    Barrier {
        /// Buffer transitions.
        buffers: Vec<BufferBarrier>,
        /// Image transitions.
        images: Vec<ImageBarrier>,
        /// Whether a global barrier was requested.
        global: bool,
    },
    /// [`copy_buffer_to_buffer`](crate::CommandEncoder::copy_buffer_to_buffer).
    CopyBufferToBuffer(BufferCopy),
    /// [`copy_buffer_to_image`](crate::CommandEncoder::copy_buffer_to_image).
    CopyBufferToImage(BufferImageCopy),
    /// [`copy_image_to_buffer`](crate::CommandEncoder::copy_image_to_buffer).
    CopyImageToBuffer(BufferImageCopy),
    /// [`copy_image_to_image`](crate::CommandEncoder::copy_image_to_image).
    CopyImageToImage(ImageCopy),
    /// [`fill_buffer`](crate::CommandEncoder::clear_buffer).
    ClearBuffer {
        /// Buffer zeroed.
        buffer: BufferHandle,
        /// Byte offset.
        offset: u64,
        /// Byte length.
        size: u64,
    },
    /// [`begin_render_pass`](crate::CommandEncoder::begin_render_pass).
    BeginRenderPass {
        /// Pass label, if the caller gave one.
        label: Option<String>,
        /// Colour attachments.
        color_attachments: Vec<crate::ColorAttachment>,
        /// Depth/stencil attachment.
        depth_stencil_attachment: Option<DepthStencilAttachment>,
        /// Region rendered.
        render_area: Rect2d,
        /// Timestamps bracketing the pass, if the caller asked for them.
        timestamp_writes: Option<crate::PassTimestampWrites>,
    },
    /// [`end_render_pass`](crate::CommandEncoder::end_render_pass).
    EndRenderPass,
    /// [`set_viewport`](crate::CommandEncoder::set_viewport).
    SetViewport(Viewport),
    /// [`set_scissor`](crate::CommandEncoder::set_scissor).
    SetScissor(Rect2d),
    /// [`set_stencil_reference`](crate::CommandEncoder::set_stencil_reference).
    SetStencilReference(u32),
    /// [`bind_graphics_pipeline`](crate::CommandEncoder::bind_graphics_pipeline).
    BindGraphicsPipeline(GraphicsPipelineHandle),
    /// [`bind_index_buffer`](crate::CommandEncoder::bind_index_buffer).
    BindIndexBuffer {
        /// Buffer bound.
        buffer: BufferHandle,
        /// Byte offset.
        offset: u64,
        /// Index width.
        format: IndexFormat,
    },
    /// [`bind_group`](crate::CommandEncoder::bind_group).
    BindGroup {
        /// Set index.
        slot: u32,
        /// Group bound.
        group: BindGroupHandle,
        /// Dynamic offsets supplied.
        dynamic_offsets: Vec<u32>,
    },
    /// [`push_constants`](crate::CommandEncoder::push_constants).
    PushConstants {
        /// Stages written.
        stages: ShaderStages,
        /// Byte offset within the block.
        offset: u32,
        /// Bytes written.
        len: usize,
    },
    /// [`draw`](crate::CommandEncoder::draw).
    Draw {
        /// Vertex range.
        vertices: Range<u32>,
        /// Instance range.
        instances: Range<u32>,
    },
    /// [`draw_indexed`](crate::CommandEncoder::draw_indexed).
    DrawIndexed {
        /// Index range.
        indices: Range<u32>,
        /// Value added to each index.
        base_vertex: i32,
        /// Instance range.
        instances: Range<u32>,
    },
    /// [`draw_indirect`](crate::CommandEncoder::draw_indirect).
    DrawIndirect(DrawIndirect),
    /// [`draw_indexed_indirect`](crate::CommandEncoder::draw_indexed_indirect).
    DrawIndexedIndirect(DrawIndirect),
    /// [`draw_indirect_count`](crate::CommandEncoder::draw_indirect_count).
    DrawIndirectCount(DrawIndirectCount),
    /// [`draw_indexed_indirect_count`](crate::CommandEncoder::draw_indexed_indirect_count).
    DrawIndexedIndirectCount(DrawIndirectCount),
    /// [`draw_mesh_tasks`](crate::CommandEncoder::draw_mesh_tasks).
    DrawMeshTasks {
        /// Workgroups on X.
        x: u32,
        /// Workgroups on Y.
        y: u32,
        /// Workgroups on Z.
        z: u32,
    },
    /// [`draw_mesh_tasks_indirect`](crate::CommandEncoder::draw_mesh_tasks_indirect).
    ///
    /// The workgroup counts are **not** here, because they are not the CPU's:
    /// what a test can assert is the buffer and the offset the driver would
    /// read them from, and the contents are the producing pass's to check.
    DrawMeshTasksIndirect(DrawIndirect),
    /// [`begin_compute_pass`](crate::CommandEncoder::begin_compute_pass).
    BeginComputePass {
        /// Pass label, if the caller gave one.
        label: Option<String>,
        /// Timestamps bracketing the pass, if the caller asked for them.
        timestamp_writes: Option<crate::PassTimestampWrites>,
    },
    /// [`end_compute_pass`](crate::CommandEncoder::end_compute_pass).
    EndComputePass,
    /// [`bind_compute_pipeline`](crate::CommandEncoder::bind_compute_pipeline).
    BindComputePipeline(ComputePipelineHandle),
    /// [`dispatch`](crate::CommandEncoder::dispatch).
    Dispatch {
        /// Workgroups on X.
        x: u32,
        /// Workgroups on Y.
        y: u32,
        /// Workgroups on Z.
        z: u32,
    },
    /// [`dispatch_indirect`](crate::CommandEncoder::dispatch_indirect).
    DispatchIndirect {
        /// Argument buffer.
        args: BufferHandle,
        /// Byte offset.
        offset: u64,
    },
    /// [`reset_query_set`](crate::CommandEncoder::reset_query_set).
    ResetQuerySet {
        /// Query set.
        set: QuerySetHandle,
        /// Query range.
        range: Range<u32>,
    },
    /// [`resolve_query_set`](crate::CommandEncoder::resolve_query_set).
    ResolveQuerySet {
        /// Query set.
        set: QuerySetHandle,
        /// Query range.
        range: Range<u32>,
        /// Destination buffer.
        dst: BufferHandle,
        /// Byte offset in the destination.
        dst_offset: u64,
    },
}

impl Command {
    /// A stable variant name.
    ///
    /// Lets a test assert the *shape* of a frame — `["BeginComputePass",
    /// "Dispatch", "EndComputePass", "BeginRenderPass", …]` — without spelling
    /// out every handle and descriptor, which is usually the assertion worth
    /// making.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::BeginDebugLabel(_) => "BeginDebugLabel",
            Self::EndDebugLabel => "EndDebugLabel",
            Self::DebugMarker(_) => "DebugMarker",
            Self::Barrier { .. } => "Barrier",
            Self::CopyBufferToBuffer(_) => "CopyBufferToBuffer",
            Self::CopyBufferToImage(_) => "CopyBufferToImage",
            Self::CopyImageToBuffer(_) => "CopyImageToBuffer",
            Self::CopyImageToImage(_) => "CopyImageToImage",
            Self::ClearBuffer { .. } => "ClearBuffer",
            Self::BeginRenderPass { .. } => "BeginRenderPass",
            Self::EndRenderPass => "EndRenderPass",
            Self::SetViewport(_) => "SetViewport",
            Self::SetScissor(_) => "SetScissor",
            Self::SetStencilReference(_) => "SetStencilReference",
            Self::BindGraphicsPipeline(_) => "BindGraphicsPipeline",
            Self::BindIndexBuffer { .. } => "BindIndexBuffer",
            Self::BindGroup { .. } => "BindGroup",
            Self::PushConstants { .. } => "PushConstants",
            Self::Draw { .. } => "Draw",
            Self::DrawIndexed { .. } => "DrawIndexed",
            Self::DrawIndirect(_) => "DrawIndirect",
            Self::DrawIndexedIndirect(_) => "DrawIndexedIndirect",
            Self::DrawIndirectCount(_) => "DrawIndirectCount",
            Self::DrawIndexedIndirectCount(_) => "DrawIndexedIndirectCount",
            Self::DrawMeshTasks { .. } => "DrawMeshTasks",
            Self::DrawMeshTasksIndirect(_) => "DrawMeshTasksIndirect",
            Self::BeginComputePass { .. } => "BeginComputePass",
            Self::EndComputePass => "EndComputePass",
            Self::BindComputePipeline(_) => "BindComputePipeline",
            Self::Dispatch { .. } => "Dispatch",
            Self::DispatchIndirect { .. } => "DispatchIndirect",
            Self::ResetQuerySet { .. } => "ResetQuerySet",
            Self::ResolveQuerySet { .. } => "ResolveQuerySet",
        }
    }

    /// The pass this command opens, if it opens one.
    ///
    /// `("render" | "compute", label)`.
    #[must_use]
    pub fn opens_pass(&self) -> Option<(&'static str, Option<&str>)> {
        match self {
            Self::BeginRenderPass { label, .. } => Some(("render", label.as_deref())),
            Self::BeginComputePass { label, .. } => Some(("compute", label.as_deref())),
            _ => None,
        }
    }
}

/// A device-level event, interleaved with the commands in one ordered stream.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// An object was created.
    Created {
        /// What kind.
        kind: ObjectKind,
        /// Its label, if it had one.
        label: Option<String>,
    },
    /// An object was destroyed.
    Destroyed {
        /// What kind.
        kind: ObjectKind,
    },
    /// The host wrote into a mappable buffer.
    BufferWritten {
        /// Buffer written.
        buffer: BufferHandle,
        /// Byte offset.
        offset: u64,
        /// Bytes written.
        len: usize,
    },
    /// A readback was requested.
    ReadbackRequested {
        /// Buffer to be read.
        buffer: BufferHandle,
        /// Byte offset.
        offset: u64,
        /// Bytes requested.
        size: u64,
    },
    /// The host read from a readback buffer — emitted by the poll that
    /// completes a request.
    BufferRead {
        /// Buffer read.
        buffer: BufferHandle,
        /// Byte offset.
        offset: u64,
        /// Bytes read.
        len: usize,
    },
    /// A recorded command. Emitted when its encoder is finished, in record
    /// order, immediately before the [`Event::Finished`] for that buffer.
    Command {
        /// Command buffer it belongs to.
        command_buffer: CommandBufferHandle,
        /// The command.
        command: Command,
    },
    /// An encoder produced a command buffer.
    Finished {
        /// Encoder label.
        label: Option<String>,
        /// Resulting command buffer.
        command_buffer: CommandBufferHandle,
    },
    /// Work was submitted to a queue.
    Submitted {
        /// Queue submitted to.
        queue: QueueHandle,
        /// Command buffers, in execution order.
        command_buffers: Vec<CommandBufferHandle>,
        /// Waits performed first.
        waits: Vec<SemaphoreWait>,
        /// Signals performed on completion.
        signals: Vec<SemaphoreSignal>,
    },
    /// A swapchain image was acquired.
    Acquired {
        /// Swapchain acquired from.
        swapchain: SwapchainHandle,
        /// Image index within the ring.
        index: u32,
    },
    /// A swapchain image was presented.
    Presented {
        /// Swapchain presented.
        swapchain: SwapchainHandle,
        /// The number the caller gave this present, from
        /// [`PresentInfo::present_id`](crate::PresentInfo::present_id).
        present_id: Option<u64>,
    },
    /// The caller asked to wait for a numbered present with
    /// [`Device::wait_until_presented`](crate::Device::wait_until_presented).
    ///
    /// Recorded even though nothing is waited for: this backend does not
    /// advertise [`Features::PRESENT_FEEDBACK`](crate::Features::PRESENT_FEEDBACK)
    /// and has no display behind it, so the call returns at once. The event is
    /// the request, which is what a test of a frame loop's pacing needs to see.
    PresentWaited {
        /// Swapchain the wait named.
        swapchain: SwapchainHandle,
        /// Present the caller waited for.
        present_id: u64,
    },
    /// A swapchain was reconfigured (resize, present-mode change).
    Reconfigured {
        /// Swapchain reconfigured.
        swapchain: SwapchainHandle,
        /// New size in pixels.
        extent: (u32, u32),
    },
    /// [`Device::wait_idle`](crate::Device::wait_idle) was called.
    WaitedIdle,
}

/// A rule the recorded stream broke.
///
/// The null backend never panics on one of these and never fails the call: it
/// records the violation and carries on, so a test sees the *whole* frame's
/// worth of problems rather than only the first. Check with
/// [`Recorder::validation_errors`] or [`Recorder::assert_valid`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// A command legal only inside a render or compute pass was recorded
    /// outside one.
    #[error("`{command}` requires an open {expected} pass")]
    OutsidePass {
        /// Command name, from [`Command::name`].
        command: &'static str,
        /// Which pass kind it needed.
        expected: &'static str,
    },
    /// A command legal only outside a pass was recorded inside one. Copies,
    /// barriers and query writes are all in this group — Vulkan forbids them
    /// inside dynamic rendering.
    #[error("`{command}` is not allowed inside a pass")]
    InsidePass {
        /// Command name, from [`Command::name`].
        command: &'static str,
    },
    /// A pass was opened while another was open. Passes do not nest.
    #[error("`{command}` opened a pass while another was already open")]
    NestedPass {
        /// Command name, from [`Command::name`].
        command: &'static str,
    },
    /// A pass was closed that had not been opened, or closed as the wrong kind.
    #[error("`{command}` closed a pass that was not open")]
    UnopenedPass {
        /// Command name, from [`Command::name`].
        command: &'static str,
    },
    /// An encoder was finished with a pass still open.
    #[error("command buffer finished with a pass still open")]
    UnclosedPass,
    /// A command referenced a handle owned by a different device — the
    /// recorded-stream counterpart of
    /// [`HalError::ForeignObject`](crate::HalError::ForeignObject).
    #[error("`{command}` referenced a {kind} handle {bits:#x} owned by another device")]
    ForeignHandle {
        /// Command name, from [`Command::name`].
        command: &'static str,
        /// Kind expected.
        kind: ObjectKind,
        /// Packed handle bits.
        bits: u64,
    },
    /// A pass asked for both of its timestamps in the same query.
    ///
    /// WebGPU requires `beginningOfPassWriteIndex` and `endOfPassWriteIndex` to
    /// differ, and a pass that wrote both ends into one query would report the
    /// second over the first on every other backend — so the pair measures
    /// nothing wherever it is accepted. See
    /// [`PassTimestampWrites`](crate::PassTimestampWrites).
    #[error("`{command}` writes both of its timestamps into query {index}")]
    CoincidentTimestamps {
        /// Command name, from [`Command::name`].
        command: &'static str,
        /// The query both writes named.
        index: u32,
    },
    /// A pass's timestamp write named a query past the end of its set.
    #[error("`{command}` writes a timestamp into query {index} of a {count}-query set")]
    TimestampOutOfRange {
        /// Command name, from [`Command::name`].
        command: &'static str,
        /// The query the pass named.
        index: u32,
        /// How many queries the set holds.
        count: u32,
    },
    /// A command referenced a handle that was never created or has been
    /// destroyed.
    #[error("`{command}` referenced a dead {kind} handle {bits:#x}")]
    DeadHandle {
        /// Command name, from [`Command::name`].
        command: &'static str,
        /// Kind expected.
        kind: ObjectKind,
        /// Packed handle bits.
        bits: u64,
    },
}

/// Backend-side detail attached to a tracked object.
#[derive(Debug)]
pub(super) enum Detail {
    /// Nothing beyond the label.
    None,
    /// A buffer: its size, where it lives, and its contents when mappable.
    Buffer {
        size: u64,
        memory: MemoryLocation,
        bytes: Vec<u8>,
    },
    /// An in-flight readback: what it covers and how many more polls report
    /// `Pending` before it completes.
    Readback {
        buffer: BufferHandle,
        offset: u64,
        size: u64,
        polls_remaining: u32,
    },
    /// A bind-group layout: the declared slots a bind group is checked against.
    BindGroupLayout {
        entries: Vec<BindGroupLayoutEntry>,
        /// Whether any slot carries
        /// [`BindingFlags::UPDATE_AFTER_BIND`](crate::BindingFlags::UPDATE_AFTER_BIND),
        /// which is what makes `update_bind_group` legal.
        update_after_bind: bool,
        /// The binding number carrying
        /// [`BindingFlags::VARIABLE_COUNT`](crate::BindingFlags::VARIABLE_COUNT),
        /// if any.
        variable_binding: Option<u32>,
    },
    /// A bind group: the layout it conforms to, resolved at creation.
    BindGroup {
        layout: BindGroupLayoutHandle,
        update_after_bind: bool,
    },
    /// A shader module: the entry points it offered a DXIL container for.
    ///
    /// Empty for a module created without DXIL, which is every call site that
    /// ships only the other three artifacts — so this is a *claim* to check
    /// rather than a requirement, and only a module that made one is held to
    /// it. See `check_stage`.
    ShaderModule { dxil_entry_points: Vec<String> },
    /// A query set: how many queries it holds, so a range can be checked.
    QuerySet { count: u32 },
    /// A semaphore: whether it is a timeline, and the value it holds.
    ///
    /// `value` starts at
    /// [`SemaphoreKind::Timeline::initial_value`](crate::SemaphoreKind::Timeline)
    /// and moves only when the host signals it. A signal a *submission* carries
    /// does not move it, for the reason nothing else here executes either: the
    /// submission is recorded, not run.
    Semaphore { timeline: bool, value: u64 },
    /// A swapchain: its image ring and rotation state.
    Swapchain {
        extent: (u32, u32),
        images: Vec<ImageHandle>,
        /// One whole-image view per ring image, owned by the swapchain and
        /// handed out through [`AcquiredFrame::view`](crate::AcquiredFrame::view).
        views: Vec<crate::ImageViewHandle>,
        acquire_semaphores: Vec<Option<Handle<crate::Semaphore>>>,
        present_semaphores: Vec<Option<Handle<crate::Semaphore>>>,
        next: u32,
    },
}

/// One tracked object.
///
/// `owner` is the id of the [`NullInstance`](super::NullInstance) or null
/// device that created it. Every lookup checks it, which is how the backend
/// detects a handle crossing from one instance or device to another — see
/// [`crate::device`] for the obligation this discharges. It is a side-table
/// field precisely because a [`Handle`] has no room for it.
#[derive(Debug)]
pub(super) struct Object {
    pub(super) label: Option<String>,
    pub(super) owner: u64,
    pub(super) detail: Detail,
}

/// The null backend's shared state: object pools plus the recorded stream.
#[derive(Debug)]
pub(super) struct State {
    pub(super) pools: Vec<Pool<Object>>,
    pub(super) events: Vec<Event>,
    pub(super) validation: Vec<ValidationError>,
    /// How many `poll_readback` calls report `Pending` before a request
    /// completes. See [`Recorder::set_readback_latency`].
    pub(super) readback_latency: u32,
    /// How many `PendingDevice::poll` calls report `Pending` before a device
    /// request completes. See [`Recorder::set_device_latency`].
    pub(super) device_latency: u32,
    /// One entry per `create_shader_module`, surviving the module's
    /// destruction. See [`Recorder::shader_modules_created`].
    pub(super) shader_modules_created: Vec<(Option<String>, ShaderSources)>,
    /// Every bind-group layout created, with its label — a log rather than a
    /// lookup, for [`shader_modules_created`](Self::shader_modules_created)'s
    /// reason exactly: a caller destroys its layouts once its pipelines exist,
    /// so by the time a test can assert anything the handles are dead.
    ///
    /// What it is for: the **binding numbers** a caller declared, which is the
    /// one thing about a layout no other observable carries and which
    /// `crcbl-mtl` turns into Metal argument-table indices by counting. See
    /// [`Recorder::bind_group_layouts_created`].
    pub(super) bind_group_layouts_created: Vec<(Option<String>, Vec<BindGroupLayoutEntry>)>,
    /// Queued out-of-band device errors. See [`Recorder::report_device_error`].
    pub(super) device_errors: std::collections::VecDeque<String>,
    /// How many `reconfigure_swapchain` calls fail before succeeding again. See
    /// [`Recorder::fail_next_reconfigures`].
    pub(super) reconfigure_failures: u32,
    /// Whether the surface has moved under the swapchain, so every presentation
    /// call reports [`SurfaceError::OutOfDate`](crate::SurfaceError::OutOfDate)
    /// until a reconfigure clears it. See
    /// [`Recorder::report_swapchain_out_of_date`].
    pub(super) swapchain_out_of_date: bool,
    /// Why the device is lost, once it is — and never cleared, which is what
    /// separates it from `device_errors`. See [`Recorder::lose_device`].
    pub(super) lost_device: Option<String>,
}

impl State {
    fn new() -> Self {
        Self {
            pools: (0..ObjectKind::ALL.len()).map(|_| Pool::new()).collect(),
            events: Vec::new(),
            validation: Vec::new(),
            readback_latency: 0,
            device_latency: 0,
            shader_modules_created: Vec::new(),
            bind_group_layouts_created: Vec::new(),
            device_errors: std::collections::VecDeque::new(),
            reconfigure_failures: 0,
            swapchain_out_of_date: false,
            lost_device: None,
        }
    }

    pub(super) fn pool(&mut self, kind: ObjectKind) -> &mut Pool<Object> {
        &mut self.pools[kind as usize]
    }

    pub(super) fn insert<T>(&mut self, kind: ObjectKind, object: Object) -> Handle<T> {
        let label = object.label.clone();
        let handle = self.pool(kind).insert(object);
        self.events.push(Event::Created { kind, label });
        handle.cast()
    }

    /// Builds and inserts an object owned by `owner`.
    pub(super) fn insert_owned<T>(
        &mut self,
        kind: ObjectKind,
        owner: u64,
        label: Option<String>,
        detail: Detail,
    ) -> Handle<T> {
        self.insert(
            kind,
            Object {
                label,
                owner,
                detail,
            },
        )
    }

    pub(super) fn get(&self, kind: ObjectKind, bits: u64) -> Option<&Object> {
        Handle::from_bits(bits).and_then(|handle| self.pools[kind as usize].get(handle))
    }

    pub(super) fn get_mut(&mut self, kind: ObjectKind, bits: u64) -> Option<&mut Object> {
        Handle::from_bits(bits).and_then(|handle| self.pools[kind as usize].get_mut(handle))
    }

    pub(super) fn remove(&mut self, kind: ObjectKind, bits: u64) {
        if let Some(handle) = Handle::from_bits(bits)
            && self.pool(kind).remove(handle).is_some()
        {
            self.events.push(Event::Destroyed { kind });
        }
    }

    pub(super) fn is_live(&self, kind: ObjectKind, bits: u64) -> bool {
        self.get(kind, bits).is_some()
    }

    /// Whether the object is live *and* owned by `owner`.
    pub(super) fn is_owned_by(&self, kind: ObjectKind, bits: u64, owner: u64) -> bool {
        self.get(kind, bits)
            .is_some_and(|object| object.owner == owner)
    }
}

/// A shared, cloneable view of a null backend's recorded stream.
///
/// Cheap to clone (one `Arc` bump). Hand one clone to
/// [`NullInstance::with_recorder`](super::NullInstance::with_recorder) and keep
/// the other to assert on.
#[derive(Clone, Debug)]
pub struct Recorder {
    state: Arc<Mutex<State>>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    /// A fresh, empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::new())),
        }
    }

    /// Locks the shared state, ignoring poisoning.
    ///
    /// A poisoned mutex here means a test panicked while holding it. The state
    /// is a plain log with no invariant a panic could have broken halfway, and
    /// turning every subsequent access into a second panic would bury the first
    /// one's message.
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every event so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.lock().events.clone()
    }

    /// Every recorded command, in record order, across all command buffers.
    #[must_use]
    pub fn commands(&self) -> Vec<Command> {
        self.lock()
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Command { command, .. } => Some(command.clone()),
                _ => None,
            })
            .collect()
    }

    /// [`Command::name`] for every recorded command, in order.
    ///
    /// The compact assertion: `assert_eq!(recorder.command_names(), ["BeginComputePass", …])`.
    #[must_use]
    pub fn command_names(&self) -> Vec<&'static str> {
        self.lock()
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Command { command, .. } => Some(command.name()),
                _ => None,
            })
            .collect()
    }

    /// The label of every pass opened, in order, with unlabelled passes
    /// rendered as `"<unlabelled render>"` / `"<unlabelled compute>"`.
    ///
    /// This is the "did this frame emit these passes in this order" assertion
    /// the graph suite is built around.
    #[must_use]
    pub fn pass_labels(&self) -> Vec<String> {
        self.lock()
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Command { command, .. } => command.opens_pass(),
                _ => None,
            })
            .map(|(kind, label)| {
                label.map_or_else(|| format!("<unlabelled {kind}>"), ToOwned::to_owned)
            })
            .collect()
    }

    /// Every rule the recorded stream broke.
    #[must_use]
    pub fn validation_errors(&self) -> Vec<ValidationError> {
        self.lock().validation.clone()
    }

    /// Panics if the stream broke any rule, listing every violation.
    ///
    /// # Panics
    ///
    /// If [`Recorder::validation_errors`] is non-empty.
    pub fn assert_valid(&self) {
        let errors = self.validation_errors();
        assert!(
            errors.is_empty(),
            "null backend recorded {} validation error(s):\n{}",
            errors.len(),
            errors
                .iter()
                .map(|error| format!("  - {error}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// How many objects of `kind` are still alive.
    ///
    /// The leak assertion: a frame that creates and destroys transients should
    /// end where it started.
    #[must_use]
    pub fn live_objects(&self, kind: ObjectKind) -> usize {
        self.lock().pools[kind as usize].len()
    }

    /// What a mappable buffer currently holds, or `None` if the handle names
    /// nothing this recorder created.
    ///
    /// [`Event::BufferWritten`] says an upload happened, at an offset, of a
    /// length — which catches a write that went to the wrong slot and cannot
    /// catch one that went to the right slot with the wrong bytes. A producer
    /// that packs a record into a storage buffer is exactly that case: the
    /// field order, the endianness and the stride are all invisible in the
    /// event and all decidable here, by decoding what landed with the same
    /// layout the shader reads it with.
    ///
    /// Only [`MemoryLocation::HostUpload`]
    /// buffers hold anything: the null backend executes no copies, so a
    /// device-local buffer's contents are whatever nothing wrote to it. It is
    /// therefore evidence about the *host's* writes, and a copy this backend
    /// recorded but did not perform leaves no trace in it.
    #[must_use]
    pub fn buffer_bytes(&self, buffer: BufferHandle) -> Option<Vec<u8>> {
        let state = self.lock();
        match &state.get(ObjectKind::Buffer, buffer.to_bits())?.detail {
            Detail::Buffer { bytes, .. } => Some(bytes.clone()),
            _ => unreachable!("a buffer handle always carries buffer detail"),
        }
    }

    /// Every shader module created so far, as `(label, formats the descriptor
    /// carried)`, in creation order.
    ///
    /// The no-GPU assertion for the seam's "offer every artifact you have"
    /// rule. A call site that stopped passing its WGSL keeps working on Vulkan
    /// and keeps working here, so nothing but running `crcbl-wgpu` on a machine
    /// with a GPU would notice — which is how the WGSL artifacts sat unused
    /// from P5.3 to P5.9. This notices, in the plain test suite.
    ///
    /// A log rather than a lookup on the handle, because a caller that builds
    /// pipelines destroys its modules immediately afterwards — by the time a
    /// test can assert anything, every handle it would ask about is dead.
    /// [`clear`](Self::clear) drops it with the rest of the stream.
    #[must_use]
    pub fn shader_modules_created(&self) -> Vec<(Option<String>, ShaderSources)> {
        self.lock().shader_modules_created.clone()
    }

    /// Every bind-group layout created, label and declared entries, in order.
    ///
    /// The assertion this exists for is about **which binding numbers a layout
    /// declares**, and it is not cosmetic on one backend: `crcbl-mtl` gives a
    /// resource the next index in its Metal argument table by counting the
    /// same-table entries of this list, so two layouts for one shader that
    /// declare different subsets of its bindings place every resource above the
    /// difference at different indices — silently, and only there.
    #[must_use]
    pub fn bind_group_layouts_created(&self) -> Vec<(Option<String>, Vec<BindGroupLayoutEntry>)> {
        self.lock().bind_group_layouts_created.clone()
    }

    /// Total live objects across every kind.
    #[must_use]
    pub fn total_live_objects(&self) -> usize {
        let state = self.lock();
        state.pools.iter().map(Pool::len).sum()
    }

    /// Drops the recorded stream, keeping live objects.
    ///
    /// Call between frames so per-frame assertions do not have to skip past the
    /// setup.
    pub fn clear(&self) {
        let mut state = self.lock();
        state.events.clear();
        state.validation.clear();
        state.shader_modules_created.clear();
        state.bind_group_layouts_created.clear();
    }

    /// Makes the next `poll_readback` calls report
    /// [`Pending`](crate::ReadbackState::Pending) `polls` times before
    /// completing.
    ///
    /// The null backend executes nothing, so a readback would otherwise be
    /// ready on its first poll and a caller's poll *loop* would never be
    /// exercised. Setting a latency simulates WebGPU's deferred `mapAsync` and
    /// Vulkan's fence-not-yet-signalled, which is the difference between
    /// testing that a screenshot path compiles and testing that it waits.
    ///
    /// Applies to readbacks requested after this call. Default `0`.
    pub fn set_readback_latency(&self, polls: u32) {
        self.lock().readback_latency = polls;
    }

    /// Makes the next device requests report
    /// [`Pending`](crate::DeviceRequestState::Pending) `polls` times before
    /// handing over the device.
    ///
    /// The sibling of [`set_readback_latency`](Self::set_readback_latency), and
    /// there for the same reason: the null backend opens a device instantly, so
    /// a request would otherwise be ready on its first poll and a caller's
    /// **poll loop** — the rAF-driven start-up path that
    /// `docs/plan/10-wasm-webgpu.md` needs — would never be exercised. Setting a
    /// latency simulates WebGPU's deferred `requestDevice` promise on a machine
    /// with no browser and no GPU.
    ///
    /// Applies to devices requested after this call. Default `0`, which is what
    /// `Instance::create_device` sees: it
    /// completes on the first poll.
    pub fn set_device_latency(&self, polls: u32) {
        self.lock().device_latency = polls;
    }

    /// Queues an error for [`Device::take_error`](crate::Device::take_error) to
    /// report, as if the driver had raised it outside any call.
    ///
    /// The sibling of [`set_device_latency`](Self::set_device_latency), and
    /// there for the same reason: the null backend never fails, so a caller's
    /// **handling** of an out-of-band failure would otherwise never run. This
    /// simulates the one case that actually produces them — WebGPU delivering a
    /// pipeline's compilation failure to the device's error channel a turn of
    /// the event loop after the call that made it returned an object.
    ///
    /// Each queued error is reported once, oldest first.
    pub fn report_device_error(&self, message: impl Into<String>) {
        self.lock().device_errors.push_back(message.into());
    }

    /// Takes the oldest queued device error, if any. The backend's half of
    /// [`report_device_error`](Self::report_device_error).
    pub(super) fn take_device_error(&self) -> Option<String> {
        self.lock().device_errors.pop_front()
    }

    /// Makes the next `reconfigure_swapchain` call(s) fail with
    /// [`SurfaceError::Hal(HalError::OutOfDeviceMemory)`](crate::SurfaceError::Hal),
    /// as if rebuilding the swapchain ran out of memory.
    ///
    /// The sibling of [`set_device_latency`](Self::set_device_latency), and
    /// there for the same reason: the null backend never fails, so a caller's
    /// **error path** would otherwise never run. A settings screen that applies
    /// a pacing change mid-run is the caller this exists for — it has to roll
    /// the request, the effective pacing and the swapchain mode back together
    /// when the rebuild fails, and a backend that cannot fail makes that
    /// rollback unobservable.
    ///
    /// Consumed one per reconfigure; after `times` failures the backend
    /// succeeds again. Default `0`.
    pub fn fail_next_reconfigures(&self, times: u32) {
        self.lock().reconfigure_failures = times;
    }

    /// Consumes one reconfigure failure, if any are owed. The backend's half of
    /// [`fail_next_reconfigures`](Self::fail_next_reconfigures).
    pub(super) fn take_reconfigure_failure(&self) -> bool {
        let mut state = self.lock();
        if state.reconfigure_failures > 0 {
            state.reconfigure_failures -= 1;
            true
        } else {
            false
        }
    }

    /// Makes every presentation call report
    /// [`SurfaceError::OutOfDate`](crate::SurfaceError::OutOfDate) until the
    /// swapchain is reconfigured, as if the window had just been resized.
    ///
    /// The sibling of [`fail_next_reconfigures`](Self::fail_next_reconfigures),
    /// and there for the same reason: the null backend never fails, so a
    /// caller's **resize path** would otherwise never run. This is the one that
    /// was missing hardest — every `SurfaceError` this backend built was
    /// [`SurfaceError::Hal`](crate::SurfaceError::Hal), so the variant the seam
    /// calls expected traffic could not be produced here at all, and the frame
    /// loop's reconfigure-and-retry arms were reachable only on a real driver
    /// while someone dragged a window edge.
    ///
    /// **Latched rather than counted**, because that is what a driver does: a
    /// swapchain whose surface moved is out of date on `acquire_next_frame`, on
    /// `present` and on `wait_until_presented` alike, and stays out of date
    /// until it is rebuilt. So a caller that handles it clears it *by* handling
    /// it and a caller that does not spins, which is the difference worth being
    /// able to test. `reconfigure_swapchain` is what clears it — a *failed*
    /// reconfigure does not, since it rebuilt nothing.
    ///
    /// [`SurfaceError::Lost`](crate::SurfaceError::Lost) and
    /// [`Timeout`](crate::SurfaceError::Timeout) get no injector: neither is
    /// recoverable traffic, so no caller in this workspace does anything with
    /// them that an assertion could see.
    pub fn report_swapchain_out_of_date(&self) {
        self.lock().swapchain_out_of_date = true;
    }

    /// Loses the device, permanently: every later call that resolves a handle —
    /// and [`Device::wait_idle`](crate::Device::wait_idle), which resolves none
    /// — fails with [`HalError::DeviceLost`](crate::HalError::DeviceLost)
    /// carrying `message`.
    ///
    /// The sibling of [`report_device_error`](Self::report_device_error) and
    /// deliberately its opposite. That one is *recoverable and one-shot*: it
    /// models a browser delivering a compilation failure late, and taking it
    /// clears it. This one models a TDR, a GPU hang or a browser reclaiming the
    /// context — everything created from the device is dead and nothing brings
    /// it back — which is a state the null backend could not express, so the
    /// engine's policy for it (`docs/backlog.md`: device loss surfaces, it does
    /// not self-heal) had no test on any backend.
    ///
    /// Nothing clears it. A test that wants a device again opens one.
    ///
    /// Object *destruction* still works, as it does on a real driver: a caller
    /// tearing down after a lost device must be able to release what it holds.
    /// Creation calls that name no handle — `create_buffer` and the rest — also
    /// still succeed, because no caller in this workspace reaches one after a
    /// loss and failing them would be surface nothing exercises.
    pub fn lose_device(&self, message: impl Into<String>) {
        self.lock().lost_device = Some(message.into());
    }

    /// Records a validation error. Used by the backend; exposed so a test can
    /// check its own harness if it needs to.
    pub(super) fn push_validation(&self, error: ValidationError) {
        self.lock().validation.push(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_kind_table_is_complete_and_ordered() {
        for (index, kind) in ObjectKind::ALL.iter().enumerate() {
            assert_eq!(
                *kind as usize, index,
                "ALL must be in declaration order: pool lookup indexes by `kind as usize`"
            );
            assert!(!kind.name().is_empty());
        }
    }

    #[test]
    fn a_fresh_recorder_is_empty_and_valid() {
        let recorder = Recorder::new();
        assert!(recorder.events().is_empty());
        assert!(recorder.commands().is_empty());
        assert!(recorder.pass_labels().is_empty());
        assert_eq!(recorder.total_live_objects(), 0);
        recorder.assert_valid();
    }

    #[test]
    fn pass_labels_name_unlabelled_passes_by_kind() {
        let mut state = State::new();
        let cb: CommandBufferHandle =
            state.insert_owned(ObjectKind::CommandBuffer, 1, None, Detail::None);
        state.events.push(Event::Command {
            command_buffer: cb,
            command: Command::BeginComputePass {
                label: None,
                timestamp_writes: None,
            },
        });
        state.events.push(Event::Command {
            command_buffer: cb,
            command: Command::BeginRenderPass {
                label: Some("gbuffer".into()),
                color_attachments: Vec::new(),
                depth_stencil_attachment: None,
                render_area: Rect2d::from_size(1, 1),
                timestamp_writes: None,
            },
        });
        let recorder = Recorder {
            state: Arc::new(Mutex::new(state)),
        };
        assert_eq!(
            recorder.pass_labels(),
            vec!["<unlabelled compute>".to_string(), "gbuffer".to_string()]
        );
    }

    #[test]
    fn clear_drops_events_but_keeps_objects() {
        let mut state = State::new();
        let _: BufferHandle =
            state.insert_owned(ObjectKind::Buffer, 1, Some("kept".into()), Detail::None);
        let recorder = Recorder {
            state: Arc::new(Mutex::new(state)),
        };
        assert_eq!(recorder.events().len(), 1);
        recorder.clear();
        assert!(recorder.events().is_empty());
        assert_eq!(recorder.live_objects(ObjectKind::Buffer), 1);
    }

    #[test]
    fn destroyed_handles_stop_resolving() {
        let mut state = State::new();
        let buffer: BufferHandle = state.insert_owned(ObjectKind::Buffer, 1, None, Detail::None);
        assert!(state.is_live(ObjectKind::Buffer, buffer.to_bits()));
        state.remove(ObjectKind::Buffer, buffer.to_bits());
        assert!(!state.is_live(ObjectKind::Buffer, buffer.to_bits()));
        // Same bits under a different kind never resolve either.
        assert!(!state.is_live(ObjectKind::Image, buffer.to_bits()));
    }

    /// The mechanism `crate::device`'s rule 3 obliges backends to provide: the
    /// same handle bits under two owners must not resolve for the wrong one.
    #[test]
    fn ownership_is_checked_independently_of_liveness() {
        let mut state = State::new();
        let buffer: BufferHandle = state.insert_owned(ObjectKind::Buffer, 7, None, Detail::None);
        assert!(state.is_live(ObjectKind::Buffer, buffer.to_bits()));
        assert!(state.is_owned_by(ObjectKind::Buffer, buffer.to_bits(), 7));
        assert!(
            !state.is_owned_by(ObjectKind::Buffer, buffer.to_bits(), 8),
            "a live handle owned by someone else must be rejected, not accepted"
        );
        state.remove(ObjectKind::Buffer, buffer.to_bits());
        assert!(!state.is_owned_by(ObjectKind::Buffer, buffer.to_bits(), 7));
    }

    /// One of every [`Command`] variant.
    ///
    /// The `match` at the end is what keeps the list honest: it is exhaustive,
    /// so a variant added to `Command` stops this module compiling until
    /// someone comes here — which is the point at which the list beside it is
    /// impossible to miss. A sampled list has no such moment; this one used to
    /// hold five of them.
    fn every_command_variant() -> Vec<Command> {
        fn handle<T>() -> Handle<T> {
            Handle::from_bits(1 << 32).expect("generation 1 is a real generation")
        }
        let layers = crate::ImageSubresourceLayers {
            aspect: crate::ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        };
        let region = BufferImageCopy {
            buffer: handle(),
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: handle(),
            image_subresource: layers,
            image_offset: crate::Offset3d::default(),
            image_extent: crate::Extent3d::d2(1, 1),
        };
        let indirect = DrawIndirect {
            args: handle(),
            offset: 0,
            draw_count: 1,
            stride: 16,
        };
        let indirect_count = DrawIndirectCount {
            args: handle(),
            args_offset: 0,
            count_buffer: handle(),
            count_offset: 0,
            max_draw_count: 1,
            stride: 16,
        };

        let samples = vec![
            Command::BeginDebugLabel("label".into()),
            Command::EndDebugLabel,
            Command::DebugMarker("marker".into()),
            Command::Barrier {
                buffers: Vec::new(),
                images: Vec::new(),
                global: true,
            },
            Command::CopyBufferToBuffer(BufferCopy {
                src: handle(),
                src_offset: 0,
                dst: handle(),
                dst_offset: 0,
                size: 4,
            }),
            Command::CopyBufferToImage(region),
            Command::CopyImageToBuffer(region),
            Command::CopyImageToImage(ImageCopy {
                src: handle(),
                src_subresource: layers,
                src_offset: crate::Offset3d::default(),
                dst: handle(),
                dst_subresource: layers,
                dst_offset: crate::Offset3d::default(),
                extent: crate::Extent3d::d2(1, 1),
            }),
            Command::ClearBuffer {
                buffer: handle(),
                offset: 0,
                size: 4,
            },
            Command::BeginRenderPass {
                label: None,
                color_attachments: Vec::new(),
                depth_stencil_attachment: None,
                render_area: Rect2d::from_size(1, 1),
                timestamp_writes: None,
            },
            Command::EndRenderPass,
            Command::SetViewport(Viewport::default()),
            Command::SetScissor(Rect2d::from_size(1, 1)),
            Command::SetStencilReference(0),
            Command::BindGraphicsPipeline(handle()),
            Command::BindIndexBuffer {
                buffer: handle(),
                offset: 0,
                format: IndexFormat::Uint32,
            },
            Command::BindGroup {
                slot: 0,
                group: handle(),
                dynamic_offsets: Vec::new(),
            },
            Command::PushConstants {
                stages: ShaderStages::VERTEX,
                offset: 0,
                len: 4,
            },
            Command::Draw {
                vertices: 0..3,
                instances: 0..1,
            },
            Command::DrawIndexed {
                indices: 0..3,
                base_vertex: 0,
                instances: 0..1,
            },
            Command::DrawIndirect(indirect),
            Command::DrawIndexedIndirect(indirect),
            Command::DrawIndirectCount(indirect_count),
            Command::DrawIndexedIndirectCount(indirect_count),
            Command::DrawMeshTasks { x: 1, y: 1, z: 1 },
            Command::DrawMeshTasksIndirect(indirect),
            Command::BeginComputePass {
                label: None,
                timestamp_writes: Some(crate::PassTimestampWrites {
                    set: handle(),
                    beginning_of_pass: 0,
                    end_of_pass: 1,
                }),
            },
            Command::EndComputePass,
            Command::BindComputePipeline(handle()),
            Command::Dispatch { x: 1, y: 1, z: 1 },
            Command::DispatchIndirect {
                args: handle(),
                offset: 0,
            },
            Command::ResetQuerySet {
                set: handle(),
                range: 0..1,
            },
            Command::ResolveQuerySet {
                set: handle(),
                range: 0..1,
                dst: handle(),
                dst_offset: 0,
            },
        ];

        for command in &samples {
            match command {
                Command::BeginDebugLabel(_)
                | Command::EndDebugLabel
                | Command::DebugMarker(_)
                | Command::Barrier { .. }
                | Command::CopyBufferToBuffer(_)
                | Command::CopyBufferToImage(_)
                | Command::CopyImageToBuffer(_)
                | Command::CopyImageToImage(_)
                | Command::ClearBuffer { .. }
                | Command::BeginRenderPass { .. }
                | Command::EndRenderPass
                | Command::SetViewport(_)
                | Command::SetScissor(_)
                | Command::SetStencilReference(_)
                | Command::BindGraphicsPipeline(_)
                | Command::BindIndexBuffer { .. }
                | Command::BindGroup { .. }
                | Command::PushConstants { .. }
                | Command::Draw { .. }
                | Command::DrawIndexed { .. }
                | Command::DrawIndirect(_)
                | Command::DrawIndexedIndirect(_)
                | Command::DrawIndirectCount(_)
                | Command::DrawIndexedIndirectCount(_)
                | Command::DrawMeshTasks { .. }
                | Command::DrawMeshTasksIndirect(_)
                | Command::BeginComputePass { .. }
                | Command::EndComputePass
                | Command::BindComputePipeline(_)
                | Command::Dispatch { .. }
                | Command::DispatchIndirect { .. }
                | Command::ResetQuerySet { .. }
                | Command::ResolveQuerySet { .. } => {}
            }
        }
        samples
    }

    /// A duplicated name would make every `command_names()` assertion in the
    /// suite ambiguous — two different commands would read as the same one, and
    /// an expected stream would match a wrong one.
    #[test]
    fn command_names_are_unique_per_variant() {
        let samples = every_command_variant();
        let mut names: Vec<_> = samples.iter().map(Command::name).collect();
        let count = names.len();
        assert!(names.iter().all(|name| !name.is_empty()));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two variants share a name");
    }

    /// The other half of the same claim: a variant that opens a pass says so,
    /// and every other variant says it does not.
    #[test]
    fn only_the_two_begin_commands_open_a_pass() {
        for command in every_command_variant() {
            let expected = match command.name() {
                "BeginRenderPass" => Some("render"),
                "BeginComputePass" => Some("compute"),
                _ => None,
            };
            assert_eq!(
                command.opens_pass().map(|(kind, _)| kind),
                expected,
                "{}",
                command.name()
            );
        }
    }

    #[test]
    fn validation_errors_render_readably() {
        let error = ValidationError::OutsidePass {
            command: "Draw",
            expected: "render",
        };
        assert_eq!(error.to_string(), "`Draw` requires an open render pass");
        let error = ValidationError::DeadHandle {
            command: "BindGroup",
            kind: ObjectKind::BindGroup,
            bits: 0x1_0000_0002,
        };
        assert!(error.to_string().contains("bind group"), "{error}");
    }
}
