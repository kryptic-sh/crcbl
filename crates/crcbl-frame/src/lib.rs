//! Frame core: the render graph, its transient pool, texture uploads, pass
//! timers and the UI pass — what a frame needs before it has a scene in it.
//!
//! ```text
//! DrawList ──▶ UiRenderer ──adds passes──▶ RenderGraph
//!                                               │ compile()
//!                                               ▼
//!                  ordered passes + exact barriers + transients
//!                                               │ execute()
//!                                               ▼
//!                             crcbl-hal ──▶ crcbl-vk / null / …
//! ```
//!
//! `crcbl-render` builds the engine's 3D frame on this crate and re-exports
//! every module of it, so the engine's own callers name these items through
//! `crcbl_render`. A program that draws only a UI — a status bar, a launcher —
//! depends on this crate, `crcbl-hal`, a backend and `crcbl-ui`, and on nothing
//! that knows a mesh, a material or a body of water.
//!
//! # The one rule
//!
//! **No manual barriers outside the graph, ever** — stage 2 §2.4. Passes say
//! what they read and write; [`graph::RenderGraph::compile`] works out the
//! transitions, the layout changes, the transient aliasing and the final
//! states, and [`graph::CompiledGraph::execute`] is the only code in the engine
//! that calls [`pipeline_barrier`](crcbl_hal::CommandEncoder::pipeline_barrier)
//! during a frame.
//!
//! The barriers outside it are the **startup uploads** — [`texture`]'s, for
//! every image a pass creates before its first frame, such as [`ui_pass`]'s
//! glyph atlas — and `crcbl-render`'s own, which its crate docs list. Each runs
//! before any frame exists and has no graph to belong to. A barrier recorded
//! during a frame from anywhere but [`graph::CompiledGraph::execute`] is a bug.
//!
//! # Nothing here knows a backend
//!
//! This crate depends on `crcbl-hal`, `crcbl-core`, `crcbl-shaders` for the UI
//! pass's compiled shader, and `crcbl-ui` for the draw lists that pass turns
//! into pixels. It contains no backend and no `#[cfg(target_os = …)]`, per the
//! HAL rules in `docs/notes/backends.md`. The graph compiles identically against
//! [`NullBackend`](crcbl_hal::null), which is what lets the graph-compile suite
//! run on every machine with no ICD in the room.
//!
//! # Example
//!
//! ```
//! use crcbl_frame::graph::RenderGraph;
//! use crcbl_frame::transient::{TransientImageDesc, TransientPool};
//! use crcbl_hal::null::NullInstance;
//! use crcbl_hal::{DeviceDesc, Format, ImageUsage, Instance, QueueKind, ResourceState};
//!
//! let instance = NullInstance::gpu_driven();
//! let adapter = instance.adapters().remove(0);
//! let device = instance.create_device(&DeviceDesc::for_adapter(adapter.id))?;
//! let queue = device.queue(QueueKind::Graphics).expect("always present");
//!
//! // The pool outlives the graph: it owns the physical images, and it is what
//! // remembers what the last frame left them in.
//! let mut pool = TransientPool::new();
//!
//! let mut graph = RenderGraph::new(queue);
//! let target = graph.create_image(
//!     "target",
//!     TransientImageDesc::new((64, 48), Format::Rgba8Unorm, ImageUsage::COLOR_ATTACHMENT),
//! );
//! graph
//!     .add_render_pass("clear")
//!     .clear_color(target, [0.0, 0.0, 0.0, 1.0])
//!     .execute(|_| {});
//!
//! let compiled = graph.compile(&pool)?;
//! assert_eq!(compiled.passes().len(), 1);
//! // Nothing has ever used this pool, so the transient really is undefined and
//! // the graph transitions it before use.
//! let barriers = compiled.passes()[0].barriers();
//! assert_eq!(barriers.images[0].from, ResourceState::Undefined);
//! assert_eq!(barriers.images[0].to, ResourceState::ColorAttachment);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod counters;
pub mod graph;
pub mod mip;
pub mod texture;
pub mod timing;
pub mod transient;
pub mod ui_pass;

pub use counters::{FrameCounters, INDIRECT, UNKNOWN};
pub use graph::{
    Attachment, BufferId, CompiledGraph, CompiledPass, GraphBarriers, GraphBufferBarrier,
    GraphError, GraphImageBarrier, ImageId, ImportField, ImportedBuffer, ImportedImage,
    InitialClaim, PassBuilder, PassContext, PassKind, RenderGraph,
};
pub use texture::{
    ClearedTextureDesc, PendingUpload, TexturePatch, UploadBatch, UploadedTexture,
    upload_cleared_texture, upload_texture, upload_texture_layers, upload_texture_mip_layers,
};
pub use timing::{FrameTimings, PassTimers, PassTiming};
pub use transient::{TransientBufferDesc, TransientImageDesc, TransientPool, TransientUse};
pub use ui_pass::{UiRenderer, UiTexture};
