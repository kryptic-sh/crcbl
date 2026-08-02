//! Renderer: render graph, frame loop, meshes, materials — everything above the
//! HAL seam.
//!
//! ```text
//! camera + light ──▶ ForwardRenderer ──adds passes──▶ RenderGraph
//!                                                          │ compile()
//!                                                          ▼
//!                            ordered passes + exact barriers + transients
//!                                                          │ execute()
//!                                                          ▼
//!                                        crcbl-hal ──▶ crcbl-vk / null / …
//! ```
//!
//! # The one rule
//!
//! **No manual barriers outside the graph, ever** — `docs/plan/02-vulkan-backend.md`
//! §2.4, and it is the reason this crate exists rather than the frame loop
//! living in each sample. Passes say what they read and write;
//! [`graph::RenderGraph::compile`] works out the transitions, the layout
//! changes, the transient aliasing and the final states, and
//! [`graph::CompiledGraph::execute`] is the only code in the engine that calls
//! [`pipeline_barrier`](crcbl_hal::CommandEncoder::pipeline_barrier) during a
//! frame.
//!
//! The barriers outside it are the **startup uploads**: [`forward`]'s staging
//! copy for the cube, and [`ui_pass`]'s for the glyph atlas. Both run before any
//! frame exists and have no graph to belong to; both are called out at the call
//! site. There are no others, and a barrier recorded during a frame from
//! anywhere but [`graph::CompiledGraph::execute`] is a bug.
//!
//! # Nothing here knows a backend
//!
//! This crate depends on `crcbl-hal`, `crcbl-core`, `crcbl-shaders` and `glam`.
//! It contains no `ash`, no `crcbl-vk`, and no `#[cfg(target_os = …)]`, per
//! `docs/plan/01-foundations.md` §1.3 — which also names the render graph
//! specifically as living above the seam rather than in it. The graph compiles
//! identically against [`NullBackend`](crcbl_hal::null), which is what makes the
//! graph-compile suite `docs/plan/12-testing.md` calls a non-negotiable anchor
//! run on every machine, with no ICD in the room.
//!
//! # Two tiers, one renderer
//!
//! `docs/plan/03-gpu-driven-rendering.md`'s rule is that **Tier B is a
//! constraint on data layout, not a separate renderer**, and [`ui_pass`] is
//! where the first one bites: WebGPU has no push constants at all, so the pass
//! that draws every sample's HUD picks its constant delivery from
//! [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) — see
//! [`ConstantDelivery`]. [`forward`] needs no such branch; its camera has been
//! in a uniform buffer since P1 for exactly this reason.
//!
//! One thing that split needs is **not** closed, and is recorded in
//! [`ui_pass`]'s own docs rather than worked around here: the Tier B *shader*
//! artifact does not exist, because `slangc` was unavailable when the Rust half
//! landed and the committed artifacts are hash-verified. The follow-up is one
//! new `.slang` file and one run of `compile-shaders.sh`.
//!
//! # What this crate is *not*, at P1
//!
//! No backend registry (see below), no materials, no scene, no culling, no
//! scheduler. The graph is a linear pass list with computed barriers, which is
//! what §2.4's risk list asks for by name: "No multi-queue scheduling, no
//! reordering. Resist."
//!
//! ## The registry deliberately did not move here
//!
//! `crcbl-vk`'s crate docs floated moving the GPU backend registry into this
//! crate at P1.3. It did not move, because it would make `crcbl-render` depend
//! on `crcbl-vk` — and `crcbl-vk`'s end-to-end suite depends on `crcbl-render`,
//! since the only honest way to test a render graph against real Vulkan is to
//! run the real graph. That is a dependency cycle through dev-dependencies:
//! legal in Cargo, and a confusing thing to have created in exchange for moving
//! four lines out of the crate whose entire job is to be the one name a game
//! depends on. The registry stays in `crcbl`.
//!
//! # Example
//!
//! ```
//! use crcbl_hal::null::NullInstance;
//! use crcbl_hal::{DeviceDesc, Instance, QueueKind, ResourceState};
//! use crcbl_render::graph::RenderGraph;
//! use crcbl_render::transient::{TransientImageDesc, TransientPool};
//!
//! let instance = NullInstance::tier_a();
//! let adapter = instance.adapters().remove(0);
//! let device = instance.create_device(&DeviceDesc::for_adapter(adapter.id))?;
//! let queue = device.queue(QueueKind::Graphics).expect("always present");
//!
//! // The pool outlives the graph: it owns the physical images, and it is what
//! // remembers what the last frame left them in.
//! let mut pool = TransientPool::new();
//!
//! let mut graph = RenderGraph::new(queue);
//! let scene = graph.create_image("scene", TransientImageDesc::scene_color((64, 48)));
//! graph
//!     .add_render_pass("clear")
//!     .clear_color(scene, [0.0, 0.0, 0.0, 1.0])
//!     .execute(|_| {});
//!
//! let compiled = graph.compile(&pool)?;
//! assert_eq!(compiled.passes().len(), 1);
//! // Nothing has ever used this pool, so the transient really is undefined and
//! // the graph transitions it before use. A second frame through the same pool
//! // would start from `ColorAttachment` — which is the barrier that orders it
//! // after this one.
//! let barriers = compiled.passes()[0].barriers();
//! assert_eq!(barriers.images[0].from, ResourceState::Undefined);
//! assert_eq!(barriers.images[0].to, ResourceState::ColorAttachment);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod camera;
pub mod forward;
pub mod graph;
pub mod texture;
pub mod timing;
pub mod transient;
pub mod ui_pass;

pub use camera::{Camera, DirectionalLight, Projection};
pub use forward::ForwardRenderer;
pub use graph::{
    Attachment, BufferId, CompiledGraph, CompiledPass, GraphBarriers, GraphBufferBarrier,
    GraphError, GraphImageBarrier, ImageId, ImportedBuffer, ImportedImage, PassBuilder,
    PassContext, PassKind, RenderGraph,
};
pub use texture::{UploadedTexture, upload_texture};
pub use timing::{FrameTimings, PassTimers, PassTiming};
pub use transient::{TransientBufferDesc, TransientImageDesc, TransientPool, TransientUse};
pub use ui_pass::{ConstantDelivery, UiRenderer};
