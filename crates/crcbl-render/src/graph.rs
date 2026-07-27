//! The render graph: passes declare what they touch, and the graph works out
//! the barriers.
//!
//! ```text
//! RenderGraph ──add_render_pass()──▶ declarations
//!      │                                  │
//!      │  compile()                       │  reads/writes of virtual resources
//!      ▼                                  ▼
//! CompiledGraph ──▶ ordered passes + exact barriers + layout transitions
//!      │                        │
//!      │  dump()                │  execute(device, pool, encoder)
//!      ▼                        ▼
//!   text a human reads    the only code in the engine that calls
//!                         `CommandEncoder::pipeline_barrier`
//! ```
//!
//! `docs/plan/02-vulkan-backend.md` §2.4 states the contract in one sentence:
//! "passes declare reads/writes of virtual resources; graph compiles to ordered
//! passes + exact `sync2` barriers + image layout transitions. **No manual
//! barriers outside the graph, ever.**" The sandbox used to hand-write two image
//! barriers around its frame; it no longer contains the word.
//!
//! # Deliberately not a scheduler
//!
//! The same section's risk list says: "**Graph over-engineering.** MVP graph =
//! linear pass list with computed barriers. No multi-queue scheduling, no
//! reordering. Resist." So:
//!
//! * **Passes execute in declaration order.** There is no topological sort,
//!   because there is nothing to sort — the order the caller wrote *is* the
//!   dependency order, and a graph that reordered would have to prove it had not
//!   changed the picture.
//! * **No dead-pass culling.** Declaring a pass runs it. Culling is the feature
//!   that makes a graph unpredictable ("why did my debug pass not run?") in
//!   exchange for saving work a caller could have simply not declared.
//! * **No multi-queue scheduling.** Every pass names a queue and the barrier
//!   model carries [`QueueTransfer`], so a dedicated transfer queue is additive
//!   later — which `docs/plan/02-vulkan-backend.md`'s corrections require "from
//!   the start so a dedicated transfer queue is additive later rather than a
//!   barrier-model rewrite". Nothing *decides* to use a second queue.
//!
//! What it does do is the part that is genuinely error-prone by hand: track
//! every resource's state, emit exactly the transitions that are needed, batch
//! them into one call per pass, alias transients whose lifetimes do not overlap,
//! and return every imported resource to the state its owner requires.
//!
//! # The physical resource outlives the frame
//!
//! A graph is built and thrown away every frame. The *images* are not: a
//! steady-state frame gets the same [`ImageHandle`] out of the
//! [`TransientPool`] it got last frame, and a graph is aliasing when two of its
//! virtual images land on one of them. So state tracking is per **physical**
//! resource — and a physical resource's history does not start at this frame's
//! first pass.
//!
//! Treating it as though it did is a real bug and not a subtle one. A barrier
//! whose source state is [`ResourceState::Undefined`] carries *no source scope*
//! at all — on Vulkan it expands to `srcStageMask = NONE, srcAccessMask = NONE`
//! — so a frame that opens by transitioning its depth buffer from `Undefined`
//! orders that transition against nothing, while the previous frame's
//! depth writes to the very same image may still be in flight. Two submissions,
//! no dependency, both writing: a write-after-write hazard that
//! `CRCBL_VK_SYNC_VALIDATION=1` reports as `write_barriers: 0`, and that a
//! driver is free to resolve either way.
//!
//! So the graph asks the pool what it left each physical resource in, and the
//! first barrier that touches one names *that* as its source:
//!
//! ```text
//! frame n-1 ─ forward pass writes depth ─▶ pool remembers DepthStencilWrite
//!                                                │
//! frame n   ─ compile(&pool) ────────────────────┘
//!             first barrier: DepthStencilWrite ──▶ DepthStencilWrite
//!             (a real source scope, so the two frames are ordered)
//! ```
//!
//! Aliasing *within* a frame is the same story one scale down, and used to have
//! the same hole: handing a slot from one virtual image to the next does not
//! change which `VkImage` it is, so the incoming resource's barrier must name
//! the state the outgoing one left, not `Undefined`. Discarding contents is only
//! free when there are contents worth discarding — with one handle and one
//! description there is nothing to discard, and pretending otherwise costs the
//! source scope that made the two passes ordered.
//!
//! **Imported resources are the owner's problem**, deliberately: the graph
//! cannot know what happened to a swapchain image before it was handed one, so
//! [`ImportedImage::initial`] is a declaration and the owner is the one who has
//! to be right. See its docs for what makes `Undefined` correct there.
//!
//! # Compilation is pure, and that is what makes it testable
//!
//! [`RenderGraph::compile`] takes no device and touches no GPU. It reads the
//! pool — a plain table of "what was this last used for" — and produces a
//! [`CompiledGraph`] whose barriers name *virtual* resources; only
//! [`CompiledGraph::execute`] resolves those to handles. That split is why
//! `docs/plan/12-testing.md` can call the graph-compile suite a
//! non-negotiable anchor and have it mean something: the interesting half runs
//! on any machine, in microseconds, with no ICD in the room — including the
//! cross-frame half, which needs two `compile` calls against one
//! [`TransientPool::new`] and no driver at all.
//!
//! # The graph explains itself
//!
//! [`CompiledGraph::dump`] prints the pass order, every barrier with its from
//! and to states, and which transients ended up sharing a physical resource.
//! `docs/plan/02-vulkan-backend.md` §2.4 asks for exactly this ("the graph must
//! be able to explain itself") and makes "graph dump readable and correct for
//! the sandbox frame" a P1 exit criterion.

use core::fmt;

use crcbl_hal::{
    Barriers, BufferBarrier, BufferHandle, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoder, ComputePassDesc, DepthStencilAttachment, Device, Format, HalError,
    ImageBarrier, ImageHandle, ImageSubresourceRange, ImageUsage, ImageViewHandle, LoadOp,
    QueueHandle, QueueTransfer, Rect2d, RenderPassDesc, ResourceState, StoreOp, Viewport, depth,
};

use crate::timing::PassTimers;
use crate::transient::{TransientBufferDesc, TransientImageDesc, TransientPool, TransientUse};

/// A virtual image inside one graph.
///
/// Only meaningful to the [`RenderGraph`] that issued it. Deliberately not a
/// [`Handle`](crcbl_core::Handle): a graph is built and thrown away every frame,
/// so there is no use-after-free to protect against, and a plain index keeps
/// compilation allocation-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(u32);

/// A virtual buffer inside one graph. See [`ImageId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(u32);

impl ImageId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

impl BufferId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// An image the graph did not create, and does not own.
///
/// The swapchain image is the canonical one: it belongs to the swapchain, it
/// arrives in an undefined layout, and it must leave in
/// [`ResourceState::Present`]. Saying that once, here, is what removes the
/// hand-written barriers from every frame loop in the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedImage {
    /// The image itself.
    pub image: ImageHandle,
    /// A view covering what the passes will render into or sample.
    pub view: ImageViewHandle,
    /// Texel format, which fixes the subresource aspects a barrier names.
    pub format: Format,
    /// Size in pixels, which fixes the render area of any pass that attaches it.
    pub extent: (u32, u32),
    /// The state it is in when the graph starts.
    ///
    /// [`ResourceState::Undefined`] for a freshly acquired swapchain image —
    /// which discards its contents, which is free, and which is the only
    /// correct source for one.
    ///
    /// Unlike a transient, this is a **declaration the owner makes**, not
    /// something the graph can look up: the graph never saw this image before
    /// today and has no way to know what was done to it. `Undefined` is right
    /// for an acquired swapchain image because the acquire's semaphore — waited
    /// on by the submission that runs this graph — already orders the frame
    /// after everything the presentation engine and the previous frame did with
    /// it, so the barrier has no ordering left to carry and only has to reach
    /// the layout. An importer with no such semaphore in the chain owes the
    /// graph the truth here instead, or it is asking for the same
    /// write-after-write the module docs describe.
    pub initial: ResourceState,
    /// The state the graph must leave it in.
    ///
    /// [`ResourceState::Present`] for a swapchain image. The graph emits a
    /// trailing barrier if the last pass left it somewhere else, and emits
    /// nothing if it did not.
    pub final_state: ResourceState,
}

