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
    /// [`fill_buffer`](crate::CommandEncoder::fill_buffer).
    FillBuffer {
        /// Buffer filled.
        buffer: BufferHandle,
        /// Byte offset.
        offset: u64,
        /// Byte length.
        size: u64,
        /// Repeating value.
        value: u32,
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
    /// [`begin_compute_pass`](crate::CommandEncoder::begin_compute_pass).
    BeginComputePass {
        /// Pass label, if the caller gave one.
        label: Option<String>,
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
    /// [`write_timestamp`](crate::CommandEncoder::write_timestamp).
    WriteTimestamp {
        /// Query set.
        set: QuerySetHandle,
        /// Query index.
        index: u32,
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
            Self::FillBuffer { .. } => "FillBuffer",
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
            Self::BeginComputePass { .. } => "BeginComputePass",
            Self::EndComputePass => "EndComputePass",
            Self::BindComputePipeline(_) => "BindComputePipeline",
            Self::Dispatch { .. } => "Dispatch",
            Self::DispatchIndirect { .. } => "DispatchIndirect",
            Self::ResetQuerySet { .. } => "ResetQuerySet",
            Self::WriteTimestamp { .. } => "WriteTimestamp",
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
            Self::BeginComputePass { label } => Some(("compute", label.as_deref())),
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
    /// A query set: how many queries it holds, so a range can be checked.
    QuerySet { count: u32 },
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
    /// [`Instance::create_device`](crate::Instance::create_device) sees: it
    /// completes on the first poll.
    pub fn set_device_latency(&self, polls: u32) {
        self.lock().device_latency = polls;
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
            command: Command::BeginComputePass { label: None },
        });
        state.events.push(Event::Command {
            command_buffer: cb,
            command: Command::BeginRenderPass {
                label: Some("gbuffer".into()),
                color_attachments: Vec::new(),
                depth_stencil_attachment: None,
                render_area: Rect2d::from_size(1, 1),
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

    #[test]
    fn command_names_are_unique_per_variant() {
        // A duplicated name would make `command_names()` assertions ambiguous.
        let samples = [
            Command::EndDebugLabel,
            Command::EndRenderPass,
            Command::EndComputePass,
            Command::SetStencilReference(0),
            Command::Dispatch { x: 1, y: 1, z: 1 },
        ];
        let mut names: Vec<_> = samples.iter().map(Command::name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
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
