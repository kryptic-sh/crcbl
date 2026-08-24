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
//! The barriers outside it are the **startup uploads**: [`mesh_pool`]'s staging
//! copy into the geometry pools, [`texture`]'s for every image ([`ui_pass`]'s
//! glyph atlas and [`sprite_pass`]'s sheets), and [`draw_gen`]'s copy zeroing
//! the LOD hysteresis state, which is device-local because a shader writes it
//! and so cannot be zeroed by a host write. Each runs before any frame exists
//! and has no graph to belong to; each is called out at the call site. There are
//! no others, and a barrier recorded during a frame from anywhere but
//! [`graph::CompiledGraph::execute`] is a bug.
//!
//! # Nothing here knows a backend
//!
//! This crate depends on `crcbl-hal`, `crcbl-core`, `crcbl-shaders`, `glam`, and
//! the two description-only crates whose data it turns into pixels — `crcbl-ui`
//! for [`ui_pass`]'s draw list and `crcbl-sprite` for [`sprite_pass`]'s sample
//! modes, neither of which depends on a backend or on this crate.
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
//! constraint on data layout, not a separate renderer**, and no pass here reads
//! a push constant. [`forward`]'s camera has been in a uniform buffer since P1
//! for exactly that reason; [`sprite_pass`] followed it; and [`ui_pass`] — which
//! draws every sample's HUD, and which WebGPU's lack of push constants had made
//! this crate's one permutation axis, with a `ConstantDelivery` enum and a
//! second `.slang` — joined them in 2026-08.
//!
//! So the tier costs one indirection where a push constant would have served,
//! and buys one artifact per shader, one code path per pass, and nothing to
//! select between at device-open time. [`ui_pass`]'s own docs carry the trade in
//! full, including what the split had cost.
//!
//! # What this crate is *not*, at P1
//!
//! No backend registry (see below), no material *system*, no scene, no
//! scheduler. [`material_table`] is §3.2's table and only that — a row of
//! factors and one base-colour texture layer that an instance's id selects; the
//! authoring, templates, permutations and the other texture slots a material
//! system means are `docs/plan/37-materials.md`'s and are not here.
//!
//! The one thing on that list that has since moved whole is culling:
//! [`cull`] is §3.3's frustum test and the CPU reference `cull.slang` is checked
//! against, [`draw_gen`] runs that shader and turns its survivors into indirect
//! draw arguments, and [`forward`] draws from those rather than recording a draw
//! per object. The graph is still a linear pass list with computed barriers,
//! which is what §2.4's risk list asks for by name: "No multi-queue scheduling,
//! no reordering. Resist."
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

mod bloom;
pub mod button_skin;
pub mod camera;
pub mod cluster_pool;
pub mod counters;
pub mod cull;
pub mod cull_stats;
pub mod draw_gen;
pub mod effects;
pub mod fly;
pub mod forward;
pub mod graph;
pub mod grid;
pub mod instance_pool;
pub mod layers;
pub mod light;
pub mod light_grid;
pub mod material_table;
pub mod menu;
pub mod mesh_pool;
pub mod nine_slice;
pub mod orbit;
mod probe;
pub mod scene;
pub mod shadow;
pub mod skinning;
pub mod sprite_pass;
mod ssao;
mod ssr;
pub mod texture;
pub mod timing;
pub mod transient;
pub mod ui_pass;

pub use button_skin::{ButtonSkin, screen_rect_to_target};
pub use camera::{Camera, DirectionalLight, Projection};
pub use cluster_pool::{ClusterPool, ClusterRange};
pub use counters::{FrameCounters, INDIRECT, UNKNOWN};
/// Re-exported because [`SheetDesc::sample`] is spelled in it: a crate whose
/// public API names a foreign type and does not export it is one every caller
/// has to add a dependency for.
///
/// [`NineSlice`] and [`Rect`] are here for the same reason —
/// [`NineSliceSource`]'s fields are spelled in both.
pub use crcbl_sprite::{NineSlice, Rect, SampleMode};
/// Re-exported for the same reason: [`MenuArt::extend`] is spelled in [`Menu`]
/// and [`MenuLayout`], and a caller that can name this crate's menu API should
/// not have to add a second dependency to build one. It is also what lets
/// `crcbl-vk`'s end-to-end suite — which depends on this crate and not on
/// `crcbl-ui` — take a golden image of the real menu rather than of a replica.
pub use crcbl_ui::menu::{Menu, MenuItem, MenuItemLayout, MenuLayout, MenuStyle};
pub use crcbl_ui::text::FontAtlas;
/// Re-exported for the same reason as [`NineSlice`]: [`ButtonSkin::source`] and
/// [`ButtonSkin::quads`] are spelled in [`ButtonState`], and
/// [`ButtonSkin::insets`] returns a [`SkinInsets`]. A caller that can name this
/// crate's button API should not have to add a second dependency to call it.
pub use crcbl_ui::{ButtonState, SkinInsets};
pub use cull::{Aabb, Frustum, visible_instances};
pub use cull_stats::{ClusterCull, CullStats, CullStatsRing};
pub use draw_gen::{DrawGen, DrawGenDesc, GeneratedDraws};
pub use effects::{EffectOverride, EffectRequest, RenderEffects};
pub use fly::{Flyer, LOOK, SPEED, TURN};
pub use forward::{
    DebugView, EXPOSURE_MAX, EXPOSURE_MIN, ForwardRenderer, SHADOW_LOD_BIAS, SkinnedInstanceDesc,
};
pub use graph::{
    Attachment, BufferId, CompiledGraph, CompiledPass, GraphBarriers, GraphBufferBarrier,
    GraphError, GraphImageBarrier, ImageId, ImportField, ImportedBuffer, ImportedImage,
    InitialClaim, PassBuilder, PassContext, PassKind, RenderGraph,
};
pub use instance_pool::{
    Instance, InstanceHandle, InstancePool, InstancePoolDesc, InstancePoolError,
};
pub use layers::{Layer, LayerStack, Parallax};
pub use light::{Light, PointLight, SpotLight, sun_row};
pub use light_grid::{FROXEL_CAPACITY, FrameView, Grid, LightGrid, LightGridDesc};
pub use material_table::{
    Material, MaterialHandle, MaterialTable, MaterialTableDesc, MaterialTableError,
};
pub use menu::{MenuArt, MenuRenderer, menu_camera, menu_view_projection};
pub use mesh_pool::{
    Mesh, MeshHandle, MeshPool, MeshPoolDesc, MeshPoolError, MeshRange, UPLOAD_TIMEOUT,
};
pub use nine_slice::{NineQuads, NineSliceSource, SliceQuad};
pub use orbit::OrbitCamera;
pub use scene::{Capacities, Geometry, InstanceDesc, MeshDesc, PageDesc, ProbeGrid, SceneDesc};
pub use shadow::Cascades;
pub use skinning::{
    SkinRange, SkinnedMesh, SkinnedRegion, Skinning, SkinningDesc, SkinningError, blend,
    normal_basis, skin_vertex,
};
pub use sprite_pass::{
    CONSTANTS_SIZE, INSTANCE_STRIDE, SAMPLE_PIXEL, SAMPLE_SMOOTH, SheetDesc, SheetId, Sprite,
    SpriteConstants, SpriteInstance, SpriteRenderer, sheet_lane,
};
pub use texture::{UploadedTexture, upload_texture, upload_texture_layers};
pub use timing::{FrameTimings, MAX_TIMED_PASSES, PassTimers, PassTiming};
pub use transient::{TransientBufferDesc, TransientImageDesc, TransientPool, TransientUse};
pub use ui_pass::UiRenderer;