/// A buffer the graph did not create, and does not own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedBuffer {
    /// The buffer itself.
    pub buffer: BufferHandle,
    /// The state it is in when the graph starts.
    pub initial: ResourceState,
    /// The state the graph must leave it in.
    pub final_state: ResourceState,
}

#[derive(Clone, Copy, Debug)]
enum ImageSource {
    Imported(ImportedImage),
    Transient(TransientImageDesc),
}

#[derive(Clone, Copy, Debug)]
enum BufferSource {
    Imported(ImportedBuffer),
    Transient(TransientBufferDesc),
}

#[derive(Clone, Debug)]
struct ImageNode {
    label: String,
    source: ImageSource,
}

#[derive(Clone, Debug)]
struct BufferNode {
    label: String,
    source: BufferSource,
}

impl ImageNode {
    fn format(&self) -> Format {
        match self.source {
            ImageSource::Imported(imported) => imported.format,
            ImageSource::Transient(desc) => desc.format,
        }
    }

    fn extent(&self) -> (u32, u32) {
        match self.source {
            ImageSource::Imported(imported) => imported.extent,
            ImageSource::Transient(desc) => desc.extent,
        }
    }
}

/// How a pass attaches an image, if it attaches it at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Attachment {
    /// A colour attachment at `index` in shader output order.
    Color {
        /// Output slot.
        index: u32,
        /// Start-of-pass behaviour.
        load: LoadOp,
        /// End-of-pass behaviour.
        store: StoreOp,
        /// Used when `load` is [`LoadOp::Clear`].
        clear: ClearValue,
    },
    /// The depth/stencil attachment.
    Depth {
        /// Start-of-pass behaviour.
        load: LoadOp,
        /// End-of-pass behaviour.
        store: StoreOp,
        /// Used when `load` is [`LoadOp::Clear`]. Defaults to the reversed-Z far
        /// plane; see [`crcbl_hal::depth`].
        clear: ClearValue,
        /// Whether the pass writes depth. `false` is a pass drawing against an
        /// existing prepass, and is the difference between
        /// [`ResourceState::DepthStencilWrite`] and
        /// [`ResourceState::DepthStencilRead`].
        write: bool,
    },
}

#[derive(Clone, Copy, Debug)]
struct ImageAccess {
    image: ImageId,
    state: ResourceState,
    attachment: Option<Attachment>,
}

#[derive(Clone, Copy, Debug)]
struct BufferAccess {
    buffer: BufferId,
    state: ResourceState,
}

/// Which scope a pass opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassKind {
    /// A render pass, with attachments.
    Render,
    /// A compute pass, with none.
    Compute,
}

impl PassKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Compute => "compute",
        }
    }
}

type PassBody<'a> = Box<dyn FnOnce(&mut PassContext<'_>) + 'a>;

struct Pass<'a> {
    label: String,
    kind: PassKind,
    queue: QueueHandle,
    images: Vec<ImageAccess>,
    buffers: Vec<BufferAccess>,
    body: PassBody<'a>,
}

impl fmt::Debug for Pass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pass")
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("images", &self.images)
            .field("buffers", &self.buffers)
            .finish_non_exhaustive()
    }
}

/// A frame's worth of passes, before compilation.
///
/// Built fresh every frame. The lifetime is the passes' closures': they borrow
/// whatever persistent state the renderer holds, and the graph never outlives a
/// frame, so nothing has to be cloned into them.
pub struct RenderGraph<'a> {
    images: Vec<ImageNode>,
    buffers: Vec<BufferNode>,
    passes: Vec<Pass<'a>>,
    default_queue: QueueHandle,
}

impl fmt::Debug for RenderGraph<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderGraph")
            .field("images", &self.images)
            .field("buffers", &self.buffers)
            .field("passes", &self.passes)
            .finish()
    }
}

impl<'a> RenderGraph<'a> {
    /// An empty graph whose passes default to `queue`.
    #[must_use]
    pub fn new(queue: QueueHandle) -> Self {
        Self {
            images: Vec::new(),
            buffers: Vec::new(),
            passes: Vec::new(),
            default_queue: queue,
        }
    }

    /// Declares an image the graph owns for the duration of the frame.
    ///
    /// Backed by the [`TransientPool`], and **aliased** with any other transient
    /// of the same description whose lifetime does not overlap this one.
    pub fn create_image(&mut self, label: impl Into<String>, desc: TransientImageDesc) -> ImageId {
        let id = ImageId(u32::try_from(self.images.len()).expect("a frame has few resources"));
        self.images.push(ImageNode {
            label: label.into(),
            source: ImageSource::Transient(desc),
        });
        id
    }

    /// Declares a buffer the graph owns for the duration of the frame.
    pub fn create_buffer(
        &mut self,
        label: impl Into<String>,
        desc: TransientBufferDesc,
    ) -> BufferId {
        let id = BufferId(u32::try_from(self.buffers.len()).expect("a frame has few resources"));
        self.buffers.push(BufferNode {
            label: label.into(),
            source: BufferSource::Transient(desc),
        });
        id
    }

    /// Brings an externally owned image into the graph.
    pub fn import_image(&mut self, label: impl Into<String>, imported: ImportedImage) -> ImageId {
        let id = ImageId(u32::try_from(self.images.len()).expect("a frame has few resources"));
        self.images.push(ImageNode {
            label: label.into(),
            source: ImageSource::Imported(imported),
        });
        id
    }

    /// Brings an externally owned buffer into the graph.
    pub fn import_buffer(
        &mut self,
        label: impl Into<String>,
        imported: ImportedBuffer,
    ) -> BufferId {
        let id = BufferId(u32::try_from(self.buffers.len()).expect("a frame has few resources"));
        self.buffers.push(BufferNode {
            label: label.into(),
            source: BufferSource::Imported(imported),
        });
        id
    }

    /// Starts declaring a render pass. Add it with
    /// [`PassBuilder::execute`].
    pub fn add_render_pass(&mut self, label: impl Into<String>) -> PassBuilder<'_, 'a> {
        PassBuilder::new(self, PassKind::Render, label.into())
    }

    /// Starts declaring a compute pass. Add it with
    /// [`PassBuilder::execute`].
    pub fn add_compute_pass(&mut self, label: impl Into<String>) -> PassBuilder<'_, 'a> {
        PassBuilder::new(self, PassKind::Compute, label.into())
    }

    /// How many passes have been declared.
    #[must_use]
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    /// Turns declarations into an ordered pass list with exact barriers.
    ///
    /// Pure: no device, no allocation of GPU memory, no side effects. See the
    /// module docs for why that split exists.
    ///
    /// `pool` is read, never touched: it is the same [`TransientPool`]
    /// [`CompiledGraph::execute`] will realise against, and the only thing
    /// asked of it is what it left each physical transient in last frame. That
    /// is what lets the first barrier of a frame carry a source scope covering
    /// the *previous* frame's writes rather than `Undefined`, which covers
    /// nothing — see the module docs. Passing a pool the graph will not execute
    /// against is the one way to get this wrong, which is why the parameter is
    /// the pool itself rather than a detachable snapshot of it.
    ///
    /// # Errors
    ///
    /// [`GraphError`] for a graph that cannot be executed as written —
    /// conflicting accesses within one pass, a render pass with no attachments
    /// or with attachments of different sizes, a transient nothing uses.
    pub fn compile(self, pool: &TransientPool) -> Result<CompiledGraph<'a>, GraphError> {
        compile(self, pool)
    }
}

/// Declares one pass. `#[must_use]` because a builder that is dropped rather
/// than [`executed`](PassBuilder::execute) adds nothing to the graph, and a
/// silently missing pass is a long afternoon.
#[must_use = "a pass reaches the graph only through `execute`"]
pub struct PassBuilder<'g, 'a> {
    graph: &'g mut RenderGraph<'a>,
    kind: PassKind,
    label: String,
    queue: QueueHandle,
    images: Vec<ImageAccess>,
    buffers: Vec<BufferAccess>,
    next_color: u32,
}

impl fmt::Debug for PassBuilder<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassBuilder")
            .field("label", &self.label)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl<'g, 'a> PassBuilder<'g, 'a> {
    fn new(graph: &'g mut RenderGraph<'a>, kind: PassKind, label: String) -> Self {
        let queue = graph.default_queue;
        Self {
            graph,
            kind,
            label,
            queue,
            images: Vec::new(),
            buffers: Vec::new(),
            next_color: 0,
        }
    }

    /// Runs this pass on a queue other than the graph's default.
    ///
    /// Nothing in the MVP does — uploads share the graphics+compute queue, as
    /// `docs/plan/02-vulkan-backend.md`'s corrections state rather than assume.
    /// It exists because the *barrier model* has to represent queue-family
    /// acquire/release from the start, and it does: a resource whose last user
    /// was on another queue gets a release/acquire pair rather than a plain
    /// transition.
    pub fn on_queue(mut self, queue: QueueHandle) -> Self {
        self.queue = queue;
        self
    }

    /// Attaches `image` as the next colour attachment.
    pub fn color(
        mut self,
        image: ImageId,
        load: LoadOp,
        store: StoreOp,
        clear: ClearValue,
    ) -> Self {
        let index = self.next_color;
        self.next_color += 1;
        self.images.push(ImageAccess {
            image,
            state: ResourceState::ColorAttachment,
            attachment: Some(Attachment::Color {
                index,
                load,
                store,
                clear,
            }),
        });
        self
    }

    /// Attaches `image` as the next colour attachment, cleared to `rgba` and
    /// stored.
    pub fn clear_color(self, image: ImageId, rgba: [f32; 4]) -> Self {
        self.color(
            image,
            LoadOp::Clear,
            StoreOp::Store,
            ClearValue::color(rgba),
        )
    }

    /// Attaches `image` as the depth attachment with depth writes on.
    pub fn depth(
        mut self,
        image: ImageId,
        load: LoadOp,
        store: StoreOp,
        clear: ClearValue,
    ) -> Self {
        self.images.push(ImageAccess {
            image,
            state: ResourceState::DepthStencilWrite,
            attachment: Some(Attachment::Depth {
                load,
                store,
                clear,
                write: true,
            }),
        });
        self
    }

    /// Attaches `image` as the depth attachment, cleared to the **reversed-Z**
    /// far plane and discarded at the end of the pass.
    ///
    /// [`depth::CLEAR`] rather than a literal, and discarded rather than stored
    /// because nothing reads a depth buffer after the pass that wrote it until
    /// there is a prepass to read — at which point [`PassBuilder::depth`] takes
    /// a [`StoreOp`] and says so.
    pub fn clear_depth(self, image: ImageId) -> Self {
        self.depth(
            image,
            LoadOp::Clear,
            StoreOp::Discard,
            ClearValue {
                depth: depth::CLEAR,
                ..ClearValue::default()
            },
        )
    }

    /// Attaches `image` as a **read-only** depth attachment: tested, not
    /// written.
    pub fn depth_read(mut self, image: ImageId) -> Self {
        self.images.push(ImageAccess {
            image,
            state: ResourceState::DepthStencilRead,
            attachment: Some(Attachment::Depth {
                load: LoadOp::Load,
                store: StoreOp::Store,
                clear: ClearValue::default(),
                write: false,
            }),
        });
        self
    }

    /// Declares that this pass samples or otherwise shader-reads `image`.
    pub fn read_image(self, image: ImageId) -> Self {
        self.use_image(image, ResourceState::ShaderRead)
    }

    /// Declares an image access in an arbitrary state — a storage-image write, a
    /// copy destination, anything the named helpers do not cover.
    pub fn use_image(mut self, image: ImageId, state: ResourceState) -> Self {
        self.images.push(ImageAccess {
            image,
            state,
            attachment: None,
        });
        self
    }

    /// Declares that this pass shader-reads `buffer` — a uniform block, a
    /// pulled vertex stream, an instance array.
    pub fn read_buffer(self, buffer: BufferId) -> Self {
        self.use_buffer(buffer, ResourceState::ShaderRead)
    }

    /// Declares that this pass reads `buffer` as an index buffer.
    pub fn index_buffer(self, buffer: BufferId) -> Self {
        self.use_buffer(buffer, ResourceState::IndexBuffer)
    }

    /// Declares a buffer access in an arbitrary state.
    pub fn use_buffer(mut self, buffer: BufferId, state: ResourceState) -> Self {
        self.buffers.push(BufferAccess { buffer, state });
        self
    }

    /// Adds the pass to the graph, with `body` as the recording callback.
    ///
    /// `body` runs inside the pass scope: the graph has already emitted the
    /// barriers, opened the pass and set a full-target viewport and scissor, so
    /// a body is normally a pipeline bind, a bind group and a draw.
    pub fn execute(self, body: impl FnOnce(&mut PassContext<'_>) + 'a) {
        self.graph.passes.push(Pass {
            label: self.label,
            kind: self.kind,
            queue: self.queue,
            images: self.images,
            buffers: self.buffers,
            body: Box::new(body),
        });
    }
}

// --- compiled form ---------------------------------------------------------

/// A barrier the graph computed, in terms of a virtual image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphImageBarrier {
    /// Image transitioned.
    pub image: ImageId,
    /// State being left.
    pub from: ResourceState,
    /// State being entered.
    pub to: ResourceState,
    /// Queue ownership transfer, when the previous user was on another queue.
    pub queue_transfer: Option<QueueTransfer>,
}

/// A barrier the graph computed, in terms of a virtual buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphBufferBarrier {
    /// Buffer transitioned.
    pub buffer: BufferId,
    /// State being left.
    pub from: ResourceState,
    /// State being entered.
    pub to: ResourceState,
    /// Queue ownership transfer, when the previous user was on another queue.
    pub queue_transfer: Option<QueueTransfer>,
}

/// One batch of barriers, emitted as a single
/// [`pipeline_barrier`](CommandEncoder::pipeline_barrier) call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphBarriers {
    /// Image transitions, in declaration order.
    pub images: Vec<GraphImageBarrier>,
    /// Buffer transitions, in declaration order.
    pub buffers: Vec<GraphBufferBarrier>,
}

impl GraphBarriers {
    /// Whether this batch would do nothing, in which case the graph emits no
    /// call at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty() && self.buffers.is_empty()
    }

    /// How many transitions it carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.images.len() + self.buffers.len()
    }
}

/// One pass, after compilation.
pub struct CompiledPass<'a> {
    label: String,
    kind: PassKind,
    queue: QueueHandle,
    /// Barriers emitted **before** this pass opens.
    barriers: GraphBarriers,
    images: Vec<ImageAccess>,
    buffers: Vec<BufferAccess>,
    render_area: Rect2d,
    body: PassBody<'a>,
}

impl fmt::Debug for CompiledPass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledPass")
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("barriers", &self.barriers)
            .finish_non_exhaustive()
    }
}

impl CompiledPass<'_> {
    /// The pass's name, as it appears in captures and in the dump.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether it is a render or a compute pass.
    #[must_use]
    pub const fn kind(&self) -> PassKind {
        self.kind
    }

    /// The barriers emitted immediately before it.
    #[must_use]
    pub const fn barriers(&self) -> &GraphBarriers {
        &self.barriers
    }

    /// The queue it was declared on.
    ///
    /// Always the graph's default in the MVP — uploads share the
    /// graphics+compute queue — but the field is what makes a second queue
    /// additive rather than a barrier-model rewrite, which
    /// `docs/plan/02-vulkan-backend.md`'s corrections require from the start.
    #[must_use]
    pub const fn queue(&self) -> QueueHandle {
        self.queue
    }

    /// The region a render pass covers. Zero-sized for a compute pass.
    #[must_use]
    pub const fn render_area(&self) -> Rect2d {
        self.render_area
    }
}

/// Which physical resource a virtual one ended up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// Externally owned; no pool involvement.
    Imported,
    /// Index into the compiled graph's transient slot list.
    Transient(usize),
}

/// A compiled graph: ordered passes, exact barriers, and a transient plan.
pub struct CompiledGraph<'a> {
    images: Vec<ImageNode>,
    buffers: Vec<BufferNode>,
    passes: Vec<CompiledPass<'a>>,
    /// Barriers returning imported resources to their required final states.
    final_barriers: GraphBarriers,
    image_slots: Vec<Slot>,
    buffer_slots: Vec<Slot>,
    transient_images: Vec<TransientImageDesc>,
    transient_buffers: Vec<TransientBufferDesc>,
    /// What this frame leaves each physical transient image in, parallel to
    /// `transient_images`. Handed to the pool by
    /// [`CompiledGraph::execute`] so the *next* frame's first barrier has a
    /// source scope; computed here because compilation is what knows it.
    transient_image_end: Vec<TransientUse>,
    /// The same for buffers.
    transient_buffer_end: Vec<TransientUse>,
}

impl fmt::Debug for CompiledGraph<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledGraph")
            .field("passes", &self.passes)
            .field("final_barriers", &self.final_barriers)
            .field("transient_images", &self.transient_images)
            .field("transient_buffers", &self.transient_buffers)
            .finish_non_exhaustive()
    }
}

impl<'a> CompiledGraph<'a> {
    /// The compiled passes, in execution order.
    #[must_use]
    pub fn passes(&self) -> &[CompiledPass<'a>] {
        &self.passes
    }

    /// The barriers emitted after the last pass, returning imported resources to
    /// the states their owners require.
    #[must_use]
    pub const fn final_barriers(&self) -> &GraphBarriers {
        &self.final_barriers
    }

    /// How many physical images the transients were packed onto.
    ///
    /// Lower than the number of transients declared exactly when aliasing
    /// happened, which is what makes the saving assertable.
    #[must_use]
    pub fn physical_image_count(&self) -> usize {
        self.transient_images.len()
    }

    /// How many physical buffers the transients were packed onto.
    #[must_use]
    pub fn physical_buffer_count(&self) -> usize {
        self.transient_buffers.len()
    }

    /// Whether two virtual images share one physical image.
    #[must_use]
    pub fn images_alias(&self, first: ImageId, second: ImageId) -> bool {
        matches!(
            (
                self.image_slots.get(first.index()),
                self.image_slots.get(second.index())
            ),
            (Some(Slot::Transient(a)), Some(Slot::Transient(b))) if a == b
        )
    }

    /// Every barrier the frame will emit, batch by batch, ending with the final
    /// batch. The shape a test asserts on.
    #[must_use]
    pub fn barrier_batches(&self) -> Vec<&GraphBarriers> {
        self.passes
            .iter()
            .map(|pass| &pass.barriers)
            .chain(core::iter::once(&self.final_barriers))
            .filter(|batch| !batch.is_empty())
            .collect()
    }

    /// The graph, as text a human reads.
    ///
    /// `docs/plan/02-vulkan-backend.md` §2.4's debug-tools principle: "the graph
    /// must be able to explain itself", and its exit criteria require the dump
    /// to be "readable and correct for the sandbox frame". Every line is derived
    /// from the compiled form rather than re-walked from the declarations, so a
    /// dump that says a barrier happened is evidence that it will.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write_dump(&mut out).expect("writing to a String");
        out
    }

    fn write_dump(&self, out: &mut impl fmt::Write) -> fmt::Result {
        let aliased = self
            .image_slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Transient(_)))
            .count()
            .saturating_sub(self.transient_images.len());
        writeln!(
            out,
            "render graph: {} pass(es), {} transient image(s) on {} physical, \
             {} transient buffer(s) on {} physical, {} barrier(s)",
            self.passes.len(),
            self.transient_image_users(),
            self.transient_images.len(),
            self.transient_buffer_users(),
            self.transient_buffers.len(),
            self.passes
                .iter()
                .map(|pass| pass.barriers.len())
                .sum::<usize>()
                + self.final_barriers.len(),
        )?;
        if aliased > 0 {
            writeln!(out, "  aliasing saved {aliased} physical image(s)")?;
        }

        for (index, pass) in self.passes.iter().enumerate() {
            self.write_barriers(out, &pass.barriers, &format!("[{index}] "))?;
            writeln!(
                out,
                "[{index}] {} pass {:?}  {}x{}",
                pass.kind.name(),
                pass.label,
                pass.render_area.width,
                pass.render_area.height
            )?;
            for access in &pass.images {
                let node = &self.images[access.image.index()];
                let role = match access.attachment {
                    Some(Attachment::Color {
                        index, load, store, ..
                    }) => {
                        format!("color[{index}] load={load:?} store={store:?}")
                    }
                    Some(Attachment::Depth {
                        load, store, write, ..
                    }) => format!(
                        "depth load={load:?} store={store:?} write={}",
                        if write { "yes" } else { "no" }
                    ),
                    None => "sampled/storage".to_string(),
                };
                writeln!(
                    out,
                    "        image  {:<20} {:<20} {role} [{}]",
                    node.label,
                    format!("{:?}", access.state),
                    self.slot_note(self.image_slots[access.image.index()]),
                )?;
            }
            for access in &pass.buffers {
                let node = &self.buffers[access.buffer.index()];
                writeln!(
                    out,
                    "        buffer {:<20} {:<20} [{}]",
                    node.label,
                    format!("{:?}", access.state),
                    self.slot_note(self.buffer_slots[access.buffer.index()]),
                )?;
            }
        }

        self.write_barriers(out, &self.final_barriers, "[final] ")?;
        Ok(())
    }

    fn transient_image_users(&self) -> usize {
        self.image_slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Transient(_)))
            .count()
    }

    fn transient_buffer_users(&self) -> usize {
        self.buffer_slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Transient(_)))
            .count()
    }

    fn slot_note(&self, slot: Slot) -> String {
        match slot {
            Slot::Imported => "imported".to_string(),
            Slot::Transient(index) => format!("transient #{index}"),
        }
    }

    fn write_barriers(
        &self,
        out: &mut impl fmt::Write,
        barriers: &GraphBarriers,
        prefix: &str,
    ) -> fmt::Result {
        if barriers.is_empty() {
            return Ok(());
        }
        writeln!(out, "{prefix}barriers ({})", barriers.len())?;
        for barrier in &barriers.images {
            writeln!(
                out,
                "        image  {:<20} {:>18} -> {:<18}{}",
                self.images[barrier.image.index()].label,
                format!("{:?}", barrier.from),
                format!("{:?}", barrier.to),
                queue_note(barrier.queue_transfer),
            )?;
        }
        for barrier in &barriers.buffers {
            writeln!(
                out,
                "        buffer {:<20} {:>18} -> {:<18}{}",
                self.buffers[barrier.buffer.index()].label,
                format!("{:?}", barrier.from),
                format!("{:?}", barrier.to),
                queue_note(barrier.queue_transfer),
            )?;
        }
        Ok(())
    }

    /// Records the whole frame into `encoder`.
    ///
    /// Realises the transients out of `pool`, then for each pass in order:
    /// writes a start timestamp, emits its barrier batch as one call, opens the
    /// pass, sets a full-target viewport and scissor, runs the body, closes the
    /// pass and writes an end timestamp. Finally emits the barriers that return
    /// imported resources to their required states, and tells `pool` what this
    /// frame left each physical transient in — which is what the *next* frame's
    /// first barrier uses as its source scope.
    ///
    /// `pool` must be the one [`RenderGraph::compile`] was given, or the
    /// barriers name states the resources are not in.
    ///
    /// The encoder is left outside any pass, with nothing to clean up.
    ///
    /// # Errors
    ///
    /// [`GraphError::Hal`] if the pool could not create a transient. Nothing
    /// else can fail here — everything checkable was checked at compile time.
    pub fn execute(
        self,
        device: &dyn Device,
        pool: &mut TransientPool,
        encoder: &mut dyn CommandEncoder,
        mut timers: Option<&mut PassTimers>,
    ) -> Result<(), GraphError> {
        let realised = self.realise(device, pool)?;
        let Self {
            images,
            passes,
            final_barriers,
            transient_images,
            transient_buffers,
            transient_image_end,
            transient_buffer_end,
            ..
        } = self;

        if let Some(timers) = timers.as_deref_mut() {
            timers.begin_frame(device, encoder, &passes);
        }

        for (index, pass) in passes.into_iter().enumerate() {
            if let Some(timers) = timers.as_deref_mut() {
                timers.pass_begin(encoder, index);
            }
            emit(encoder, &images, &realised, &pass.barriers);

            match pass.kind {
                PassKind::Render => {
                    let (colors, depth_stencil) = attachments(&realised, &pass.images);
                    encoder.begin_render_pass(&RenderPassDesc {
                        label: Some(&pass.label),
                        color_attachments: &colors,
                        depth_stencil_attachment: depth_stencil,
                        render_area: pass.render_area,
                    });
                    // Dynamic state, set by the graph rather than by every pass
                    // body: a pipeline that baked a viewport in would have to be
                    // rebuilt on every resize, and every body would otherwise
                    // repeat the same two lines with the same chance of using
                    // the wrong extent.
                    encoder.set_viewport(&Viewport::from_size(
                        pass.render_area.width,
                        pass.render_area.height,
                    ));
                    encoder.set_scissor(&pass.render_area);
                }
                PassKind::Compute => {
                    encoder.begin_compute_pass(&ComputePassDesc {
                        label: Some(&pass.label),
                    });
                }
            }

            {
                let mut context = PassContext {
                    device,
                    encoder,
                    realised: &realised,
                    render_area: pass.render_area,
                };
                (pass.body)(&mut context);
            }

            match pass.kind {
                PassKind::Render => encoder.end_render_pass(),
                PassKind::Compute => encoder.end_compute_pass(),
            }
            if let Some(timers) = timers.as_deref_mut() {
                timers.pass_end(encoder, index);
            }
        }

        emit(encoder, &images, &realised, &final_barriers);

        // The handover to the next frame. It happens after the commands are
        // recorded rather than during compilation because a graph that is
        // compiled and dropped ran nothing, and a pool that remembered a frame
        // that never happened would order the following frame against writes
        // the GPU never performed.
        for (slot, desc) in transient_images.iter().enumerate() {
            pool.set_image_use(
                *desc,
                ordinal(&transient_images, slot),
                transient_image_end[slot],
            );
        }
        for (slot, desc) in transient_buffers.iter().enumerate() {
            pool.set_buffer_use(
                *desc,
                ordinal(&transient_buffers, slot),
                transient_buffer_end[slot],
            );
        }
        Ok(())
    }

    fn realise(
        &self,
        device: &dyn Device,
        pool: &mut TransientPool,
    ) -> Result<Realised, GraphError> {
        pool.begin_frame();
        let mut physical_images = Vec::with_capacity(self.transient_images.len());
        for desc in &self.transient_images {
            physical_images.push(pool.image(device, *desc)?);
        }
        let mut physical_buffers = Vec::with_capacity(self.transient_buffers.len());
        for desc in &self.transient_buffers {
            physical_buffers.push(pool.buffer(device, *desc)?);
        }

        let images = self
            .images
            .iter()
            .zip(&self.image_slots)
            .map(|(node, slot)| match (slot, node.source) {
                (Slot::Imported, ImageSource::Imported(imported)) => {
                    (imported.image, imported.view)
                }
                (Slot::Transient(index), _) => physical_images[*index],
                // Unreachable: `compile` assigns `Imported` only to imported
                // nodes. Stated as a panic rather than an `unwrap` so the
                // invariant is named where it is relied on.
                (Slot::Imported, ImageSource::Transient(_)) => {
                    unreachable!("a transient image was assigned the imported slot")
                }
            })
            .collect();
        let buffers = self
            .buffers
            .iter()
            .zip(&self.buffer_slots)
            .map(|(node, slot)| match (slot, node.source) {
                (Slot::Imported, BufferSource::Imported(imported)) => imported.buffer,
                (Slot::Transient(index), _) => physical_buffers[*index],
                (Slot::Imported, BufferSource::Transient(_)) => {
                    unreachable!("a transient buffer was assigned the imported slot")
                }
            })
            .collect();

        Ok(Realised { images, buffers })
    }
}

fn queue_note(transfer: Option<QueueTransfer>) -> String {
    transfer.map_or_else(String::new, |transfer| {
        format!("  queue {:?} -> {:?}", transfer.from, transfer.to)
    })
}

/// Virtual resources resolved to real handles, for the duration of one
/// [`CompiledGraph::execute`].
#[derive(Debug)]
struct Realised {
    images: Vec<(ImageHandle, ImageViewHandle)>,
    buffers: Vec<BufferHandle>,
}

/// What a pass body is handed.
///
/// It has an encoder already inside the pass scope, the device (for the rare
/// body that has to build something against a transient's handle), and a way to
/// turn a virtual id into the handle it landed on.
#[derive(Debug)]
pub struct PassContext<'a> {
    device: &'a dyn Device,
    encoder: &'a mut dyn CommandEncoder,
    realised: &'a Realised,
    render_area: Rect2d,
}

impl PassContext<'_> {
    /// The encoder, already inside this pass's scope.
    pub fn encoder(&mut self) -> &mut dyn CommandEncoder {
        self.encoder
    }

    /// The device, for a body that must create something against a transient's
    /// handle — a bind group naming a graph-owned image view, typically.
    #[must_use]
    pub fn device(&self) -> &dyn Device {
        self.device
    }

    /// The handle a virtual image landed on.
    ///
    /// # Panics
    ///
    /// If `id` came from a different graph.
    #[must_use]
    pub fn image(&self, id: ImageId) -> ImageHandle {
        self.realised.images[id.index()].0
    }

    /// The view of a virtual image.
    ///
    /// Stable frame to frame while the graph's shape is, which is what lets a
    /// caller cache a bind group against it and rebuild only on a resize.
    ///
    /// # Panics
    ///
    /// If `id` came from a different graph.
    #[must_use]
    pub fn image_view(&self, id: ImageId) -> ImageViewHandle {
        self.realised.images[id.index()].1
    }

    /// The handle a virtual buffer landed on.
    ///
    /// # Panics
    ///
    /// If `id` came from a different graph.
    #[must_use]
    pub fn buffer(&self, id: BufferId) -> BufferHandle {
        self.realised.buffers[id.index()]
    }

    /// The region this pass renders, in pixels. The viewport and scissor are
    /// already set to it.
    #[must_use]
    pub const fn render_area(&self) -> Rect2d {
        self.render_area
    }
}

fn emit(
    encoder: &mut dyn CommandEncoder,
    images: &[ImageNode],
    realised: &Realised,
    barriers: &GraphBarriers,
) {
    if barriers.is_empty() {
        return;
    }
    let image_barriers: Vec<ImageBarrier> = barriers
        .images
        .iter()
        .map(|barrier| ImageBarrier {
            image: realised.images[barrier.image.index()].0,
            range: ImageSubresourceRange::all(images[barrier.image.index()].format()),
            from: barrier.from,
            to: barrier.to,
            queue_transfer: barrier.queue_transfer,
        })
        .collect();
    let buffer_barriers: Vec<BufferBarrier> = barriers
        .buffers
        .iter()
        .map(|barrier| BufferBarrier {
            buffer: realised.buffers[barrier.buffer.index()],
            from: barrier.from,
            to: barrier.to,
            queue_transfer: barrier.queue_transfer,
        })
        .collect();
    encoder.pipeline_barrier(&Barriers {
        buffers: &buffer_barriers,
        images: &image_barriers,
        global: false,
    });
}

fn attachments(
    realised: &Realised,
    accesses: &[ImageAccess],
) -> (Vec<ColorAttachment>, Option<DepthStencilAttachment>) {
    let mut colors: Vec<(u32, ColorAttachment)> = Vec::new();
    let mut depth_stencil = None;
    for access in accesses {
        let view = realised.images[access.image.index()].1;
        match access.attachment {
            Some(Attachment::Color {
                index,
                load,
                store,
                clear,
            }) => colors.push((
                index,
                ColorAttachment {
                    view,
                    resolve: None,
                    load,
                    store,
                    clear,
                },
            )),
            Some(Attachment::Depth {
                load, store, clear, ..
            }) => {
                depth_stencil = Some(DepthStencilAttachment {
                    view,
                    depth_load: load,
                    depth_store: store,
                    // No stencil plane in anything the engine renders yet; a
                    // `D32_SFLOAT` attachment has none to load or store.
                    stencil_load: LoadOp::DontCare,
                    stencil_store: StoreOp::Discard,
                    clear,
                });
            }
            None => {}
        }
    }
    // Attachment order in the descriptor is shader-output order, which is the
    // `index` a caller gave — not the order it happened to declare them in.
    colors.sort_by_key(|(index, _)| *index);
    (
        colors
            .into_iter()
            .map(|(_, attachment)| attachment)
            .collect(),
        depth_stencil,
    )
}

// --- compilation -----------------------------------------------------------

/// Why a graph could not be compiled.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// One pass declared two different states for the same resource.
    #[error(
        "pass `{pass}` accesses `{resource}` as both {first:?} and {second:?}; a resource is in \
         one state per pass, so this needs two passes or one state"
    )]
    ConflictingAccess {
        /// Pass label.
        pass: String,
        /// Resource label.
        resource: String,
        /// The first state declared.
        first: ResourceState,
        /// The second, incompatible state.
        second: ResourceState,
    },
    /// A render pass attached nothing.
    #[error(
        "render pass `{pass}` has no attachments; a pass that renders nowhere is a declaration \
         mistake, not an optimisation"
    )]
    NoAttachments {
        /// Pass label.
        pass: String,
    },
    /// A render pass attached images of different sizes.
    #[error(
        "render pass `{pass}` attaches `{first}` at {first_extent:?} and `{second}` at \
         {second_extent:?}; every attachment must be the same size"
    )]
    AttachmentExtentMismatch {
        /// Pass label.
        pass: String,
        /// The first attachment's label.
        first: String,
        /// Its size.
        first_extent: (u32, u32),
        /// The disagreeing attachment's label.
        second: String,
        /// Its size.
        second_extent: (u32, u32),
    },
    /// A render pass attached two depth images.
    #[error("render pass `{pass}` attaches more than one depth image")]
    DuplicateDepthAttachment {
        /// Pass label.
        pass: String,
    },
    /// A compute pass attached something.
    #[error("compute pass `{pass}` attaches `{resource}`; compute passes have no attachments")]
    AttachmentInComputePass {
        /// Pass label.
        pass: String,
        /// Resource label.
        resource: String,
    },
    /// A transient was declared and never used, so it has no lifetime and no
    /// reason to exist.
    #[error(
        "`{resource}` is a transient no pass reads or writes; either a pass forgot to declare it \
         or the resource should not have been created"
    )]
    UnusedTransient {
        /// Resource label.
        resource: String,
    },
    /// The seam refused something at execution time.
    #[error(transparent)]
    Hal(#[from] HalError),
}

/// Per-physical-resource tracking during compilation.
///
/// Keyed on the *physical* resource, and seeded from what that resource was
/// last used for — [`ImportedImage::initial`] for an import, the pool's memory
/// of the previous frame for a transient. There is no "and now the slot is
/// fresh" case: a slot handed from one virtual resource to another is the same
/// object, in the state the previous one left it, and the module docs explain
/// why pretending otherwise loses the barrier.
#[derive(Clone, Copy, Debug)]
struct Tracked {
    state: ResourceState,
    queue: Option<QueueHandle>,
}

impl Tracked {
    const fn carried(last: TransientUse) -> Self {
        Self {
            state: last.state,
            queue: last.queue,
        }
    }

    const fn end(self) -> TransientUse {
        TransientUse {
            state: self.state,
            queue: self.queue,
        }
    }
}

/// How many earlier entries of `descs` carry the same description as `descs[at]`.
///
/// The pool hands out the *n*-th resource of a description for the *n*-th
/// request, and [`CompiledGraph::realise`] requests them in slot order — so this
/// is the number that turns "physical slot 3" into "the pool entry slot 3 will
/// get", both before it has been realised and after.
fn ordinal<D: PartialEq>(descs: &[D], at: usize) -> usize {
    descs[..at]
        .iter()
        .filter(|other| **other == descs[at])
        .count()
}

fn compile<'a>(
    graph: RenderGraph<'a>,
    pool: &TransientPool,
) -> Result<CompiledGraph<'a>, GraphError> {
    let RenderGraph {
        images,
        buffers,
        passes,
        default_queue: _,
    } = graph;

    validate(&images, &buffers, &passes)?;

    let image_lifetimes = lifetimes(passes.len(), passes.iter().map(|pass| &pass.images), |a| {
        a.image.0
    });
    let buffer_lifetimes = lifetimes(passes.len(), passes.iter().map(|pass| &pass.buffers), |a| {
        a.buffer.0
    });

    // Aliasing: pack transients onto as few physical resources as their
    // lifetimes allow.
    let mut transient_images = Vec::new();
    let image_slots = assign(
        &images,
        &image_lifetimes,
        |node| match node.source {
            ImageSource::Transient(desc) => Some(desc),
            ImageSource::Imported(_) => None,
        },
        |node| node.label.clone(),
        &mut transient_images,
    )?;
    let mut transient_buffers = Vec::new();
    let buffer_slots = assign(
        &buffers,
        &buffer_lifetimes,
        |node| match node.source {
            BufferSource::Transient(desc) => Some(desc),
            BufferSource::Imported(_) => None,
        },
        |node| node.label.clone(),
        &mut transient_buffers,
    )?;

    // State tracking is per **physical** resource, because that is what a
    // barrier actually names. Imported resources get one tracker each (keyed by
    // their own id); transients share one per physical slot.
    //
    // Every tracker starts where its resource actually is. For an import that
    // is what the owner declared; for a transient it is what the pool remembers
    // of the last frame that used the very same handle, which is the whole of
    // the cross-frame fix.
    let mut image_state: Vec<Tracked> = images
        .iter()
        .zip(&image_slots)
        .map(|(node, slot)| Tracked {
            state: match (slot, node.source) {
                (Slot::Imported, ImageSource::Imported(imported)) => imported.initial,
                _ => ResourceState::Undefined,
            },
            queue: None,
        })
        .collect();
    let mut transient_image_state: Vec<Tracked> = (0..transient_images.len())
        .map(|slot| {
            Tracked::carried(
                pool.image_use(transient_images[slot], ordinal(&transient_images, slot)),
            )
        })
        .collect();
    let mut buffer_state: Vec<Tracked> = buffers
        .iter()
        .zip(&buffer_slots)
        .map(|(node, slot)| Tracked {
            state: match (slot, node.source) {
                (Slot::Imported, BufferSource::Imported(imported)) => imported.initial,
                _ => ResourceState::Undefined,
            },
            queue: None,
        })
        .collect();
    let mut transient_buffer_state: Vec<Tracked> = (0..transient_buffers.len())
        .map(|slot| {
            Tracked::carried(
                pool.buffer_use(transient_buffers[slot], ordinal(&transient_buffers, slot)),
            )
        })
        .collect();

    let mut compiled = Vec::with_capacity(passes.len());
    for pass in passes {
        let mut barriers = GraphBarriers::default();

        for access in &pass.images {
            let tracked = tracker(
                image_slots[access.image.index()],
                access.image.0,
                &mut image_state,
                &mut transient_image_state,
            );
            if let Some(barrier) = transition(tracked, access.state, pass.queue) {
                barriers.images.push(GraphImageBarrier {
                    image: access.image,
                    from: barrier.0,
                    to: access.state,
                    queue_transfer: barrier.1,
                });
            }
        }
        for access in &pass.buffers {
            let tracked = tracker(
                buffer_slots[access.buffer.index()],
                access.buffer.0,
                &mut buffer_state,
                &mut transient_buffer_state,
            );
            if let Some(barrier) = transition(tracked, access.state, pass.queue) {
                barriers.buffers.push(GraphBufferBarrier {
                    buffer: access.buffer,
                    from: barrier.0,
                    to: access.state,
                    queue_transfer: barrier.1,
                });
            }
        }

        let render_area = render_area(&images, &pass)?;
        compiled.push(CompiledPass {
            label: pass.label,
            kind: pass.kind,
            queue: pass.queue,
            barriers,
            images: pass.images,
            buffers: pass.buffers,
            render_area,
            body: pass.body,
        });
    }

    // Return every imported resource to the state its owner requires. This is
    // the barrier that used to be hand-written after the render pass in
    // `apps/sandbox`, and the reason it can be computed rather than remembered.
    let mut final_barriers = GraphBarriers::default();
    for (index, node) in images.iter().enumerate() {
        let ImageSource::Imported(imported) = node.source else {
            continue;
        };
        let tracked = image_state[index];
        // `!=` rather than `needs_barrier`: this transition guards nothing —
        // the graph is finished with the resource — so its only job is to leave
        // the layout its owner asked for. A write-after-write between two passes
        // is a hazard and gets a barrier; a write followed by *nothing* is not.
        if tracked.state != imported.final_state {
            final_barriers.images.push(GraphImageBarrier {
                image: ImageId(u32::try_from(index).expect("a frame has few resources")),
                from: tracked.state,
                to: imported.final_state,
                queue_transfer: None,
            });
        }
    }
    for (index, node) in buffers.iter().enumerate() {
        let BufferSource::Imported(imported) = node.source else {
            continue;
        };
        let tracked = buffer_state[index];
        if tracked.state != imported.final_state {
            final_barriers.buffers.push(GraphBufferBarrier {
                buffer: BufferId(u32::try_from(index).expect("a frame has few resources")),
                from: tracked.state,
                to: imported.final_state,
                queue_transfer: None,
            });
        }
    }

    // What the pool is told once these commands are actually recorded. Read off
    // the trackers rather than re-derived, so the state the next frame barriers
    // against is by construction the state this frame's last barrier reached.
    let transient_image_end = transient_image_state.iter().map(|t| t.end()).collect();
    let transient_buffer_end = transient_buffer_state.iter().map(|t| t.end()).collect();

    Ok(CompiledGraph {
        images,
        buffers,
        passes: compiled,
        final_barriers,
        image_slots,
        buffer_slots,
        transient_images,
        transient_buffers,
        transient_image_end,
        transient_buffer_end,
    })
}

fn tracker<'s>(
    slot: Slot,
    id: u32,
    per_resource: &'s mut [Tracked],
    per_slot: &'s mut [Tracked],
) -> &'s mut Tracked {
    match slot {
        Slot::Imported => &mut per_resource[id as usize],
        Slot::Transient(index) => &mut per_slot[index],
    }
}

/// Decides whether a transition is needed, and updates the tracker.
///
/// Returns `(from, queue_transfer)` when a barrier must be emitted.
///
/// There is no special case for a physical slot changing hands. It used to
/// start such a transition from [`ResourceState::Undefined`], on the argument
/// that the outgoing resource's pixels are garbage to the incoming one and
/// discarding them is free. The pixels are indeed garbage — but `Undefined` on
/// this seam does not only mean "discard", it means "no source scope", and the
/// outgoing resource's *writes* are not garbage: they are in flight. Naming the
/// real previous state costs nothing here (one handle, one description, so
/// there is no layout change and nothing to decompress) and is the difference
/// between the two uses being ordered and racing.
fn transition(
    tracked: &mut Tracked,
    want: ResourceState,
    queue: QueueHandle,
) -> Option<(ResourceState, Option<QueueTransfer>)> {
    let queue_transfer = match tracked.queue {
        Some(previous) if previous != queue => Some(QueueTransfer {
            from: previous,
            to: queue,
        }),
        _ => None,
    };
    let from = tracked.state;
    tracked.queue = Some(queue);

    // A queue-family transfer is a barrier even between two identical states:
    // ownership has to move, or the second queue reads memory the first has not
    // released.
    if !ResourceState::needs_barrier(from, want) && queue_transfer.is_none() {
        return None;
    }
    tracked.state = want;
    Some((from, queue_transfer))
}

/// First and last pass index that touches each resource.
fn lifetimes<'p, A: 'p, I>(
    pass_count: usize,
    accesses: impl Iterator<Item = I>,
    id_of: impl Fn(&A) -> u32,
) -> Vec<Option<(usize, usize)>>
where
    I: IntoIterator<Item = &'p A>,
{
    let mut spans: Vec<Option<(usize, usize)>> = Vec::new();
    for (pass, list) in accesses.enumerate() {
        debug_assert!(pass < pass_count);
        for access in list {
            let id = id_of(access) as usize;
            if spans.len() <= id {
                spans.resize(id + 1, None);
            }
            spans[id] = Some(match spans[id] {
                Some((first, last)) => (first.min(pass), last.max(pass)),
                None => (pass, pass),
            });
        }
    }
    spans
}

/// Packs transients onto physical slots, reusing one whose lifetime has ended.
fn assign<N, D: Copy + PartialEq>(
    nodes: &[N],
    spans: &[Option<(usize, usize)>],
    desc_of: impl Fn(&N) -> Option<D>,
    label_of: impl Fn(&N) -> String,
    physical: &mut Vec<D>,
) -> Result<Vec<Slot>, GraphError> {
    // `free_from[slot]` is the first pass at which the slot is available again.
    let mut free_from: Vec<usize> = Vec::new();
    let mut slots = Vec::with_capacity(nodes.len());

    // Ordered by first use so a single left-to-right sweep is optimal for the
    // interval-packing problem this is: identical descriptions, no preemption.
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|index| {
        spans
            .get(*index)
            .copied()
            .flatten()
            .map_or(usize::MAX, |s| s.0)
    });

    slots.resize(nodes.len(), Slot::Imported);
    for index in order {
        let Some(desc) = desc_of(&nodes[index]) else {
            continue;
        };
        let Some((first, last)) = spans.get(index).copied().flatten() else {
            return Err(GraphError::UnusedTransient {
                resource: label_of(&nodes[index]),
            });
        };
        let reused =
            (0..physical.len()).find(|slot| physical[*slot] == desc && free_from[*slot] <= first);
        let slot = match reused {
            Some(slot) => slot,
            None => {
                physical.push(desc);
                free_from.push(0);
                physical.len() - 1
            }
        };
        free_from[slot] = last + 1;
        slots[index] = Slot::Transient(slot);
    }
    Ok(slots)
}

fn validate(
    images: &[ImageNode],
    buffers: &[BufferNode],
    passes: &[Pass<'_>],
) -> Result<(), GraphError> {
    for pass in passes {
        let mut seen_depth = false;
        for (position, access) in pass.images.iter().enumerate() {
            if pass.kind == PassKind::Compute && access.attachment.is_some() {
                return Err(GraphError::AttachmentInComputePass {
                    pass: pass.label.clone(),
                    resource: images[access.image.index()].label.clone(),
                });
            }
            if matches!(access.attachment, Some(Attachment::Depth { .. })) {
                if seen_depth {
                    return Err(GraphError::DuplicateDepthAttachment {
                        pass: pass.label.clone(),
                    });
                }
                seen_depth = true;
            }
            for other in &pass.images[position + 1..] {
                if other.image == access.image && other.state != access.state {
                    return Err(GraphError::ConflictingAccess {
                        pass: pass.label.clone(),
                        resource: images[access.image.index()].label.clone(),
                        first: access.state,
                        second: other.state,
                    });
                }
            }
        }
        for (position, access) in pass.buffers.iter().enumerate() {
            for other in &pass.buffers[position + 1..] {
                if other.buffer == access.buffer && other.state != access.state {
                    return Err(GraphError::ConflictingAccess {
                        pass: pass.label.clone(),
                        resource: buffers[access.buffer.index()].label.clone(),
                        first: access.state,
                        second: other.state,
                    });
                }
            }
        }
    }
    Ok(())
}

fn render_area(images: &[ImageNode], pass: &Pass<'_>) -> Result<Rect2d, GraphError> {
    if pass.kind == PassKind::Compute {
        return Ok(Rect2d::from_size(0, 0));
    }
    let mut area: Option<(&str, (u32, u32))> = None;
    for access in &pass.images {
        if access.attachment.is_none() {
            continue;
        }
        let node = &images[access.image.index()];
        let extent = node.extent();
        match area {
            None => area = Some((&node.label, extent)),
            Some((first, first_extent)) if first_extent != extent => {
                return Err(GraphError::AttachmentExtentMismatch {
                    pass: pass.label.clone(),
                    first: first.to_string(),
                    first_extent,
                    second: node.label.clone(),
                    second_extent: extent,
                });
            }
            Some(_) => {}
        }
    }
    match area {
        Some((_, (width, height))) => Ok(Rect2d::from_size(width, height)),
        None => Err(GraphError::NoAttachments {
            pass: pass.label.clone(),
        }),
    }
}

/// Convenience descriptions for the two transients every frame in this engine
/// has.
impl TransientImageDesc {
    /// The HDR scene target: `Rgba16Float`, rendered into and then sampled.
    ///
    /// **HDR from P1**, per `docs/plan/ROADMAP.md`'s correction. `Rgba16Float`
    /// rather than the swapchain's format is the entire structural commitment;
    /// the tonemap pass that resolves it is the other half.
    #[must_use]
    pub const fn scene_color(extent: (u32, u32)) -> Self {
        Self {
            extent,
            format: Format::Rgba16Float,
            usage: ImageUsage::COLOR_ATTACHMENT
                .union(ImageUsage::SAMPLED)
                // So a headless render can read the HDR values back before the
                // tonemap flattens them, which is what makes "the RGBA16F path
                // is real" checkable.
                .union(ImageUsage::TRANSFER_SRC),
            samples: 1,
        }
    }

    /// The depth buffer: `D32Float`, reversed-Z, never sampled.
    #[must_use]
    pub const fn scene_depth(extent: (u32, u32)) -> Self {
        Self {
            extent,
            format: Format::D32Float,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            samples: 1,
        }
    }
}

impl TransientBufferDesc {
    /// A device-local storage buffer of `size` bytes.
    #[must_use]
    pub const fn storage(size: u64) -> Self {
        Self {
            size,
            usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_DST),
        }
    }
}
