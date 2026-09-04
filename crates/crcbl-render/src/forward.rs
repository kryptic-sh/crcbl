//! The frame milestones 3, 4 and 5 draw: a lit mesh into an HDR target, then a
//! tonemap into whatever the caller is presenting to.
//!
//! ```text
//! ┌ cull ───────────┐ ┌ draw-args ──────┐ ┌ forward ──────────┐ ┌ tonemap ──────┐
//! │ instances ─────▶│ │ visible ───────▶│ │ scene-color Clear │ │ reads it      │
//! │ visible list    │ │ per-bucket runs │ │ scene-depth Clear │ │ writes the    │
//! │ + a GPU count   │ │ + indirect args │ │ one indirect call │ │ target        │
//! │                 │ │ + a draw count  │ │ per bucket        │ │               │
//! └─────────────────┘ └─────────────────┘ └───────────────────┘ └───────────────┘
//! ```
//!
//! Two passes and one graph. Everything that makes them ordered, everything that
//! transitions `scene-color` from a colour attachment into a sampled texture,
//! and everything that returns the target to [`ResourceState::Present`] is
//! computed by [`crate::graph`] — **there is not one hand-written barrier in
//! this file, or anywhere above the seam.**
//!
//! # Scope
//!
//! `docs/plan/02-vulkan-backend.md`'s ladder rungs 3–5, and nothing beyond:
//!
//! * **3** — depth-tested spinning mesh, perspective camera in a uniform buffer.
//! * **4** — one directional light (in `mesh.slang`). The rung's own wording is
//!   "Lambert+Blinn"; what shipped is Lambert plus a Cook-Torrance GGX lobe over
//!   a glTF metallic-roughness row, which is that rung's "real material model"
//!   arriving early rather than a different rung.
//! * **5** — orthographic mode, which is [`Camera::projection`] and *nothing
//!   else*: no second pipeline, no branch in this file, no shader permutation.
//!
//! Explicitly not here: bindless at scale (P3), shadows and real post (P7),
//! material authoring (topic 37), asset loading (P9). The mesh is a constant in
//! `crcbl-shaders` because rung 3 says "hardcoded cube/sphere".
//!
//! What *is* here since 2026-08 is [`crate::material_table`]: §3.2's material
//! table, holding **both** halves of "texture indices + factors", with
//! [`mesh::GpuInstance::material`] selecting a row. Three rows are resident and
//! two of them exist to be seen: [`scene::DEMO_TINTED`] differs from the default
//! row in its factor and its roughness, and [`scene::DEMO_TEXTURED`] in its
//! base-colour page layer alone. Which rows those are is [`crate::scene::demo`]'s
//! to say, and so is why the shading factor rides on that row rather than on a
//! fourth.
//!
//! The page is the texture side, and it is one `D2Array` image rather than an
//! array of descriptors — `BindingModel::ArrayPages` rather than `Bindless`.
//! That is the whole of the binding-model decision §3.2 leaves open, taken
//! here because a layer index needs nothing of a device where a descriptor
//! array needs `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws. So
//! there is one lookup, one layout and one artifact rather than a permutation,
//! and a device that reports the feature runs the same declaration. What
//! bindless buys later is capacity, not a second path — see [`crate::scene::PageDesc`]
//! and the layout entry for binding 7.
//!
//! What *was* on that list until 2026-08 is GPU culling and the indirect draw
//! count, and both are here now: [`crate::draw_gen`] runs `cull.slang` and
//! `draw_gen.slang` in front of this pass, and the draws below come out of the
//! arguments they wrote. See that module for what a bucket is and why the
//! arguments are per bucket rather than per surviving instance.
//!
//! # The CPU records a fixed number of draws
//!
//! One per bucket, whatever the scene holds — `docs/plan/03-gpu-driven-rendering.md`'s
//! headline goal, "10 objects and 10,000 objects record roughly the same
//! commands". Adding an object is an instance in the pool and nothing else;
//! removing one is [`InstancePool::remove`] and nothing else. Neither changes a
//! single command this file records, because what varies — how many instances a
//! bucket draws, and which — is written by a compute pass into a buffer the
//! draw reads.
//!
//! Which *call* those draws are is the one branch: [`GeometryPath::IndirectCount`]
//! takes the count from GPU memory too, and [`GeometryPath::IndirectPerBatch`] —
//! Metal, whose API has multi-draw-indirect and no GPU-side count — reads each
//! bucket's one argument structure unconditionally and leans on its instance
//! count being zero.
//!
//! What *is* here, since 2026-08, is [`crate::mesh_pool`]: the cube is the first
//! resident of the global vertex and index pools rather than owning two buffers
//! of its own, so it is `{base_vertex, base_index, index_count}` like everything
//! P7 will draw. Same geometry, different residence.
//!
//! And since the same month, [`crate::instance_pool`]: the cube's model matrix
//! is a `GpuInstance` in a storage buffer rather than a field of the frame's
//! uniform block, uploaded by delta. It holds the same matrix `begin_frame` used
//! to write into the uniform, which is why the golden images did not move.
//!
//! # A second resident, and how it finds its own vertices
//!
//! A pool whose only mesh is at base vertex 0 cannot tell a base vertex that
//! works from one that is silently cancelled out, and `mesh.slang`'s header
//! shows the four targets disagreeing about exactly that: the SPIR-V subtracts
//! `BaseVertex` back out of `SV_VertexID`, so the pool's base — passed through
//! `draw_indexed`'s own argument — reached the shader as zero, while WGSL and
//! MSL kept it. The same disagreement covers `SV_InstanceID`, which is what
//! [`crate::sprite_pass`] paid for first.
//!
//! So **every draw this pass records passes zero for both of its bases**, and
//! the real ones arrive as data. The base vertex comes from
//! [`MeshPool`]'s mesh table, indexed by the drawn instance's
//! [`mesh::GpuInstance::mesh`] — so it is resolved per instance, by the GPU,
//! and one draw could cover instances of different meshes. The instance comes
//! from the bucket's run of survivors, walked with `SV_InstanceID`, which counts
//! from zero on every target *because* the first instance is zero. All a draw
//! says for itself is where its run starts, in a [`mesh::DrawConstants`] block
//! reached through a dynamic offset: one block per bucket, written once at
//! build.
//!
//! That is what makes the pool's *second* resident,
//! [`mesh::pyramid_vertices`], draw its own geometry rather than the cube's.
//!
//! The pyramid is in the pools from `new` but has no *instance* until a caller
//! places one with [`ForwardRenderer::add_instance`] — an instance in the pool
//! is an object in the scene now that culling decides what draws. The frame the
//! samples draw is still the cube alone, which is why their golden images did
//! not move either.
//!
//! # A third resident, and how it finds its own clusters
//!
//! The same argument one layer down. A mesh that fits **one** cluster cannot
//! tell a per-cluster offset that works from one that is lost, and the cube and
//! the pyramid fit one each — 24 and 16 vertices against a bound of 64. So the
//! pool's third resident is [`mesh::open_box_vertices`], five clusters of one
//! flat face each, four of them at a `Meshlet::vertex_offset` that is not zero.
//! It is [`crate::cluster_pool`]'s counterpart to what the pyramid is for
//! [`crate::mesh_pool`], and it is what §3.5's per-cluster culling will need
//! before a surviving-cluster count can mean anything.
//!
//! It arrives on the pyramid's terms exactly: resident from `new`, in the scene
//! only when a caller places it, so no golden moved for it either.
//!
//! # Uniforms are a ring, and they have to be
//!
//! The camera spins, so the uniform block is rewritten every frame — and the
//! previous frame may still be reading it. One buffer would be a
//! read-after-write hazard *across* submissions, which is precisely what
//! `CRCBL_VK_SYNC_VALIDATION=1` exists to find and precisely what
//! `docs/plan/02-vulkan-backend.md` calls this stage's headline risk. So there
//! is one uniform buffer and one bind group per frame in flight, and
//! [`ForwardRenderer::begin_frame`] rotates them.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ColorTargetState, CompareOp, CullMode, DepthBias, DepthStencilState, Device,
    DeviceCaps, DrawIndirect, DrawIndirectCount, Features, FilterMode, Format, GeometryPath,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType,
    IndexFormat, LoadOp, MemoryLocation, MeshPipelineDesc, MultisampleState, PipelineLayoutDesc,
    PipelineLayoutHandle, PolygonMode, PrimitiveState, QueueHandle, Rect2d, ResourceState,
    SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry, ShaderModuleDesc,
    ShaderModuleHandle, ShaderStages, StoreOp, Viewport, check_portable_storage_buffers,
};
use crcbl_shaders::meshlet::MeshClusters;
use crcbl_shaders::probe_gather::PunctualProducer;
use crcbl_shaders::{
    MESH, MESH_CLUSTER, Stage, TONEMAP, contact_shadows, dfg, level_select, ltc, mesh, ssao, ssr,
    tonemap,
};
use glam::{Mat4, Quat, Vec3};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::camera::{Camera, DirectionalLight, Fog, Sky};
use crate::cluster_pool::{ClusterPool, ClusterRange, PooledMesh};
use crate::contact_shadows::ContactShadows;
use crate::counters::FrameCounters;
use crate::cull::Frustum;
use crate::cull_stats::CullStatsRing;
use crate::debug_draw::DebugDraw;
use crate::draw_gen::{DrawGen, DrawGenDesc, GeneratedDraws};
use crate::effects::{EffectRequest, RenderEffects};
use crate::graph::{
    BufferId, ImageId, ImportedBuffer, ImportedImage, InitialClaim, PassBuilder, RenderGraph,
};
// Renamed on the way in, because [`crate::light_grid::Grid`] already holds the
// bare name here and means the froxel grid — the collision [`crate::grid`]'s
// header predicted.
use crate::atlas_view::AtlasView;
use crate::bloom::Bloom;
use crate::exposure::{Exposure, ExposureAdaptation, ExposureBuffers};
use crate::fxaa::Fxaa;
use crate::grid::{Grid as GroundGrid, GridStyle};
use crate::hiz::Hiz;
use crate::instance_pool::{InstanceHandle, InstancePool, InstancePoolDesc, InstancePoolError};
use crate::light::{Light, sun_row};
use crate::light_grid::{FROXEL_CAPACITY, FrameView, Grid, LightGrid, LightGridDesc};
use crate::material_table::{MaterialTable, MaterialTableDesc};
use crate::mesh_pool::{MeshHandle, MeshPool, MeshPoolDesc, MeshPoolError, MeshUpload};
use crate::probe::{ProbeTable, ProbeTableDesc};
use crate::probe_gather::{ProbeGather, ProbeGatherDesc, RsmImages, RsmTargets};
use crate::rsm;
use crate::scene::{self, Geometry, InstanceDesc, ProbeUpdate, SceneDesc};
use crate::shadow::{self, Cascades};
use crate::skinning::{SkinRange, SkinnedMesh, Skinning, SkinningError};
use crate::sky_pass::SkyPass;
use crate::smaa::Smaa;
use crate::ssao::{Ssao, cached_group};
use crate::ssr::{Ssr, SsrImages};
use crate::texture::{
    UploadedTexture, upload_texture, upload_texture_layers, upload_texture_mip_layers,
};
use crate::transient::{TransientImageDesc, TransientPool};
use crate::upscale::Upscale;
use crate::volumetric::{FroxelBuffers, Medium, Volumetric, VolumetricImages};

/// The clear behind the mesh, in **linear** light.
///
/// The scene target is `Rgba16Float`, so this is a linear value and the tonemap
/// pass plus the swapchain's sRGB encode are what turn it into pixels. Writing a
/// display-referred colour here would come out visibly paler, which is the
/// classic HDR-pipeline mistake and worth a sentence rather than a surprise.
pub const SCENE_CLEAR: [f32; 4] = [0.012, 0.016, 0.030, 1.0];

/// How many frames of uniform buffers to keep. Matches the frame loop's
/// frames-in-flight.
pub const FRAMES_IN_FLIGHT: usize = 2;

/// The darkest exposure [`ForwardRenderer::set_exposure`] will accept: **five
/// stops below** [`crcbl_shaders::tonemap::DEFAULT_EXPOSURE`].
///
/// The floor exists so that the control cannot be driven somewhere it cannot be
/// driven back from. The operator clamps its own *output* to `[0, 1]`, so an
/// unclamped input has two dead ends: far enough down and every pixel rounds to
/// the same black byte, far enough up and every pixel saturates to white — and
/// in both the picture stops carrying the information a user would need to see
/// which way to go. Five stops is well inside either: it is a sixteenth of the
/// scene's brightness, dark but plainly still a picture, and it is the range a
/// camera's exposure-compensation dial covers.
pub const EXPOSURE_MIN: f32 = 1.0 / 32.0;

/// The brightest exposure [`ForwardRenderer::set_exposure`] will accept: **five
/// stops above** [`crcbl_shaders::tonemap::DEFAULT_EXPOSURE`], for
/// [`EXPOSURE_MIN`]'s reason.
pub const EXPOSURE_MAX: f32 = 32.0;

/// The default has to be reachable from both ends of the range, or a control
/// that steps by a ratio starts somewhere it can only leave in one direction.
const _: () = assert!(
    EXPOSURE_MIN > 0.0
        && EXPOSURE_MIN < crcbl_shaders::tonemap::DEFAULT_EXPOSURE
        && EXPOSURE_MAX > crcbl_shaders::tonemap::DEFAULT_EXPOSURE
);

/// The format the frame's depth buffer is created with, taken from the
/// description that creates it rather than written down a second time.
///
/// Read by [`ForwardRenderer::set_ground_grid`], whose pipeline is built for a
/// depth attachment before there is a frame to ask. The extent is irrelevant to
/// a format, which is why any is passed.
pub(crate) const SCENE_DEPTH_FORMAT: Format = TransientImageDesc::scene_depth((1, 1)).format;

/// The format the frame's HDR scene target is created with, taken from the
/// description that creates it rather than written down a second time.
///
/// Read by [`crate::debug_draw`], whose pipeline blends into whichever image the
/// chain of HDR passes ended with — every one of them is a
/// [`TransientImageDesc::scene_color`]. The extent is irrelevant to a format,
/// which is why any is passed.
pub(crate) const SCENE_COLOR_FORMAT: Format = TransientImageDesc::scene_color((1, 1)).format;

/// The format §3.2's base-colour page is created with.
///
/// **`Rgba8UnormSrgb`, and that is the colour-space decision** — the argument
/// for it is at the `upload_texture_mip_layers` call inside
/// [`ForwardRenderer::with_scene`]. Named rather than spelled twice because
/// [`ForwardRenderer::base_color_page_import`] has to declare the same format
/// the image was created with, and a barrier naming a format the image is not in
/// picks the wrong aspects.
const BASE_COLOR_PAGE_FORMAT: Format = Format::Rgba8UnormSrgb;

/// The format `docs/plan/43-render-standards.md` §2's **normal** page is
/// created with.
///
/// **`Rgba8Unorm`, and the missing `Srgb` is the whole decision.**
/// [`docs/plan/44-lighting.md`]'s rung 2 argues it: a base-colour texel is a
/// colour and glTF defines it as sRGB-encoded, so the format above is what makes
/// the sampler decode it; a normal texel is a *number* — three components of a
/// direction, stored as `n * 0.5 + 0.5` — and pushing it through the sRGB
/// transfer curve is wrong by a gamma. What that looks like is not an error but
/// a surface that leans further off vertical than the author drew, which is why
/// the mistake survives review everywhere it is made.
///
/// `the_two_page_formats_differ` is the whole guard, and it is worth having
/// because the two constants sit four lines apart and the wrong one compiles.
///
/// [`docs/plan/44-lighting.md`]: https://docs.rs/crcbl-render
const NORMAL_PAGE_FORMAT: Format = Format::Rgba8Unorm;

/// The format the motion-vector target is created with: two channels of half
/// float, `docs/plan/43-render-standards.md` §9's third colour attachment.
///
/// **Named here rather than in [`TransientImageDesc::motion`]**, on
/// [`crate::smaa`]'s `EDGES_FORMAT` terms: the description and
/// [`MeshModules::COLOR_TARGETS`] have to agree, and the pass that builds the
/// pipeline is the one that owns the answer.
///
/// # The convention the target holds, so a consumer never has to guess it
///
/// **Current minus previous, in texture coordinates, `+y` down.** A pass
/// holding the previous frame reads its history at `uv - motion`, and a surface
/// travelling right and down the screen has both components positive.
/// `mesh.slang`'s `motion_vector` is where the mapping is written out and this
/// is the same sentence on the host side, because the first consumer will read
/// one of the two and not both.
///
/// # Two channels and half float
///
/// A motion vector is a screen-space offset and has no third component. Half
/// float rather than a normalised integer because the value is signed and
/// unbounded — an object crossing the frame in one frame is a motion past `1.0`
/// — and because the interesting magnitudes are small: a surface moving a
/// tenth of a pixel is the case a temporal filter is most sensitive to, and
/// eight or even sixteen unsigned bits over a `-1..1` range would quantise it
/// into rest. It is also the format `crcbl_hal::Format::Rg16Float`'s own doc
/// names motion vectors as the use for.
pub(crate) const MOTION_FORMAT: Format = Format::Rg16Float;

/// Which picture the colour pass draws instead of the shaded one.
///
/// The engine's debug views, as one value rather than as independent booleans,
/// because they are not independent: every one of them rides in a single lane of
/// the frame's uniform block and the shaders test the thresholds outermost
/// first, so a renderer with two of them set draws exactly one.
/// [`ForwardRenderer::debug_view`] is where that order lives, and this is what it
/// answers with — so a menu row, a debug panel and the picture cannot disagree
/// about which view is on.
///
/// **The declaration order is not the precedence** — each view was appended as
/// it arrived, and [`ForwardRenderer::debug_view`] is the one place the order is
/// decided. The sentinels in
/// [`FrameUniforms`](crcbl_shaders::mesh::FrameUniforms) ascend with it, though:
/// see its `HEATMAP_VIEW_ON`, and the shaders test the outermost threshold
/// first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugView {
    /// The lit picture. What every frame draws unless a caller asked otherwise.
    #[default]
    Shaded,
    /// Each cluster shaded by the projected screen-space error the LOD
    /// selection judged it on — [`ForwardRenderer::set_heatmap`].
    Heatmap,
    /// Each cluster tinted by the DAG level it was decimated to —
    /// [`ForwardRenderer::set_lod_view`].
    LodTint,
    /// World-space surface normals — [`ForwardRenderer::set_normals_view`].
    Normals,
    /// The ambient-occlusion channel alone, as grey —
    /// [`ForwardRenderer::set_occlusion_view`].
    AmbientOcclusion,
    /// The motion vector each fragment wrote, stretched and centred on grey —
    /// [`ForwardRenderer::set_motion_view`].
    Motion,
    /// The bent direction the occlusion channel carries beside its scalar, as
    /// `n * 0.5 + 0.5` — [`ForwardRenderer::set_bent_normal_view`]. **Wins over
    /// every other view.**
    BentNormal,
    /// The sun's cascades and the lit tiles beside them — the `D32Float` shadow
    /// atlas itself, letterboxed over the finished frame with a border round
    /// every occupied slot — [`ForwardRenderer::set_atlas_view`].
    ///
    /// **The one view that is a pass rather than a branch in `mesh.slang`**, and
    /// therefore the one whose lane is the off sentinel: the colour pass draws
    /// the shaded picture exactly as it always did and `crate::atlas_view`
    /// replaces it afterwards. That module's header is where the placement is
    /// argued.
    ///
    /// It resolves **below every view above and above [`Self::Cascades`]** — see
    /// [`ForwardRenderer::debug_view`].
    ShadowAtlas,
    /// The shaded picture tinted by the cascade each sun-lit fragment's shadow
    /// was sampled from, with the cross-fade band drawn as the blend of the two
    /// — [`ForwardRenderer::set_cascade_view`].
    ///
    /// **Loses to every other view**, and it is the only one that does. The views
    /// above draw something *instead of* the shaded picture; this one draws the
    /// shaded picture and multiplies it, so there is nothing left for it to
    /// modulate as soon as one of them is on. See
    /// [`ForwardRenderer::debug_view`].
    Cascades,
}

impl DebugView {
    /// What a panel row or a summary line calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shaded => "shaded",
            Self::Heatmap => "heatmap",
            Self::LodTint => "lod tint",
            Self::Normals => "normals",
            Self::AmbientOcclusion => "ambient occlusion",
            Self::Motion => "motion",
            Self::BentNormal => "bent normal",
            Self::ShadowAtlas => "shadow atlas",
            Self::Cascades => "cascades",
        }
    }
}

/// The bind-group slot the sun's shadow atlas is read through.
///
/// **15, not 9**, and the gap is `mesh_cluster.slang`'s: bindings 9 to 14 belong
/// to the mesh-shader path, and `mesh.slang`'s `fragmentMain` — which declares
/// this one — is the fragment stage of *that* pipeline too. A binding number is
/// a property of the shader source, so the two files have to agree even where
/// only one of them uses the slot.
const SHADOW_ATLAS_BINDING: u32 = 15;

/// The bind-group slot the shadow atlas's **comparison** sampler is bound to.
const SHADOW_SAMPLER_BINDING: u32 = 16;

/// The bind-group slot the base-colour page's sampler is bound to.
///
/// Named because it is the one entry [`ForwardRenderer::set_anisotropy`]
/// rewrites in every group of the mesh layout: a sampler is a resource a group
/// holds by handle, so a new sampler is a new group, and the rebuild finds the
/// entry by this number.
const PAGE_SAMPLER_BINDING: u32 = 8;

/// The bind-group slot topic 18's light list is read through.
///
/// **20, and the gap below it is `mesh_cluster.slang`'s** on
/// [`SHADOW_ATLAS_BINDING`]'s terms exactly: 17 to 19 are that file's, declared
/// by the mesh and amplification stages of a pipeline this file's fragment stage
/// completes, so the light bindings resume above them. Both files declare both
/// numbers, because a binding is a property of the source and Metal numbers a
/// stage's resources by declaration order.
const LIGHT_LIST_BINDING: u32 = 20;

/// The bind-group slot the froxel grid is read through.
const LIGHT_GRID_BINDING: u32 = 21;

/// The bind-group slot `docs/plan/18-render-features.md`'s screen-space
/// occlusion is fetched through.
///
/// **Last of the set, and that is what keeps `crcbl-mtl` honest.** That backend
/// gives a resource the next index in its Metal argument table by counting the
/// same-table entries of the layout list, and Slang numbers a stage's arguments
/// by declaration order — so the two agree only while both lists ascend. 22 is
/// past everything `mesh_cluster.slang` declares as well, which is why that file
/// needs no mirror of this one: no index it already owns moves.
const AMBIENT_OCCLUSION_BINDING: u32 = 22;

/// The bind-group slot `docs/plan/18-render-features.md`'s irradiance probes are
/// read through.
///
/// **Appended past [`AMBIENT_OCCLUSION_BINDING`], never inserted**, for exactly
/// the reason that constant gives: `crcbl-mtl` gives a resource the next index
/// in its Metal argument table by counting the same-table entries of the layout
/// list, and Slang numbers a stage's arguments by declaration order, so the two
/// agree only while both ascend. Appending changes no index below it —
/// `msl/mesh.metal` still takes `lights [[buffer(7)]]` and
/// `cluster_lights [[buffer(8)]]`, and this one lands on `buffer(9)`.
///
/// **`mesh_cluster.slang` mirrors this one**, declared and read by nothing, and
/// it did not have to until [`LEVEL_GROUP_TABLE_BINDING`] existed. That file's
/// own bindings used to stop at 21, so every number here was past everything it
/// declared and nothing of its could move; now it declares 24, and a source that
/// skipped this row would put *that* buffer one Metal index below where
/// `crcbl-mtl` binds it. It also mirrors the frame block's members, which is a
/// different obligation: the two files read one uniform buffer.
const PROBE_TABLE_BINDING: u32 = 23;

/// The bind-group slot the mesh path reads `docs/plan/25-lod.md`'s group records
/// through — `DrawGen`'s packed table buffer, the same one the draw-argument
/// pass judges the cut from.
///
/// **Appended past [`PROBE_TABLE_BINDING`], never inserted**, for that
/// constant's reason exactly, and the committed artifact is the evidence rather
/// than the argument: `msl/mesh_cluster.metal` takes `tables [[buffer(19)]]`,
/// which is the number this backend computes for binding 24 by counting the
/// buffer entries of the mesh path's layout.
///
/// Bound on [`GeometryPath::MeshShader`] and nowhere else, exactly as bindings 9
/// to 12 and 17 are: the raster path's vertex stage never reads it, and that
/// layout is already at the WebGPU storage-buffer ceiling with no headroom — see
/// the check at the end of the layout below.
///
/// **What it is for is the screen-error heatmap**, and only that. The *cut* is
/// still the draw-argument pass's decision and still arrives through the
/// hysteresis state; what this adds is the number that decision was made on, so
/// an overlay can shade a cluster by how close its producing group is to the
/// budget.
const LEVEL_GROUP_TABLE_BINDING: u32 = 24;

/// The bind-group slot the split-sum `DFG` table is fetched through — how much
/// of the light arriving at a GGX lobe that lobe hands back.
///
/// **Appended past [`LEVEL_GROUP_TABLE_BINDING`], never inserted**, for
/// [`PROBE_TABLE_BINDING`]'s reason exactly, and 25 is past everything
/// `mesh_cluster.slang` declares as well — that file reaches 24 — so it needs no
/// mirror there and no index it already owns moves. The evidence rather than the
/// argument: `msl/mesh.metal` takes `specular_albedo [[texture(3)]]` and
/// `ambient_occlusion [[texture(2)]]`, which are the numbers this backend
/// computes for bindings 25 and 22 by counting the sampled-image entries of this
/// layout.
///
/// **An image rather than a storage buffer, and that is forced.** The table is
/// 4096 pairs and a buffer would carry it exactly; but the raster path's layout
/// is at the WebGPU storage-buffer ceiling in its *vertex* stage, and Slang's
/// Metal backend materialises every global into every entry point — so a row
/// bound to the fragment stage alone would be a global the vertex stage names
/// and nothing fills. See [`crcbl_hal::check_portable_storage_buffers`] and
/// binding 7's `geometry` visibility.
const SPECULAR_DFG_BINDING: u32 = 25;

/// The bind-group slot §2's normal page is sampled through.
///
/// **Appended past [`SPECULAR_DFG_BINDING`], never inserted**, for
/// [`PROBE_TABLE_BINDING`]'s reason exactly: `crcbl-mtl` gives a resource the
/// next index in its Metal argument table by counting the same-table entries of
/// this layout, and Slang numbers a stage's arguments by declaration order, so
/// the two agree only while both ascend. 26 is past everything
/// `mesh_cluster.slang` declares as well — that file reaches 24 — so it needs no
/// mirror there and no index anything already owns moves.
///
/// **No sampler of its own.** The page is read through
/// [`PAGE_SAMPLER_BINDING`]'s sampler, the same trilinear anisotropic one the
/// base-colour page uses, because it is sampled at the same UV over an image of
/// the same extent. A second sampler would be a second layout entry, a second
/// index in Metal's sampler argument table, and a second thing
/// [`ForwardRenderer::set_anisotropy`] has to rebuild — for a filter that would
/// be identical.
const NORMAL_PAGE_BINDING: u32 = 26;

/// The bind-group slot `docs/plan/44-lighting.md`'s rung 5 reads its linearly
/// transformed cosine table through.
///
/// **Appended past [`NORMAL_PAGE_BINDING`], never inserted**, for that
/// constant's reason exactly: `crcbl-mtl` gives a resource the next index in its
/// Metal argument table by counting the same-table entries of this layout, and
/// Slang numbers a stage's arguments by declaration order, so the two agree only
/// while both ascend. 27 is past everything `mesh_cluster.slang` declares, so it
/// needs no mirror there and no index anything already owns moves.
///
/// **No sampler**, like the table two rows below it: four `Load`s and a bilinear
/// blend written out in the shader, because a hardware filter's weights are
/// fixed-function arithmetic four rasterisers compute independently and these
/// goldens are compared across all four.
const LTC_TABLE_BINDING: u32 = 27;

/// The bind-group slot `docs/plan/45-shadows.md`'s contact-shadow channel is
/// read through.
///
/// **Appended past [`LTC_TABLE_BINDING`], never inserted**, for that constant's
/// reason exactly: `crcbl-mtl` gives a resource the next index in its Metal
/// argument table by counting the same-table entries of this layout, and Slang
/// numbers a stage's arguments by declaration order, so the two agree only while
/// both ascend. 28 is past everything `mesh_cluster.slang` declares, so it needs
/// no mirror there and no index anything already owns moves.
///
/// **No sampler**, like the occlusion channel: `mesh.slang`'s `contact_at` is a
/// clamped `Load` at `SV_Position.xy`, and the channel is exactly the size of
/// the frame on every pass that computed one.
const CONTACT_SHADOW_BINDING: u32 = 28;

/// The bind-group slot `docs/plan/50-irradiance-probes.md`'s per-probe
/// visibility maps are read through: one `Rg32Float` layer per probe.
///
/// **Appended past [`CONTACT_SHADOW_BINDING`], never inserted**, for that
/// constant's reason exactly: `crcbl-mtl` gives a resource the next index in its
/// Metal argument table by counting the same-table entries of this layout, and
/// Slang numbers a stage's arguments by declaration order, so the two agree only
/// while both ascend. 29 is past everything `mesh_cluster.slang` declares, so it
/// needs no mirror there and no index anything already owns moves —
/// `msl/mesh.metal` puts it at `texture(7)`, the next free index of that table.
///
/// **A texture rather than a storage buffer, and that is forced**: the raster
/// path's vertex stage is already at every storage buffer a WebGPU device
/// guarantees, which is the same reason [`SPECULAR_DFG_BINDING`] is an image.
///
/// **No sampler**, like the occlusion channel and both cooked tables:
/// `mesh.slang`'s `probe_moments` is four clamped `Load`s and a blend written
/// out, because a hardware filter's weights are fixed-function arithmetic four
/// rasterisers compute independently — and because `Rg32Float` is not a
/// filterable format on WebGPU without an optional feature.
const PROBE_VISIBILITY_BINDING: u32 = 29;

/// [`crcbl_shaders::dfg::DFG_SIZE`] as an image extent.
///
/// The table is square, and its size is a compile-time constant, so the
/// conversion belongs here rather than as a fallible one at the upload.
const DFG_SIZE_U32: u32 = dfg::DFG_SIZE as u32;

/// [`crcbl_shaders::ltc::LTC_SIZE`] as an image extent.
///
/// The same number as [`DFG_SIZE_U32`], and a separate constant rather than a
/// reuse of it: `crcbl_shaders::ltc`'s `the_grid_is_the_dfg_table_s_grid` is
/// what says the two tables are the same square, and each upload naming its own
/// module is what would make a day they were not into a compile error here
/// rather than a stretched image.
const LTC_SIZE_U32: u32 = ltc::LTC_SIZE as u32;

/// The `Rgba8Unorm` texel that occludes nothing and steers nothing.
///
/// What [`ForwardRenderer::ambient_occlusion_placeholder`] holds, and therefore
/// what `mesh.slang` reads on a frame drawing without
/// [`RenderEffects::AMBIENT_OCCLUSION`].
///
/// **`r` is `0xFF`, which is `1.0`**: the shading multiplies its ambient by
/// this and the term is untouched. Any other value would be a silent global
/// ambient scale.
///
/// **`gba` are [`ssao::BENT_NORMAL_NONE`], which decodes to a zero-length
/// direction** — the sentinel the occlusion chain writes wherever it has no
/// bent direction to report, and which `mesh.slang`'s `bent_normal_at` answers
/// with the fragment's own shading normal. So "no occlusion pass ran" and "no
/// direction at this pixel" are one case with one answer, and a frame drawn
/// with the effect off is the frame this renderer drew before the channel was
/// widened. Encoding a direction here instead would tilt every ambient term in
/// such a frame towards one world axis.
const AMBIENT_OCCLUSION_NONE: [u8; 4] = [
    0xFF,
    ssao::BENT_NORMAL_NONE,
    ssao::BENT_NORMAL_NONE,
    ssao::BENT_NORMAL_NONE,
];

/// The `R8Unorm` texel the sun reaches unobstructed: `1.0`, which is `0xFF`.
///
/// What [`ForwardRenderer::contact_shadow_placeholder`] holds, and therefore
/// what `mesh.slang` reads on a frame drawing without
/// [`RenderEffects::CONTACT_SHADOWS`] — it multiplies `sun_visibility` by this
/// and the cascades have the last word alone. Any other value would be a silent
/// global scale on the sun.
///
/// The same byte as [`AMBIENT_OCCLUSION_NONE`]'s first and a constant of its
/// own, for the reason [`LTC_SIZE_U32`] is not [`DFG_SIZE_U32`]: the two
/// placeholders stand for different facts, and a day one of them wanted a
/// different value should be an edit here rather than a search for which
/// callers of a shared name meant which thing.
const CONTACT_SHADOW_NONE: u8 = 0xFF;

/// The one `Rg32Float` texel that occludes nothing: a mean distance of
/// [`probe_visibility::FAR`](crcbl_shaders::probe_visibility::FAR) and its
/// square.
///
/// What [`ForwardRenderer::probe_visibility_placeholder`] holds, and therefore
/// what `mesh.slang` reads on a frame drawn before anything was captured or with
/// `r_probe_visibility` off. No surface in any scene is further away than that,
/// so `probe_weight` answers one for every probe and every fragment, and the
/// grid is the unweighted trilinear blend it was before this rung — which is
/// what makes the off position bit-identical rather than merely close.
///
/// Any *smaller* value would be a silent occluder: every probe in every scene
/// would read as being behind a wall, and the whole grid would collapse onto
/// `PROBE_OCCLUDED_WEIGHT`.
fn probe_visibility_none() -> [u8; crcbl_shaders::probe_visibility::TEXEL_BYTES] {
    let mut texel = [0u8; crcbl_shaders::probe_visibility::TEXEL_BYTES];
    let far = crcbl_shaders::probe_visibility::FAR;
    texel[..4].copy_from_slice(&far.to_le_bytes());
    texel[4..].copy_from_slice(&(far * far).to_le_bytes());
    texel
}

/// The reversed-Z far plane, in the two places that have to agree about it.
///
/// `ssao.slang` leaves early at exactly this depth because the unprojection there
/// divides by a `clip.w` an infinite reversed-Z projection takes to zero. The
/// shader cannot include the seam and `crcbl-shaders` does not depend on it, so
/// the mirror is checked here — which is the only place both names are in scope.
const _: () = assert!(crcbl_hal::depth::CLEAR == crcbl_shaders::ssao::DEPTH_FAR);

/// How many buckets a resident mesh occupies, which is **not the same on every
/// geometry path** — the one place in this renderer the two differ in shape
/// rather than in the call they record.
///
/// A flat mesh is one bucket either way. A DAG is one bucket on
/// [`EmitTail::Mesh`]: a bucket is a mesh's run of clusters, and the point of
/// `docs/plan/25-lod.md`'s per-cluster selection is that one dispatch covers
/// several levels at once, so the run is every level's clusters end to end and
/// the bucket's mesh id is level 0's.
///
/// On the two indirect tails a DAG is one bucket **per level**, because there
/// the same plan takes a uniform cut and a level is drawn as an ordinary index
/// range: a DAG's levels are separate mesh table entries, `draw_gen.slang`
/// selects one per instance, and a bucket is what an indexed draw of one index
/// range *is*. See [`ForwardRenderer::level_buckets`].
const fn buckets_for(levels: usize, emit: EmitTail) -> usize {
    if emit.is_mesh() { 1 } else { levels }
}

/// The pixel budget `docs/plan/25-lod.md`'s descent compares a group's projected
/// error against, unless a caller sets another with
/// [`ForwardRenderer::set_lod_error_budget`].
///
/// One pixel, because that is what the metric is *for*: a level's error is how
/// far its simplification may have moved the surface, so a budget of a pixel is
/// the point at which a level change stops being something a viewer can see.
/// The plan's "correct thresholds make pops sub-pixel by definition" is this
/// number.
const LOD_ERROR_BUDGET: f32 = 1.0;

/// Bytes one bucket's draw-constant block occupies, whichever of the two blocks
/// it holds.
///
/// The larger of [`mesh::DRAW_CONSTANTS_SIZE`] and
/// [`CLUSTER_DRAW_CONSTANTS_SIZE`](crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE),
/// because which one a bucket holds is the geometry path's decision and the
/// buffer, its dynamic stride and the range the bind group names are all fixed
/// before that decision reaches them.
///
/// **The bound range is the half that bites.** A range sized for the smaller
/// block leaves the larger one's tail outside it, and a uniform read past the
/// bound range is not a fault — it is a zero. Sized at sixteen bytes, the mesh
/// path's `group_stride` read back as zero and every instance descended against
/// instance zero's LOD state.
const DRAW_CONSTANTS_BLOCK: u64 =
    if mesh::DRAW_CONSTANTS_SIZE > crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE {
        mesh::DRAW_CONSTANTS_SIZE as u64
    } else {
        crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE as u64
    };

/// How far below the budget an already-expanded group is held before it
/// collapses again — `docs/plan/25-lod.md`'s "switch-up and switch-down differ",
/// as a fraction of [`LOD_ERROR_BUDGET`].
///
/// A ratio and not an offset because the band has to scale with the budget: a
/// fixed number of pixels is most of a one-pixel budget and none of a fifty-pixel
/// one. A fifth of the budget is a deadband a camera has to move decisively out
/// of, and one a camera drifting along a boundary never leaves — which is the
/// flicker the plan is about. `crcbl_shaders::cluster_select::LodBudgets` is the
/// pair this produces, and a ratio of one is that type's `sharp`: no band, and
/// the setting that shows the band is what stops the flicker.
const LOD_HOLD_RATIO: f32 = 0.8;

/// How much larger than the camera's the pixel budget a shadow cascade selects
/// under is — `docs/plan/25-lod.md`'s "**Shadow LOD bias**: shadow-pass culling
/// selects +1/+2 coarser levels — casters are cheap where it never shows".
///
/// # A budget factor, because a level is not a parameter of the rule
///
/// The descent compares a group's projected error to a pixel budget; it never
/// names a level. So "one or two levels coarser" is an *intent*, and the budget
/// that produces it is the setting — and the two do not convert, because how far
/// apart two levels' errors are is a property of the mesh. The committed dunes
/// DAG is the demonstration: the step from one level's error to the next is
/// several times larger in the middle of that hierarchy than at its base, and
/// across its top levels there is no step at all — they report one error and are
/// never separately selected by any budget. A bias expressed in levels would
/// mean something different at each of those. `cook-clusters` prints the per
/// level errors it built.
///
/// Four, then, is two doublings of the budget: enough that a caster is drawn
/// coarser than the same object is in the colour pass, and small enough to stay
/// inside what a shadow hides. What hides it is that a shadow edge is filtered —
/// the atlas is read through a comparison sampler with linear filtering, so an
/// edge arrives already spread over neighbouring texels — and that the atlas
/// quantises every caster to [`shadow::TILE`] texels per cascade before anything
/// samples it.
///
/// # Why one number, applied to the whole pass
///
/// **A cut is a cover only while expansion is monotone up the DAG**, and this
/// scaling is what keeps it so. Multiplying both budgets by one positive
/// constant leaves the rule in
/// [`crcbl_shaders::cluster_select`]'s form with different constants in it, and
/// that module's induction turns on the projected error being monotone up the
/// DAG rather than on what the constants are — so the shadow pass's cut is a
/// cover for exactly the reason the camera's is. A bias that varied by cluster,
/// by group, by level or by cascade would not be: two groups on one branch would
/// be judged against different budgets, and a child expanding under an
/// unexpanded parent is a hole.
///
/// Being larger than one also makes the shadow pass's expanded set a **subset**
/// of the colour pass's, since the two now select from one eye at one scale: a
/// group over `k` times the budget is over the budget. So the shadow cut is
/// never finer than the camera's anywhere, which is the property
/// `crcbl-vk`'s `the_shadow_cascades_select_coarser_than_the_camera` reads back
/// and asserts.
pub const SHADOW_LOD_BIAS: f32 = 4.0;

/// How many culls the shadow atlas needs: one per cascade and **one per shadowed
/// light**, whatever kind of light it is.
///
/// Topic 18's fourth decision, as a number. A point light's six faces share the
/// light's sphere — which is what a cull tests against — so they share one
/// [`DrawGen`] and its visible set, and the six draws differ in a matrix and a
/// viewport. Six generators per light would be six copies of
/// [`TileBuffers::group_state`], which is where the cost of one is written down.
const SHADOW_CULLS: usize = shadow::CASCADES + shadow::LIGHT_SLOTS;

/// How many shadow *views* there are: one per cascade, and one per face of every
/// light slot.
///
/// A view is what has a matrix, so it is what has a frame-block copy, a bind
/// group and a cut of its own — while a *tile* is where a view lands in the
/// atlas and a *cull* is which visible set it draws. The three were one number
/// until a light could own six tiles.
///
/// Every slot carries [`shadow::POINT_FACES`] views whether the light in it is a
/// point or a spot, because a bind group names its buffers when it is created
/// and the kind of light holding a slot changes per frame. A spot uses face 0's
/// view and the other five are never recorded — they cost a uniform buffer and a
/// descriptor each, which is nothing beside the [`DrawGen`] a second *cull*
/// would cost.
const SHADOW_VIEWS: usize = shadow::CASCADES + shadow::LIGHT_SLOTS * shadow::POINT_FACES;

/// Cascade 0's view, which is the one `docs/plan/50-irradiance-probes.md`'s
/// reflective shadow map is drawn through.
///
/// **A cascade is its own view index**, which is what `add_shadow_pass`'s view
/// list pushes; [`shadow_view`] numbers a *light slot's* faces and starts past
/// [`shadow::CASCADES`], so it is not the function to reach for here.
const CASCADE_ZERO_VIEW: usize = 0;

const _: () = assert!(
    CASCADE_ZERO_VIEW < shadow::CASCADES,
    "cascade 0's view has to be one of the cascades' own, not a light slot's"
);

/// Which view is face `face` of light slot `slot`.
const fn shadow_view(slot: usize, face: usize) -> usize {
    shadow::CASCADES + slot * shadow::POINT_FACES + face
}

/// Which cull light slot `slot` draws from.
const fn shadow_cull(slot: usize) -> usize {
    shadow::CASCADES + slot
}

/// One punctual shadow face this frame draws into the punctual reflective
/// shadow map, and everything both halves of the updater need of it.
///
/// `docs/plan/50-irradiance-probes.md`'s punctual producer. It carries no
/// matrix and no visible set: a face's transform is already in the uniform block
/// [`ForwardRenderer::shadow_groups`] binds, and its draws are the ones its
/// slot's cull generated for the atlas — which is what makes this rung cost no
/// second cull and no second matrix.
#[derive(Clone, Copy, Debug)]
struct PunctualFace {
    /// Which shadow view this face is — [`shadow_view`]'s answer, and the index
    /// of the bind group whose block holds that face's own `view_proj`.
    view: usize,
    /// Which of the frame's shadow culls generated its draws — [`shadow_cull`]'s
    /// answer for the slot this face belongs to.
    cull: usize,
    /// Where it lands in the punctual map — [`rsm::punctual_tile`], which is the
    /// rectangle the pass sets its viewport to *and* the one the gather reads
    /// this face's texels out of.
    tile: shadow::TileRect,
    /// The light this is a face of, as the gather weighs its texels.
    producer: PunctualProducer,
}

/// One light's row in the gather's producer table, for the face standing at
/// `tile`.
///
/// **Built out of the light's own row**, which is the one place a light becomes
/// numbers — so the falloff, the cone and the colour the bounce is weighed by
/// are the very ones `mesh.slang` weighs the same light's direct term by, rather
/// than a second reading of the same fields that could round or widen
/// differently. [`Light::row`](crate::light::Light::row) is handed [`None`]
/// because a producer occludes through nothing: the gather gates a texel with
/// the probe's own visibility map, not with the light's.
fn producer_row(light: &Light, tile: shadow::TileRect) -> PunctualProducer {
    let row = light.row(None);
    PunctualProducer {
        position: [row.position[0], row.position[1], row.position[2]],
        radius: row.position[3],
        color: [row.color[0], row.color[1], row.color[2]],
        cos_outer: row.direction[3],
        axis: [row.direction[0], row.direction[1], row.direction[2]],
        cos_inner: row.cos_inner,
        origin: [tile.x, tile.y],
        side: tile.side,
        kind: row.kind,
    }
}

const _: () = assert!(
    SHADOW_CULLS == shadow::GROUPS,
    "a cull is a cadence group: `shadow::Cadence` schedules what this file \
     dispatches, and two different counts would schedule a group nothing draws"
);

/// Vertices in the over-sized triangle `mesh.slang`'s `depthClearVertexMain`
/// generates from `SV_VertexID`. No geometry is bound.
const TILE_CLEAR_VERTICES: u32 = 3;

const _: () = assert!(
    crcbl_shaders::mesh::SHADOW_ATLAS_CLEAR_DEPTH == crcbl_hal::depth::CLEAR,
    "the stage that resets one tile writes a depth the attachment's own clear \
     does not, so a held tile and a cleared one would answer differently where \
     nothing was drawn"
);

/// Narrows the shadow pass's viewport and scissor to one tile of the atlas.
///
/// The rectangle the allocator handed out — the same one the fragment stage
/// samples through, out of [`shadow::Selection`]. The graph set a viewport over
/// the whole atlas before the pass body ran, and this is what narrows it: the
/// same clip-space matrix mapped into a different part of the image, and for a
/// point light six different matrices into six of them off one visible set.
///
/// A function rather than the two calls inline, because the tile clear and the
/// draws that follow it walk the same tiles and a viewport set one way for one
/// of them would clear a rectangle nothing draws into.
fn set_shadow_tile(encoder: &mut dyn crcbl_hal::CommandEncoder, tile: shadow::TileRect) {
    let rect = Rect2d {
        x: i32::try_from(tile.x).unwrap_or(i32::MAX),
        y: i32::try_from(tile.y).unwrap_or(i32::MAX),
        width: tile.side,
        height: tile.side,
    };
    encoder.set_viewport(&Viewport {
        x: rect.x as f32,
        y: rect.y as f32,
        width: rect.width as f32,
        height: rect.height as f32,
        ..Viewport::from_size(rect.width, rect.height)
    });
    encoder.set_scissor(&rect);
}

/// What the last executed graph left the renderer-owned import `image` in, as an
/// [`ImportedImage::initial`].
///
/// **[`ResourceState::Undefined`] is the answer to an empty ledger, not a
/// fallback around one.** The ledger records what a graph *executed*, so nothing
/// recorded means nothing has run against this image on this pool — a freshly
/// created image before its first frame, and an image whose pool has been
/// through [`TransientPool::destroy`] — and `Undefined` is what an image no
/// barrier has moved is in. It is also the only declaration that lets the
/// incoming barrier hand the image a layout at all: every other state names an
/// old layout the image is not in.
///
/// The reason this is a lookup and not a field the renderer advances: a graph is
/// built, then compiled, and [`RenderGraph::compile`] can refuse it. A field
/// stepped forward while the passes were declared would have moved for a frame
/// that never reached the GPU, and the next frame's declaration — now wrong —
/// has no ledger entry to contradict it, so [`InitialClaim::Tracked`] would pass
/// it through. One source of truth is what removes that gap rather than
/// narrowing it.
fn imported_state(pool: &TransientPool, image: ImageHandle) -> ResourceState {
    pool.imported_image_use(image)
        .unwrap_or(ResourceState::Undefined)
}

/// The passes [`ForwardRenderer::add_passes`] records itself: the shadow
/// atlas's, the two reflective shadow maps, the depth prepass, the forward pass,
/// the debug draw layer, the tonemap, the ground grid and the
/// culling-statistics copy — plus [`Ssao::MAX_PASSES`], [`Ssr::PASSES`],
/// [`ProbeGather::PASSES`], [`Bloom::MAX_PASSES`], [`Exposure::PASSES`],
/// [`POST_TONEMAP_PASSES`] and [`Upscale::PASSES`] beside them.
///
/// [`AtlasView::PASSES`] has no term of its own and is inside
/// [`POST_TONEMAP_PASSES`] instead, which is where that is argued: the atlas
/// viewer and the resolve are never both recorded.
///
/// The reflective shadow maps and [`ProbeGather::PASSES`] are counted on the
/// ground grid's terms: all three are off unless a scene's
/// [`ProbeUpdate`] asks for them, and a bound short
/// of the frame that does ask would silently stop timing its last pass. The
/// punctual map is a pass whether or not the frame has a punctual light to draw
/// into it — see `ForwardRenderer::add_shadow_pass`, where the clear is what the
/// gather reads on a frame that lights none.
///
/// Every other pass in the frame belongs to a [`DrawGen`] or to the
/// [`LightGrid`], which is why this is the only count written here rather than
/// derived — see [`ForwardRenderer::MAX_PASSES`]. The copy is counted even
/// though a device that refused the readback records one fewer: this is a
/// ceiling, and one that came up short would silently stop timing the last pass
/// of every frame. It is the **all-effects-on** count for the same reason: a
/// frame that switched one off records fewer, and a bound that tracked the
/// toggles would have to be re-sized whenever they moved.
///
/// The ground grid is counted on the same terms, even though it is off by
/// default and is not a [`RenderEffects`] bit: a caller that switches it on
/// records one more pass, and a bound short of that would silently stop timing
/// the last pass of every frame it drew. The render-scale upscale is the second
/// of those — see [`ForwardRenderer::set_render_scale`], which is `1.0` and
/// therefore absent until a caller moves it. The debug draw layer is the third,
/// and [`DebugDraw::MAX_PASSES`] is its own ceiling.
///
/// **The bloom, Hi-Z and occlusion terms are ceilings where the others are
/// counts**: a chain's length is a function of the target's extent — see
/// [`crate::bloom`] and [`crate::hiz`] — and the occlusion pair's is a function
/// of [`crate::ssao::r_ssao_blur_passes`], while this constant is read before
/// either an extent or a console is known. The frame that lands on
/// it is a large one with bloom switched on; every smaller frame records fewer,
/// exactly as a frame with a free shadow slot does.
const RENDER_PASSES: u32 = 8
    + DebugDraw::MAX_PASSES
    + ProbeGather::PASSES
    + Ssao::MAX_PASSES
    + ContactShadows::PASSES
    + Hiz::PASSES
    + Ssr::PASSES
    + Volumetric::PASSES
    + Exposure::PASSES
    + Bloom::MAX_PASSES
    + POST_TONEMAP_PASSES
    + Upscale::PASSES;

/// What the frame's one resolve slot costs at its widest: the larger of the two
/// antialiasing tiers.
///
/// **The larger and not the sum**, because the two are never both recorded —
/// [`RenderEffects::SMAA`] carries that in writing and [`fullscreen_passes`] is
/// where the choice is made. So this is the all-effects-on count as well as the
/// ceiling, which is the property [`RENDER_PASSES`] needs from every term it
/// adds up.
const RESOLVE_PASSES: u32 = if Fxaa::PASSES > Smaa::PASSES {
    Fxaa::PASSES
} else {
    Smaa::PASSES
};

/// What the frame's one **post-tonemap** slot costs at its widest: the resolve
/// above, or [`crate::atlas_view`]'s pass, which replaces the picture instead of
/// filtering it.
///
/// **The larger and not the sum**, on [`RESOLVE_PASSES`]' terms and for a
/// reason of the same shape: [`ForwardRenderer::resolved_effects`] takes *both*
/// antialiasing tiers off for every debug view, and the atlas viewer records
/// only on the frame [`ForwardRenderer::debug_view`] resolved to
/// [`DebugView::ShadowAtlas`] — which is a debug view. So a frame that draws the
/// atlas resolves nothing, a frame that resolves is not drawing the atlas, and
/// neither can be short of this. `the_pass_bound_is_the_widest_frame_the_renderer_records`
/// is what holds the arithmetic to a frame that was actually recorded.
const POST_TONEMAP_PASSES: u32 = if RESOLVE_PASSES > AtlasView::PASSES {
    RESOLVE_PASSES
} else {
    AtlasView::PASSES
};

/// Draws a full-screen pass records: the over-sized triangle, drawn once.
///
/// The smallest fraction of the target extent a frame may be drawn at.
///
/// A quarter in each dimension, which is a sixteenth of the pixels — below that
/// the internal frame is small enough that a Catmull-Rom reconstruction of it
/// stops being a picture of the scene and starts being a picture of the filter,
/// and the Hi-Z pyramid and the bloom chain both run out of levels. Every
/// engine's resolution slider bottoms out somewhere; this is where this one
/// does, and [`ForwardRenderer::set_render_scale`] clamps to it rather than
/// refusing, so a settings slider cannot hand the renderer a frame it cannot
/// draw.
pub const MIN_RENDER_SCALE: f32 = 0.25;

/// The anisotropy the base-colour page is sampled with where the device grants
/// [`Features::SAMPLER_ANISOTROPY`].
///
/// Eight, which is `docs/plan/43-render-standards.md`'s filtering rung's
/// default and the figure current engines default their own slider to: past it
/// the footprint's long axis is already sampled finely enough that sixteen is
/// invisible at a grazing angle and costs the same again in fetches. A device
/// whose [`Limits::max_sampler_anisotropy`](crcbl_hal::Limits) is lower gets
/// that limit instead — [`ForwardRenderer::anisotropy_for`] is the clamp — and a
/// device without the feature gets one, which is the value that turns it off.
pub const DEFAULT_ANISOTROPY: f32 = 8.0;

/// Named rather than written into [`ForwardRenderer::counters`]'s arithmetic,
/// because these are the only draws in this file whose instance and triangle
/// counts the CPU knows exactly — everything else here goes through an indirect
/// call.
const FULLSCREEN_DRAWS: u64 = 1;

/// How many full-screen passes a frame drawing `effects` at `extent` records:
/// the tonemap, always, and each switched-on effect's passes beside it.
///
/// **A function of the frame rather than a constant**, since
/// `docs/plan/18-render-features.md`'s toggles: it was the tonemap alone, then
/// five, and either written down is a number that stops matching the frame. The
/// tonemap is the one that is not conditional — a frame has to reach the
/// swapchain.
///
/// **`extent` is here because the bloom chain's length depends on it** — see
/// [`crate::bloom`] — where the two pairs above are the same two passes at every
/// size. A frame too small for even one chain level records none of them, which
/// is the same arithmetic as the toggle being off. It is the **internal** render
/// extent for the same reason: that is the extent every one of these passes runs
/// at.
///
/// `upscaling` is not a [`RenderEffects`] bit and could not be one — it is a
/// resolution, not an effect — so it arrives as its own argument, on the ground
/// grid's terms. It is false on every frame at full render scale, which is the
/// frame this function counted before the knob existed.
///
/// `ssao_blurs` arrives the same way and for the same reason: it is a console
/// variable rather than an effect bit, and it is the caller's job to read it
/// **once** for the frame. Reading it here as well would be a second read that
/// a command landing between the two could disagree with, and the disagreement
/// would be a counter describing a frame that was not recorded.
fn fullscreen_passes(
    effects: RenderEffects,
    extent: (u32, u32),
    upscaling: bool,
    ssao_blurs: u32,
) -> u64 {
    let mut passes = 1;
    if upscaling {
        passes += Upscale::PASSES as u64;
    }
    if effects.contains(RenderEffects::AMBIENT_OCCLUSION) {
        passes += u64::from(Ssao::passes(ssao_blurs));
    }
    if effects.contains(RenderEffects::CONTACT_SHADOWS) {
        passes += u64::from(ContactShadows::PASSES);
    }
    if effects.contains(RenderEffects::REFLECTIONS) {
        // The march's pair, and the pyramid it climbs — which is as long as the
        // extent allows and is zero on a frame too small to halve once, so this
        // term is the only one here that is not a constant per effect.
        passes += Ssr::PASSES as u64 + u64::from(crate::hiz::levels_for(extent));
    }
    if effects.contains(RenderEffects::BLOOM) {
        passes += u64::from(Bloom::passes_for(extent));
    }
    // **One resolve slot, and the higher tier takes it.** Both bits set is a
    // frame with SMAA's three passes and no FXAA pass at all, which is what
    // [`RenderEffects::SMAA`] means by the two never both running — so this is
    // an `else if` rather than two independent terms.
    if effects.contains(RenderEffects::SMAA) {
        passes += Smaa::PASSES as u64;
    } else if effects.contains(RenderEffects::ANTIALIASING) {
        passes += Fxaa::PASSES as u64;
    }
    if effects.contains(RenderEffects::VOLUMETRIC_FOG) {
        // **One of [`Volumetric::PASSES`], not all three.** The scatter and the
        // column scan are compute dispatches; what this function counts is
        // full-screen *draws*, and only the composite is one.
        passes += 1;
    }
    // [`RenderEffects::AUTO_EXPOSURE`] adds none, and that is the whole of its
    // entry here: all three of its passes are compute dispatches, and the
    // exposure they measure reaches the picture through the tonemap's own draw.
    passes
}

/// The budget an **orthographic** camera selects under: one no group satisfies,
/// so every group expands and the base level is drawn whole.
///
/// The metric divides a projected error by the distance to the group's sphere,
/// and an orthographic projection has no such falloff — see
/// [`Projection::pixels_per_unit`], which is where the trade is written down.
/// Drawing the finest level is the conservative answer and the honest one; a
/// distance term invented for a projection that has none would be neither.
///
/// [`Projection::pixels_per_unit`]: crate::camera::Projection::pixels_per_unit
const LOD_BUDGET_NONE: f32 = f32::NEG_INFINITY;

/// Which indirect call the forward pass records per bucket.
///
/// Derived from [`GeometryPath`] at build and stored, because the answer cannot
/// change while a device is open and re-deriving it inside the pass body would
/// be a capability query per draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmitTail {
    /// [`GeometryPath::MeshShader`]: one `draw_mesh_tasks` per bucket, of a
    /// **mesh** pipeline — no vertex stage, no index buffer and no indirect
    /// arguments anywhere in the call. §3.5's primary geometry path; see
    /// `shaders/mesh_cluster.slang`.
    Mesh,
    /// [`GeometryPath::IndirectCount`]: the draw count comes from GPU memory
    /// too, so the CPU never learns whether a bucket drew anything.
    Count,
    /// [`GeometryPath::IndirectPerBatch`]: one `draw_indexed_indirect` per
    /// bucket with a count of one. An empty bucket's argument structure carries
    /// an instance count of zero and draws nothing, which is what makes this the
    /// same picture rather than an approximation of it.
    PerBatch,
}

impl EmitTail {
    /// What `caps` selects.
    ///
    /// One value per [`GeometryPath`] since 2026-08: the mesh-shader path used
    /// to degrade to an indirect tail and log that it had, because there was no
    /// mesh pipeline to select.
    const fn from_caps(caps: &crcbl_hal::DeviceCaps) -> Self {
        match caps.geometry_path() {
            GeometryPath::MeshShader => Self::Mesh,
            GeometryPath::IndirectCount => Self::Count,
            GeometryPath::IndirectPerBatch => Self::PerBatch,
        }
    }

    /// Whether this tail draws through a mesh pipeline, which decides the bind
    /// group layout, the pipeline kind and the constant block — everything the
    /// two shapes of this pass differ in.
    const fn is_mesh(self) -> bool {
        matches!(self, Self::Mesh)
    }
}

/// Where a skinned object is, and what it draws: [`InstanceDesc`] with a
/// [`SkinnedMesh`] where the description's mesh index would be.
///
/// The mesh is a reservation rather than an index because a skinned draw is not
/// only a mesh — it also names a *run of the vertex pool* the skinning dispatch
/// fills, and which of the reservation's two runs that is changes every frame.
/// [`ForwardRenderer::begin_skinned_frame`] is what keeps it current, so nothing
/// here carries a parity.
#[derive(Clone, Copy, Debug)]
pub struct SkinnedInstanceDesc<'a> {
    /// The region this object is drawn out of, from
    /// [`ForwardRenderer::reserve_skinned`].
    pub mesh: &'a SkinnedMesh,
    /// Which [`SceneDesc::materials`] row it shades through, exactly as
    /// [`InstanceDesc::material`].
    pub material: usize,
    /// Where it is: a model matrix, on [`InstanceDesc::transform`]'s terms.
    ///
    /// The joints are **not** in it — a palette carries each joint's global
    /// transform times its inverse bind matrix, and this is what puts the
    /// deformed mesh in the world. See
    /// [`SkinRange::palette`](crate::skinning::SkinRange::palette).
    pub transform: Mat4,
}

/// Everything the forward frame owns, created once.
#[derive(Debug)]
pub struct ForwardRenderer {
    // Geometry: the global pools, and the meshes resident in them. Each
    // range is resolved once, at build, and `MeshPool::mesh` only hands one out
    // for a mesh whose upload has completed — so there is no way to reach a
    // draw below with a mesh the GPU has not received.
    pool: MeshPool,
    /// The dynamic offset of each bucket's [`mesh::DrawConstants`] block, in
    /// [`SceneDesc::meshes`] order. Its length is the bucket count, which
    /// [`buckets_for`] says is a property of the path as well as of the scene.
    ///
    /// **The whole of what the CPU still says per draw.** Everything else a
    /// draw needs — how many instances, which indices, which vertices — is in
    /// the arguments the GPU wrote or in a table it resolves them through.
    bucket_constants: Vec<u32>,

    // The instance array, and the objects' places in it. Nothing here is
    // rewritten per frame: the pool uploads what a caller changed and nothing
    // else, so a scene nobody moved costs no transfer at all.
    instances: InstancePool,
    /// The mesh table id of each description mesh's **level 0**, in
    /// [`SceneDesc::meshes`] order — what an [`InstanceDesc::mesh`] index
    /// resolves through.
    ///
    /// Level 0's and not the whole run, because that is the entry an instance
    /// carries: `cull.slang` reads a bounding box out of it and a DAG's coarser
    /// levels approximate the same surface inside the same box. The coarser
    /// entries are resident and no instance names one — a cluster reaches its
    /// own level's vertices through
    /// [`crcbl_shaders::cluster_select::ClusterSelect::vertex_base`], and the
    /// uniform cut reaches them through the bucket table.
    ///
    /// Kept rather than resolved per call because every write of an instance
    /// writes the whole record, and an instance that lost its mesh id would
    /// resolve to entry 0 — which is a mesh, and the wrong one.
    mesh_ids: Vec<u32>,
    /// Level 0's pool handle for each description mesh that can be skinned, in
    /// [`SceneDesc::meshes`] order, and [`None`] for the ones that cannot — see
    /// [`ResidentMesh::skinnable`].
    ///
    /// What [`ForwardRenderer::reserve_skinned`] resolves an
    /// [`InstanceDesc::mesh`] index through. Kept rather than looked up, because
    /// there is no way back from a table id to the handle that owns it: an id is
    /// a bare index and a [`MeshHandle`] is generational.
    skinnable_meshes: Vec<Option<MeshHandle>>,
    /// Every object [`ForwardRenderer::add_skinned_instance`] placed, with the
    /// two base vertices it alternates between.
    ///
    /// The list [`ForwardRenderer::begin_skinned_frame`] walks to point each one
    /// at the half of its region *this* frame's dispatch fills. Entries whose
    /// instance has gone stale are dropped as it walks, so
    /// [`ForwardRenderer::remove_instance`] needs to know nothing about
    /// skinning.
    skinned_instances: Vec<SkinnedInstance>,

    /// §3.2's material table, and the rows in it.
    ///
    /// One buffer shared by every frame's bind group, not a ring — see
    /// [`crate::material_table`], which is where that decision lives.
    materials: MaterialTable,
    /// The table row of each description material, in [`SceneDesc::materials`]
    /// order — what an [`InstanceDesc::material`] index resolves through.
    ///
    /// Kept for the reason [`ForwardRenderer::mesh_ids`] is: a `set` writes the
    /// whole record, and an instance that lost its material id would name row 0
    /// by accident, which is a material and only *happens* to be the untinted
    /// one.
    material_ids: Vec<u32>,
    /// `docs/plan/18-render-features.md`'s irradiance grid, row by row.
    ///
    /// One buffer shared by every frame's bind group, like the material table
    /// and for a stronger version of its reason: a probe is written when the
    /// scene is made resident and there is no call that rewrites one. See
    /// [`crate::probe`].
    probes: ProbeTable,
    /// Where those rows are, which rides in the frame block rather than in a
    /// buffer of its own — a fragment needs it before it knows which row to
    /// fetch.
    ///
    /// Kept rather than re-derived because [`SceneDesc`] is read once, at build.
    /// [`ProbeVolume::default`](crcbl_shaders::probe::ProbeVolume::default) is a
    /// grid of nothing, which is what a description with no probes leaves here
    /// and what makes the whole feature add zero.
    probe_volume: crcbl_shaders::probe::ProbeVolume,
    /// §3.2's texture side: one `D2Array` image whose layers the material rows
    /// index. One page, bound once, for every material — see the module docs on
    /// why this is [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) and not
    /// a bindless descriptor array.
    base_color_page: UploadedTexture,
    /// §2's second page: one `D2Array` image of tangent-space normals, whose
    /// layers [`GpuMaterial::normal_texture`](mesh::GpuMaterial::normal_texture)
    /// indexes.
    ///
    /// The base-colour page's own shape at [`NORMAL_PAGE_BINDING`], sampled
    /// through the same UV and the same sampler, and **created linear** — see
    /// [`NORMAL_PAGE_FORMAT`]. Its extent is the base-colour page's, because
    /// [`PageDesc`](crate::scene::PageDesc) carries one for both.
    normal_page: UploadedTexture,
    /// The page's size in texels, as a width and a height.
    ///
    /// Square, because [`PageDesc`](crate::scene::PageDesc) carries one extent
    /// for every layer. Kept rather than re-derived because
    /// [`add_passes`](Self::add_passes) declares the page to the graph and an
    /// [`ImportedImage`] names an extent — and the description it came from is
    /// the caller's, read once at build.
    base_color_page_extent: (u32, u32),
    /// The sampler the page is read through.
    base_color_sampler: SamplerHandle,
    /// The anisotropy [`ForwardRenderer::base_color_sampler`] was created
    /// with, after [`set_anisotropy`](ForwardRenderer::set_anisotropy)'s clamp.
    anisotropy: f32,
    /// `[frame]`: the page sampler that slot's mesh-layout groups name at
    /// [`PAGE_SAMPLER_BINDING`].
    ///
    /// A slot moves onto [`ForwardRenderer::base_color_sampler`] at its own
    /// `begin_frame` and not before — see
    /// [`ForwardRenderer::adopt_page_sampler`] — so this is what says which
    /// slots still name a sampler
    /// [`set_anisotropy`](ForwardRenderer::set_anisotropy) replaced.
    slot_page_samplers: Vec<SamplerHandle>,
    /// Page samplers [`set_anisotropy`](ForwardRenderer::set_anisotropy)
    /// replaced and some slot may still name. Each is destroyed as the last
    /// slot naming it moves off, or at [`destroy`](ForwardRenderer::destroy).
    retired_page_samplers: Vec<SamplerHandle>,

    /// The cull and draw-argument passes, and the indirect arguments they
    /// produce.
    draws: DrawGen,
    /// Draw calls the last [`ForwardRenderer::add_passes`] recorded — see
    /// [`ForwardRenderer::counters`], which is the only thing that reads it.
    ///
    /// Kept rather than recomputed because the shadow pass's share is the number
    /// of **occupied** tiles, which [`ForwardRenderer::add_shadow_pass`] works
    /// out from `docs/plan/18-render-features.md`'s slot allocation and nothing outside
    /// it can restate without becoming the second copy that drifts.
    recorded_draws: u64,
    /// Which call the forward pass records — the device's [`GeometryPath`],
    /// resolved once.
    emit: EmitTail,
    /// §3.5's clusters, and **only on [`EmitTail::Mesh`]**: the two indirect
    /// tails draw the same geometry out of the index pool and read none of
    /// this. `None` is therefore the ordinary state on most devices rather than
    /// a failure, and it is also what makes a fall-through impossible — see
    /// [`ForwardRenderer::geometry_path`].
    clusters: Option<ClusterPool>,
    /// Whether the mesh pipeline was built with §3.5's amplification stage in
    /// front of it — see [`ForwardRenderer::culls_clusters`], which is where
    /// what it means is written down.
    culls_clusters: bool,
    /// Where each description mesh's clusters are in
    /// [`ForwardRenderer::clusters`] — every level of it, as one run, in
    /// [`SceneDesc::meshes`] order. Empty off the mesh path, where there is no
    /// cluster pool at all. See [`ForwardRenderer::cluster_range`].
    mesh_clusters: Vec<ClusterRange>,
    /// The bucket each level of each description mesh draws through, finest
    /// first, in [`SceneDesc::meshes`] order — and empty on the mesh path. See
    /// [`ForwardRenderer::level_buckets`].
    mesh_level_buckets: Vec<Vec<u32>>,
    /// One buffer per frame in flight holding the cut the descent chose, or
    /// empty where there is no amplification stage to choose one. See
    /// [`ForwardRenderer::cluster_selection`].
    cluster_selection: Vec<BufferHandle>,
    /// The pixel budget `docs/plan/25-lod.md`'s descent compares a group's
    /// projected error against. [`LOD_ERROR_BUDGET`] until
    /// [`ForwardRenderer::set_lod_error_budget`] says otherwise.
    lod_error_budget: f32,
    /// How far below that an expanded group is held before it collapses again.
    /// [`LOD_HOLD_RATIO`] until
    /// [`ForwardRenderer::set_lod_hold_ratio`] says otherwise.
    lod_hold_ratio: f32,
    /// What [`begin_frame`](ForwardRenderer::begin_frame) last handed
    /// [`DrawGen::begin_frame`], kept so a reader can compute the same cut
    /// host-side without re-deriving it from the camera.
    ///
    /// Pixels per unit, the budget a group starts expanding over, and the budget
    /// it is held down to — `docs/plan/25-lod.md`'s hysteresis, and
    /// [`LOD_HOLD_RATIO`] is what puts the third below the second.
    lod_params: [f32; 3],
    /// The same three numbers the shadow cascades selected under, which is
    /// [`ForwardRenderer::lod_params`] with both budgets scaled by
    /// [`SHADOW_LOD_BIAS`] and the same pixels-per-unit.
    ///
    /// Kept beside the camera's for its reason: a reader comparing the two cuts
    /// takes both pairs from here rather than re-deriving the bias, which would
    /// be comparing two copies of one arithmetic instead of two selections.
    shadow_lod_params: [f32; 3],

    /// Topic 18's light list and froxel grid, and the compute pass between them.
    lights: LightGrid,
    /// The lights a caller set beside the sun, as
    /// [`ForwardRenderer::set_lights`] was handed them.
    ///
    /// Kept rather than converted on the way in, because a row is the *frame's*
    /// artefact: the sun arrives per frame at
    /// [`begin_frame`](ForwardRenderer::begin_frame) and is row 0, so the list a
    /// frame uploads cannot be assembled until then.
    extra_lights: Vec<Light>,
    /// This frame's froxel grid, as [`begin_frame`](ForwardRenderer::begin_frame)
    /// decided it from the viewport and the camera.
    ///
    /// Held rather than recomputed at [`ForwardRenderer::add_passes`], because
    /// the number of froxels the dispatch covers and the number the frame block
    /// tells the fragment stage about have to be the same one.
    grid: Grid,

    // Per-frame uniforms, one set per frame in flight.
    uniforms: Vec<BufferHandle>,
    /// One [`mesh::DrawConstants`] block per draw, a device dynamic-offset
    /// alignment apart. Shared by every frame's bind group rather than ringed,
    /// because nothing rewrites it after `build`: a mesh's base vertex and an
    /// object's instance index are decided when the pools allocate them.
    draw_constants: BufferHandle,
    mesh_groups: Vec<BindGroupHandle>,
    /// Which frame-in-flight slot is current. Set from
    /// [`InstancePool::begin_frame`]'s return rather than counted here, so the
    /// uniform ring and the instance ring cannot drift apart.
    frame: usize,

    mesh_layout: BindGroupLayoutHandle,
    mesh_pipeline_layout: PipelineLayoutHandle,
    mesh_pipeline: GraphicsPipelineHandle,
    /// [`ForwardRenderer::mesh_pipeline`] again in
    /// [`PolygonMode::Line`], for
    /// [`set_wireframe`](ForwardRenderer::set_wireframe).
    ///
    /// [`None`] until a caller first switches the view on — the ground grid's
    /// arrangement exactly, and for its reason: a second pipeline compiled at
    /// build would be paid for by every sample, golden and headless run that
    /// never asks for a wireframe, which is all of them. Once built it is kept,
    /// because switching the view off is
    /// [`ForwardRenderer::wireframe_on`] and not a release.
    wireframe_pipeline: Option<GraphicsPipelineHandle>,
    /// Whether [`add_passes`](ForwardRenderer::add_passes) drives the forward
    /// pass through [`ForwardRenderer::wireframe_pipeline`]. **`false` by
    /// default**: a wireframe is a tool's view of a document, and every sample
    /// and every golden image predates it.
    wireframe_on: bool,
    /// Whether [`begin_frame`](ForwardRenderer::begin_frame) writes
    /// [`FrameUniforms::NORMALS_VIEW_ON`] into the frame block, which makes
    /// `mesh.slang`'s fragment stage draw world-space normals instead of shading
    /// — see [`set_normals_view`](ForwardRenderer::set_normals_view).
    ///
    /// **`false` by default**, on [`ForwardRenderer::wireframe_on`]'s terms
    /// exactly: it is a tool's view of a document, and every sample and every
    /// golden image predates it.
    ///
    /// [`FrameUniforms::NORMALS_VIEW_ON`]: crcbl_shaders::mesh::FrameUniforms::NORMALS_VIEW_ON
    normals_view: bool,

    /// The exponential height fog the colour pass composites over its
    /// radiance — see [`set_fog`](ForwardRenderer::set_fog).
    ///
    /// **[`Fog::NONE`] by default**, on [`normals_view`](Self::normals_view)'s
    /// terms: a caller who never asks for fog draws the frame this renderer
    /// drew before the feature existed, and exactly that frame — the shader's
    /// composite is an identity at zero density rather than an interpolation
    /// that nearly is.
    fog: Fog,

    /// The gradient sky whose irradiance the colour pass adds to the ambient
    /// term — see [`set_sky`](ForwardRenderer::set_sky).
    ///
    /// **[`Sky::NONE`] by default**, on [`fog`](Self::fog)'s terms: a black
    /// gradient projects to coefficients that are all zero, so the three dot
    /// products the fragment stage adds are zero and no frame drawn before a
    /// sky existed moves.
    sky: Sky,

    /// Whether the colour pass tints each cluster by its DAG level instead of
    /// shading — see [`set_lod_view`](ForwardRenderer::set_lod_view).
    ///
    /// **Wins over [`normals_view`](Self::normals_view) when both are set**,
    /// because one uniform lane carries both switches and the fragment stage
    /// tests the LOD threshold first. `crcbl_shaders`'
    /// `the_lod_view_threshold_lies_above_the_normals_view` is what keeps that
    /// order true.
    lod_view: bool,

    /// Whether the colour pass shades each cluster by the projected error the
    /// LOD selection judged it on, instead of shading it or tinting it — see
    /// [`set_heatmap`](ForwardRenderer::set_heatmap).
    ///
    /// **Wins over both [`lod_view`](Self::lod_view) and
    /// [`normals_view`](Self::normals_view)**, on the same terms and for the same
    /// reason: one uniform lane carries all three switches and the shaders test
    /// the outermost threshold first. `crcbl_shaders`'
    /// `the_heatmap_view_threshold_lies_above_the_lod_view` is what keeps that
    /// order true, and
    /// `the_three_debug_views_resolve_in_one_order_however_they_are_set` is what
    /// holds this side to it.
    heatmap: bool,

    /// Whether the colour pass draws the ambient-occlusion channel as grey
    /// instead of shading — see
    /// [`set_occlusion_view`](ForwardRenderer::set_occlusion_view).
    ///
    /// **Wins over every other view**, on [`heatmap`](Self::heatmap)'s terms and
    /// for its reason: one uniform lane, outermost threshold first.
    /// `crcbl_shaders`' `the_occlusion_view_threshold_lies_above_the_heatmap` is
    /// what keeps that order true, and
    /// `the_debug_views_resolve_in_one_order_however_they_are_set` is what holds
    /// this side to it.
    occlusion_view: bool,

    /// Whether the colour pass draws the motion vector instead of shading — see
    /// [`set_motion_view`](ForwardRenderer::set_motion_view).
    ///
    /// **Wins over every other view**, on [`occlusion_view`](Self::occlusion_view)'s
    /// terms and for its reason: one uniform lane, outermost threshold first.
    /// `crcbl_shaders`' `the_motion_view_threshold_lies_above_the_occlusion_view`
    /// is what keeps that order true, and
    /// `the_debug_views_resolve_in_one_order_however_they_are_set` is what holds
    /// this side to it.
    motion_view: bool,
    /// Whether the colour pass draws the occlusion channel's bent direction
    /// instead of shading — see
    /// [`set_bent_normal_view`](ForwardRenderer::set_bent_normal_view).
    ///
    /// **Wins over every other view**, on [`motion_view`](Self::motion_view)'s
    /// terms: it is the outermost sentinel in the lane, and
    /// `crcbl_shaders`' `the_bent_normal_view_threshold_lies_above_the_motion_view`
    /// is what holds the shader's branch order to it.
    bent_normal_view: bool,

    /// Whether the colour pass tints the shaded picture by the cascade each
    /// sun-lit fragment's shadow was sampled from — see
    /// [`set_cascade_view`](ForwardRenderer::set_cascade_view).
    ///
    /// **Loses to every other view**, which is the opposite of every field
    /// above and for the reason [`DebugView::Cascades`] carries: every one of them
    /// replaces the shaded picture and this one multiplies it, so a frame drawing
    /// any of them has nothing for this tint to reach. `crcbl_shaders`'
    /// `the_cascade_view_threshold_lies_below_every_replacing_view` is what
    /// holds the shader to the same order, from the other end of the lane.
    cascade_view: bool,

    /// Whether the frame draws the shadow atlas over the finished picture — see
    /// [`set_atlas_view`](ForwardRenderer::set_atlas_view).
    ///
    /// **Not a lane of the frame block**, unlike every field above: this view is
    /// a pass rather than a branch in `mesh.slang`, so what reads this is
    /// [`ForwardRenderer::add_passes`] deciding whether to record
    /// [`crate::atlas_view`]'s. It resolves below every replacing view and above
    /// [`cascade_view`](Self::cascade_view) — see
    /// [`ForwardRenderer::debug_view`].
    atlas_view: bool,

    /// Where [`begin_frame`](ForwardRenderer::begin_frame) projects
    /// `docs/plan/25-lod.md`'s selection from, when that is not the camera's own
    /// eye — see
    /// [`set_frozen_selection_eye`](ForwardRenderer::set_frozen_selection_eye).
    ///
    /// **[`None`] is not a position, it is "follow the camera"**, and it is what
    /// every frame the engine has drawn so far did: the eye handed to
    /// [`DrawGen::begin_frame`] is `camera.eye`, byte for byte, so a caller that
    /// never sets this writes the parameter block it wrote before this field
    /// existed.
    ///
    /// **Not a [`DebugView`]**, and deliberately: the three views are one
    /// picture chosen between, while this is a change to what the *selection*
    /// answers and leaves the picture's choice alone. A reviewer freezing the
    /// cut and then reading it off the LOD tint is the combination the feature
    /// exists for, so the two cannot be alternatives.
    frozen_selection_eye: Option<Vec3>,

    /// Topic 18's shadow atlas: one `D32Float` image holding
    /// [`shadow::TILES`] square tiles in a fixed grid — the sun's cascades
    /// first, then one per shadowed light.
    ///
    /// Owned here rather than created by the graph per frame, because its
    /// extent is a **quality setting and not a window size** — nothing about it
    /// changes on a resize, so a transient would be re-created for no reason
    /// and could not be read back by a test that does not own the graph.
    shadow_atlas: ImageHandle,
    shadow_atlas_view: ImageViewHandle,
    /// A 1×1 depth image that exists so the depth-only pipeline's bind group can
    /// fill [`SHADOW_ATLAS_BINDING`] without naming the atlas it is writing.
    ///
    /// Both halves of that sentence are forced:
    ///
    /// * **The slot has to be filled.** Slang's Metal backend materialises every
    ///   global into every entry point — `msl/mesh.metal`'s `vertexMain` takes
    ///   `shadow_atlas [[texture(1)]]` whether it reads it or not — so a
    ///   depth-only pipeline sharing this layout still declares the argument. A
    ///   layout with the slot removed would break Metal alone, on a runner this
    ///   team cannot debug on, which is the trap binding 6's comment already
    ///   records.
    /// * **It cannot be the atlas.** WebGPU refuses a texture that is a render
    ///   attachment and a bind-group resource in the same pass, and the shadow
    ///   pass is exactly that pass.
    ///
    /// Nothing samples it: the shadow pipeline has no fragment stage. It is
    /// brought into the graph beside the atlas so its layout transition is the
    /// graph's like every other, rather than a hand-written barrier.
    shadow_placeholder: ImageHandle,
    shadow_placeholder_view: ImageViewHandle,
    /// The **comparison** sampler the atlas is read through — hardware PCF.
    shadow_sampler: SamplerHandle,
    /// The depth-only pipeline every tile is rendered with — and, driven with
    /// the camera's own draws, the depth prepass's: `mesh.slang`'s
    /// `depthVertexMain` where the geometry comes from a vertex stage, the same
    /// geometry stage as [`ForwardRenderer::mesh_pipeline`] where it comes from
    /// a mesh one, and no fragment stage in either case.
    shadow_pipeline: GraphicsPipelineHandle,
    /// The pipeline that resets one tile of the atlas before it is redrawn, on a
    /// frame that is keeping some of the others — see
    /// [`MeshModules::depth_clear_pipeline`], which is where the whole argument
    /// for it lives.
    shadow_clear_pipeline: GraphicsPipelineHandle,
    /// `docs/plan/50-irradiance-probes.md`'s reflective shadow map — see
    /// [`MeshModules::rsm_pipeline`]. Built on every scene and bound only by a
    /// frame whose volume asked for [`ProbeUpdate::EveryFrame`].
    rsm_pipeline: GraphicsPipelineHandle,
    /// The compute pass that reads that map into the probe rows, and [`None`]
    /// for every scene whose volume is [`ProbeUpdate::Authored`] — which is the
    /// whole of what the default costs such a scene: no pipeline, no buffers, no
    /// pass and no frame time.
    probe_gather: Option<ProbeGather>,
    /// Whether cascade 0's view block and cull parameters were written for the
    /// frame being recorded, so the reflective shadow map can be drawn from
    /// them.
    ///
    /// Not [`ForwardRenderer::shadow_group_redrawn`], which is a claim about the
    /// *atlas*: a frame drawing without
    /// [`RenderEffects::SHADOWS`](crate::effects::RenderEffects::SHADOWS) fits
    /// cascade 0 for the map and writes no tile, and overloading that flag would
    /// report a cascade as redrawn when nothing drew it.
    rsm_cull_ready: bool,
    /// What the scene said fills the probe rows.
    ///
    /// Read by [`ForwardRenderer::begin_frame`] — which keeps cascade 0 off the
    /// shadow atlas's static-cache hold when the updater is on, because the map
    /// is that cascade's own draws — and by the pass recording below.
    probe_update: ProbeUpdate,
    /// How far cascade 0 reached on the frame being recorded, which is the
    /// radius of the sphere it was fitted to — `crate::rsm::texel_area` is what
    /// reads it.
    ///
    /// Written by [`ForwardRenderer::begin_frame`] beside the cascade matrices
    /// themselves, rather than recomputed at pass-record time: a frame that
    /// scaled its gather by one cascade's footprint while drawing the map
    /// through another's would be a bounce that is uniformly wrong, and nothing
    /// in the frame could see it.
    cascade_reach: f32,
    /// The sun's colour premultiplied by its intensity, this frame.
    ///
    /// The reflective shadow map holds albedo rather than flux — `RsmOutput` in
    /// `mesh.slang` argues why — so this is what the gather multiplies by, and it
    /// is read off the same [`DirectionalLight`] the cascades were fitted to.
    /// The sun's *direction* is not carried beside it: the gather needs none, on
    /// `probe_gather.slang`'s own derivation.
    sun_color: Vec3,
    /// One cull and draw-argument pass **per cascade and per shadowed light**,
    /// which is topic 18's "one cull dispatch per cascade against the same
    /// instance/geometry pools" with a shadowed light counting as a cascade does.
    ///
    /// A whole [`DrawGen`] rather than just its cull half: the shadow pass emits
    /// the same indirect call the colour pass does, so it needs the same
    /// arguments, and there is no "cull only" constructor to reach for. The cost
    /// is that each duplicates the clear and draw-argument pipelines and
    /// [`DrawGen`]'s own per-instance state — see
    /// [`TileBuffers::group_state`], which is where that price is written down,
    /// and [`SHADOW_CULLS`], which is why a point light has one of these rather
    /// than six.
    ///
    /// **Indexed by cull**: `0..shadow::CASCADES` are the sun's and
    /// [`shadow_cull`] is where a light slot's is.
    shadow_draws: Vec<DrawGen>,
    /// `[frame][view]`: a copy of the frame block whose `view_proj` **is** that
    /// view's matrix, so the depth-only pipeline runs the unmodified vertex and
    /// mesh stages rather than a second transform path.
    shadow_uniforms: Vec<Vec<BufferHandle>>,
    /// `[frame][view]`: the mesh layout again, reading that view's uniforms and
    /// the survivors of the cull its slot owns.
    shadow_groups: Vec<Vec<BindGroupHandle>>,
    /// `[frame][view]`: the cut that view's amplification stage chose, one word
    /// per resident cluster — and empty where there is no such stage.
    ///
    /// **A buffer per view rather than the camera's**, unlike every other
    /// resource these passes share: the colour pass is recorded last and writes
    /// [`ForwardRenderer::cluster_selection`] over whatever a view left in it,
    /// so a view writing there leaves nothing behind to read. Without one of
    /// these the shadow pass's descent is unobservable — which is the state
    /// [`SHADOW_LOD_BIAS`] arrived in, and it is what a bias nothing can measure
    /// would have stayed in.
    shadow_selection: Vec<Vec<BufferHandle>>,
    /// Which of [`ForwardRenderer::extra_lights`] holds each shadow slot, where
    /// in the atlas's light region its tiles are, and the memory that keeps that
    /// answer from flickering.
    ///
    /// Re-run every [`ForwardRenderer::begin_frame`] and read by
    /// [`ForwardRenderer::add_shadow_pass`], which records a cull for each
    /// occupied slot and a viewport per face it has, and nothing at all for the
    /// free ones — so an unheld tile keeps the reversed-Z clear, which reads as
    /// "nothing stored, fully lit" wherever anything did sample it.
    shadow_lights: shadow::Selection,
    /// `[group]`: everything that group's maps would be drawn from, as
    /// [`ForwardRenderer::shadow_group_record`] spells it.
    ///
    /// Kept only to tell one frame's answer from the last one's: it is compared
    /// and then replaced, and what travels from there is the number below.
    ///
    /// **Per group rather than per atlas**, since the cadence rung: a group is a
    /// cascade or a light slot's whole run of tiles, and holding one map while
    /// redrawing another is the whole of what that rung buys.
    shadow_group_inputs: Vec<Vec<u8>>,
    /// `[group]`: a number that moves whenever that group's entry of
    /// [`ForwardRenderer::shadow_group_inputs`] does, and never comes back to a
    /// value it has had.
    ///
    /// So "the image already holds this" is one integer comparison, which is
    /// what lets the pass body below carry the answer out of the graph.
    shadow_group_id: Vec<u64>,
    /// The number given to the shadow pass this renderer recorded last.
    ///
    /// Moves once per recorded pass, so the body below can say *which* pass ran
    /// with one integer.
    shadow_pass_id: u64,
    /// The [`ForwardRenderer::shadow_pass_id`] of the last shadow pass that
    /// **ran**, written by that pass's own body.
    ///
    /// By the body rather than by the call that recorded it, and that is the
    /// whole of why this is shared state rather than a plain field: a frame
    /// whose graph was refused — or built and dropped — recorded a pass that
    /// never executed, so the image still holds whatever the frame before it
    /// left there. `a_refused_frame_leaves_the_shadow_atlas_where_it_was` is
    /// that case, and it is the same boundary [`TransientPool`]'s ledger of
    /// imported images uses for the same reason.
    ///
    /// Zero until a pass has run, and the ids start above it, so the first frame
    /// of all draws.
    shadow_pass_ran: Arc<AtomicU64>,
    /// What the shadow pass recorded last would leave the image holding, if its
    /// body ran.
    ///
    /// Taken and applied by the next [`ForwardRenderer::begin_frame`], which is
    /// the only place that can tell whether it did — see
    /// [`ForwardRenderer::shadow_pass_ran`].
    shadow_pending: Option<ShadowCommit>,
    /// `[group]`: the id, the centre that group's maps were projected from and
    /// how far they reach, as the record the **image** holds describes them.
    ///
    /// The id is what says a map is out of date. The centre and reach are what
    /// say it is about somewhere else: a map whose own centre has moved further
    /// than its own reach covers ground it was never drawn for, and holding
    /// that is worse than a shadow that lags — see
    /// [`ForwardRenderer::shadow_cadence_reset`].
    shadow_group_held: Vec<(u64, Vec3, f32)>,
    /// `[group]`: whether this frame redraws that group's maps.
    ///
    /// Frozen by [`ForwardRenderer::begin_frame`] for
    /// [`ForwardRenderer::frame_effects`]' reason: this call decides it and
    /// `add_passes` acts on it, and a value that moved between them would skip
    /// a cull whose draws were recorded, or the other way about.
    shadow_group_redrawn: Vec<bool>,
    /// Where every atlas slot's map was laid out on the frame the **image** was
    /// drawn from, so a frame can tell whether the texels it is keeping still
    /// describe the layout it is about to sample through.
    ///
    /// A layout that has moved is what makes a frame clear the whole attachment
    /// and redraw every group: a tile left over from a different layout is
    /// texels nothing this frame can account for. Promoted beside
    /// [`ForwardRenderer::shadow_group_held`].
    ///
    /// [`None`] until a pass has drawn, which is **not** the layout a frame
    /// with shadows off produces: an image no pass has touched holds undefined
    /// memory rather than a clear, so the first frame of all has to write one
    /// whatever it is asked for.
    shadow_atlas_layout: Option<Vec<shadow::TileRect>>,
    /// Whether any map this frame samples is one an earlier frame drew, rather
    /// than one this frame did.
    ///
    /// The cadence's own readback — see
    /// [`ForwardRenderer::shadow_atlas_cached`], which is this for the whole
    /// atlas.
    shadow_atlas_cached: bool,
    /// Whether this frame ignored the cadence because some map's region moved
    /// out from under it — see [`ForwardRenderer::shadow_cadence_reset`].
    shadow_cadence_reset: bool,
    /// How many tiles this frame redraws — see
    /// [`ForwardRenderer::shadow_faces_redrawn`].
    shadow_faces_redrawn: u32,
    /// A shadow cadence the caller pinned, or [`None`] to take the console's.
    ///
    /// [`ForwardRenderer::set_effect_request`]'s shape at this knob and for its
    /// reason: `shadow::r_shadow_cadence` and `shadow::r_shadow_faces` are
    /// process-global, and an application that wants a tier's own pair — or a
    /// test that wants a frame nothing else in the process can move — needs one
    /// that is not.
    shadow_cadence_request: Option<shadow::Cadence>,
    /// Whether this frame's shadow pass clears the whole attachment, rather
    /// than loading it and resetting only the tiles it redraws.
    ///
    /// Frozen beside [`ForwardRenderer::shadow_group_redrawn`] and for its
    /// reason: the decision is `begin_frame`'s and the recording is
    /// `add_shadow_pass`'s, and a value that moved between them would load an
    /// attachment whose tiles nothing had reset.
    shadow_atlas_clears: bool,
    /// How many frames have been opened, which is what the cadence is keyed to.
    ///
    /// **Monotonic, unlike [`ForwardRenderer::frame`]**, which is the index of
    /// the frame in flight and wraps every [`FRAMES_IN_FLIGHT`]. A schedule
    /// keyed to that would repeat every few frames and bound nothing.
    shadow_frame: u64,
    /// Whether this frame dispatches skinning, and so rewrites part of the
    /// vertex pool the shadow pass draws from.
    ///
    /// The atlas's one **always redraw**: a skinned caster's vertices are
    /// produced by a compute pass into the pool, so there is nothing on the
    /// host to compare a frame's geometry against — the palette a caller hands
    /// over is not the vertices, and the vertices are never read back. Set by
    /// [`ForwardRenderer::begin_skinned_frame`], which is the only call that
    /// can make it true, and cleared by [`ForwardRenderer::begin_frame`], which
    /// dispatches none.
    frame_skins: bool,

    /// Topic 03 §3.6's one permitted readback: the camera cull's statistics, on
    /// a delayed ring — see [`crate::cull_stats`], which is where the shape and
    /// the latency are argued.
    ///
    /// **The camera's cull only.** [`ForwardRenderer::shadow_draws`] each have a
    /// statistics buffer of their own and none of them is read: a cascade's
    /// survivor count is a different question about a different frustum, and
    /// added into this one it would produce a number larger than the pool holds.
    ///
    /// [`None`] on a device that would not give out the buffers, which is what
    /// makes the counters row degrade to `indirect` rather than fail.
    cull_stats: Option<CullStatsRing>,

    tonemap_layout: BindGroupLayoutHandle,
    tonemap_pipeline_layout: PipelineLayoutHandle,
    tonemap_pipeline: GraphicsPipelineHandle,
    sampler: SamplerHandle,
    /// `[frame]`: `tonemap.slang`'s exposure block, written by
    /// [`begin_frame`](ForwardRenderer::begin_frame).
    ///
    /// One per frame in flight for the frame uniforms' reason exactly — the
    /// previous frame may still be reading last frame's while this one is
    /// written.
    tonemap_uniforms: Vec<BufferHandle>,
    /// `[frame]`: the tonemap group, cached against the scene target's view.
    ///
    /// Rebuilt only when that view changes, which is only on a resize. The graph
    /// hands the view to the pass body; caching against it is what keeps a
    /// steady-state frame free of descriptor writes.
    ///
    /// **One per frame in flight**, for [`crate::ssao`]'s reason: this group
    /// names [`ForwardRenderer::tonemap_uniforms`] as well as the scene
    /// transient, and that is a ring — a single cache keyed on the view alone
    /// would hand the even frames' block to the odd frames.
    tonemap_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// The multiplier the tonemap pass applies before its clamp — see
    /// [`set_exposure`](ForwardRenderer::set_exposure).
    ///
    /// Always within [`EXPOSURE_MIN`]`..=`[`EXPOSURE_MAX`], because the setter is
    /// the only thing that writes it and it clamps.
    exposure: f32,
    /// How far auto-exposure may move in one frame — see
    /// [`set_exposure_adaptation`](ForwardRenderer::set_exposure_adaptation).
    ///
    /// [`None`] until a caller says otherwise, and that is the whole distance in
    /// one frame: the picture the pass drew before adaptation existed.
    exposure_adaptation: Option<ExposureAdaptation>,
    /// Which operator the tonemap pass runs — see
    /// [`set_tonemap_curve`](ForwardRenderer::set_tonemap_curve).
    ///
    /// [`TonemapCurve::Clamp`](crcbl_shaders::tonemap::TonemapCurve::Clamp)
    /// until a caller says otherwise, which is what keeps a renderer nobody has
    /// configured drawing the frame it drew before the selector existed.
    tonemap_curve: tonemap::TonemapCurve,

    /// The format the tonemap pipeline was built for. A swapchain format change
    /// needs a new pipeline, which is why it is remembered rather than assumed.
    target_format: Format,

    /// A 1×1 `R8Unorm` image holding [`AMBIENT_OCCLUSION_NONE`], so a group of
    /// the mesh layout can fill [`AMBIENT_OCCLUSION_BINDING`] without naming an
    /// occlusion image that does not exist yet.
    ///
    /// [`ForwardRenderer::shadow_placeholder`]'s argument, one binding along.
    /// The shadow pass and the depth prepass name it and never sample it — a
    /// depth-only pipeline has no fragment stage — and **the forward pass names
    /// it too on a frame drawing without [`RenderEffects::AMBIENT_OCCLUSION`]**,
    /// where it is the whole of that effect's off-switch. An AO-on frame binds
    /// the occlusion pair's blurred output instead.
    ///
    /// **It is white because that is the value that occludes nothing**, which is
    /// what makes it an honest stand-in for "no occlusion was computed" rather
    /// than decoration. That it *reads* as white is `mesh.slang`'s doing: the
    /// binding is fetched with a `Load`, and a `Load` past a 1×1 image's one
    /// texel is zero on every backend, so the shader clamps the coordinate to the
    /// image's extent. See [`add_passes`](ForwardRenderer::add_passes).
    ///
    /// It is uploaded rather than cleared, so it is in
    /// [`ResourceState::ShaderRead`] from the moment it exists and no pass has to
    /// declare it to give it a layout.
    ambient_occlusion_placeholder: UploadedTexture,
    /// A 1×1 `R8Unorm` image holding [`CONTACT_SHADOW_NONE`], so a group of the
    /// mesh layout can fill [`CONTACT_SHADOW_BINDING`] without naming a
    /// contact-shadow image that does not exist yet.
    ///
    /// [`ForwardRenderer::ambient_occlusion_placeholder`]'s argument, one
    /// binding along and with one difference worth stating: an unclamped `Load`
    /// past this one's single texel would cost the frame its **sun**, not its
    /// ambient. `mesh.slang`'s `contact_at` is what clamps it.
    contact_shadow_placeholder: UploadedTexture,
    /// A 1×1×1 `Rg32Float` image holding [`probe_visibility_none`], so a group
    /// of the mesh layout can fill [`PROBE_VISIBILITY_BINDING`] before anything
    /// has been captured — and so the console switch has something to name when
    /// the maps are turned off.
    ///
    /// [`ForwardRenderer::ambient_occlusion_placeholder`]'s argument, with the
    /// same clamp on the shader's side: `mesh.slang`'s `probe_moments` clamps
    /// against `GetDimensions`, so all four of its taps land on this image's one
    /// texel and every probe reads as unobstructed.
    probe_visibility_placeholder: UploadedTexture,
    /// The captured maps, one `Rg32Float` layer per probe — `None` until
    /// [`ForwardRenderer::capture_probe_visibility`] has run, which is every
    /// frame drawn by a caller that never calls it.
    probe_visibility: Option<UploadedTexture>,
    /// The scene's static triangles, kept for that capture and for nothing else.
    ///
    /// Empty for a description that reserved no probes, which is what makes the
    /// retention cost of this rung zero for every caller that does not use it —
    /// see [`crate::probe_visibility`].
    probe_occluders: crate::probe_visibility::Occluders,
    /// The split-sum `DFG` table as an `Rgba8Unorm` image — binding
    /// [`SPECULAR_DFG_BINDING`], read by multi-scatter energy compensation and
    /// by every area light's specular term.
    ///
    /// Uploaded once from `crcbl_shaders::dfg::pair_texels` and never written
    /// again: it is a property of the GGX lobe rather than of a scene, a view or
    /// a frame. Unlike [`ForwardRenderer::ambient_occlusion_placeholder`] beside
    /// it this is not a stand-in for anything — there is no path on which the
    /// table is absent, because there is no path on which the lobe is.
    specular_dfg: UploadedTexture,
    /// The linearly transformed cosine fit as an `Rgba16Float` image — binding
    /// [`LTC_TABLE_BINDING`], and the whole of an area light's specular shape.
    ///
    /// Uploaded once from `crcbl_shaders::ltc::texels`, on the table above it's
    /// terms exactly and for the same reason: it is a property of the lobe, and
    /// it is bound on every path because Slang's Metal backend materialises
    /// every global into every entry point.
    ltc_table: UploadedTexture,
    /// `[frame]`: the entries [`ForwardRenderer::mesh_groups`] was built from.
    ///
    /// Kept because the occlusion image is a graph transient: its view is known
    /// only at execute time, so the camera's group has to be rebuilt inside the
    /// forward pass, and re-deriving twenty bindings there would mean carrying
    /// half of `build`'s locals into the frame. Exactly two entries —
    /// [`AMBIENT_OCCLUSION_BINDING`]'s and [`CONTACT_SHADOW_BINDING`]'s — differ
    /// between the stored list and what the rebuild writes.
    mesh_group_entries: Vec<Vec<BindGroupEntry>>,
    /// `[frame]`: the entries [`ForwardRenderer::prepass_groups`] was built
    /// from, and `[frame][view]` those of [`ForwardRenderer::shadow_groups`].
    ///
    /// Kept for one rebuild only: the page sampler's, which
    /// [`ForwardRenderer::adopt_page_sampler`] performs on every group of the
    /// mesh layout a slot holds. Handles, so the cost is a few words a group.
    prepass_group_entries: Vec<Vec<BindGroupEntry>>,
    shadow_group_entries: Vec<Vec<Vec<BindGroupEntry>>>,
    /// `[frame]`: the camera's group rebuilt against the two screen-space
    /// channels the forward pass reads — the blurred occlusion and the contact
    /// shadow — cached against both views together.
    ///
    /// [`ForwardRenderer::tonemap_groups`]'s shape, one per frame in flight
    /// because the group it replaces is per frame in flight. Rebuilt only when
    /// a view changes, which is only on a resize or a toggle.
    ///
    /// **One cache for both channels rather than one each**, because they are
    /// two bindings of *one* group: rebuilding it twice a frame would be a
    /// descriptor write per pass per frame, which is what
    /// [`crate::ssao::cached_group`] exists to avoid. Its key is every view, so
    /// either one moving is a miss.
    ///
    /// [`ForwardRenderer::mesh_groups`] is the fallback and is *not* dead weight:
    /// it is what the depth prepass binds, because that pass runs before there is
    /// any occlusion to name.
    screen_channel_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `[frame]`: the depth prepass's group — the camera's, with the occlusion
    /// placeholder and **a culling-statistics buffer of its own**.
    ///
    /// The second half is the whole reason this is a group rather than
    /// [`ForwardRenderer::mesh_groups`] reused. On the mesh-shader path the
    /// prepass runs the same amplification stage the forward pass does, and that
    /// stage counts every surviving cluster into the buffer bound at binding 14 —
    /// so sharing the camera's would make
    /// [`CullStats::clusters`](crate::cull_stats::CullStats::clusters) report
    /// every cluster of the frame twice, which is a plausible number and a wrong
    /// one.
    ///
    /// **Nothing reads what this counts and nothing clears it.** It is a sink: a
    /// wrapping `u32` whose value is never looked at, which is the honest price
    /// of a prepass that shares a pipeline with the pass it precedes.
    prepass_groups: Vec<BindGroupHandle>,
    /// `[frame]`: the sink [`ForwardRenderer::prepass_groups`] counts into.
    ///
    /// Held so the prepass can declare it and the graph can barrier it. A ring
    /// rather than one buffer for every other per-frame resource's reason: the
    /// previous frame's submission may still be writing last frame's.
    prepass_stats: Vec<BufferHandle>,

    /// The ground grid's pipeline and uniform ring — see [`crate::grid`].
    ///
    /// [`None`] until a caller first switches it on with
    /// [`set_ground_grid`](ForwardRenderer::set_ground_grid), which is what
    /// keeps a sample that never asks for a grid from building a pipeline and a
    /// ring of blocks for a pass it does not record. Once built it is kept —
    /// switching the grid off is [`ForwardRenderer::ground_grid_on`] and not a
    /// release, because releasing a uniform ring the frames in flight may still
    /// be reading is exactly what the ring exists to prevent.
    ground_grid: Option<GroundGrid>,
    /// Whether [`add_passes`](ForwardRenderer::add_passes) records the ground
    /// grid's pass. **`false` by default**: the grid is a tool's chrome, and
    /// every sample and every golden image predates it.
    ground_grid_on: bool,

    /// The debug draw layer's immediate-mode buffer and its pass — see
    /// [`crate::debug_draw`].
    ///
    /// Always present and empty by default, because appending to it is what a
    /// system does and the buffer is a `Vec`; the pipeline and the rings inside
    /// it are what stay unbuilt until a frame has both a segment and
    /// [`crate::debug_draw::r_debug_draw`] switched on.
    debug_draw: DebugDraw,

    /// This frame's camera view-projection, as
    /// [`begin_frame`](ForwardRenderer::begin_frame) computed it.
    ///
    /// Kept because the ground grid's pass needs it and `add_passes` has no
    /// camera: recomputing it there would be a second `aspect` to get wrong, and
    /// a grid drawn through a camera the frame is not drawn with lands on the
    /// wrong pixels while still looking like a grid.
    camera_view_proj: Mat4,

    /// The view-projection the **previous** frame was drawn with, or [`None`]
    /// before there was one.
    ///
    /// The camera-side twin of
    /// [`GpuInstance::previous_transform`](crcbl_shaders::mesh::GpuInstance::previous_transform):
    /// the pool says where each object was and this says where the viewer was,
    /// and `mesh.slang`'s `motion_vector` is what turns the pair into a screen
    /// offset. It reaches the shader as
    /// [`FrameUniforms::previous_view_proj`](crcbl_shaders::mesh::FrameUniforms::previous_view_proj).
    ///
    /// **Advanced once per frame, in
    /// [`begin_frame_body`](ForwardRenderer::begin_frame_body)** — which runs
    /// exactly once per [`InstancePool::rotate`], so the camera's history and
    /// the instances' settle on the same boundary. Advancing it anywhere a
    /// frame can reach twice would report a camera that moved half as far as it
    /// did, and a still scene would never come back to rest.
    ///
    /// [`None`] means the first frame, which is drawn with its own matrix in
    /// both slots: a camera that has not moved yet has not moved, and an
    /// identity or a zero here would put every pixel of the first frame in
    /// motion.
    previous_view_projection: Option<Mat4>,

    /// `docs/plan/18-render-features.md`'s occlusion pair — see [`crate::ssao`].
    ssao: Ssao,
    /// `docs/plan/45-shadows.md`'s contact-shadow march — see
    /// [`crate::contact_shadows`].
    contact_shadows: ContactShadows,
    /// `docs/plan/18-render-features.md`'s depth pyramid, which the reflection
    /// march climbs — see [`crate::hiz`].
    hiz: Hiz,
    /// `docs/plan/18-render-features.md`'s reflection march — see
    /// [`crate::ssr`].
    ssr: Ssr,
    /// `docs/plan/51-volumetrics.md`'s froxel volume and its composite — see
    /// [`crate::volumetric`].
    volumetric: Volumetric,
    /// `docs/plan/43-render-standards.md` §6's auto-exposure — see
    /// [`crate::exposure`]. Named for what it owns rather than for the value:
    /// [`exposure`](Self::exposure) is the number a caller set.
    auto_exposure: Exposure,
    /// `docs/plan/18-render-features.md`'s bloom chain — see [`crate::bloom`].
    bloom: Bloom,
    /// `docs/plan/49-antialiasing.md`'s cheap antialiasing tier — see
    /// [`crate::fxaa`].
    fxaa: Fxaa,
    /// `docs/plan/49-antialiasing.md`'s higher antialiasing tier — see
    /// [`crate::smaa`]. It takes the resolve slot from
    /// [`fxaa`](Self::fxaa) on the frames [`RenderEffects::SMAA`] is set for,
    /// and neither is built per frame: both exist, and at most one records.
    smaa: Smaa,
    /// [`crate::upscale`], and it draws nothing at a
    /// [`render_scale`](Self::render_scale) of `1.0`.
    upscale: Upscale,
    /// `docs/plan/43-render-standards.md` §8's background pass — see
    /// [`crate::sky_pass`]. It draws on no frame whose sky is [`Sky::NONE`],
    /// which is every frame until a caller calls [`set_sky`](Self::set_sky).
    sky_pass: SkyPass,
    /// The shadow atlas viewer — see [`crate::atlas_view`]. It draws on no frame
    /// but the one [`debug_view`](Self::debug_view) resolved to
    /// [`DebugView::ShadowAtlas`], which is none until a caller calls
    /// [`set_atlas_view`](Self::set_atlas_view).
    atlas_viewer: AtlasView,
    /// How large the internal render target is as a fraction of the extent a
    /// caller hands `begin_frame` — see [`set_render_scale`](Self::set_render_scale).
    render_scale: f32,

    /// The three layers a caller supplies — see [`crate::effects`].
    effect_request: EffectRequest,
    /// The fourth layer: what this device can draw, which clamps last and
    /// absolutely.
    ///
    /// **Every effect, and that is a fact about the effects rather than an
    /// unfinished clamp.** Topic 18 says so of the occlusion pair in as many
    /// words — "there is no device fact to gate on … inventing a capability that
    /// is really a performance opinion is what topic 39 exists to prevent" — and
    /// the reflection pair's module says it of itself. The shadow atlas is a
    /// `D32Float` image and a depth-only pass, which is not something a device
    /// that got this far can be missing either: one too small for the atlas
    /// fails to *build* this renderer rather than degrading past it.
    ///
    /// So the clamp is real and its rule set is empty. The first rule arrives
    /// with the ray-traced variants of these same three effects, which
    /// [`LightingPath`](crcbl_hal::LightingPath) selects and which nothing
    /// builds yet.
    device_effects: RenderEffects,
    /// What the frame [`begin_frame`](ForwardRenderer::begin_frame) opened
    /// draws, resolved once there.
    ///
    /// **Frozen for the frame on purpose.** `begin_frame` skips a shadow cull's
    /// parameter write when shadows are off and
    /// [`add_passes`](ForwardRenderer::add_passes) skips its dispatch; a request
    /// changed between the two would dispatch a cull whose counters nothing
    /// zeroed, which is a plausible frame drawn off last frame's numbers.
    frame_effects: RenderEffects,
    /// How many blur passes the frame [`begin_frame`](ForwardRenderer::begin_frame)
    /// opened runs over the raw occlusion channel — [`crate::ssao::blur_passes`],
    /// read once there.
    ///
    /// **Frozen for the frame on [`ForwardRenderer::frame_effects`]'s terms.**
    /// [`add_passes`](ForwardRenderer::add_passes) both counts the frame's
    /// full-screen triangles and records its passes, and a console command
    /// landing between the two reads would leave `counters` describing a frame
    /// that was never submitted.
    frame_ssao_blurs: u32,
    /// Full-screen passes the last [`add_passes`](ForwardRenderer::add_passes)
    /// recorded, which is what [`counters`](ForwardRenderer::counters) reports
    /// as submitted beside the pool's instances.
    ///
    /// Stored rather than re-derived from [`ForwardRenderer::frame_effects`]:
    /// `counters` describes the frame that *was* recorded, and a request set
    /// after it would otherwise change the count of a frame already submitted.
    recorded_fullscreen: u64,

    /// Whether the last [`add_passes`](ForwardRenderer::add_passes) recorded the
    /// debug draw layer's pass, as the one direct draw it costs.
    ///
    /// Separate from [`ForwardRenderer::recorded_fullscreen`] because it is not
    /// a full-screen triangle: it is one line-list draw of every segment this
    /// frame appended. Both are direct draws of one instance, which is what
    /// [`ForwardRenderer::direct_draws`] adds up.
    recorded_debug_draw: u64,
    /// How many tiles of the shadow atlas the last
    /// [`add_passes`](ForwardRenderer::add_passes) reset with a draw rather
    /// than with the attachment's own clear.
    ///
    /// Zero on every frame that clears the whole attachment, which is every
    /// frame until the cadence is switched on — see
    /// [`MeshModules::depth_clear_pipeline`]. One over-sized triangle of one
    /// instance each, so it joins the full-screen draws rather than the
    /// indirect ones.
    recorded_tile_clears: u64,
}

/// What a partly-built [`ForwardRenderer`] has to give back.
///
/// `new` creates a dozen objects with `?` between them, and the seam's
/// `destroy_*` is explicit — so a failure half way through used to leak
/// everything created before it. The recorder's leak assertions cover the happy
/// path; this covers the other one.
#[derive(Debug, Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<GraphicsPipelineHandle>,
    samplers: Vec<SamplerHandle>,
    /// The shadow atlas and its placeholder, which are created here rather than
    /// uploaded — so unlike [`Rollback::textures`] they are a plain handle each,
    /// with their views beside them.
    images: Vec<ImageHandle>,
    image_views: Vec<ImageViewHandle>,
    /// Uploaded images, which own a view each and so cannot be rolled back as
    /// one list of handles.
    textures: Vec<UploadedTexture>,
    /// The geometry pool, which owns two buffers, a semaphore and anything
    /// still staged — so it cannot be rolled back as a list of handles.
    pool: Option<MeshPool>,
    /// The instance pool, which owns one buffer per frame in flight.
    instances: Option<InstancePool>,
    /// The material table, which owns one buffer.
    materials: Option<MaterialTable>,
    /// The irradiance probe table, which owns one buffer.
    probes: Option<ProbeTable>,
    /// `docs/plan/50-irradiance-probes.md`'s updater, which owns one pipeline,
    /// one layout and a ring of blocks — and only exists for a scene whose
    /// volume asked to be updated.
    probe_gather: Option<ProbeGather>,
    /// The cull and draw-argument passes, which own two pipelines and a ring of
    /// buffers each — and which clean themselves up on their own failure path,
    /// so this only carries one that was built.
    draws: Option<DrawGen>,
    /// One more of the same per shadow-atlas tile — a `Vec` rather than an
    /// `Option`, because a failure part way through the tiles has to release the
    /// ones already built.
    shadow_draws: Vec<DrawGen>,
    /// The cluster buffers, which own three buffers and are built on the
    /// mesh-shader path alone.
    clusters: Option<ClusterPool>,
    /// Topic 18's light list and froxel grid, which own a pipeline and a ring of
    /// three buffers — and which clean themselves up on their own failure path,
    /// so this only carries one that was built.
    lights: Option<LightGrid>,
    /// `docs/plan/18-render-features.md`'s occlusion pair, which owns two
    /// pipelines, two layouts and a ring of blocks.
    ssao: Option<Ssao>,
    /// `docs/plan/45-shadows.md`'s contact-shadow march, which owns one
    /// pipeline, one layout and a ring of blocks.
    contact_shadows: Option<ContactShadows>,
    /// `docs/plan/18-render-features.md`'s depth pyramid, which owns one
    /// pipeline, one layout and a ring of blocks per level.
    hiz: Option<Hiz>,
    /// `docs/plan/18-render-features.md`'s reflection march, which owns one
    /// pipeline, one layout and a ring of blocks.
    ssr: Option<Ssr>,
    /// `docs/plan/51-volumetrics.md`'s froxel volume, which owns three
    /// pipelines, two layouts and two rings of buffers.
    volumetric: Option<Volumetric>,
    /// `docs/plan/43-render-standards.md` §6's auto-exposure, which owns three
    /// pipelines, one layout and three rings of buffers.
    exposure: Option<Exposure>,
    /// `docs/plan/18-render-features.md`'s bloom chain, which owns three
    /// pipelines, two layouts, a sampler and a ring of blocks.
    bloom: Option<Bloom>,
    /// `docs/plan/49-antialiasing.md`'s cheap antialiasing tier, which owns one
    /// pipeline, one layout, a sampler and a ring of blocks.
    fxaa: Option<Fxaa>,
    /// `docs/plan/49-antialiasing.md`'s higher antialiasing tier, which owns
    /// three pipelines, three layouts, two samplers, two uploaded tables and a
    /// ring of blocks.
    smaa: Option<Smaa>,
    upscale: Option<Upscale>,
    /// The background pass, which owns one pipeline, one layout and a ring of
    /// blocks and groups.
    sky_pass: Option<SkyPass>,
    /// The shadow atlas viewer, which owns one pipeline, one layout and a ring
    /// of blocks and groups.
    atlas_viewer: Option<AtlasView>,
}

impl Rollback {
    /// Releases everything, in the same dependency order as
    /// [`ForwardRenderer::destroy`].
    fn run(self, device: &dyn Device) {
        for texture in self.textures {
            texture.destroy(device);
        }
        for handle in self.samplers {
            device.destroy_sampler(handle);
        }
        for handle in self.image_views {
            device.destroy_image_view(handle);
        }
        for handle in self.images {
            device.destroy_image(handle);
        }
        for handle in self.pipelines {
            device.destroy_graphics_pipeline(handle);
        }
        for handle in self.pipeline_layouts {
            device.destroy_pipeline_layout(handle);
        }
        for handle in self.bind_groups {
            device.destroy_bind_group(handle);
        }
        for handle in self.bind_group_layouts {
            device.destroy_bind_group_layout(handle);
        }
        for handle in self.buffers {
            device.destroy_buffer(handle);
        }
        if let Some(clusters) = self.clusters {
            clusters.destroy(device);
        }
        for draws in self.shadow_draws {
            draws.destroy(device);
        }
        if let Some(lights) = self.lights {
            lights.destroy(device);
        }
        if let Some(atlas_viewer) = self.atlas_viewer {
            atlas_viewer.destroy(device);
        }
        if let Some(sky_pass) = self.sky_pass {
            sky_pass.destroy(device);
        }
        if let Some(upscale) = self.upscale {
            upscale.destroy(device);
        }
        if let Some(smaa) = self.smaa {
            smaa.destroy(device);
        }
        if let Some(fxaa) = self.fxaa {
            fxaa.destroy(device);
        }
        if let Some(bloom) = self.bloom {
            bloom.destroy(device);
        }
        if let Some(exposure) = self.exposure {
            exposure.destroy(device);
        }
        if let Some(volumetric) = self.volumetric {
            volumetric.destroy(device);
        }
        if let Some(ssr) = self.ssr {
            ssr.destroy(device);
        }
        if let Some(hiz) = self.hiz {
            hiz.destroy(device);
        }
        if let Some(contact_shadows) = self.contact_shadows {
            contact_shadows.destroy(device);
        }
        if let Some(ssao) = self.ssao {
            ssao.destroy(device);
        }
        if let Some(draws) = self.draws {
            draws.destroy(device);
        }
        // Before the table, because its bind groups name that table's rows.
        if let Some(gather) = self.probe_gather {
            gather.destroy(device);
        }
        if let Some(table) = self.probes {
            table.destroy(device);
        }
        if let Some(table) = self.materials {
            table.destroy(device);
        }
        if let Some(pool) = self.instances {
            pool.destroy(device);
        }
        if let Some(pool) = self.pool {
            pool.destroy(device);
        }
    }
}

/// What every bind group of the mesh layout names identically.
///
/// Split from [`MeshGroup`] rather than folded into it because the split *is*
/// the claim: a resource here cannot differ between the colour pass's group and
/// a cascade's, and one there must be looked at.
struct SharedBindings<'a> {
    vertices: BufferHandle,
    draw_constants: BufferHandle,
    mesh_table: BufferHandle,
    materials: BufferHandle,
    page: ImageViewHandle,
    /// Binding [`NORMAL_PAGE_BINDING`], §2's normal page — shared for the
    /// base-colour page's reason exactly, and read through the same sampler
    /// beside it.
    normal_page: ImageViewHandle,
    page_sampler: SamplerHandle,
    /// `Some` on [`GeometryPath::MeshShader`] and on no other path, which is
    /// what decides whether bindings 9 to 12 and 17 exist at all.
    clusters: Option<&'a ClusterPool>,
    shadow_sampler: SamplerHandle,
    /// Bindings [`LIGHT_LIST_BINDING`] and [`LIGHT_GRID_BINDING`], this frame's
    /// slots of the light ring.
    ///
    /// Shared rather than per-group, unlike the instance array beside them, and
    /// that is the claim: **a cascade shades nothing**, so it reads the same
    /// list and the same grid the colour pass does — the depth-only pipeline
    /// simply never looks. Two buffers per pass would be two clustering
    /// dispatches for one camera.
    lights: BufferHandle,
    light_grid: BufferHandle,
    /// Binding [`PROBE_TABLE_BINDING`], **this frame's slot** of the irradiance
    /// grid's ring.
    ///
    /// Shared across the *groups of one frame* rather than across frames, which
    /// is the light list's arrangement exactly: a cascade shades nothing, so it
    /// reads the same rows the colour pass does and the depth-only pipeline
    /// simply never looks. What it is not is shared across the ring — see
    /// [`crate::probe`] for why the table is per-frame at all.
    probes: BufferHandle,
    /// Binding [`LEVEL_GROUP_TABLE_BINDING`], `DrawGen`'s packed table buffer.
    ///
    /// Shared rather than per-group for the probe grid's reason exactly: the
    /// regions are written when a mesh becomes resident and never per frame, so
    /// a cascade and the colour pass read the same words. What varies between
    /// them is the *budget* the mesh stage projects against, and that rides in
    /// each pass's own frame block.
    tables: BufferHandle,
    /// Binding [`SPECULAR_DFG_BINDING`], the split-sum `DFG` pair — see
    /// [`ForwardRenderer::specular_dfg`].
    ///
    /// Shared rather than per-group for the probe grid's reason and more
    /// strongly: it is cooked into the binary, uploaded once at build and
    /// identical for every view this engine will ever draw. A cascade names it
    /// and never looks.
    specular_dfg: ImageViewHandle,
    /// Binding [`LTC_TABLE_BINDING`], the area lights' fit — see
    /// [`ForwardRenderer::ltc_table`]. Shared for the table above it's reason.
    ltc_table: ImageViewHandle,
}

/// The half of a mesh-layout bind group that differs between the colour pass and
/// each shadow cascade.
///
/// One description for both, so the two cannot drift: a cascade's group is the
/// colour pass's with six resources swapped, and swapping the wrong five of them
/// is a shadow map rendered from the camera — which looks like a shadow map.
struct MeshGroup {
    /// Binding 0. The colour pass's frame block, or a cascade's copy whose
    /// `view_proj` is that cascade's matrix.
    uniforms: BufferHandle,
    /// Binding 2. This frame's slot of the instance ring.
    instances: BufferHandle,
    /// Binding 5. Whose survivors this group draws.
    runs: BufferHandle,
    /// Binding 12, on the mesh path: the same survivors' indirect arguments,
    /// read as data.
    args: BufferHandle,
    /// Binding 13, where there is an amplification stage: the frustum it culls
    /// clusters against.
    cull_params: BufferHandle,
    /// Binding 14, likewise: what it counts survivors into.
    cull_stats: BufferHandle,
    /// Binding 18, likewise: the cut the descent chose, one word per resident
    /// cluster. `None` exactly where there is no amplification stage, which is
    /// where the layout has no binding 18 either.
    ///
    /// **A buffer per pass**, not one shared: the colour pass is recorded last,
    /// so a cascade writing the camera's would leave nothing of its own to read.
    /// See [`ForwardRenderer::shadow_selection`].
    cluster_selection: Option<BufferHandle>,
    /// Binding 19, likewise: `docs/plan/25-lod.md`'s hysteresis state, which the
    /// draw-argument pass wrote this frame and this one only reads.
    ///
    /// **A buffer per pass** as well, because the colour pass and a cascade
    /// judge the same groups under different budgets — see
    /// [`TileBuffers::group_state`].
    group_state: Option<BufferHandle>,
    /// Binding [`SHADOW_ATLAS_BINDING`]. The atlas for the pass that reads it,
    /// and the placeholder for the pass that writes it — see
    /// [`ForwardRenderer::shadow_placeholder`], which is where that is argued.
    shadow_map: ImageViewHandle,
    /// Binding [`AMBIENT_OCCLUSION_BINDING`]. The blurred occlusion channel for
    /// a forward pass that computed one, and the white placeholder everywhere
    /// else — see [`ForwardRenderer::ambient_occlusion_placeholder`].
    ///
    /// **Every group built at `build` names the placeholder**, including the
    /// camera's: the occlusion image may be a graph transient whose view exists
    /// only at execute time, so the camera's group is rebuilt against whatever
    /// the frame bound inside the forward pass and cached — the shape
    /// [`ForwardRenderer::tonemap_groups`] already has.
    ambient_occlusion: ImageViewHandle,
    /// Binding [`CONTACT_SHADOW_BINDING`]. The contact-shadow channel for a
    /// forward pass that marched one, and the white placeholder everywhere else
    /// — see [`ForwardRenderer::contact_shadow_placeholder`].
    ///
    /// [`MeshGroup::ambient_occlusion`]'s terms exactly, including that every
    /// group built at `build` names the placeholder.
    contact_shadow: ImageViewHandle,
    /// Binding [`PROBE_VISIBILITY_BINDING`]. The captured per-probe visibility
    /// maps for the camera's pass, and the one-texel placeholder everywhere
    /// else — see [`ForwardRenderer::probe_visibility_placeholder`].
    ///
    /// [`MeshGroup::ambient_occlusion`]'s terms, with one difference: the image
    /// is not a graph transient but an ordinary uploaded texture, so what makes
    /// this a per-frame swap is the console switch and whether a capture has
    /// happened yet — neither of which is known at `build`.
    probe_visibility: ImageViewHandle,
}

impl MeshGroup {
    fn entries(&self, shared: &SharedBindings<'_>) -> Vec<BindGroupEntry> {
        let mut entries = vec![
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(self.uniforms),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::whole_buffer(shared.vertices),
            },
            BindGroupEntry {
                binding: 2,
                array_index: 0,
                // **This frame's slot of the instance ring, not a shared
                // buffer.** Binding one buffer here for every group would undo
                // the ring and reintroduce the cross-submission
                // read-after-write hazard it exists to prevent.
                resource: BindingResource::whole_buffer(self.instances),
            },
            BindGroupEntry {
                binding: 3,
                array_index: 0,
                // **One block, not the whole buffer.** The binding is dynamic,
                // so the bind's offset is added on top of this one and both
                // Vulkan and WebGPU require `offset + dynamic + size` to stay
                // inside the buffer — bound whole, the very first non-zero
                // dynamic offset would be out of range.
                resource: BindingResource::Buffer {
                    buffer: shared.draw_constants,
                    offset: 0,
                    size: DRAW_CONSTANTS_BLOCK,
                },
            },
            BindGroupEntry {
                binding: 4,
                array_index: 0,
                // The same table in every group, unlike the instance array
                // above it: the pool writes an entry when a mesh is uploaded or
                // freed, neither of which happens between frames here.
                resource: BindingResource::whole_buffer(shared.mesh_table),
            },
            BindGroupEntry {
                binding: 5,
                array_index: 0,
                // Whose cull this group draws from. This is the binding a
                // cascade's group differs in most consequentially: it is what
                // makes the shadow pass draw what the *light* can see rather
                // than what the camera can.
                resource: BindingResource::whole_buffer(self.runs),
            },
            BindGroupEntry {
                binding: 6,
                array_index: 0,
                resource: BindingResource::whole_buffer(shared.materials),
            },
            BindGroupEntry {
                binding: 7,
                array_index: 0,
                // **One entry, `array_index: 0`**, because the page is one
                // image and the layer is chosen in the shader. A bindless array
                // would be one entry per texture at ascending array indices,
                // which is the write path `BindGroupEntry`'s own docs describe
                // and the one this pass does not take.
                resource: BindingResource::ImageView(shared.page),
            },
            BindGroupEntry {
                binding: PAGE_SAMPLER_BINDING,
                array_index: 0,
                resource: BindingResource::Sampler(shared.page_sampler),
            },
        ];
        if let Some(clusters) = shared.clusters {
            entries.extend([
                BindGroupEntry {
                    binding: 9,
                    array_index: 0,
                    // The same three buffers in every group, on the mesh table's
                    // terms: clusters are written when the pool is built and
                    // never again.
                    resource: BindingResource::whole_buffer(clusters.clusters()),
                },
                BindGroupEntry {
                    binding: 10,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(clusters.vertices()),
                },
                BindGroupEntry {
                    binding: 11,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(clusters.corners()),
                },
                BindGroupEntry {
                    binding: 12,
                    array_index: 0,
                    // **Arguments read as data.** The mesh path records no
                    // indirect draw at all; what it wants out of this buffer is
                    // the one word only the GPU knows — how many instances
                    // survived into each bucket.
                    resource: BindingResource::whole_buffer(self.args),
                },
            ]);
        }
        // **Keyed on the cluster pool, not on the amplification stage**, for
        // the reason binding 17 and the layout give: `mesh_cluster.slang`
        // declares 13 and 14 on every path, and Slang's Metal target numbers
        // every buffer in that module's declaration order — so a group that
        // omitted them would put 17 two buffer slots below where
        // `msl/mesh_cluster.metal` reads it. On a device with the mesh stage
        // and no task stage `meshMain` never dereferences either, and takes
        // both as arguments regardless.
        if shared.clusters.is_some() {
            entries.extend([
                BindGroupEntry {
                    binding: 13,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(self.cull_params),
                },
                BindGroupEntry {
                    binding: 14,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(self.cull_stats),
                },
            ]);
        }
        entries.push(BindGroupEntry {
            binding: SHADOW_ATLAS_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(self.shadow_map),
        });
        entries.push(BindGroupEntry {
            binding: SHADOW_SAMPLER_BINDING,
            array_index: 0,
            resource: BindingResource::Sampler(shared.shadow_sampler),
        });
        // **After the shadow pair, because 15 and 16 are taken.** Those two are
        // `mesh.slang`'s, declared by the fragment stage of this very pipeline,
        // so `mesh_cluster.slang`'s own additions start at 17 — and the list
        // stays ascending, which is the order `crcbl-mtl` counts a Metal
        // argument table in.
        if let Some(clusters) = shared.clusters {
            entries.push(BindGroupEntry {
                binding: 17,
                array_index: 0,
                // Bound with the geometry rather than with the cull, because
                // `vertex_base` *is* geometry: it is which level of a DAG a
                // cluster's vertices live in, and both mesh entry points resolve
                // a vertex through it.
                resource: BindingResource::whole_buffer(clusters.selection()),
            });
        }
        if let Some(selection) = self.cluster_selection {
            entries.push(BindGroupEntry {
                binding: 18,
                array_index: 0,
                resource: BindingResource::whole_buffer(selection),
            });
        }
        if let Some(state) = self.group_state {
            entries.push(BindGroupEntry {
                binding: 19,
                array_index: 0,
                resource: BindingResource::whole_buffer(state),
            });
        }
        // Topic 18's pair, on every path and in every group — see the layout,
        // which has no configuration that omits them either.
        entries.push(BindGroupEntry {
            binding: LIGHT_LIST_BINDING,
            array_index: 0,
            resource: BindingResource::whole_buffer(shared.lights),
        });
        entries.push(BindGroupEntry {
            binding: LIGHT_GRID_BINDING,
            array_index: 0,
            resource: BindingResource::whole_buffer(shared.light_grid),
        });
        // Last, on [`AMBIENT_OCCLUSION_BINDING`]'s terms: the list has to ascend
        // or `crcbl-mtl`'s argument table and Slang's stop agreeing.
        entries.push(BindGroupEntry {
            binding: AMBIENT_OCCLUSION_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(self.ambient_occlusion),
        });
        // And the irradiance grid past it — see [`PROBE_TABLE_BINDING`].
        entries.push(BindGroupEntry {
            binding: PROBE_TABLE_BINDING,
            array_index: 0,
            resource: BindingResource::whole_buffer(shared.probes),
        });
        // The group records at the top of the list, on the mesh path alone,
        // where the layout has the row — see [`LEVEL_GROUP_TABLE_BINDING`].
        // Keyed on the cluster pool for binding 17's reason: it is what the
        // layout keys the same rows on.
        if shared.clusters.is_some() {
            entries.push(BindGroupEntry {
                binding: LEVEL_GROUP_TABLE_BINDING,
                array_index: 0,
                resource: BindingResource::whole_buffer(shared.tables),
            });
        }
        // The `DFG` table above all of them, on the same ascending terms — see
        // [`SPECULAR_ALBEDO_BINDING`]. Unconditional, because unlike the row
        // below it this one is read by the fragment stage of every path.
        entries.push(BindGroupEntry {
            binding: SPECULAR_DFG_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(shared.specular_dfg),
        });
        // And §2's normal page above that, on the same ascending terms — see
        // [`NORMAL_PAGE_BINDING`]. Unconditional and `array_index: 0` for
        // binding 7's reason: one image, and the layer is chosen in the shader.
        entries.push(BindGroupEntry {
            binding: NORMAL_PAGE_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(shared.normal_page),
        });
        // And the area lights' table above that, on the same ascending terms —
        // see [`LTC_TABLE_BINDING`].
        entries.push(BindGroupEntry {
            binding: LTC_TABLE_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(shared.ltc_table),
        });
        // And the contact-shadow channel above that, last of the set and on the
        // same ascending terms — see [`CONTACT_SHADOW_BINDING`].
        entries.push(BindGroupEntry {
            binding: CONTACT_SHADOW_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(self.contact_shadow),
        });
        // And the probe visibility maps above that, last of the set and on the
        // same ascending terms — see [`PROBE_VISIBILITY_BINDING`].
        entries.push(BindGroupEntry {
            binding: PROBE_VISIBILITY_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(self.probe_visibility),
        });
        entries
    }
}

/// The two `ArrayPages` images every group of the mesh layout names, as the
/// graph knows them.
///
/// One value rather than two arguments because the two travel together
/// everywhere: every pass that declares one declares the other, and a pass
/// handed them the wrong way round would order a caller's copy into the wrong
/// image — which is a barrier bug, and therefore one that reproduces on one
/// backend and not the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MaterialPages {
    /// §3.2's base-colour page — [`ForwardRenderer::base_color_page_import`].
    base_color: ImageId,
    /// §2's normal page — [`ForwardRenderer::normal_page_import`].
    normal: ImageId,
}

/// Everything [`ForwardRenderer::add_shadow_pass`] declares and does not write,
/// as the graph knows it.
///
/// One value rather than an argument apiece for [`MaterialPages`]' reason, and
/// the list grew past what an argument list should be the moment the probe rows
/// joined it: each of these is in every cascade's bind group and read by no
/// cascade, so what the pass does with them is one thing — declare them, so the
/// graph can order whoever writes them against these draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShadowReads {
    /// The occlusion placeholder every cascade's group stands the real channel
    /// in for — [`ForwardRenderer::ambient_occlusion_placeholder`].
    occlusion_placeholder: ImageId,
    /// §3.2's and §2's pages, which the cascades name and never sample.
    pages: MaterialPages,
    /// The skinned vertex region, where a frame has one — the cascades pull the
    /// same geometry the colour pass does.
    skinned: Option<BufferId>,
    /// This frame's slot of the probe ring — see [`crate::probe`].
    probes: BufferId,
}

/// What the mesh pass's bind-group layout is called.
///
/// Named rather than written twice because a test asserts on it: the layout's
/// *binding numbers* are what `crcbl-mtl` turns into Metal argument-table
/// indices by counting, so `the_mesh_layout_declares_the_same_bindings_with_and_without_a_task_stage`
/// has to find this one layout among the recorder's.
const MESH_LAYOUT_LABEL: &str = "mesh frame";

/// Everything a geometry pass needs to record one indirect call per bucket.
///
/// **One description for the three passes that record them**: the depth prepass,
/// the forward pass and each shadow view. They differ in the pipeline and in
/// which bind group and which cull's arguments they draw from, and in nothing
/// else — so the emit tail, the index-buffer bind and the per-bucket loop are one
/// piece of code rather than three that agree today. The shadow pass gained a
/// viewport per tile around this; the prepass gained nothing at all, which is the
/// point of it being the colour pass's twin.
#[derive(Clone)]
struct BucketDraws {
    pipeline: GraphicsPipelineHandle,
    layout: PipelineLayoutHandle,
    /// The whole index pool, bound at offset zero — see [`BucketDraws::record`].
    indices: BufferHandle,
    emit: EmitTail,
    /// Per bucket: the dynamic offset of its constant block, and the offsets of
    /// its argument structure, its count word and its dispatch extents.
    calls: Vec<(u32, u64, u64, u64)>,
}

impl BucketDraws {
    /// Binds the pipeline and, unless this is a mesh pipeline, the index pool.
    ///
    /// Recorded once per pass; [`BucketDraws::record`] is once per bind group.
    fn open(&self, encoder: &mut dyn crcbl_hal::CommandEncoder) {
        encoder.bind_graphics_pipeline(self.pipeline);
        if !self.emit.is_mesh() {
            // The index pool is bound whole, at offset zero, for every mesh in
            // it: the mesh's place is the draw's first index and its table entry,
            // not a buffer offset. That is what makes one bind enough for the
            // scene P7 puts in here.
            //
            // A mesh pipeline has no index buffer at all — the corner triples
            // come out of the cluster records — so binding one would be a bind no
            // stage could read.
            encoder.bind_index_buffer(self.indices, 0, IndexFormat::Uint32);
        }
    }

    /// Records one call per bucket, drawing `group`'s view of `draws`.
    ///
    /// One call per bucket **always** — the number the CPU records does not depend
    /// on what is in the scene, which is the whole of what topic 03 §3.3 asks for.
    /// An empty bucket's arguments carry an instance count of zero.
    fn record(
        &self,
        encoder: &mut dyn crcbl_hal::CommandEncoder,
        group: BindGroupHandle,
        draws: &GeneratedDraws,
    ) {
        let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u32;
        let mesh_stride = crcbl_shaders::draw_gen::MESH_ARGS_SIZE as u32;
        for (constant_offset, args_offset, count_offset, mesh_args_offset) in &self.calls {
            // The block written at build for this bucket: where its run of
            // surviving instances starts. `SV_InstanceID` walks the run from
            // there, each entry names an instance, the instance names its mesh,
            // and the mesh table says where that mesh's vertices start — none of
            // which the draw call carries. The mesh path's block says the same and
            // three things more; see `meshlet::ClusterDrawConstants`.
            encoder.bind_group(0, group, &[*constant_offset], self.layout);
            match self.emit {
                EmitTail::Mesh => {
                    // One workgroup per (cluster, **surviving** instance), and
                    // neither extent is the CPU's: they are the three words the
                    // draw-argument pass wrote for this bucket.
                    //
                    // That is the whole difference between culling that skips
                    // output and culling that skips work. A dispatch sized here
                    // would have to cover every slot the instance pool ever handed
                    // out — a removed instance leaves a hole and the live ones
                    // above it stay in the array — and launch a workgroup for
                    // each, which then reads the survivor count and returns.
                    //
                    // Recorded unconditionally, unlike a CPU-sized dispatch: an
                    // extent of zero is a legal indirect dispatch of no
                    // workgroups, so an empty scene needs no branch here and the
                    // recorded stream stays the same whatever the scene holds.
                    encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                        // The buffer the per-bucket draw counts are also in —
                        // the extents follow them, which is what
                        // `DrawGen::mesh_args_offset` accounts for. See
                        // [`GeneratedDraws::counts`].
                        args: draws.counts,
                        offset: *mesh_args_offset,
                        draw_count: 1,
                        stride: mesh_stride,
                    });
                }
                EmitTail::Count => {
                    encoder.draw_indexed_indirect_count(&DrawIndirectCount {
                        args: draws.args,
                        args_offset: *args_offset,
                        count_buffer: draws.counts,
                        count_offset: *count_offset,
                        // One argument structure per bucket, so this is the
                        // ceiling rather than a guess: the count in the buffer is
                        // zero or one and the GPU decides which.
                        max_draw_count: 1,
                        stride,
                    });
                }
                EmitTail::PerBatch => {
                    encoder.draw_indexed_indirect(&DrawIndirect {
                        args: draws.args,
                        offset: *args_offset,
                        // Read the bucket's one structure unconditionally — a
                        // device without a GPU-side count cannot ask whether there
                        // is anything in it, and an instance count of zero draws
                        // nothing anyway. That is why the two paths are the same
                        // picture and not an approximation of each other.
                        draw_count: 1,
                        stride,
                    });
                }
            }
        }
    }
}

/// Declares the buffers a geometry pass reads to find its draws.
///
/// The other half of [`BucketDraws`]: what the *graph* has to be told, where that
/// struct is what the encoder is told. Three passes declare exactly this set, and
/// the seam calls the barrier it produces the single most important one in a
/// GPU-driven frame — its absence produces "sometimes nothing draws".
///
/// A caller with a cluster-selection buffer of its own appends it; that is the
/// one declaration that differs between the colour pass, a cascade and the depth
/// prepass.
fn read_draw_sources<'g, 'a>(
    pass: PassBuilder<'g, 'a>,
    draws: &GeneratedDraws,
    emit: EmitTail,
) -> PassBuilder<'g, 'a> {
    // The buffers the draws come out of. Declaring them is what makes the graph
    // transition them out of the compute pass's `ShaderReadWrite`.
    let pass = pass.read_buffer(draws.runs_id);
    if emit.is_mesh() {
        // **The same arguments, read as data rather than executed**, which is
        // what the stages use to bound the run of surviving instances they index.
        // The per-bucket draw *counts* are not read at all, because nothing here
        // is a draw whose count could come from memory.
        //
        // The dispatch *extents* are a real indirect read — one structure per
        // bucket, written by the same pass. They are in a **different buffer**
        // from the arguments above and that separation is load-bearing: a
        // resource is in exactly one state per pass and these two are in
        // different ones. They share a buffer with the counts instead, which
        // this path does not read at all — see [`GeneratedDraws::counts`].
        let pass = pass
            .read_buffer(draws.args_id)
            .use_buffer(draws.counts_id, ResourceState::IndirectArgument);
        // The amplification stage counts its survivors into the culling
        // statistics, which the draw-argument pass read a moment ago — so this
        // is a write-after-read the graph has to order, and declaring it is
        // the whole of how it learns to.
        //
        // `docs/plan/25-lod.md`'s hysteresis state is read here and written by
        // the draw-argument pass a moment ago. Declaring it is what orders the
        // two — and what puts it back into `ShaderReadWrite` at the end of the
        // graph, which is where the next frame's draw-argument pass expects to
        // find it.
        //
        // **Declared on the whole mesh path, not only where there is an
        // amplification stage**, because that is where the bind group now
        // names both — see the layout, which is where the Metal argument-table
        // arithmetic that forced it is argued. A buffer bound writable and
        // never transitioned is a descriptor in the wrong resource state, which
        // is what the graph exists to prevent.
        pass.use_buffer(draws.visible_count_id, ResourceState::ShaderReadWrite)
            .read_buffer(draws.group_state_id)
    } else {
        pass.use_buffer(draws.args_id, ResourceState::IndirectArgument)
            .use_buffer(draws.counts_id, ResourceState::IndirectArgument)
    }
}

/// What one [`MeshDesc`](crate::scene::MeshDesc) became once it was resident.
///
/// One of these per description mesh, in description order — so a mesh's index
/// here is its index there, which is what lets the bucket table, the level
/// tables and the cluster pool all be filled in by walking one list.
struct ResidentMesh {
    /// This mesh's table ids, finest first: one entry for a flat mesh, one per
    /// level for a DAG. Never empty.
    ///
    /// Every level is its own vertex range and so its own table entry. Level 0's
    /// id is the one the *instance* carries and the one the cull pass reads a
    /// bounding box out of; the coarser ones are named by the bucket table on a
    /// path that takes a uniform cut, and by nothing at all on the mesh path,
    /// where a cluster reaches its own level through
    /// [`vertex_bases`](Self::vertex_bases) instead.
    levels: Vec<u32>,
    /// Per level, how far that level's vertices start past level 0's — so a flat
    /// mesh's single entry is zero.
    ///
    /// What [`crcbl_shaders::cluster_select::ClusterSelect::vertex_base`] holds,
    /// and the whole of what makes a DAG drawable from one instance: the mesh
    /// stage adds this to the instance's own `base_vertex`, so a cut spanning
    /// three levels reads three different vertex ranges out of one pool.
    vertex_bases: Vec<u32>,
    /// Level 0's pool handle, for a mesh that can be **skinned** — and [`None`]
    /// for a [`Geometry::Dag`].
    ///
    /// A DAG is refused because its coarser levels are separate vertex runs no
    /// skinning dispatch writes: an instance drawn out of a skinned region
    /// resolves every level through [`vertex_bases`](Self::vertex_bases) added
    /// to the region's base, which for anything but level 0 is memory the
    /// dispatch never touched.
    skinnable: Option<MeshHandle>,
}

impl ResidentMesh {
    /// The table id an instance of this mesh carries: **level 0's**, whatever
    /// the geometry path, because it is the entry `cull.slang` reads a bounding
    /// box out of and the coarser levels approximate the same surface inside the
    /// same box.
    fn id(&self) -> u32 {
        self.levels[0]
    }
}

/// One skinned object the renderer re-points every frame: the instance, and the
/// base vertex it draws out of under each parity.
///
/// The bases are copied out of the [`SkinnedMesh`] rather than borrowed from it,
/// so placing an object does not tie the renderer's lifetime to the caller's
/// reservation — the renderer never needs the region again, only the two
/// numbers.
#[derive(Clone, Copy, Debug)]
struct SkinnedInstance {
    handle: InstanceHandle,
    /// Indexed by parity, in [`crate::skinning::SkinnedRegion::base`]'s
    /// indexing — so the half a frame of parity `p` draws out of is
    /// `bases[p & 1]` and the half the frame before it filled is
    /// `bases[(p ^ 1) & 1]`, which is
    /// [`SkinnedRegion::previous_base`](crate::skinning::SkinnedRegion::previous_base)'s
    /// answer for the same `p`.
    bases: [u32; 2],
    /// Whether a frame has already pointed this entry at a half of the region
    /// it now names.
    ///
    /// **What makes the previous base meaningful.** The half a frame did not
    /// write holds whatever its reservation came with until a dispatch fills
    /// it, so an object's first frame out of a region has no previous
    /// deformation to point at — naming that half would give it a motion vector
    /// against undefined vertices. Such an entry is put at rest instead, which
    /// is the rule [`InstancePool::insert`](crate::instance_pool::InstancePool::insert)
    /// applies to `previous_transform` for the same reason: a spawn did not
    /// travel from anywhere.
    pointed: bool,
}

/// The two base vertices an object drawn out of `mesh` alternates between, in
/// [`SkinnedInstance::bases`]' indexing.
fn skinned_bases(mesh: &SkinnedMesh) -> [u32; 2] {
    [mesh.region().base(0), mesh.region().base(1)]
}

/// One shadow cull's per-frame buffers, read out of its [`DrawGen`] before that
/// generator is handed to the rollback.
struct TileBuffers {
    runs: Vec<BufferHandle>,
    args: Vec<BufferHandle>,
    cull_params: Vec<BufferHandle>,
    cull_stats: Vec<BufferHandle>,
    /// This cull's own hysteresis state, and not the camera's.
    ///
    /// A shadow view selects from the camera's eye at the camera's scale, like
    /// the colour pass, but under budgets [`SHADOW_LOD_BIAS`] times as large — so
    /// it reaches a different answer for the same group, and an answer carried
    /// between frames needs somewhere of its own to be carried. Sharing the
    /// camera's buffer would be two rules writing one history and each undoing
    /// the other's band every frame.
    ///
    /// One per cull rather than one for the shadow pass, even though every one of
    /// them selects identically: each cull is its own [`DrawGen`], and one buffer
    /// between them would be several dispatches writing one element with nothing
    /// ordering them.
    ///
    /// **This is what a cull costs that is not a dispatch.** It is
    /// [`Capacities::instances`](crate::scene::Capacities::instances) × the
    /// resident group count words of device-local
    /// memory, held for the renderer's whole life whether anything is drawn
    /// through it or not, and it is the reason [`shadow::LIGHT_SLOTS`] is a
    /// budget rather than "however many lights the scene has" — and the reason a
    /// point light's six faces share one of these rather than owning six.
    group_state: BufferHandle,
}

impl ForwardRenderer {
    /// Builds both pipelines and makes [`scene::demo`] resident.
    ///
    /// [`ForwardRenderer::with_scene`] with the engine's own description, and
    /// nothing else — so the frame this draws is the one the golden suite has
    /// always compared, and every existing caller is untouched.
    ///
    /// `target_format` must be the format the tonemap pass will render into —
    /// dynamic rendering checks pipeline and attachment formats against each
    /// other at pass-begin time, not at creation.
    ///
    /// # Errors
    ///
    /// [`HalError`], on [`ForwardRenderer::with_scene`]'s terms.
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
    ) -> Result<Self, HalError> {
        Self::with_scene(device, queue, target_format, &scene::demo())
    }

    /// Builds both pipelines and makes `scene` resident.
    ///
    /// Everything in the description is created here and never grows: the
    /// geometry pools, the cluster pool, the material table, the page, the
    /// instance array and the bucket table are all sized and filled once. See
    /// [`crate::scene`] for why that split is where the seam is, and for the
    /// four places a description's *order* decides what the frame looks like.
    ///
    /// `target_format` must be the format the tonemap pass will render into —
    /// dynamic rendering checks pipeline and attachment formats against each
    /// other at pass-begin time, not at creation.
    ///
    /// # No description is too small
    ///
    /// One mesh and one material row is a scene, and so is
    /// [`scene::demo`]'s four and three. Nothing on this type names a
    /// description entry by position any more — the five `set_*` demo wrappers
    /// that did are gone, and [`ForwardRenderer::add_instance`] takes the mesh
    /// and the row a caller wrote its **own** description in.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if the description cannot be made
    /// resident as written: a page layer that is not the extent's worth of RGBA8, a page
    /// whose layer 0 is not opaque white, a material row naming a layer the page
    /// has not got, a DAG carrying a vertex array for anything but each of its
    /// levels, a cluster array with a read outside itself in it (see
    /// [`MeshClusters::check`]), or more vertices, indices, mesh table entries or
    /// material rows than [`SceneDesc::capacities`] reserves — that last naming the pool, the
    /// capacity and what the description needs, because the answer to it is to
    /// raise the number. Every one of those is settled **before the first device
    /// object exists**, so a refusal leaks nothing.
    ///
    /// [`HalError`] otherwise, from any seam call — including the geometry a
    /// pool refuses for itself, such as vertex bytes that are not a whole number
    /// of vertices. A backend that cannot build a pipeline says so here rather
    /// than drawing nothing later, and a failure part-way through releases
    /// everything already created, so a caller that retries or exits leaves
    /// nothing behind.
    pub fn with_scene(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
        scene: &SceneDesc<'_>,
    ) -> Result<Self, HalError> {
        let mut rollback = Rollback::default();
        match Self::build(device, queue, target_format, scene, &mut rollback) {
            Ok(renderer) => Ok(renderer),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    /// Everything `build` needs to be true of a description, checked **before
    /// the first device object exists**.
    ///
    /// That placement is the whole design of this function. `build` hands the
    /// geometry pool and the material table to the rollback and creates a dozen
    /// objects with `?` between them; a check that ran part way down that list
    /// would be a new early exit on the wrong side of a handover, and a rejected
    /// description would leak two device-local buffers. Run from the top of
    /// `build`, a refusal costs nothing to unwind because nothing was made.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] naming the entry that is wrong and what
    /// is wrong with it.
    fn check_scene(scene: &SceneDesc<'_>) -> Result<(), HalError> {
        let refuse = |what: String| Err(HalError::InvalidDescriptor(what));
        // Every layer's length, and **layer 0's whiteness** — the invariant
        // whose failure is a global albedo scale on every untextured surface.
        // Checked by the type that owns the page rather than here, because that
        // is where a page can be built wrong and where the check can be made to
        // fail; see `PageDesc::check`.
        scene.page.check()?;
        // The probe grid's counts against the rows it carries, which is what
        // bounds the shader's fetch. Checked by the type that owns the grid, on
        // `PageDesc::check`'s terms exactly — see `ProbeGrid::check`.
        scene.probes.check()?;
        // A row naming a layer the page does not have is an out-of-range sample,
        // which nothing below the seam reports.
        let layers = scene.page.layers().len();
        let normal_layers = scene.page.normal_layers().len();
        for (row, material) in scene.materials.iter().enumerate() {
            if material.base_color_texture as usize >= layers {
                return refuse(format!(
                    "material row {row} samples page layer {}, and the page has {layers}",
                    material.base_color_texture
                ));
            }
            // The normal page on the same terms, and it is worth its own check
            // rather than sharing the one above: the two pages have separate
            // layer lists, so a row pointed at a base-colour layer number would
            // pass that check and sample past the end of this image.
            if material.normal_texture as usize >= normal_layers {
                return refuse(format!(
                    "material row {row} samples normal page layer {}, and the normal page \
                     has {normal_layers}",
                    material.normal_texture
                ));
            }
        }

        for (index, desc) in scene.meshes.iter().enumerate() {
            let Geometry::Dag { levels, dag, .. } = &desc.geometry else {
                continue;
            };
            // Zipped together at upload and again at
            // `ClusterDag::selection_records`, so a short list would silently
            // leave a level's clusters reading the level below's vertices.
            if levels.len() != dag.levels.len() {
                return refuse(format!(
                    "mesh {index} ({}) carries {} vertex array(s) for a DAG of {} level(s)",
                    desc.label,
                    levels.len(),
                    dag.levels.len()
                ));
            }
            for (depth, (bytes, level)) in levels.iter().zip(&dag.levels).enumerate() {
                let want = level.positions.len() * mesh::VERTEX_STRIDE;
                if bytes.len() != want {
                    return refuse(format!(
                        "mesh {index} ({}) level {depth} carries {} vertex byte(s) for {} \
                         position(s), which is {want}",
                        desc.label,
                        bytes.len(),
                        level.positions.len()
                    ));
                }
            }
        }

        // **Every read the mesh stage makes from a cluster array lands inside
        // it.** Checked by the type that owns the arrays, on `PageDesc::check`'s
        // terms exactly — see `MeshClusters::check`, which is where the reason
        // it cannot be checked below the seam is argued. Both variants carry
        // the same type beside a vertex count, so both are checked the same way.
        for (index, desc) in scene.meshes.iter().enumerate() {
            let levels: Vec<(usize, &MeshClusters)> = match &desc.geometry {
                Geometry::Flat {
                    vertices, clusters, ..
                } => vec![(vertices.len() / mesh::VERTEX_STRIDE, clusters)],
                Geometry::Dag { dag, .. } => dag
                    .levels
                    .iter()
                    .map(|level| (level.positions.len(), &level.clusters))
                    .collect(),
            };
            for (depth, (vertices, clusters)) in levels.into_iter().enumerate() {
                if let Err(fault) = clusters.check(vertices) {
                    return refuse(format!(
                        "mesh {index} ({}) level {depth} {fault}",
                        desc.label
                    ));
                }
            }
        }

        // **Capacity, summed over the whole description**, which is the one
        // refusal no pool can make for itself. `MeshPool` and `MaterialTable`
        // do refuse their own overflow, but they refuse it part way through
        // filling something that already exists, and the pair of numbers
        // [`MeshPoolError::PoolExhausted`](crate::mesh_pool::MeshPoolError::PoolExhausted)
        // carries to tell fragmentation from a full pool says nothing *here*:
        // the pool was created empty a few lines below, nothing has been freed
        // in it, so its free list is one block and the largest block and the
        // total are the same number. At build the answer is always "raise this
        // capacity", so it is said by name and before anything is created.
        let mut vertices: u64 = 0;
        let mut indices: u64 = 0;
        for desc in &scene.meshes {
            match &desc.geometry {
                Geometry::Flat {
                    vertices: bytes,
                    indices: list,
                    ..
                } => {
                    // Bytes that are not a whole number of vertices are the
                    // pool's to refuse, and it refuses before it allocates.
                    vertices += (bytes.len() / mesh::VERTEX_STRIDE) as u64;
                    indices += list.len() as u64;
                }
                Geometry::Dag { levels, dag, .. } => {
                    for bytes in levels {
                        vertices += (bytes.len() / mesh::VERTEX_STRIDE) as u64;
                    }
                    for level in &dag.levels {
                        // The call the upload itself makes, so the two cannot
                        // disagree about what a level costs.
                        indices += level.indices().len() as u64;
                    }
                }
            }
        }
        let entries: u64 = scene
            .meshes
            .iter()
            .map(|desc| desc.geometry.levels() as u64)
            .sum();
        for (pool, needed, capacity) in [
            ("vertex pool", vertices, scene.capacities.vertices),
            ("index pool", indices, scene.capacities.indices),
            ("mesh table", entries, scene.capacities.meshes),
            (
                "material table",
                scene.materials.len() as u64,
                scene.capacities.materials,
            ),
            (
                "probe table",
                scene.probes.probes.len() as u64,
                scene.capacities.probes,
            ),
        ] {
            if needed > u64::from(capacity) {
                return refuse(format!(
                    "the {pool} holds {capacity} and the description needs {needed}"
                ));
            }
        }
        Ok(())
    }

    /// The body of [`ForwardRenderer::with_scene`], recording what it has
    /// created into `rollback` as it goes.
    fn build(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
        scene: &SceneDesc<'_>,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        // Before anything exists, so a refused description leaks nothing — see
        // `check_scene`, which is where that placement is argued.
        Self::check_scene(scene)?;

        // Resolved before anything is created, because it decides the *shape*
        // of what is created — the bind group layout, which pipeline kind the
        // pass builds, and what a bucket's constant block says. A device on the
        // mesh path never builds the raster pipeline and a device on either
        // indirect tail never builds the mesh one, which is what makes "the
        // frame came out of the mesh stage" a fact about the object graph
        // rather than a claim about a branch.
        let emit = EmitTail::from_caps(&device.caps());
        // **A second capability, asked separately.** `Features::TASK_SHADER` is
        // not implied by `MESH_SHADER`, so §3.5's per-cluster cull is an
        // amplification stage this renderer builds where the device has one and
        // leaves out where it does not — and a device without it keeps drawing
        // through `mesh_cluster.slang`'s un-amplified `meshMain`, which culls
        // nothing and emits every cluster. Same picture, more work.
        let culls_clusters = emit.is_mesh()
            && device
                .caps()
                .features
                .contains(crcbl_hal::Features::TASK_SHADER);

        let (pool, residents) = Self::build_geometry(device, queue, scene)?;
        // What an `InstanceDesc::mesh` index resolves through: one id per
        // description mesh, in description order, and each of them that mesh's
        // **level 0** whatever the path — because it is the entry the cull pass
        // reads a bounding box out of, and a DAG's coarser levels approximate
        // the same surface inside the same box.
        let mesh_ids: Vec<u32> = residents.iter().map(ResidentMesh::id).collect();
        // The scene's own triangles, kept for the probe-visibility capture and
        // for nothing else — and kept at all only when the description reserves
        // a probe, so a scene without one pays no memory for this rung. Taken
        // here because `scene` is the caller's and is read once.
        let probe_occluders = crate::probe_visibility::Occluders::from_scene(scene, &mesh_ids);
        // The same list one level down: what a caller reserving a skinned region
        // names its mesh with. Read here beside the ids rather than from
        // `residents` later, because the descriptions this came from are the
        // caller's and are read once.
        let skinnable_meshes: Vec<Option<MeshHandle>> =
            residents.iter().map(|mesh| mesh.skinnable).collect();
        // The handles are `Copy`, so they can be read out before the pool
        // becomes the rollback's — which it must be before the first `?` below,
        // or a failed pipeline would leak two device-local buffers. The index
        // pool is not named here at all: it is bound at draw time rather than
        // descriptor-written, so only the vertex pool and the mesh table reach a
        // bind group.
        let vertices = pool.vertex_buffer();
        let mesh_table = pool.table_buffer();
        rollback.pool = Some(pool);

        // §3.2's texture side, before the table that indexes it: a material row
        // is written with a layer number, so the layer has to exist to be named.
        //
        // **`Rgba8UnormSrgb`, and that is the colour-space decision.** A
        // description's texels are sRGB-encoded, which is what glTF defines a
        // base-colour texture to be, so the format is what makes the sampler
        // decode them — and `mesh.slang` then multiplies a linear texel by a
        // linear `base_color` and lights in linear, exactly as it did before
        // there was a texture. Taking the decode off the format would mean
        // multiplying an encoded value by a linear one, which is
        // `crate::sprite_pass`'s "darkens every half-transparent edge" defect
        // in a different place.
        //
        // Layer 0's whiteness and every layer's length were settled by
        // `check_scene` above, before this device object existed.
        //
        // **Every layer goes up with its mip chain**, built here on the host by
        // [`crate::mip`] — `docs/plan/43-render-standards.md`'s filtering rung.
        // The chain is what a trilinear sampler needs to stop shimmering on a
        // minified surface, and it is built at upload rather than by a compute
        // pass because the page's sRGB format is what decodes it and a host
        // chain is the same bytes on every backend.
        let extent = scene.page.extent();
        let chains: Vec<Vec<Vec<u8>>> = scene
            .page
            .layers()
            .iter()
            .map(|texels| crate::mip::chain(texels, extent))
            .collect();
        let page_levels: Vec<Vec<&[u8]>> = scene
            .page
            .layers()
            .iter()
            .zip(&chains)
            .map(|(level0, below)| {
                std::iter::once(level0.as_ref())
                    .chain(below.iter().map(Vec::as_slice))
                    .collect()
            })
            .collect();
        let page_layers: Vec<&[&[u8]]> = page_levels.iter().map(Vec::as_slice).collect();
        let base_color_page = upload_texture_mip_layers(
            device,
            queue,
            "material base colour",
            BASE_COLOR_PAGE_FORMAT,
            extent,
            extent,
            &page_layers,
        )?;
        rollback.textures.push(base_color_page);

        // §2's normal page, on exactly the terms above and with one difference
        // that is the whole of the colour-space decision: it is created
        // `Rgba8Unorm` where the page above is `Rgba8UnormSrgb`, so nothing
        // decodes its texels through a transfer curve. See
        // [`NORMAL_PAGE_FORMAT`], and `docs/plan/44-lighting.md`'s rung 2.
        //
        // **Its mips are a chain of its own**, and not the base-colour page's:
        // `crate::mip::normal_chain` averages the decoded vectors plainly and
        // renormalises, where `chain` decodes an sRGB curve and weights by
        // alpha. A normal map mipped through the colour filter leans further off
        // vertical at every level than the map it was built from, and nothing in
        // a frame reports it. What the renormalise still does *not* recover is
        // the length the average lost, which is `docs/plan/44-lighting.md`'s
        // rung 4 and `docs/backlog.md`'s.
        let normal_chains: Vec<Vec<Vec<u8>>> = scene
            .page
            .normal_layers()
            .iter()
            .map(|texels| crate::mip::normal_chain(texels, extent))
            .collect();
        let normal_levels: Vec<Vec<&[u8]>> = scene
            .page
            .normal_layers()
            .iter()
            .zip(&normal_chains)
            .map(|(level0, below)| {
                std::iter::once(level0.as_ref())
                    .chain(below.iter().map(Vec::as_slice))
                    .collect()
            })
            .collect();
        let normal_page_layers: Vec<&[&[u8]]> = normal_levels.iter().map(Vec::as_slice).collect();
        let normal_page = upload_texture_mip_layers(
            device,
            queue,
            "material normals",
            NORMAL_PAGE_FORMAT,
            extent,
            extent,
            &normal_page_layers,
        )?;
        rollback.textures.push(normal_page);

        // The page's sampler at the device's default — see `create_page_sampler`
        // for what it filters, and `set_anisotropy` for how a caller moves it.
        let anisotropy = Self::anisotropy_for(&device.caps());
        let base_color_sampler = Self::create_page_sampler(device, anisotropy)?;
        rollback.samplers.push(base_color_sampler);

        // The material table, before the instances: an instance is written with
        // the material id it carries, so the row has to exist to be named.
        let (materials, material_ids) = Self::build_materials(device, scene)?;
        let material_buffer = materials.buffer();
        rollback.materials = Some(materials);

        // `docs/plan/18-render-features.md`'s irradiance grid, filled once and
        // never again — see [`crate::probe`], which is where that decision
        // lives. **Created even for a description with no probes**, because
        // every group of this layout has to fill [`PROBE_TABLE_BINDING`] and the
        // honest filler for "no probes were authored" is the zeroed row the
        // shader's clamp lands on.
        let probe_table = ProbeTable::new(
            device,
            queue,
            &ProbeTableDesc {
                label: Some("forward"),
                capacity: scene.capacities.probes,
                frames: FRAMES_IN_FLIGHT,
            },
        )?;
        // Into the rollback before it is filled, so a copy that fails releases
        // the buffers rather than leaking them.
        rollback.probes = Some(probe_table);
        let probe_table = rollback.probes.as_ref().expect("just stored");
        probe_table.fill(device, queue, &scene.probes.probes)?;
        // Read out here, like the instance ring's below and for the same
        // reason: the handles are `Copy`, and the loop that builds the bind
        // groups mutates the rollback the table now lives in.
        let probe_buffers: Vec<BufferHandle> = (0..FRAMES_IN_FLIGHT)
            .map(|frame| probe_table.buffer(frame))
            .collect();
        // `docs/plan/50-irradiance-probes.md`'s updater, built **only** for a
        // description that asked for it. A volume left at
        // [`ProbeUpdate::Authored`] gets no pipeline, no buffers and no pass, so
        // the default really is the frame every scene drew before this landed.
        let probe_gather = match scene.probes.update {
            ProbeUpdate::Authored => None,
            ProbeUpdate::EveryFrame => Some(ProbeGather::new(
                device,
                &ProbeGatherDesc {
                    label: Some("forward probes"),
                    volume: scene.probes.volume,
                    probes: &probe_buffers,
                },
            )?),
        };
        rollback.probe_gather = probe_gather;
        let probe_gather = rollback.probe_gather.take();

        // **Empty.** Every object in the scene arrives through
        // [`ForwardRenderer::add_instance`], including the demo scene's cube: a
        // renderer that inserted one of its description's meshes here would put
        // an object in a caller's frame that the caller never asked for.
        let instances = InstancePool::new(
            device,
            &InstancePoolDesc {
                label: Some("forward instances"),
                capacity: scene.capacities.instances,
                frames_in_flight: FRAMES_IN_FLIGHT,
            },
        )?;
        // Same handle-then-hand-over dance as the geometry pool above: the
        // buffers are `Copy` and are read out before the pool becomes the
        // rollback's, which it must be before the first `?` below — after which
        // nothing needs the pool again until `begin_frame`.
        let instance_buffers: Vec<BufferHandle> = instances.buffers().to_vec();
        rollback.instances = Some(instances);

        // The two compute passes, and the buffers the draws come out of. Built
        // here rather than beside the pipelines below because the mesh bind
        // group names one of its buffers, so it has to exist first.
        //
        // The bucket table, and the one place its order is decided.
        //
        // **Built by walking the description**, which is what makes a duplicate
        // structurally impossible rather than refused: every bucket's mesh id is
        // the id of the description mesh it came from, and two description
        // meshes are two uploads with two table entries however alike their
        // geometry is. That matters because `draw_gen.slang`'s scatter takes the
        // *first* bucket whose mesh id matches, so a second bucket naming one
        // mesh would never draw anything at all.
        //
        // One bucket per mesh, except that a DAG is one bucket where an
        // amplification stage picks its cut per cluster and one per level where
        // the cull pass picks a uniform one — see [`buckets_for`].
        let mut bucket_meshes: Vec<u32> = Vec::with_capacity(residents.len());
        // Where each description mesh's buckets start, so the tables below can
        // be filled in mesh by mesh rather than by re-deriving the arithmetic.
        let mut bucket_bases: Vec<usize> = Vec::with_capacity(residents.len());
        for resident in &residents {
            bucket_bases.push(bucket_meshes.len());
            bucket_meshes
                .extend_from_slice(&resident.levels[..buckets_for(resident.levels.len(), emit)]);
        }
        let bucket_count = u32::try_from(bucket_meshes.len())
            .unwrap_or_else(|_| unreachable!("a table of a few buckets"));

        // `docs/plan/25-lod.md`'s selection tables, and the one thing that
        // decides whether `draw_gen.slang` takes a uniform cut at all.
        //
        // Every mesh id gets an entry, because the shader indexes this with
        // `GpuInstance::mesh` and an instance can name any of them; the default
        // is `MeshLevels::FLAT` pointing at a level run that is the mesh itself,
        // so a mesh with no hierarchy resolves back to itself with no branch.
        // **A DAG keeps that default on the mesh path**, which is how a device
        // already descending it per cluster avoids a second, coarser cut on top
        // of it — the suppression is data rather than a branch in the shader.
        //
        // **Sized by the mesh table's capacity, not by the meshes resident in
        // it**, because that is the invariant `draw_gen.slang`'s
        // `mesh_levels_of` states and indexes on: "every entry an instance can
        // name is filled". The read is unbounded and an instance's mesh id is a
        // bare table index, so a table sized to the description alone is an
        // out-of-range read of whatever the packer laid down next for any id the
        // description did not fill. What it finds there decides the frame: zeros
        // resolve to mesh 0 and draw another mesh's geometry, and a live
        // `group_count` sends `select_level`'s loop over a count no allocation
        // backs, which on radv is a hard GPU recovery rather than a wrong
        // picture. That is not a hypothetical: it is what a skinned instance
        // naming an alias entry did until 2026-08, before it learned to name its
        // source mesh instead.
        let table_len = usize::try_from(scene.capacities.meshes)
            .unwrap_or(usize::MAX)
            .max(
                usize::try_from(
                    residents
                        .iter()
                        .flat_map(|resident| resident.levels.iter().copied())
                        .max()
                        .unwrap_or_else(|| {
                            unreachable!("check_scene refused an empty description")
                        })
                        + 1,
                )
                .unwrap_or_else(|_| unreachable!("a table of a few meshes")),
            );
        let mut level_meshes: Vec<u32> = (0..table_len)
            .map(|id| u32::try_from(id).unwrap_or_else(|_| unreachable!("bounded by table_len")))
            .collect();
        let mut mesh_levels: Vec<level_select::MeshLevels> = level_meshes
            .iter()
            .map(|&id| level_select::MeshLevels {
                first_level: id,
                ..level_select::MeshLevels::FLAT
            })
            .collect();
        // Every DAG's groups end to end, in description order, and where each
        // one's run starts. `ClusterSelect` records name a group by its index in
        // *this* array — see `ClusterDag::selection_records`' `first_group` — so
        // several DAGs concatenate without `crcbl-shaders` knowing they did.
        let mut level_groups: Vec<level_select::LevelGroup> = Vec::new();
        let mut first_groups: Vec<u32> = Vec::with_capacity(residents.len());
        for (index, desc) in scene.meshes.iter().enumerate() {
            first_groups.push(
                u32::try_from(level_groups.len())
                    .unwrap_or_else(|_| unreachable!("a few dozen groups per DAG")),
            );
            let Geometry::Dag { dag, .. } = &desc.geometry else {
                continue;
            };
            let groups = dag.level_groups();
            let id = residents[index].id();
            mesh_levels[id as usize] = level_select::MeshLevels {
                first_group: first_groups[index],
                group_count: u32::try_from(groups.len())
                    .unwrap_or_else(|_| unreachable!("a DAG of a few dozen groups")),
                // **The mesh path suppresses the uniform cut with a top level of
                // zero, not with a group count of zero**, and the difference is
                // the hysteresis state: `draw_gen.slang` judges every group it is
                // given whatever level it answers, and the amplification stage
                // reads those answers. A record naming no groups would leave the
                // state untouched and every cluster of the patch collapsed. A
                // top level of zero makes the level loop's minimum unreachable,
                // so the instance routes to the mesh it already names — which is
                // level 0.
                first_level: if emit.is_mesh() {
                    id
                } else {
                    u32::try_from(level_meshes.len())
                        .unwrap_or_else(|_| unreachable!("a table of a few meshes"))
                },
                top_level: if emit.is_mesh() {
                    0
                } else {
                    u32::try_from(residents[index].levels.len() - 1)
                        .unwrap_or_else(|_| unreachable!("a DAG of a few levels"))
                },
            };
            level_groups.extend(groups);
            if !emit.is_mesh() {
                level_meshes.extend_from_slice(&residents[index].levels);
            }
        }

        // §3.5's clusters, on the path that reads them and on no other. The
        // records are `crcbl-shaders`' — cooked, because the builder is
        // `crcbl-scene`'s and the renderer must not depend on that crate — and
        // they arrive in bucket-mesh order so a bucket's range is its own
        // index. See `crate::cluster_pool`.
        //
        // Before the draw generation below rather than after it, because the
        // per-bucket cluster counts are the x extent of each bucket's mesh
        // dispatch and that pass writes the argument structure carrying them.
        let mut bucket_clusters = vec![0u32; bucket_meshes.len()];
        let mut bucket_cluster_bases = vec![0u32; bucket_meshes.len()];
        let mut mesh_clusters: Vec<ClusterRange> = Vec::new();
        if emit.is_mesh() {
            // A flat mesh is one pool entry and its clusters carry
            // `ClusterSelect::ALWAYS`, so the descent draws them from every
            // camera; a DAG is **one entry per level**, laid end to end.
            let mut cooked: Vec<PooledMesh> = Vec::with_capacity(residents.len());
            // Where each description mesh's entries start, on `bucket_bases`'
            // terms — the two differ exactly where a DAG's levels are one bucket
            // and several entries.
            let mut entry_bases: Vec<usize> = Vec::with_capacity(residents.len());
            for (index, desc) in scene.meshes.iter().enumerate() {
                entry_bases.push(cooked.len());
                match &desc.geometry {
                    Geometry::Flat { clusters, .. } => {
                        cooked.push(PooledMesh::without_lod(clusters.clone()));
                    }
                    Geometry::Dag { dag, .. } => {
                        let selection = dag
                            .selection_records(&residents[index].vertex_bases, first_groups[index]);
                        for (level, records) in dag.levels.iter().zip(selection) {
                            cooked.push(PooledMesh {
                                clusters: level.clusters.clone(),
                                selection: records,
                            });
                        }
                    }
                }
            }

            let clusters = ClusterPool::new(device, "forward", &cooked)?;
            let range = |entry: usize| {
                clusters
                    .range(entry)
                    .unwrap_or_else(|| unreachable!("one range per entry, in order"))
            };
            // **A DAG's levels are one run, not one per bucket.** `concatenate`
            // lays the entries down in the order it was given them, so the
            // levels are contiguous: the bucket starts where level 0 does and
            // reaches to the end of the last level. That is what lets one
            // dispatch cover a cut spanning several of them — and it is why a
            // flat mesh, whose one entry is the whole sum, needs no second case.
            mesh_clusters.reserve(residents.len());
            for (index, resident) in residents.iter().enumerate() {
                let entry = entry_bases[index];
                let bucket = bucket_bases[index];
                bucket_cluster_bases[bucket] = range(entry).base;
                bucket_clusters[bucket] = (entry..entry + resident.levels.len())
                    .map(|entry| range(entry).count)
                    .sum();
                mesh_clusters.push(ClusterRange {
                    base: bucket_cluster_bases[bucket],
                    count: bucket_clusters[bucket],
                });
            }

            rollback.clusters = Some(clusters);
        }

        let draws = DrawGen::new(
            device,
            queue,
            &DrawGenDesc {
                label: Some("forward"),
                instances: &instance_buffers,
                mesh_table,
                bucket_meshes: &bucket_meshes,
                bucket_clusters: &bucket_clusters,
                mesh_levels: &mesh_levels,
                level_groups: &level_groups,
                level_meshes: &level_meshes,
                instance_capacity: scene.capacities.instances,
            },
        )?;
        let runs: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.runs(frame))
            .collect();
        let args: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.args(frame))
            .collect();
        // What the amplification stage reads and writes, and nothing else does:
        // this frame's frustum, and the culling statistics its surviving
        // clusters are counted into.
        let cull_params: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.cull_params(frame))
            .collect();
        let cull_stats: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.visible_count(frame))
            .collect();
        rollback.draws = Some(draws);

        // `docs/plan/25-lod.md`'s observable: one word per resident cluster,
        // holding the cut the descent chose. Empty where there is no
        // amplification stage, which is the same condition binding 18 exists
        // under — and the two cannot disagree, because this vector is what
        // decides whether the entry is written.
        //
        // **One buffer per frame in flight**, on `cull_stats`' terms exactly: a
        // frame still in flight is a frame still writing, and one buffer shared
        // across the ring would have the next frame's dispatch overwriting what
        // this one recorded. `TRANSFER_SRC` because reading it is the point.
        let mut cluster_selection: Vec<BufferHandle> = Vec::new();
        // And one ring per shadow view, indexed `[view][frame]` — see
        // `ForwardRenderer::shadow_selection` for why a view cannot share the
        // buffer above.
        let mut tile_selection: Vec<Vec<BufferHandle>> = Vec::new();
        // Allocated on the whole mesh path rather than only where there is an
        // amplification stage to write them, because the layout declares
        // binding 18 there — see the layout, which is where that is argued. On
        // a device with no task stage nothing writes them and
        // `ForwardRenderer::cluster_selection` still answers `None`, so the
        // cost is the allocation and nothing else.
        if emit.is_mesh() {
            let count = rollback
                .clusters
                .as_ref()
                .unwrap_or_else(|| unreachable!("the mesh path implies a cluster pool"))
                .count();
            let mut ring = |label: &str| -> Result<Vec<BufferHandle>, HalError> {
                let mut buffers = Vec::with_capacity(instance_buffers.len());
                for frame in 0..instance_buffers.len() {
                    let buffer = device.create_buffer(&BufferDesc {
                        label: Some(&format!("{label} {frame}")),
                        size: u64::from(count) * 4,
                        usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_SRC),
                        memory: MemoryLocation::DeviceLocal,
                    })?;
                    rollback.buffers.push(buffer);
                    buffers.push(buffer);
                }
                Ok(buffers)
            };
            cluster_selection = ring("cluster selection")?;
            for view in 0..SHADOW_VIEWS {
                tile_selection.push(ring(&format!("shadow selection {view}"))?);
            }
        }

        // --- the mesh pass ---
        //
        // No `BindingFlags` anywhere: this layout is legal on **both** tiers,
        // because a lit mesh is not a Tier A feature. The bindless shape — a
        // trailing `VARIABLE_COUNT | PARTIALLY_BOUND | UPDATE_AFTER_BIND`
        // texture array — is topic 03's, at P3.
        // **Which stage pulls the geometry**, and the only thing the two shapes
        // of this layout disagree about for bindings 0 to 8. `ShaderStages::MESH`
        // is outside `GRAPHICS` and `ALL`, so it is named rather than implied,
        // and every backend refuses a layout carrying it on a device without
        // `Features::MESH_SHADER` — which is exactly the device that never
        // reaches this arm.
        //
        // **`TASK` joins it when there is an amplification stage**, and the
        // union is deliberately over every binding rather than the eight
        // `taskMain` actually reads. Slang's Metal backend materialises every
        // global in every entry point of a module — the argument bindings 6 to
        // 8 already carry — so a layout that named only the task stage's own
        // reads would be a layout `msl/mesh_cluster.metal` disagrees with, on a
        // runner this team cannot debug on.
        let geometry = match (emit.is_mesh(), culls_clusters) {
            (true, true) => ShaderStages::MESH.union(ShaderStages::TASK),
            (true, false) => ShaderStages::MESH,
            (false, _) => ShaderStages::VERTEX,
        };
        let mut mesh_entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: geometry.union(ShaderStages::FRAGMENT),
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The shader says `StructuredBuffer`, not
                    // `RWStructuredBuffer`, so this is the truth rather than a
                    // hint — and it is what lets the graph merge read-after-read.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // `StructuredBuffer` again: the vertex stage reads its
                    // instance and writes nothing. The *host* writes this one
                    // every frame, which is a mapped write rather than a shader
                    // one and does not make it read-write here.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: geometry,
                // `dynamic: true`, and it is the whole mechanism: it is what
                // lets a draw say where its run of instances starts without
                // `draw_indexed`'s own base instance, which the module docs and
                // `mesh.slang`'s header explain the targets disagree about.
                // Same shape, for the same reason, as `crate::sprite_pass`'s.
                kind: BindingKind::UniformBuffer { dynamic: true },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The mesh table. `StructuredBuffer` again — the vertex
                    // stage looks its mesh up and writes nothing — and one
                    // buffer shared by every frame's group, because unlike the
                    // instance array nothing rewrites it between frames.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The per-bucket runs of surviving instances. Read-only
                    // here and written by the draw-argument pass, which is a
                    // different bind group and a different pass — the graph is
                    // what orders the two.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 6,
                // **Both stages, and the union is not belt-and-braces.** The
                // fragment stage is where `mesh.slang` now reads the table, so
                // it plainly needs it — a pipeline layout that leaves it out is
                // refused outright by wgpu and reported as
                // `VUID-VkGraphicsPipelineCreateInfo-layout-07988` by Vulkan.
                // The vertex half has to stay because Slang's Metal backend
                // materialises every global in every entry point: `vertexMain`
                // in `msl/mesh.metal` still takes `materials [[buffer(6)]]`
                // whether it reads it or not, so dropping VERTEX would break
                // Metal alone, on a runner this team cannot debug on.
                visibility: geometry.union(ShaderStages::FRAGMENT),
                kind: BindingKind::StorageBuffer {
                    // The material table. One buffer in every frame's group,
                    // like the mesh table above and unlike the instance ring:
                    // a material is written when it is created, not per frame.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 7,
                // Both stages, for binding 6's reason exactly: the fragment
                // stage samples it, and Slang's Metal backend materialises
                // every global in every entry point — `vertexMain` in
                // `msl/mesh.metal` takes the texture whether it reads it or
                // not, so a layout without VERTEX would break Metal alone.
                visibility: geometry.union(ShaderStages::FRAGMENT),
                // **`D2Array`, and the layout is where wgpu wants to hear it.**
                // The other three backends read the dimension off the bound
                // view and never consult this; WebGPU compares the two at
                // pipeline creation and refuses the pair, which is what a
                // layout claiming `D2` over an array view got — "expects
                // dimension = D2, but given a view with dimension = D2Array".
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                // **`count: 1`, and this is the binding-model decision.** One
                // `D2Array` image whose *layers* the material rows index is
                // `BindingModel::ArrayPages`; an array of `count` descriptors
                // indexed per fragment would be `Bindless` and would need
                // `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws.
                // So this layout is legal on every device, exactly as the six
                // buffers above are, and no `BindingFlags` appear anywhere in
                // this file yet.
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: PAGE_SAMPLER_BINDING,
                visibility: geometry.union(ShaderStages::FRAGMENT),
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        if emit.is_mesh() {
            // §3.5's four extra reads, and only on the path that has them.
            // Adding them unconditionally would make every device declare four
            // buffers `mesh.slang` does not name — and WebGPU, which checks a
            // bind group against its layout entry for entry, would then need
            // four buffers created for a path that never reads them.
            //
            // `read_only: true` on all four, which is the truth rather than a
            // hint: the mesh stage indexes them and writes nothing, and it is
            // what lets the graph merge read-after-read.
            let cluster_read = BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            };
            mesh_entries.extend((9..=12).map(|binding| BindGroupLayoutEntry {
                binding,
                // **Not the fragment stage.** These four are the geometry
                // stage's alone, and `mesh_cluster.slang` has no fragment entry
                // point of its own to materialise them into — its fragment
                // stage is `mesh.slang`'s, which names bindings 0 to 8 and
                // nothing above them.
                visibility: geometry,
                kind: cluster_read,
                count: 1,
                flags: BindingFlags::empty(),
            }));
        }
        // **`emit.is_mesh()`, not `culls_clusters`, and that is a fix rather
        // than a widening.** §3.5's per-cluster cull is the only thing that
        // *reads* these two — the frustum, and somewhere to count what survived
        // — so gating them on the amplification stage was the obvious shape.
        // It was wrong on Metal: `mesh_cluster.slang` declares them
        // unconditionally, Slang's Metal target ignores `[[vk::binding]]` and
        // hands each resource the next index in its stage's flat table in
        // declaration order, and `crcbl-mtl` derives that index by counting the
        // same-table entries of *this list*. A layout that skipped 13 and 14
        // therefore placed binding 17 at `buffer(11)` while
        // `msl/mesh_cluster.metal` reads it at `buffer(13)`, and every binding
        // above it was off by two — a wrong picture with a clean log, on the
        // one backend nobody here can debug on. Bindings 6, 7, 8, 20, 21 and 23
        // are declared by that shader for exactly this reason and this is the
        // same rule applied to the layout side.
        if emit.is_mesh() {
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 13,
                visibility: geometry,
                // **The same block the instance cull reads**, not a copy: one
                // camera, one frustum, and no way for the two rejections to
                // disagree about which. `dynamic: false` because there is one
                // block per frame rather than one per bucket.
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            });
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 14,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // **The one writable binding this pass has.** The stage adds
                    // to a counter, so `read_only: false` is the truth rather
                    // than a hint, and it is what makes the graph order this
                    // write after the draw-argument pass's read of the same
                    // buffer.
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
        }
        // Topic 18's cascades, read by the fragment stage and declared last —
        // above `mesh_cluster.slang`'s range rather than inside it, for the
        // reason `SHADOW_ATLAS_BINDING` gives.
        //
        // Both carry `geometry` in their visibility beside `FRAGMENT`, for
        // binding 7's reason exactly: Slang's Metal backend materialises every
        // global into every entry point, and `msl/mesh.metal`'s `vertexMain`
        // takes `shadow_atlas [[texture(1)]]` and `shadow_sampler
        // [[sampler(1)]]` whether it reads them or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: SHADOW_ATLAS_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // **`Depth`, and this is the field the seam grew for this
                // pass.** WebGPU will only bind a `D32Float` view through a
                // depth sample type, and its shader side agrees — the WGSL
                // artifact declares `texture_depth_2d`. The other three
                // backends take the interpretation off the view's format and
                // never read this.
                sample_type: SampleType::Depth,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        mesh_entries.push(BindGroupLayoutEntry {
            binding: SHADOW_SAMPLER_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            // The sampler-side twin of the line above: `sampler_comparison` in
            // the WGSL, `SamplerComparisonState` in the HLSL, and a
            // `compareEnable` on the Vulkan sampler object. The layout has to
            // say so on WebGPU and cannot on the other three.
            kind: BindingKind::Sampler { comparison: true },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // `docs/plan/25-lod.md`'s two, **after the shadow pair rather than
        // beside their own kin**: 15 and 16 are taken by the fragment stage of
        // this very pipeline, so `mesh_cluster.slang` resumes at 17. Ascending
        // order matters here and not only for readability — `crcbl-mtl` gives a
        // resource the next index in its Metal argument table by counting the
        // same-table entries of this list.
        if emit.is_mesh() {
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 17,
                visibility: geometry,
                // Read-only and read by **both** mesh entry points, unlike the
                // pair below: a cluster's `vertex_base` is which level of a DAG
                // its geometry lives in, which an un-amplified stage needs just
                // as much as an amplified one.
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
        }
        // Both on the whole mesh path, for the two above's reason exactly: the
        // amplification stage is the only writer and the only reader, and the
        // Metal argument table counts entries rather than reading binding
        // numbers.
        if emit.is_mesh() {
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 18,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The second writable binding: the amplification stage
                    // records the cut it chose, one word per resident cluster.
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 19,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // `docs/plan/25-lod.md`'s hysteresis state, and read-only
                    // here: the draw-argument pass is its only writer, which is
                    // what lets a stage with one workgroup per cluster use a
                    // decision that has to survive a frame.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
        }

        // Topic 18's light list and froxel grid, next because
        // `mesh_cluster.slang` reaches 19 and both files declare these two above
        // it. Bound on **every** path and in every group, unlike the four
        // conditional ranges above: `mesh.slang`'s fragment stage reads them
        // whatever geometry produced the primitive, so there is no configuration
        // in which the layout can omit them.
        //
        // `geometry` joins `FRAGMENT` in the visibility for binding 7's reason
        // exactly: Slang's Metal backend materialises every global into every
        // entry point, so the vertex and mesh stages take these buffers whether
        // they read them or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: LIGHT_LIST_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        mesh_entries.push(BindGroupLayoutEntry {
            binding: LIGHT_GRID_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::StorageBuffer {
                // **Read-only here**, and written by the clustering pass through
                // a layout of its own. The graph is what orders the two.
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // `docs/plan/18-render-features.md`'s occlusion channel, last of the set
        // — see [`AMBIENT_OCCLUSION_BINDING`] on why last is structural rather
        // than tidy.
        //
        // `geometry` beside `FRAGMENT` for binding 7's reason exactly, and
        // `msl/mesh.metal` is the proof rather than the theory: its `vertexMain`
        // takes `ambient_occlusion [[texture(2)]]` whether it reads it or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: AMBIENT_OCCLUSION_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // **`Float`, unlike the shadow atlas above.** This one is an
                // ordinary `R8Unorm` colour image and the WGSL artifact declares
                // `texture_2d<f32>` for it; `Depth` here would be a layout
                // claiming a depth format the view does not have.
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // The irradiance grid, past the occlusion channel — see
        // [`PROBE_TABLE_BINDING`] on why the list only ever grows at its top.
        //
        // `geometry` beside `FRAGMENT` for binding 7's reason exactly, and
        // `msl/mesh.metal` is again the proof rather than the theory: its
        // `vertexMain` takes `probes [[buffer(9)]]` whether it reads it or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: PROBE_TABLE_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::StorageBuffer {
                // **Read-only, and it stays read-only when the updater lands.**
                // The rows are device-local now — see [`crate::probe`], which is
                // where that is argued — so the seam no longer forces this;
                // what forces it is that the shading path does not write a
                // probe and this layout has no headroom to pretend otherwise.
                // The raster path is at the WebGPU per-stage storage-buffer
                // ceiling already, which the check at the end of the layout
                // holds it to, so `read_only: false` here would buy a write
                // nothing performs at the price of a binding budget there is
                // none of. `docs/plan/50-irradiance-probes.md`'s gather is a
                // compute pass with a layout of its own, and that is where the
                // writable binding of these rows belongs.
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        if emit.is_mesh() {
            // `docs/plan/25-lod.md`'s group records, and the top of the list —
            // see [`LEVEL_GROUP_TABLE_BINDING`], which is where "the list only
            // ever grows at its top" is argued.
            //
            // **Conditional like bindings 9 to 12 and 17**, and not for tidiness:
            // the raster path's layout is at the WebGPU storage-buffer ceiling
            // already, so a row bound unconditionally would be a renderer that
            // cannot be built in a browser. The mesh path's own reads sit outside
            // that count because `ShaderStages::MESH` is not one of the three
            // stages it sums.
            mesh_entries.push(BindGroupLayoutEntry {
                binding: LEVEL_GROUP_TABLE_BINDING,
                // **Not the fragment stage**, on bindings 9 to 12's terms: the
                // heatmap's colour is chosen where the cluster is known, and
                // `mesh.slang`'s fragment stage — which is this pipeline's —
                // names nothing above 23.
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // Read-only and read by **both** mesh entry points, like
                    // binding 17: an un-amplified stage draws through the same
                    // `emit_cluster`, so a row present for one and absent for the
                    // other is a descriptor read out of an empty slot.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
        }

        // The `DFG` table, above everything — see [`SPECULAR_DFG_BINDING`].
        //
        // `geometry` beside `FRAGMENT` for binding 7's reason exactly: Slang's
        // Metal backend materialises every global into every entry point, so
        // `msl/mesh.metal`'s `vertexMain` takes `specular_dfg [[texture(3)]]`
        // whether it reads it or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: SPECULAR_DFG_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // `Float`, as the occlusion channel above: an `Rgba8Unorm`
                // colour image, declared `texture_2d<f32>` by the WGSL artifact.
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });

        // §2's normal page, above everything — see [`NORMAL_PAGE_BINDING`].
        //
        // `geometry` beside `FRAGMENT` for binding 7's reason exactly: Slang's
        // Metal backend materialises every global into every entry point, so
        // `msl/mesh.metal`'s `vertexMain` takes the page whether it samples it
        // or not. **A sampled image and not a storage buffer**, which costs this
        // layout nothing at the WebGPU ceiling checked below — that limit counts
        // storage buffers, and this row is a texture.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: NORMAL_PAGE_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2Array,
                // `Float`: an `Rgba8Unorm` image, declared
                // `texture_2d_array<f32>` by the WGSL artifact — the same
                // sample type the sRGB base-colour page reports, since both
                // decode to floats.
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // The area lights' table, above the page — see [`LTC_TABLE_BINDING`].
        //
        // `geometry` beside `FRAGMENT` for the two rows above it's reason.
        // `Float` because `Rgba16Float` is a filterable sampled format on every
        // target this engine opens, and the WGSL artifact declares it
        // `texture_2d<f32>` — the shader only ever `Load`s it, but the layout
        // has to say what the module declared.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: LTC_TABLE_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // `docs/plan/45-shadows.md`'s contact-shadow channel, last of the set —
        // see [`CONTACT_SHADOW_BINDING`] on why last is structural rather than
        // tidy.
        //
        // `geometry` beside `FRAGMENT` and `Float` for the occlusion channel's
        // reasons exactly: it is the same `R8Unorm` shape, and Slang's Metal
        // backend materialises every global into every entry point whether that
        // stage reads it or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: CONTACT_SHADOW_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // `docs/plan/50-irradiance-probes.md`'s per-probe visibility maps, last
        // of the set — see [`PROBE_VISIBILITY_BINDING`] on why last is
        // structural.
        //
        // `D2Array` has to be *declared*, not merely bound: WebGPU compares the
        // dimension a layout entry claims against the view handed to it at
        // pipeline creation, which is the normal page's own note.
        // `UnfilterableFloat` for the same reason and measured the same way: the
        // image is `Rg32Float`, which is unfilterable without WebGPU's
        // `float32-filterable`, and this slot declared `Float` made Chromium
        // refuse the whole mesh bind group — "None of the supported sample types
        // (UnfilterableFloat) ... match the expected sample types (Float)" —
        // which took every 3D demo's frame to black. Reading it only with `Load`
        // does not lift that: the check is on the layout against the view's
        // format, not on how the shader gets at it.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: PROBE_VISIBILITY_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2Array,
                sample_type: SampleType::UnfilterableFloat,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        let mesh_desc = BindGroupLayoutDesc {
            label: Some(MESH_LAYOUT_LABEL),
            entries: &mesh_entries,
        };
        // **This layout is at the guaranteed limit with no headroom on the
        // raster path**: eight storage buffers in the vertex stage, which is
        // every one a WebGPU device promises. A ninth is a renderer that cannot
        // be built in a browser or on SwiftShader, so it fails here rather than
        // at somebody else's `createPipelineLayout` — see
        // [`crcbl_hal::check_portable_storage_buffers`], which also says why the
        // mesh path's extra reads are outside the count.
        // `Some("mesh")`, which is the *pipeline* layout's label, not
        // `mesh_desc.label` — the error text reads "pipeline layout {label}
        // binds …", so naming the bind group layout there sends the reader to
        // the wrong object.
        check_portable_storage_buffers(Some("mesh"), &[&mesh_desc])?;
        let mesh_layout = device.create_bind_group_layout(&mesh_desc)?;
        rollback.bind_group_layouts.push(mesh_layout);

        // A dynamic offset must be a multiple of the device's alignment, and one
        // block has to fit inside one stride. Read from the device rather than
        // assumed, exactly as `crate::sprite_pass` does: 256 on WebGPU, 64 on a
        // typical desktop Vulkan driver.
        //
        // One stride for both blocks, sized for the larger — see
        // [`DRAW_CONSTANTS_BLOCK`]. Which of the two a bucket's block holds is
        // decided below.
        let alignment = device.caps().limits.min_uniform_buffer_offset_alignment;
        let draw_stride =
            u32::try_from(DRAW_CONSTANTS_BLOCK.next_multiple_of(alignment)).map_err(|_| {
                HalError::InvalidDescriptor(format!(
                    "min_uniform_buffer_offset_alignment is {alignment}, which no dynamic \
                         offset can express"
                ))
            })?;
        let draw_constants = device.create_buffer(&BufferDesc {
            label: Some("mesh draw constants"),
            size: u64::from(draw_stride) * u64::from(bucket_count),
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        })?;
        rollback.buffers.push(draw_constants);
        // Written here and never again: where a bucket's run of instances
        // starts is fixed by the bucket table, and the table is fixed at build.
        // What varies per frame is how much of the run is filled, and the GPU
        // writes that into the bucket's indirect arguments.
        let draws = rollback
            .draws
            .as_ref()
            .unwrap_or_else(|| unreachable!("draw generation was placed in the rollback above"));
        let mut bucket_constants = vec![0u32; bucket_meshes.len()];
        for (bucket, offset) in bucket_constants.iter_mut().enumerate() {
            let index = bucket;
            let bucket =
                u32::try_from(bucket).unwrap_or_else(|_| unreachable!("a table of a few buckets"));
            *offset = bucket * draw_stride;
            let base = draws.bucket_base(bucket);
            // The mesh path's block says three more things — where this
            // bucket's mesh's clusters are, how many it has, and which element
            // of the indirect arguments holds its instance count — because a
            // dispatch carries none of them and a `draw_indexed_indirect` does
            // not need them. All three are fixed when the bucket table is,
            // exactly like `base`.
            let block = if emit.is_mesh() {
                crcbl_shaders::meshlet::ClusterDrawConstants {
                    base,
                    cluster_base: bucket_cluster_bases[index],
                    cluster_count: bucket_clusters[index],
                    bucket,
                    // The same number the draw-argument pass indexes the state
                    // with, taken from the object that owns the buffer rather
                    // than recomputed from the group table — the two indexing it
                    // differently is a cluster reading another instance's
                    // decision.
                    group_stride: draws.group_stride(),
                    // Where the group records are in the table buffer, from the
                    // object that packed it — the screen-error heatmap's one
                    // input that is not already in the frame block. Taken from
                    // `DrawGen` rather than recomputed here for `group_stride`'s
                    // reason: two spellings of one offset is an overlay reading
                    // another region's words as a group.
                    level_groups_at: draws.table_offsets().level_groups_at,
                }
                .to_bytes()
                .to_vec()
            } else {
                // **The bucket's mesh, which is not always the drawn instance's.**
                // A uniform cut selects one of a DAG's levels and each level is
                // a mesh table entry of its own, so this is what says which
                // geometry the draw's index range belongs to; the instance goes
                // on naming level 0. See `mesh::DrawConstants::mesh`.
                mesh::DrawConstants {
                    base,
                    mesh: bucket_meshes[index],
                }
                .to_bytes()
                .to_vec()
            };
            device.write_buffer(draw_constants, u64::from(*offset), &block)?;
        }

        // --- the shadow map's own resources ---
        //
        // Before the bind groups, because every one of them names the atlas or
        // the placeholder standing in for it.
        let (atlas_width, atlas_height) = shadow::atlas_extent();
        let shadow_atlas = device.create_image(&ImageDesc {
            label: Some("shadow atlas"),
            image_type: ImageType::D2,
            format: Format::D32Float,
            extent: crcbl_hal::Extent3d::d2(atlas_width, atlas_height),
            mip_levels: 1,
            samples: 1,
            // `TRANSFER_SRC` so a test can copy the cascades back and check
            // that something was written into them. A shadow map that stayed at
            // its clear value produces a frame that looks entirely plausible —
            // everything is lit — so the readback is the only thing that can
            // tell a working pass from one that drew nothing.
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                .union(ImageUsage::SAMPLED)
                .union(ImageUsage::TRANSFER_SRC),
        })?;
        rollback.images.push(shadow_atlas);
        let shadow_atlas_view = device.create_image_view(&ImageViewDesc {
            label: Some("shadow atlas"),
            image: shadow_atlas,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: ImageSubresourceRange::all(Format::D32Float),
        })?;
        rollback.image_views.push(shadow_atlas_view);

        let shadow_placeholder = device.create_image(&ImageDesc {
            label: Some("shadow placeholder"),
            image_type: ImageType::D2,
            format: Format::D32Float,
            extent: crcbl_hal::Extent3d::d2(1, 1),
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT.union(ImageUsage::SAMPLED),
        })?;
        rollback.images.push(shadow_placeholder);
        let shadow_placeholder_view = device.create_image_view(&ImageViewDesc {
            label: Some("shadow placeholder"),
            image: shadow_placeholder,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: ImageSubresourceRange::all(Format::D32Float),
        })?;
        rollback.image_views.push(shadow_placeholder_view);

        // The occlusion placeholder, beside the shadow one and for the same
        // reason: every group of this layout has to fill
        // [`AMBIENT_OCCLUSION_BINDING`], and most of them are built before the
        // graph has an occlusion image to name.
        //
        // **Uploaded rather than cleared**, unlike the depth placeholder above,
        // and the four bytes are the whole point — see
        // [`AMBIENT_OCCLUSION_NONE`], which is where each of them is argued.
        // Left undefined this would be a random ambient scale *and* a random
        // direction to steer it along, on any frame that fell back to it.
        let ambient_occlusion_placeholder = upload_texture(
            device,
            queue,
            "ssao placeholder",
            Format::Rgba8Unorm,
            1,
            1,
            &AMBIENT_OCCLUSION_NONE,
        )?;
        rollback.textures.push(ambient_occlusion_placeholder);

        // The contact-shadow placeholder, beside the occlusion one and on its
        // terms exactly: every group of this layout has to fill
        // [`CONTACT_SHADOW_BINDING`], and most of them are built before the graph
        // has a contact-shadow image to name. `0xFF` is `1.0` through `R8Unorm`,
        // so a fragment that reads this one is a fragment the sun reaches
        // unobstructed.
        let contact_shadow_placeholder = upload_texture(
            device,
            queue,
            "contact shadow placeholder",
            Format::R8Unorm,
            1,
            1,
            &[CONTACT_SHADOW_NONE],
        )?;
        rollback.textures.push(contact_shadow_placeholder);

        // The probe-visibility placeholder, on the two above it's terms: every
        // group of this layout has to fill [`PROBE_VISIBILITY_BINDING`], and
        // nothing has been captured when they are built. One texel of
        // [`probe_visibility_none`], which no surface is further away than, so a
        // probe read through it keeps all of its weight.
        let probe_visibility_placeholder = upload_texture_layers(
            device,
            queue,
            "probe visibility placeholder",
            Format::Rg32Float,
            1,
            1,
            &[&probe_visibility_none()],
        )?;
        rollback.textures.push(probe_visibility_placeholder);

        // The split-sum `DFG` table, uploaded once and read by every frame this
        // renderer will draw — `crcbl_shaders::dfg` is where it is integrated,
        // committed and encoded, and `SPECULAR_ALBEDO_BINDING` is where it lands.
        //
        // **Cooked bytes rather than a computed image**: the integrator
        // importance-samples a lobe, which is `sin`, `cos` and a `powf`, and this
        // engine's goldens are compared across four backends with no tolerance.
        // Baking here would be four slightly different tables.
        let specular_dfg = upload_texture(
            device,
            queue,
            "dfg table",
            // Two bytes of fixed point per channel, high byte first — see
            // `crcbl_shaders::dfg::PAIR_TEXEL_BYTES`, which argues why this is
            // not the `Rg16Float` the format's own doc anticipated.
            Format::Rgba8Unorm,
            DFG_SIZE_U32,
            DFG_SIZE_U32,
            &dfg::pair_texels(),
        )?;
        rollback.textures.push(specular_dfg);

        // The area lights' fit, beside it and on its terms — cooked bytes for
        // the same reason, and `Rgba16Float` rather than fixed point because
        // these are matrix entries rather than shares of arriving light.
        // `crcbl_shaders::ltc::LTC_TEXEL_BYTES` argues the format.
        let ltc_table = upload_texture(
            device,
            queue,
            "ltc table",
            Format::Rgba16Float,
            LTC_SIZE_U32,
            LTC_SIZE_U32,
            &ltc::texels(),
        )?;
        rollback.textures.push(ltc_table);

        // **A comparison sampler, and that is the PCF.** Each
        // `SampleCmpLevelZero` returns the filtered fraction of four texels that
        // passed the test, so the shader's 3×3 kernel is nine hardware-bilinear
        // comparisons rather than nine raw fetches it averages itself.
        //
        // `Greater` for the engine's reversed-Z, exactly as the depth *test*
        // is: the receiver survives when it is nearer the light than whatever
        // the map stored. `SamplerDesc::compare` carries the same warning.
        //
        // Clamped on every axis so a receiver just outside a cascade's box
        // reads the edge rather than wrapping into the opposite corner — the
        // shader's own bounds check is what actually handles that case, and this
        // is the second line of defence one texel wide.
        let shadow_sampler = device.create_sampler(&SamplerDesc {
            label: Some("shadow comparison"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            compare: Some(crcbl_hal::CompareOp::Greater),
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(shadow_sampler);

        // §3.3's cull, once per cascade and once per shadowed light. Each gets
        // its own `DrawGen` and therefore its own frustum, survivor list and
        // indirect arguments — which is what "one cull dispatch per cascade
        // against the same instance/geometry pools" means, and why the instance
        // ring and the mesh table below are the same handles the camera's cull
        // was given.
        //
        // **One per light rather than one per tile**, which is topic 18's fourth
        // decision and what `SHADOW_CULLS` is: a point light's six faces cull
        // against the light's sphere once and draw that one visible set through
        // six matrices.
        //
        // **Built for every slot, including the ones no light may ever hold.** A
        // generator's buffers are sized by the instance capacity rather than by
        // the scene, so building one on demand would mean allocating device
        // memory inside `begin_frame` — and a frame that cannot allocate is a
        // frame that cannot draw. The unused ones cost memory and no dispatch:
        // `add_shadow_pass` records passes for the occupied slots alone.
        for cull in 0..SHADOW_CULLS {
            // Into the rollback as each is built, on the same terms as the
            // camera's: a failure two culls in has to release the first one,
            // and the rollback is the only thing that knows about it.
            let label = if cull < shadow::CASCADES {
                format!("shadow cascade {cull}")
            } else {
                format!("shadow light {}", cull - shadow::CASCADES)
            };
            let draws = DrawGen::new(
                device,
                queue,
                &DrawGenDesc {
                    label: Some(&label),
                    instances: &instance_buffers,
                    mesh_table,
                    bucket_meshes: &bucket_meshes,
                    bucket_clusters: &bucket_clusters,
                    mesh_levels: &mesh_levels,
                    level_groups: &level_groups,
                    level_meshes: &level_meshes,
                    instance_capacity: scene.capacities.instances,
                },
            )?;
            rollback.shadow_draws.push(draws);
        }
        // The handles are `Copy` and are read out here for the same reason the
        // camera's are above: the pool they came from is the rollback's now.
        let tile_buffers: Vec<TileBuffers> = rollback
            .shadow_draws
            .iter()
            .map(|draws| TileBuffers {
                runs: (0..instance_buffers.len())
                    .map(|frame| draws.runs(frame))
                    .collect(),
                args: (0..instance_buffers.len())
                    .map(|frame| draws.args(frame))
                    .collect(),
                cull_params: (0..instance_buffers.len())
                    .map(|frame| draws.cull_params(frame))
                    .collect(),
                cull_stats: (0..instance_buffers.len())
                    .map(|frame| draws.visible_count(frame))
                    .collect(),
                group_state: draws.group_state(),
            })
            .collect();

        // Topic 18's light list and the froxel grid its compute pass fills.
        //
        // Built here rather than beside the cull because it needs the
        // culling-statistics ring: its overflow counter is a word of that
        // buffer, which is what keeps topic 03 §3.6's readback at one.
        let lights = LightGrid::new(
            device,
            &LightGridDesc {
                label: Some("lights"),
                frames: instance_buffers.len(),
                lights: scene.capacities.lights,
                froxels: FROXEL_CAPACITY,
                stats: &cull_stats,
            },
        )?;
        rollback.lights = Some(lights);
        let lights = rollback.lights.as_ref().expect("just stored");

        let mut uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut mesh_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut mesh_group_entries = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut prepass_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut prepass_stats = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_selection = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut prepass_group_entries = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_group_entries = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for (frame, &slot_instances) in instance_buffers.iter().enumerate() {
            // Everything a group of this layout names that is the same in all of
            // this frame's. The per-group half is what `MeshGroup` below varies,
            // and the two exist so the colour pass's group and the shadow pass's
            // are one description rather than two that agree today.
            let shared = SharedBindings {
                vertices,
                draw_constants,
                mesh_table,
                materials: material_buffer,
                page: base_color_page.view,
                normal_page: normal_page.view,
                page_sampler: base_color_sampler,
                clusters: rollback.clusters.as_ref(),
                shadow_sampler,
                lights: lights.lights(frame),
                light_grid: lights.grid(frame),
                probes: probe_buffers[frame],
                tables: draws.tables(),
                specular_dfg: specular_dfg.view,
                ltc_table: ltc_table.view,
            };
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("mesh frame uniforms"),
                size: mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(buffer);
            let entries = MeshGroup {
                uniforms: buffer,
                instances: slot_instances,
                runs: runs[frame],
                args: args[frame],
                cull_params: cull_params[frame],
                cull_stats: cull_stats[frame],
                cluster_selection: cluster_selection.get(frame).copied(),
                group_state: emit.is_mesh().then(|| draws.group_state()),
                // The colour pass reads the finished atlas. Its own pass writes
                // nothing to it, so there is no conflict to avoid here.
                shadow_map: shadow_atlas_view,
                // The placeholder even for the camera's group: the occlusion
                // image is a graph transient and its view does not exist until
                // execute time. `add_passes` rebuilds this group against the real
                // one and caches it, and *this* group is what the depth prepass
                // binds — which runs before there is any occlusion to name.
                ambient_occlusion: ambient_occlusion_placeholder.view,
                // The same, one binding along, and for the same reason.
                contact_shadow: contact_shadow_placeholder.view,
                probe_visibility: probe_visibility_placeholder.view,
            }
            .entries(&shared);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("mesh frame"),
                layout: mesh_layout,
                entries: &entries,
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            uniforms.push(buffer);
            mesh_groups.push(group);
            // Kept so the forward pass can rebuild this group against the
            // occlusion image the graph realised, without re-deriving twenty
            // bindings out of fields that no longer exist by then. Only the
            // screen-space channels and the probe visibility maps differ — see
            // [`ForwardRenderer::screen_channel_groups`].
            mesh_group_entries.push(entries);

            // The depth prepass's group: this one again, counting its clusters
            // somewhere the camera's counter cannot see. See
            // [`ForwardRenderer::prepass_groups`] for why that matters, and note
            // that binding 14 exists at all only where there is an amplification
            // stage — so on every other path this buffer is bound nowhere and the
            // group is the camera's under another handle.
            //
            // `DeviceLocal`, because a shader writes it: D3D12 has no unordered
            // access view of a host-visible resource, and `create_bind_group`
            // enforces it.
            let stats = device.create_buffer(&BufferDesc {
                label: Some("depth prepass cluster survivors"),
                size: u64::from(crcbl_shaders::cull::STATS_WORDS) * 4,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::DeviceLocal,
            })?;
            rollback.buffers.push(stats);
            let entries = MeshGroup {
                uniforms: buffer,
                instances: slot_instances,
                runs: runs[frame],
                args: args[frame],
                cull_params: cull_params[frame],
                cull_stats: stats,
                cluster_selection: cluster_selection.get(frame).copied(),
                group_state: emit.is_mesh().then(|| draws.group_state()),
                shadow_map: shadow_atlas_view,
                ambient_occlusion: ambient_occlusion_placeholder.view,
                contact_shadow: contact_shadow_placeholder.view,
                probe_visibility: probe_visibility_placeholder.view,
            }
            .entries(&shared);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("depth prepass"),
                layout: mesh_layout,
                entries: &entries,
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            prepass_groups.push(group);
            prepass_stats.push(stats);
            // For the page sampler's rebuild — see [`ForwardRenderer::prepass_group_entries`].
            prepass_group_entries.push(entries);

            // The same layout again, once per shadow view, differing in exactly
            // the things a view is: which matrix, and which cull's survivors.
            //
            // **The view's own uniforms and the cull's shared buffers**, which
            // is the whole of what a point light's six faces are: one visible
            // set, six matrices. `shadow_cull` is the map from a view to the
            // generator behind it, and it is a compile-time one because a bind
            // group names its buffers when it is created — where a face's *tile*
            // is decided per frame and reaches the device as a viewport.
            let mut frame_shadow_uniforms = Vec::with_capacity(SHADOW_VIEWS);
            let mut frame_shadow_groups = Vec::with_capacity(SHADOW_VIEWS);
            let mut frame_shadow_selection = Vec::with_capacity(SHADOW_VIEWS);
            let mut frame_shadow_entries = Vec::with_capacity(SHADOW_VIEWS);
            for view in 0..SHADOW_VIEWS {
                let buffers = &tile_buffers[if view < shadow::CASCADES {
                    view
                } else {
                    shadow_cull((view - shadow::CASCADES) / shadow::POINT_FACES)
                }];
                let tile_uniforms = device.create_buffer(&BufferDesc {
                    label: Some("shadow view uniforms"),
                    size: mesh::FRAME_UNIFORMS_SIZE as u64,
                    usage: BufferUsage::UNIFORM,
                    memory: MemoryLocation::HostUpload,
                })?;
                rollback.buffers.push(tile_uniforms);
                let entries = MeshGroup {
                    uniforms: tile_uniforms,
                    instances: slot_instances,
                    runs: buffers.runs[frame],
                    args: buffers.args[frame],
                    // The amplification stage culls clusters against whatever
                    // frustum is in this block, and against
                    // `frame.camera_position` — which in a view's copy is the
                    // *light*. So the per-cluster cull rejects what faces away
                    // from the light, which is the right question for a shadow
                    // map and the wrong one to have asked with the camera's
                    // frustum.
                    cull_params: buffers.cull_params[frame],
                    cull_stats: buffers.cull_stats[frame],
                    // **This view's own**, and not the colour pass's: that pass
                    // is recorded last and would write over it. See
                    // `ForwardRenderer::shadow_selection`.
                    cluster_selection: tile_selection.get(view).map(|ring| ring[frame]),
                    // Its cull's — see `TileBuffers::group_state`, which is
                    // where that budget is argued.
                    group_state: emit.is_mesh().then_some(buffers.group_state),
                    shadow_map: shadow_placeholder_view,
                    // A cascade shades nothing — the pipeline has no fragment
                    // stage — so this slot exists only because Metal
                    // materialises every global into every entry point.
                    ambient_occlusion: ambient_occlusion_placeholder.view,
                    contact_shadow: contact_shadow_placeholder.view,
                    probe_visibility: probe_visibility_placeholder.view,
                }
                .entries(&shared);
                let group = device.create_bind_group(&BindGroupDesc {
                    label: Some("shadow view"),
                    layout: mesh_layout,
                    entries: &entries,
                    variable_count: None,
                })?;
                rollback.bind_groups.push(group);
                frame_shadow_uniforms.push(tile_uniforms);
                frame_shadow_groups.push(group);
                frame_shadow_entries.push(entries);
                frame_shadow_selection.extend(tile_selection.get(view).map(|ring| ring[frame]));
            }
            shadow_uniforms.push(frame_shadow_uniforms);
            shadow_groups.push(frame_shadow_groups);
            shadow_group_entries.push(frame_shadow_entries);
            shadow_selection.push(frame_shadow_selection);
        }

        let mesh_set_layouts = [mesh_layout];
        let mesh_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("mesh"),
            bind_group_layouts: &mesh_set_layouts,
            // The camera goes in the uniform buffer above rather than here:
            // `Features::PUSH_CONSTANTS` is absent on Tier B and the seam is
            // explicit that the substitute is a data-layout decision. So this
            // file needs no tier branch at all — it has been on Tier B's shape
            // since P1, and `crate::ui_pass` is where the split actually bites.
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(mesh_pipeline_layout);

        // The colour pass's pipeline and its depth-only twin, out of one set of
        // modules — see [`MeshModules`], which is where the two shapes and the
        // reasons for them live. Both are created before either is unwrapped and
        // the modules are released between, so a failing creation leaks nothing.
        let modules = MeshModules::new(device, emit, culls_clusters)?;
        let mesh_pipeline =
            modules.color_pipeline(device, mesh_pipeline_layout, PolygonMode::Fill, "forward");
        let shadow_pipeline_result = modules.depth_pipeline(device, mesh_pipeline_layout);
        // And the one that resets a tile, which the cadence rung needs and the
        // attachment's load operation cannot express — see
        // [`MeshModules::depth_clear_pipeline`].
        let shadow_clear_result = modules.depth_clear_pipeline(device, mesh_pipeline_layout);
        // And `docs/plan/50-irradiance-probes.md`'s reflective shadow map, built
        // from the same modules and the same layout — see
        // [`MeshModules::rsm_pipeline`]. Built on every scene rather than only
        // on one whose probes ask to be updated: the modules are released a line
        // below and a pipeline built later would need them back, which is a
        // second shader compilation for a field a `ProbeGrid` decides.
        let rsm_result = modules.rsm_pipeline(device, mesh_pipeline_layout);
        modules.destroy(device);
        let mesh_pipeline = mesh_pipeline?;
        rollback.pipelines.push(mesh_pipeline);
        let shadow_pipeline = shadow_pipeline_result?;
        rollback.pipelines.push(shadow_pipeline);
        let shadow_clear_pipeline = shadow_clear_result?;
        rollback.pipelines.push(shadow_clear_pipeline);
        let rsm_pipeline = rsm_result?;
        rollback.pipelines.push(rsm_pipeline);

        // --- the tonemap pass ---
        let tonemap_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            // The exposure — see [`set_exposure`](Self::set_exposure). A uniform
            // buffer rather than a push constant, which is [`crate::ui_pass`]'s
            // decision restated: WebGPU has no push constants at all, so a range
            // here would split this pass into a tier A and a tier B form —
            // two layouts, two buffers and, because one Slang entry point reads
            // either a `[[vk::push_constant]]` block or a `[[vk::binding]]`ed
            // one and never both, a second copy of `tonemap.slang`. Four bytes
            // once a frame do not buy that.
            //
            // `FRAGMENT` alone, and the artifact agrees: `spirv/tonemap.spv`
            // lists `%params` in the fragment `OpEntryPoint`'s interface and not
            // in the vertex one, because the vertex stage generates three
            // corners out of `SV_VertexID` and reads nothing.
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            // The measured exposure — see [`crate::exposure`]. **Bound on every
            // frame, whether or not anything measured**, because a binding that
            // comes and goes is a second pipeline layout and a second group
            // cache keyed on which one is live, to save four bytes. The block
            // above carries the switch that decides whether the shader reads it.
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::StorageBuffer {
                    // Written by `exposure.slang`'s reduce and read here —
                    // `StructuredBuffer` in this shader, which is the truth
                    // rather than a hint.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let tonemap_desc = BindGroupLayoutDesc {
            label: Some("tonemap scene"),
            entries: &tonemap_entries,
        };
        check_portable_storage_buffers(Some("tonemap"), &[&tonemap_desc])?;
        let tonemap_layout = device.create_bind_group_layout(&tonemap_desc)?;
        rollback.bind_group_layouts.push(tonemap_layout);
        let tonemap_set_layouts = [tonemap_layout];
        let tonemap_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("tonemap"),
            bind_group_layouts: &tonemap_set_layouts,
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(tonemap_pipeline_layout);
        let tonemap_vertex = entry(&TONEMAP, Stage::Vertex)?;
        let tonemap_fragment = entry(&TONEMAP, Stage::Fragment)?;
        let tonemap_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("tonemap.slang"),
            spirv: TONEMAP.spirv(),
            wgsl: TONEMAP.wgsl(),
            msl: TONEMAP.msl(),
            // A container per entry point, for the reason the mesh module above
            // gives.
            dxil: &TONEMAP.dxil_containers(),
        })?;
        let tonemap_targets = [ColorTargetState::opaque(target_format)];
        let tonemap_pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("tonemap"),
            layout: tonemap_pipeline_layout,
            vertex: ShaderEntry {
                module: tonemap_module,
                entry_point: tonemap_vertex,
            },
            fragment: Some(ShaderEntry {
                module: tonemap_module,
                entry_point: tonemap_fragment,
            }),
            // The full-screen triangle is deliberately oversized, so two of its
            // vertices are outside the viewport and its winding is not worth
            // reasoning about. `CullMode::None` is the honest setting.
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &tonemap_targets,
        });
        device.destroy_shader_module(tonemap_module);
        let tonemap_pipeline = tonemap_pipeline?;
        rollback.pipelines.push(tonemap_pipeline);

        // Nearest, not linear: see `tonemap.slang` on why a 1:1 blit must not
        // leave a golden image depending on two rasterisers agreeing about
        // texel-centre arithmetic.
        let sampler = device.create_sampler(&SamplerDesc {
            label: Some("tonemap scene"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(sampler);

        // The exposure block, one per frame in flight for the frame uniforms'
        // reason exactly — the previous frame may still be reading last frame's
        // while this one is written. See the module docs on the ring.
        let mut tonemap_uniforms = Vec::with_capacity(instance_buffers.len());
        for _ in 0..instance_buffers.len() {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("tonemap params"),
                size: tonemap::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(buffer);
            tonemap_uniforms.push(buffer);
        }

        // --- the screen-space occlusion pair ---
        //
        // Stored in the rollback whole, like the light grid: it owns two
        // pipelines and a ring of buffers, and `Ssao::destroy` is the one place
        // their release order lives.
        rollback.ssao = Some(Ssao::new(
            device,
            instance_buffers.len(),
            Self::build_fullscreen,
        )?);

        // --- the screen-space contact-shadow march ---
        //
        // Stored whole for the pair above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction.
        rollback.contact_shadows = Some(ContactShadows::new(
            device,
            instance_buffers.len(),
            Self::build_fullscreen,
        )?);

        // --- the screen-space reflection march ---
        //
        // Stored whole for the pair above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction.
        rollback.ssr = Some(Ssr::new(
            device,
            queue,
            instance_buffers.len(),
            Self::build_fullscreen,
        )?);

        // --- the Hi-Z pyramid the march climbs ---
        //
        // After the march it serves and before the chain below, on their reason:
        // `Rollback::run` releases in the reverse order of construction. Its
        // pipeline is the only one in this file with a depth attachment and no
        // colour one — see [`Self::build_depth_fullscreen`].
        rollback.hiz = Some(Hiz::new(
            device,
            instance_buffers.len(),
            Self::build_depth_fullscreen,
        )?);

        // --- the froxel volume ---
        //
        // Stored whole for the three above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction. It sits
        // between the march and the chain in the frame as well: the medium is
        // scene content and the chain is a lens. Its volume holds the same
        // [`FROXEL_CAPACITY`] the clustering pass's grid does, because it is
        // subdivided by the same [`Grid`] — see [`crate::volumetric`].
        rollback.volumetric = Some(Volumetric::new(
            device,
            instance_buffers.len(),
            crate::light_grid::FROXEL_CAPACITY,
            shadow_atlas_view,
            shadow_sampler,
            lights,
            Self::build_fullscreen,
        )?);

        // --- auto-exposure ---
        //
        // Stored whole for the volume's reason, and after it because
        // `Rollback::run` releases in the reverse order of construction. It runs
        // between the chain and the tonemap in the frame as well: it bins the
        // picture the tonemap is about to read, which is the one with the lens
        // already on it — see [`crate::exposure`].
        rollback.exposure = Some(Exposure::new(device, queue, instance_buffers.len())?);

        // --- the bloom chain ---
        //
        // Stored whole for the pair above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction. It owns
        // a **linear** sampler of its own; `Self::sampler` above is `Nearest` on
        // purpose and `crate::bloom` says why the chain cannot share it.
        rollback.bloom = Some(Bloom::new(
            device,
            instance_buffers.len(),
            Self::build_fullscreen,
        )?);

        // --- the antialiasing resolve ---
        //
        // Stored whole for the three above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction. It is
        // the one of the four that needs `target_format`: it writes the caller's
        // target where the others write `Rgba16Float` transients of their own
        // choosing — see [`crate::fxaa`]. Its sampler is **linear** for the
        // chain's reason and not the tonemap's.
        rollback.fxaa = Some(Fxaa::new(
            device,
            instance_buffers.len(),
            target_format,
            Self::build_fullscreen,
        )?);

        // --- the higher antialiasing tier ---
        //
        // The same slot, filled by SMAA's three passes instead of FXAA's one —
        // see [`crate::smaa`], which says why the two are built together and at
        // most one recorded. It is the only pass group in this frame that takes
        // `queue`: its two lookup tables are committed bytes and are uploaded
        // here, once, the way the `dfg` table above is.
        rollback.smaa = Some(Smaa::new(
            device,
            queue,
            instance_buffers.len(),
            target_format,
            Self::build_fullscreen,
        )?);

        // --- the render-scale upscale ---
        //
        // The second pass that writes the caller's target rather than a
        // transient of its own, and the last built for `Rollback::run`'s reverse
        // order. It draws on no frame at a render scale of `1.0`, which is every
        // frame until a caller moves it — see [`crate::upscale`].
        rollback.upscale = Some(Upscale::new(
            device,
            instance_buffers.len(),
            target_format,
            Self::build_fullscreen,
        )?);

        // --- the background ---
        //
        // Built last, so `Rollback::run` releases it first. It writes the scene
        // target rather than the caller's — it is scene content, drawn before
        // the operator, unlike the ground grid — and takes the depth format as
        // well, because it is the one full-screen pass in this frame that tests
        // against an attachment. See [`crate::sky_pass`].
        rollback.sky_pass = Some(SkyPass::new(
            device,
            instance_buffers.len(),
            Format::Rgba16Float,
            Format::D32Float,
            Self::build_tested_fullscreen,
        )?);

        // --- the shadow atlas viewer ---
        //
        // Built last, so `Rollback::run` releases it first. It writes the
        // caller's target rather than a transient — it replaces the picture the
        // tonemap put there, which is [`crate::atlas_view`]'s argument — and it
        // records on no frame but the one that resolved
        // [`DebugView::ShadowAtlas`].
        rollback.atlas_viewer = Some(AtlasView::new(
            device,
            instance_buffers.len(),
            target_format,
            Self::build_fullscreen,
        )?);

        Ok(Self {
            pool: rollback
                .pool
                .take()
                .unwrap_or_else(|| unreachable!("the pool was placed in the rollback above")),
            bucket_constants,
            instances: rollback
                .instances
                .take()
                .unwrap_or_else(|| unreachable!("the pool was placed in the rollback above")),
            mesh_ids,
            skinnable_meshes,
            skinned_instances: Vec::new(),
            materials: rollback
                .materials
                .take()
                .unwrap_or_else(|| unreachable!("the table was placed in the rollback above")),
            material_ids,
            probes: rollback
                .probes
                .take()
                .unwrap_or_else(|| unreachable!("the table was placed in the rollback above")),
            probe_volume: scene.probes.volume,
            base_color_page,
            normal_page,
            base_color_page_extent: (scene.page.extent(), scene.page.extent()),
            base_color_sampler,
            anisotropy,
            // Every slot's groups were just built naming this sampler.
            slot_page_samplers: vec![base_color_sampler; FRAMES_IN_FLIGHT],
            retired_page_samplers: Vec::new(),
            draws: rollback.draws.take().unwrap_or_else(|| {
                unreachable!("draw generation was placed in the rollback above")
            }),
            // No frame has been recorded yet, and the counters say so rather
            // than reporting the count a frame *would* have.
            recorded_draws: 0,
            emit,
            clusters: rollback.clusters.take(),
            culls_clusters,
            uniforms,
            draw_constants,
            mesh_clusters,
            // One entry per description mesh, each holding one bucket per level,
            // where the cull pass takes a uniform cut — and nothing at all where
            // the amplification stage takes a per-cluster one. Read off the
            // bucket table that was actually built rather than re-derived from
            // the path.
            mesh_level_buckets: if emit.is_mesh() {
                Vec::new()
            } else {
                residents
                    .iter()
                    .enumerate()
                    .map(|(index, resident)| {
                        let base = bucket_bases[index];
                        (base..base + resident.levels.len())
                            .map(|bucket| {
                                u32::try_from(bucket)
                                    .unwrap_or_else(|_| unreachable!("a table of a few buckets"))
                            })
                            .collect()
                    })
                    .collect()
            },
            cluster_selection,
            lod_error_budget: LOD_ERROR_BUDGET,
            lod_hold_ratio: LOD_HOLD_RATIO,
            // Overwritten by the first `begin_frame`, which is the only thing
            // that can know the viewport. A zero scale with a budget of zero
            // selects nothing at all, and there is no frame yet to select for.
            lod_params: [0.0, 0.0, 0.0],
            shadow_lod_params: [0.0, 0.0, 0.0],
            lights: rollback.lights.take().expect("the light grid was built"),
            extra_lights: Vec::new(),
            // Overwritten by the first `begin_frame` on `lod_params`' terms: a
            // one-froxel grid is the smallest legal one, and there is no
            // viewport yet to size a real one against.
            grid: Grid {
                x: 1,
                y: 1,
                slices: 1,
                tile_pixels: 1,
            },
            mesh_groups,
            frame: 0,
            mesh_layout,
            mesh_pipeline_layout,
            mesh_pipeline,
            // Off, and unbuilt, on the ground grid's terms below: the wireframe
            // view is opt-in, so a caller that never asks for one draws the
            // frame it drew before `set_wireframe` existed.
            wireframe_pipeline: None,
            wireframe_on: false,
            // Off, on the line above's terms: the normals view is opt-in, so a
            // caller that never asks for one draws the frame it drew before
            // `set_normals_view` existed. It builds nothing, so there is no
            // second field here.
            normals_view: false,
            // Off on the same terms, and "off" here is a density of zero: the
            // shader's composite is exactly the identity there, so a caller who
            // never calls `set_fog` gets the frame byte for byte.
            fog: Fog::NONE,
            sky: Sky::NONE,
            lod_view: false,
            heatmap: false,
            occlusion_view: false,
            motion_view: false,
            bent_normal_view: false,
            cascade_view: false,
            atlas_view: false,
            // Following the camera, on the line above's terms: the selection eye
            // is the camera's until a caller pins it, so a renderer nobody calls
            // `set_frozen_selection_eye` on hands `begin_frame` exactly what it
            // handed before this field existed.
            frozen_selection_eye: None,
            shadow_atlas,
            shadow_atlas_view,
            shadow_placeholder,
            shadow_placeholder_view,
            shadow_sampler,
            shadow_pipeline,
            shadow_clear_pipeline,
            rsm_pipeline,
            probe_gather,
            probe_update: scene.probes.update,
            // Overwritten by the first `begin_frame`, which is the only reader's
            // only source. Zero is the honest placeholder: no cascade has been
            // fitted yet, and a gather recorded against it would scale by zero
            // rather than by a number nobody chose.
            cascade_reach: 0.0,
            // Nothing has been fitted before the first `begin_frame`.
            rsm_cull_ready: false,
            sun_color: Vec3::ZERO,
            shadow_draws: std::mem::take(&mut rollback.shadow_draws),
            shadow_uniforms,
            shadow_groups,
            shadow_selection,
            shadow_lights: shadow::Selection::default(),
            // Nothing has drawn the atlas, so the first frame draws every group
            // whatever it is asked for — which is also what gives the image a
            // defined content before anything samples it. A record is never
            // empty, so the first frame's is a change and takes every id past
            // the zero below it.
            shadow_group_inputs: vec![Vec::new(); SHADOW_CULLS],
            shadow_group_id: vec![0; SHADOW_CULLS],
            shadow_pass_id: 0,
            shadow_pass_ran: Arc::new(AtomicU64::new(0)),
            shadow_pending: None,
            // A reach of zero, so every centre on the first frame is further
            // from what the image holds than the nothing it holds reaches —
            // which is the reset that frame should be.
            shadow_group_held: vec![(0, Vec3::ZERO, 0.0); SHADOW_CULLS],
            shadow_group_redrawn: vec![false; SHADOW_CULLS],
            shadow_atlas_layout: None,
            shadow_atlas_cached: false,
            shadow_cadence_reset: false,
            shadow_faces_redrawn: 0,
            shadow_cadence_request: None,
            shadow_atlas_clears: true,
            shadow_frame: 0,
            frame_skins: false,
            // [`FRAMES_IN_FLIGHT`], the number every other ring here is sized
            // by, so a slot has been through the whole loop before it is read.
            // `culls_clusters` rather than the device's feature bits: what
            // decides whether the cluster word is a count is whether *this
            // renderer* built an amplification stage.
            cull_stats: CullStatsRing::new(device, FRAMES_IN_FLIGHT, culls_clusters),
            tonemap_layout,
            tonemap_pipeline_layout,
            tonemap_pipeline,
            sampler,
            tonemap_uniforms,
            tonemap_groups: vec![None; instance_buffers.len()],
            // The value `tonemap.slang`'s `EXPOSURE` constant held, so a
            // renderer nobody has called `set_exposure` on writes the frame it
            // wrote before the block existed.
            exposure: tonemap::DEFAULT_EXPOSURE,
            exposure_adaptation: None,
            // And the operator that constant is the identity under.
            tonemap_curve: tonemap::TonemapCurve::Clamp,
            target_format,
            ambient_occlusion_placeholder,
            contact_shadow_placeholder,
            probe_visibility_placeholder,
            // Nothing has been captured, so every frame drawn before
            // `capture_probe_visibility` reads the placeholder and weighs every
            // probe at one — which is the frame this renderer drew before the
            // maps existed.
            probe_visibility: None,
            probe_occluders,
            specular_dfg,
            ltc_table,
            mesh_group_entries,
            prepass_group_entries,
            shadow_group_entries,
            screen_channel_groups: vec![None; instance_buffers.len()],
            prepass_groups,
            prepass_stats,
            // Off, and unbuilt: the grid is opt-in, so a caller that never asks
            // for one draws the frame it drew before this module existed.
            ground_grid: None,
            ground_grid_on: false,
            // Empty, and unbuilt: nothing has appended a segment, so this
            // renderer records the frame it recorded before the layer existed.
            debug_draw: DebugDraw::default(),
            // Replaced by every `begin_frame`, which `add_passes` documents as
            // having to run first.
            camera_view_proj: Mat4::IDENTITY,
            // No frame has been drawn, so there is no previous camera — see the
            // field, which says why that is not the identity.
            previous_view_projection: None,
            ssao: rollback
                .ssao
                .take()
                .unwrap_or_else(|| unreachable!("the pair was placed in the rollback above")),
            contact_shadows: rollback
                .contact_shadows
                .take()
                .unwrap_or_else(|| unreachable!("the march was placed in the rollback above")),
            hiz: rollback
                .hiz
                .take()
                .unwrap_or_else(|| unreachable!("the pyramid was placed in the rollback above")),
            ssr: rollback
                .ssr
                .take()
                .unwrap_or_else(|| unreachable!("the march was placed in the rollback above")),
            volumetric: rollback
                .volumetric
                .take()
                .unwrap_or_else(|| unreachable!("the volume was placed in the rollback above")),
            auto_exposure: rollback
                .exposure
                .take()
                .unwrap_or_else(|| unreachable!("the histogram was placed in the rollback above")),
            bloom: rollback
                .bloom
                .take()
                .unwrap_or_else(|| unreachable!("the chain was placed in the rollback above")),
            fxaa: rollback
                .fxaa
                .take()
                .unwrap_or_else(|| unreachable!("the resolve was placed in the rollback above")),
            smaa: rollback
                .smaa
                .take()
                .unwrap_or_else(|| unreachable!("the tier was placed in the rollback above")),
            upscale: rollback
                .upscale
                .take()
                .unwrap_or_else(|| unreachable!("the upscale was placed in the rollback above")),
            sky_pass: rollback
                .sky_pass
                .take()
                .unwrap_or_else(|| unreachable!("the sky was placed in the rollback above")),
            atlas_viewer: rollback
                .atlas_viewer
                .take()
                .unwrap_or_else(|| unreachable!("the viewer was placed in the rollback above")),
            // Full resolution, which is the frame every caller of this type drew
            // before there was a knob — see [`Self::set_render_scale`].
            render_scale: 1.0,
            // The default stack the view wants, no quality clamp and no
            // override, which resolves to every effect this device permits but
            // the lens one — the frame every caller of this type drew before
            // there were toggles. See [`RenderEffects::DEFAULT_STACK`].
            effect_request: EffectRequest::default(),
            device_effects: RenderEffects::all(),
            frame_effects: RenderEffects::DEFAULT_STACK,
            // What the switch says today, so a renderer built and never opened
            // still describes the frame it would draw. `begin_frame` reads it
            // again for the frame it opens.
            frame_ssao_blurs: crate::ssao::blur_passes(),
            // No frame has been recorded yet, on `recorded_draws`' terms.
            recorded_fullscreen: 0,
            // No frame has been recorded yet, on `recorded_fullscreen`'s terms.
            recorded_debug_draw: 0,
            recorded_tile_clears: 0,
        })
    }

    /// Builds a full-screen-triangle pipeline out of `shader`'s vertex and
    /// fragment entry points.
    ///
    /// One helper because every post pass in this crate wants the same
    /// pipeline — the tonemap's, `ssao`'s, `ssao-blur`'s, `ssr`'s and the rest —
    /// differing in the module, the layout and the target format alone. The
    /// tonemap's stays written out where it is: it is the one that carries the
    /// *why* of the shape, and this is the shape repeated.
    ///
    /// The module is destroyed before the pipeline result is unwrapped, so a
    /// failing creation leaks nothing.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the manifest lookup, the module or the pipeline.
    fn build_fullscreen(
        device: &dyn Device,
        label: &str,
        shader: &crcbl_shaders::Shader,
        layout: PipelineLayoutHandle,
        color_targets: &[ColorTargetState],
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Self::build_fullscreen_with(device, label, shader, layout, color_targets, None)
    }

    /// [`Self::build_fullscreen`] for a pass that **depth-tests** what it draws
    /// against an attachment it does not write.
    ///
    /// The state is [`DepthStencilState::equal_depth_read_only`], whose
    /// [`CompareOp::GreaterOrEqual`] is the reversed-Z pair for "the fragment's
    /// depth must equal what is already there". One caller, [`crate::sky_pass`],
    /// which emits its triangle at the far plane so that the test selects
    /// exactly the pixels no geometry covered — see that module.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the manifest lookup, the module or the pipeline.
    fn build_tested_fullscreen(
        device: &dyn Device,
        label: &str,
        shader: &crcbl_shaders::Shader,
        layout: PipelineLayoutHandle,
        color_targets: &[ColorTargetState],
        depth_format: Format,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Self::build_fullscreen_with(
            device,
            label,
            shader,
            layout,
            color_targets,
            Some(DepthStencilState::equal_depth_read_only(depth_format)),
        )
    }

    /// The body of the two above, which differ in their depth state and in
    /// nothing else.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the manifest lookup, the module or the pipeline.
    fn build_fullscreen_with(
        device: &dyn Device,
        label: &str,
        shader: &crcbl_shaders::Shader,
        layout: PipelineLayoutHandle,
        color_targets: &[ColorTargetState],
        depth_stencil: Option<DepthStencilState>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let vertex = entry(shader, Stage::Vertex)?;
        let fragment = entry(shader, Stage::Fragment)?;
        let module = device.create_shader_module(&ShaderModuleDesc {
            label: Some(shader.source()),
            spirv: shader.spirv(),
            wgsl: shader.wgsl(),
            msl: shader.msl(),
            // A container per entry point, for the reason the mesh module gives.
            dxil: &shader.dxil_containers(),
        })?;
        let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some(label),
            layout,
            vertex: ShaderEntry {
                module,
                entry_point: vertex,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: fragment,
            }),
            // The triangle is deliberately oversized, so two of its vertices are
            // outside the viewport and its winding is not worth reasoning about.
            primitive: PrimitiveState::default(),
            depth_stencil,
            multisample: MultisampleState::default(),
            color_targets,
        });
        device.destroy_shader_module(module);
        pipeline
    }

    /// Builds a full-screen-triangle pipeline that writes a **depth**
    /// attachment and no colour one.
    ///
    /// [`Self::build_fullscreen`]'s shape with the two halves of that swapped:
    /// no colour target at all, and a depth state whose comparison always
    /// passes so every texel of the destination takes the value the fragment
    /// computed. The reversed-Z [`CompareOp::Greater`] every geometry pipeline
    /// in this file uses would be wrong here — this pass is not depth-testing
    /// anything, it is writing a reduction, and a test against the target's
    /// undefined contents would drop texels at random.
    ///
    /// One caller, [`crate::hiz`], and it stays a method rather than moving
    /// there because the module lookup and the destroy-before-unwrap above are
    /// this file's, and `crate::hiz` takes it in exactly as `crate::ssao` takes
    /// the sibling.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the manifest lookup, the module or the pipeline.
    fn build_depth_fullscreen(
        device: &dyn Device,
        label: &str,
        shader: &crcbl_shaders::Shader,
        layout: PipelineLayoutHandle,
        format: Format,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let vertex = entry(shader, Stage::Vertex)?;
        let fragment = entry(shader, Stage::Fragment)?;
        let module = device.create_shader_module(&ShaderModuleDesc {
            label: Some(shader.source()),
            spirv: shader.spirv(),
            wgsl: shader.wgsl(),
            msl: shader.msl(),
            dxil: &shader.dxil_containers(),
        })?;
        let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some(label),
            layout,
            vertex: ShaderEntry {
                module,
                entry_point: vertex,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: fragment,
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: Some(DepthStencilState {
                format,
                depth_write: true,
                depth_compare: CompareOp::Always,
                stencil: None,
                bias: DepthBias::default(),
            }),
            multisample: MultisampleState::default(),
            color_targets: &[],
        });
        device.destroy_shader_module(module);
        pipeline
    }

    /// Creates the geometry pool and makes every mesh of `scene` resident in it.
    ///
    /// Separate from [`ForwardRenderer::build`] because it is **self-cleaning**:
    /// the pool is not the rollback's until this has returned, so a failure
    /// between creating it and flushing the last upload releases it here.
    fn build_geometry(
        device: &dyn Device,
        queue: QueueHandle,
        scene: &SceneDesc<'_>,
    ) -> Result<(MeshPool, Vec<ResidentMesh>), HalError> {
        let mut pool = MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("forward geometry"),
                vertex_capacity: scene.capacities.vertices,
                index_capacity: scene.capacities.indices,
                mesh_capacity: scene.capacities.meshes,
            },
        )?;
        match Self::residents(device, queue, &mut pool, scene) {
            Ok(residents) => Ok((pool, residents)),
            Err(error) => {
                pool.destroy(device);
                Err(error)
            }
        }
    }

    /// Creates the material table and fills it with `scene`'s rows, returning it
    /// and the id of each.
    ///
    /// Self-cleaning for the same reason [`ForwardRenderer::build_geometry`] is:
    /// the table is not the rollback's until this has returned.
    fn build_materials(
        device: &dyn Device,
        scene: &SceneDesc<'_>,
    ) -> Result<(MaterialTable, Vec<u32>), HalError> {
        let mut materials = MaterialTable::new(
            device,
            &MaterialTableDesc {
                label: Some("forward"),
                capacity: scene.capacities.materials,
            },
        )?;
        match Self::material_rows(device, &mut materials, &scene.materials) {
            Ok(ids) => Ok((materials, ids)),
            Err(error) => {
                materials.destroy(device);
                Err(error)
            }
        }
    }

    /// Inserts `rows` **in order** and returns the id of each.
    ///
    /// Split out of [`ForwardRenderer::build_materials`] only so the table can
    /// be released on a failure without the borrow that filling it takes, which
    /// is the same shape [`ForwardRenderer::residents`] has.
    ///
    /// **In order, because row 0 is not this function's to choose**: it is what
    /// [`mesh::GpuInstance::default`] names, so it is what an instance written
    /// without a material id shades with. Inserting a description's rows in any
    /// other order would move that row under every caller — see
    /// [`SceneDesc::materials`], which is where it is a caller's decision.
    fn material_rows(
        device: &dyn Device,
        materials: &mut MaterialTable,
        rows: &[mesh::GpuMaterial],
    ) -> Result<Vec<u32>, HalError> {
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            let handle = materials.insert(device, row)?;
            // The id is asked for rather than assumed, because the number an
            // instance carries is this one and nothing else knows it.
            ids.push(materials.index(handle).ok_or_else(|| {
                HalError::Backend(
                    "a material inserted into an empty table did not resolve".to_string(),
                )
            })?);
        }
        Ok(ids)
    }

    /// Uploads every mesh of `scene` and returns their mesh ids — **only** once
    /// the transfers have completed.
    ///
    /// The calls are §3.1's upload path in order: the copies are recorded and
    /// submitted against timeline values, [`MeshPool::flush`] is what makes those
    /// values pass, and [`MeshPool::mesh`] is what refuses to hand out a range
    /// before they have.
    ///
    /// **In description order**, which is what makes a mesh's table id a
    /// property of the description rather than of this loop — the second mesh
    /// uploaded is the pool's first resident at a non-zero base vertex, which is
    /// what the module docs call the one thing that can tell a working base
    /// vertex from a cancelled one.
    ///
    /// A DAG is **one upload per level**. Every level was decimated separately,
    /// so a coarser level's vertices are wherever the collapses put them and
    /// belong to no vertex of the level below — a DAG is several vertex ranges,
    /// and the pool suballocates in vertices, so several ranges means several
    /// uploads. Level 0 goes first, so every other level starts past it and the
    /// offsets a cluster carries are non-negative; that is checked rather than
    /// assumed, because the pool's free list makes no promise about where a mesh
    /// lands.
    fn residents(
        device: &dyn Device,
        queue: QueueHandle,
        pool: &mut MeshPool,
        scene: &SceneDesc<'_>,
    ) -> Result<Vec<ResidentMesh>, HalError> {
        let mut uploaded: Vec<Vec<crate::mesh_pool::MeshHandle>> =
            Vec::with_capacity(scene.meshes.len());
        for desc in &scene.meshes {
            match &desc.geometry {
                Geometry::Flat {
                    vertices,
                    indices,
                    uv_range,
                    flags,
                    ..
                } => {
                    uploaded.push(vec![pool.upload(
                        device,
                        queue,
                        &MeshUpload {
                            label: &desc.label,
                            vertices,
                            indices,
                            uv_range: *uv_range,
                            flags: *flags,
                        },
                    )?]);
                }
                Geometry::Dag {
                    levels,
                    dag,
                    uv_range,
                    flags,
                } => {
                    let mut handles = Vec::with_capacity(levels.len());
                    for (depth, (vertices, level)) in levels.iter().zip(&dag.levels).enumerate() {
                        handles.push(pool.upload(
                            device,
                            queue,
                            &MeshUpload {
                                label: &format!("{} level {depth}", desc.label),
                                vertices,
                                indices: &level.indices(),
                                // Level 0's range for every level, which is the
                                // one the mesh path reads whichever level it
                                // draws — see `Geometry::Dag::uv_range`.
                                uv_range: *uv_range,
                                // And the description's flags for every level,
                                // for the same reason: the levels are one
                                // surface, so a tangent frame either describes
                                // all of them or none.
                                flags: *flags,
                            },
                        )?);
                    }
                    uploaded.push(handles);
                }
            }
        }

        pool.flush(device)?;
        // The table index is the only number that leaves here — where the
        // geometry actually is reaches the GPU through the mesh table, and the
        // draw arguments are built from that table by a shader. `MeshPool::mesh`
        // is still asked, because it is the call that refuses a mesh whose
        // upload has not completed, and an id for a mesh that is not there yet
        // would be a draw of nothing.
        let resolve = |handle| match (pool.mesh(handle), pool.table_index(handle)) {
            (Some(_), Some(id)) => Ok(id),
            _ => Err(MeshPoolError::NotResident { handle }),
        };
        let base_vertex = |handle| match pool.mesh(handle) {
            Some(range) => Ok(range.base_vertex),
            None => Err(MeshPoolError::NotResident { handle }),
        };

        let mut residents = Vec::with_capacity(uploaded.len());
        for (desc, handles) in scene.meshes.iter().zip(&uploaded) {
            // **Relative to level 0's base**, because that is the base the
            // instance resolves through its mesh id and the one the stage adds
            // this on top of. A level that landed *below* level 0 would make the
            // sum wrap, so it is refused here rather than drawn as another
            // mesh's vertices. A flat mesh is one level and reads zero.
            let level_zero = base_vertex(handles[0])?;
            let mut vertex_bases = Vec::with_capacity(handles.len());
            for (depth, &handle) in handles.iter().enumerate() {
                let base = base_vertex(handle)?;
                vertex_bases.push(base.checked_sub(level_zero).ok_or_else(|| {
                    HalError::InvalidDescriptor(format!(
                        "{} level {depth} landed at vertex {base}, below level 0's \
                         {level_zero}, so its clusters cannot name their own geometry",
                        desc.label
                    ))
                })?);
            }
            residents.push(ResidentMesh {
                levels: handles
                    .iter()
                    .map(|&handle| resolve(handle))
                    .collect::<Result<_, _>>()?,
                vertex_bases,
                // Matched on the description rather than counted off `handles`,
                // so a DAG decimated to a single level is still refused: what
                // makes a mesh skinnable is that every draw of it reads one
                // vertex run, and that is a property of the geometry rather than
                // of how many levels it happened to arrive with.
                skinnable: matches!(desc.geometry, Geometry::Flat { .. }).then(|| handles[0]),
            });
        }
        Ok(residents)
    }

    /// Rotates to the next frame's uniform buffer and instance buffer, writes
    /// this frame's camera and light into the first, and uploads whatever
    /// changed into the second.
    ///
    /// Must be called once per frame, before [`ForwardRenderer::add_passes`].
    /// Separate from `add_passes` because the write happens on the CPU and the
    /// passes are recorded on the GPU timeline: fusing them would hide the ring
    /// rotation inside a function whose name says nothing about it.
    ///
    /// **It moves nothing.** Every object's transform is the caller's, written
    /// through [`ForwardRenderer::set_instance`] whenever it changes, so the
    /// transfer this records is [`InstancePool::begin_frame`]'s delta and a
    /// frame in which nothing moved uploads no instance bytes at all. The
    /// sandbox's spinning cube is one 80-byte write a frame because the sandbox
    /// spins it; a still scene is free.
    ///
    /// It also writes this frame's **cull parameters** — the camera's frustum,
    /// extracted from the very same view-projection the uniform block carries,
    /// so the pass that decides what is on screen and the pass that draws it
    /// cannot be looking at two different cameras — and zeroes the counters the
    /// two compute passes only ever add to. See [`crate::draw_gen`] on why those
    /// zeroes come from the host.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the uniform buffer, the instance buffer or any of the
    /// draw-generation buffers could not be written.
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        camera: &Camera,
        light: &DirectionalLight,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        // The instance pool owns the ring, so its slot is the frame index the
        // uniform buffer and the bind group below are picked with.
        self.frame = self.instances.rotate();
        self.frame_skins = false;
        self.instances.flush(device)?;
        self.begin_frame_body(device, camera, light, extent)
    }

    /// [`begin_frame`](Self::begin_frame) for a frame that **skins**: the same
    /// call with the skinning plan folded into it.
    ///
    /// It does four things in the one order that is correct, which is why it is
    /// a call and not a note in someone's docs:
    ///
    /// 1. rotates the instance ring, so the frame slot exists;
    /// 2. hands that slot and `ranges` to `skinning`, which validates them,
    ///    uploads the palettes and **moves the ping-pong** — the only place the
    ///    parity ever moves;
    /// 3. points every object placed by
    ///    [`add_skinned_instance`](Self::add_skinned_instance) at the half of its
    ///    region the parity now names;
    /// 4. uploads the instance delta, so those bases are the ones this frame
    ///    draws.
    ///
    /// Do those three and four in the other order and the objects carry the
    /// previous frame's half — a character posed one frame late, every frame,
    /// with nothing to report it. A caller never sees a parity or a table id, and
    /// so cannot get that wrong.
    ///
    /// Build `ranges` with [`SkinnedMesh::skin_range`], one per reserved region
    /// this frame animates. A frame with **no** skinned object still calls this
    /// with an empty slice — see [`Skinning::begin_frame`], whose slot would
    /// otherwise re-dispatch whichever frame last used it.
    ///
    /// [`add_skinned_passes`](Self::add_skinned_passes) is the other half, and
    /// the one that orders the dispatch before the draws.
    ///
    /// # Errors
    ///
    /// Whatever [`Skinning::begin_frame`] refuses — a joint index past its
    /// palette, a binding count that is not the region's vertex count, more than
    /// the pass was built for — and [`SkinningError::Hal`] for anything the seam
    /// refused, this renderer's own uniform and instance writes included.
    ///
    /// # Panics
    ///
    /// If `skinning` was not built with at least [`FRAMES_IN_FLIGHT`] frames:
    /// its rings are indexed by this renderer's frame slot, and
    /// [`Skinning::begin_frame`] refuses a slot it has no buffers for.
    pub fn begin_skinned_frame(
        &mut self,
        device: &dyn Device,
        skinning: &mut Skinning,
        ranges: &[SkinRange<'_>],
        camera: &Camera,
        light: &DirectionalLight,
        extent: (u32, u32),
    ) -> Result<(), SkinningError> {
        self.frame = self.instances.rotate();
        // An empty plan dispatches nothing — see [`Skinning::begin_frame`] — so
        // it leaves the vertex pool where it was, and the shadow atlas with it.
        self.frame_skins = !ranges.is_empty();
        skinning.begin_frame(device, self.frame, ranges)?;
        self.point_skinned_instances(skinning.parity());
        self.instances.flush(device)?;
        self.begin_frame_body(device, camera, light, extent)?;
        Ok(())
    }

    /// Everything a frame's start does once its slot has been chosen and its
    /// instances uploaded — the shared body of [`begin_frame`](Self::begin_frame)
    /// and [`begin_skinned_frame`](Self::begin_skinned_frame).
    ///
    /// # Errors
    ///
    /// [`HalError`] on [`begin_frame`](Self::begin_frame)'s terms.
    fn begin_frame_body(
        &mut self,
        device: &dyn Device,
        camera: &Camera,
        light: &DirectionalLight,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        // This slot's groups first, so everything below binds groups naming
        // the sampler in force — see `adopt_page_sampler`.
        self.adopt_page_sampler(device)?;

        // `docs/plan/39-capabilities.md`'s four layers, applied here and nowhere
        // else. Frozen for the frame because this call and `add_passes` have to
        // agree: the loops below skip a shadow cull's parameter write when
        // shadows are off, and a request changed between the two would dispatch
        // that cull against numbers nothing zeroed.
        self.frame_effects = self.resolved_effects();
        // The occlusion rung's second switch, frozen here for the field's
        // reason. Its sibling — how many planes the march sweeps — needs no
        // field: it goes straight into the uniform block written below, and the
        // block is what `add_passes` records against.
        self.frame_ssao_blurs = crate::ssao::blur_passes();

        // Requests the readback the *last* frame's copy earned and resolves the
        // slot that has come round, which is why it is here rather than beside
        // the copy: a readback covers work already submitted, and last frame's
        // copy was submitted before this call and this frame's has not been
        // recorded yet. No fence, no wait — see [`crate::cull_stats`].
        if let Some(stats) = self.cull_stats.as_mut() {
            stats.begin_frame(device);
        }

        // **The aspect is the target's and everything below it is the internal
        // render extent's.** What a viewer sees is the target: the upscale maps
        // the whole internal image onto the whole of it, so a frame composed for
        // the internal extent's own aspect would be composed for a rectangle
        // nobody looks at. What the rounding of the two extents leaves is a
        // sub-pixel non-squareness in the internal target, which the upscale
        // undoes on the way out.
        //
        // A minimised window reports a zero extent in *either* dimension, and
        // `Projection::matrix` asserts a finite positive aspect. Guarding only
        // the height left `extent.0 == 0` producing `0.0`, which trips that
        // assert and takes the frame loop down with it.
        let aspect = if extent.0 == 0 || extent.1 == 0 {
            1.0
        } else {
            extent.0 as f32 / extent.1 as f32
        };
        // From here down `extent` is the extent this frame is *drawn* at, which
        // is the caller's at a render scale of `1.0` and smaller below it. Every
        // use of it beneath this line sizes something — the cluster grid, the
        // LOD metric's pixel budget, the Hi-Z pyramid's height, the bloom chain,
        // the resolve's texel size — and every one of those wants the extent the
        // pixels are actually at. See [`Self::set_render_scale`].
        let extent = self.internal_extent(extent);
        let direction = light.direction.normalize_or_zero();
        // One matrix, used twice. Recomputing it for the frustum below would be
        // two chances to pass a different aspect ratio, and the failure that
        // produces — geometry culled against a camera the frame does not draw
        // with — is invisible until something at the edge of the screen
        // disappears.
        let view_projection = camera.view_projection(aspect);
        // The same matrix again for the ground grid, whose pass `add_passes`
        // records and which has no camera to ask.
        self.camera_view_proj = view_projection;
        // `docs/plan/25-lod.md`'s two selection numbers, from this frame's
        // viewport and this frame's camera. An orthographic projection has no
        // distance falloff for the metric to divide by, so it selects under a
        // budget nothing satisfies and draws the base level whole — see
        // [`LOD_BUDGET_NONE`].
        let lod_scale = camera.projection.pixels_per_unit(extent.1 as f32);
        let lod_budget = if camera.projection.is_orthographic() {
            LOD_BUDGET_NONE
        } else {
            self.lod_error_budget
        };
        self.lod_params = [lod_scale, lod_budget, lod_budget * self.lod_hold_ratio];
        // **The cascades select from this same camera at this same scale**, and
        // differ from it in the budgets alone — `docs/plan/25-lod.md`'s shadow
        // LOD bias, and see [`SHADOW_LOD_BIAS`] for why the bias is one factor
        // over the whole pass rather than a level count.
        //
        // `LOD_BUDGET_NONE` survives the scaling, which is what an orthographic
        // camera needs: negative infinity times a positive constant is still
        // negative infinity, so a camera with no distance falloff still draws
        // the base level into the shadow map rather than a level chosen by a
        // budget the metric cannot reach.
        self.shadow_lod_params = [
            lod_scale,
            self.lod_params[1] * SHADOW_LOD_BIAS,
            self.lod_params[2] * SHADOW_LOD_BIAS,
        ];
        // Topic 18's cascades. Built from the camera and the light alone, so a
        // frame that culls against them and a fragment that samples through them
        // cannot disagree about where they are.
        let cascades = Cascades::new(camera, direction);
        // **Cascade 0's own reach, kept for the reflective shadow map.** The map
        // is that cascade's box, so how much world one of its texels covers is a
        // function of this number — see `crate::rsm::texel_area`. Read here
        // rather than at pass-record time for the reason the matrices are: a
        // frame that scaled its gather against one fitting while it drew the map
        // through another would be a bounce nothing in the frame could see was
        // wrong.
        self.cascade_reach = cascades.far[0];
        // And the sun the map is drawn through, as the light table carries it.
        // The map holds albedo rather than flux — `RsmOutput` in `mesh.slang`
        // argues why — so this is what the gather multiplies by.
        self.sun_color = light.color;
        let mut shadow_view_proj = [[0.0f32; 16]; shadow::CASCADES];
        for (matrix, cascade) in shadow_view_proj.iter_mut().zip(&cascades.view_proj) {
            *matrix = cascade.to_cols_array();
        }
        // **Topic 18's light list, and the sun is row 0 of it.** Its direction
        // and colour were two fields of the block below until the list existed;
        // `sun_row` normalises exactly as this function did, so the row carries
        // the same bits and `mesh.slang`'s loop over one directional light is
        // the same arithmetic the single-light form performed. The goldens are
        // what say so.
        //
        // Rebuilt every frame rather than kept: the sun arrives per frame, and a
        // cached list would be a second place for it to be stale.
        //
        // **The tile budget is spent first**, because a row carries the tile it
        // was given. Topic 18's rule — projected screen influence, ties by index,
        // an incumbent held until a challenger clearly beats it — lives in
        // `shadow::Selection`, and a light it refuses is a light whose row says
        // `NO_SHADOW_TILE`: it still lights, it just does not occlude.
        self.shadow_lights.update(&self.extra_lights, camera);
        let mut rows = Vec::with_capacity(1 + self.extra_lights.len());
        rows.push(sun_row(light));
        rows.extend(
            self.extra_lights
                .iter()
                .enumerate()
                .map(|(index, extra)| extra.row(self.shadow_lights.base_of(index))),
        );

        // And where each of those maps went in the atlas, out of the allocator
        // the selection above just spent: a scale and an offset per slot, which
        // is the whole of what the sampling side knows about the image's shape.
        // Read once and handed to both blocks that carry it — the frame's and
        // the froxel volume's — because a froxel reading one frame's rectangle
        // against another's matrix would sample the right map's place for the
        // wrong map.
        let atlas_rects = self.shadow_lights.atlas_rects();

        // One matrix per held light tile, and the identity in a free one — a
        // spot fills the one tile it was given and a point light the six from
        // its base, in `shadow::face_axis`' order, which is the order
        // `mesh.slang`'s `point_face` selects between. The identity is not a
        // projection anything samples through — the rows that could name a free
        // tile carry `NO_SHADOW_TILE` — and it is written rather than left stale
        // so a block dumped for debugging says plainly that the tile is empty.
        let mut light_view_proj = [Mat4::IDENTITY.to_cols_array(); shadow::LIGHT_TILES];
        for assignment in self.shadow_lights.slots().iter().flatten() {
            match self.extra_lights.get(assignment.light) {
                Some(Light::Spot(spot)) => {
                    light_view_proj[assignment.base] = shadow::spot_matrix(spot).to_cols_array();
                }
                Some(Light::Point(point)) => {
                    for face in 0..shadow::POINT_FACES {
                        light_view_proj[assignment.base + face] =
                            shadow::point_matrix(point, face).to_cols_array();
                    }
                }
                // A rectangle is refused a tile by `shadow::can_be_shadowed`,
                // so it never holds a slot and never reaches this — the arm is
                // here because the alternative is a wildcard that would also
                // swallow the next kind of light somebody adds.
                Some(Light::Rect(_)) | None => {}
            }
        }

        // The grid this frame's viewport and camera get. An orthographic camera
        // has no view depth to slice by — its `clip.w` is 1 everywhere — so it
        // runs with one slice, which `light_cluster.slang` builds a different
        // way rather than pretending it is a perspective frustum.
        let perspective = !camera.projection.is_orthographic();
        self.grid = Grid::for_frame(extent, perspective, self.lights.froxel_capacity());
        self.lights.begin_frame(
            device,
            self.frame,
            &rows,
            self.grid,
            FrameView {
                extent,
                view_projection,
                eye: camera.eye,
                perspective,
            },
        )?;
        // The froxel volume's block: the same grid, the same camera and the
        // medium. **The same `Grid` value the clustering pass was just given**,
        // not a second `Grid::for_frame` — the composite converts a pixel to a
        // froxel index with these numbers, and two grids would have it read a
        // column built for somewhere else.
        //
        // Written whether or not this frame adds the passes, on every other
        // block's terms: one written only on the frames that draw is stale on
        // the frame a caller first switches the effect on.
        self.volumetric.begin_frame(
            device,
            self.frame,
            self.grid,
            FrameView {
                extent,
                view_projection,
                eye: camera.eye,
                perspective,
            },
            Medium {
                fog: self.fog,
                sun: light,
                cascades: &cascades,
                light_view_proj: &light_view_proj,
                atlas_rects: &atlas_rects,
            },
        )?;

        // The frame's gradient, resolved once and handed to all three of the
        // things that want a sky: the L1 projection below for the ambient term,
        // the march's block for a reflection that hit nothing, and the pass that
        // draws the background. One `SkyGradient` rather than three readings of
        // the field, so the sky a surface is lit by cannot differ from the one
        // behind it.
        let gradient = self.sky.gradient();
        // Projected once per frame on the host, which is also why the shading
        // rule that governs `mesh.slang` has nothing to say about it: these
        // coefficients reach every backend as uploaded numbers.
        let sky = gradient.irradiance();
        let uniforms = mesh::FrameUniforms {
            view_proj: view_projection.to_cols_array(),
            camera_position: camera.eye.extend(1.0).to_array(),
            // The ambient's `w` is the normals view's switch — see
            // `set_normals_view` and the constants it names. A renderer nobody
            // has called that on writes the `0.0` this line has always written,
            // which is what makes every golden image untouched by the feature.
            ambient: light.ambient.extend(self.debug_view_lane()).to_array(),
            shadow_view_proj,
            cascade_far: cascades.far,
            shadow_params: Cascades::params(),
            cluster_grid: self.grid.to_frame_block(),
            light_view_proj,
            // The scene's grid, unchanged since `with_scene` read it: the
            // probes are static and nothing here varies them per frame. A
            // description with no probes leaves the default, which evaluates to
            // exactly zero in the shader.
            probes: self.probe_volume,
            // The very numbers the draw-argument pass selected under, carried
            // into the geometry stage so the screen-error heatmap shades by the
            // metric the cut was chosen with rather than by a second derivation
            // of it. `w` is padding — the block's rows are sixteen bytes wide
            // whatever is in them.
            lod_params: [
                self.lod_params[0],
                self.lod_params[1],
                self.lod_params[2],
                0.0,
            ],
            // `w` is padding on both rows — the block's rows are sixteen bytes
            // wide whatever is in them. A renderer nobody called `set_fog` on
            // writes a zero density here, which the fragment stage composites
            // as the identity.
            //
            // **Zeroed on a frame that runs the froxel volume**, which
            // integrates the same medium along the same rays: leaving it would
            // charge the air twice, once here and once in
            // `volumetric_composite.slang`, and the frame would be plausibly
            // over-fogged rather than wrong in a way anything reports. See
            // [`RenderEffects::VOLUMETRIC_FOG`].
            fog_params: [
                if self.frame_effects.contains(RenderEffects::VOLUMETRIC_FOG) {
                    0.0
                } else {
                    self.fog.density
                },
                self.fog.falloff,
                self.fog.reference_height,
                0.0,
            ],
            fog_color: self.fog.color.extend(0.0).to_array(),
            sky_sh_r: sky.sh_r,
            sky_sh_g: sky.sh_g,
            sky_sh_b: sky.sh_b,
            // **Last frame's camera, and this frame's on the first frame** —
            // see the field, which says why the fallback is this matrix rather
            // than the identity.
            previous_view_proj: self
                .previous_view_projection
                .unwrap_or(view_projection)
                .to_cols_array(),
            // Where the pool's attribute region begins, which is the one thing
            // about `docs/plan/43-render-standards.md` §2's two streams that a
            // shader cannot derive: it is the pool's *capacity*, and a shader
            // has never been told that. Fixed for the life of the pool and
            // written every frame anyway, because this is the block that
            // carries it and there is no cheaper place. `yzw` are padding.
            vertex_pool: [self.pool.attribute_base(), 0, 0, 0],
            // Where each atlas slot's map went, out of the allocator
            // `shadow::Selection::update` just spent — the *whole* of what the
            // sampling side knows about the atlas's shape, so a map is read
            // from the rectangle it was rendered into whatever size that was.
            shadow_atlas_rect: atlas_rects,
            // Which filter each side of the comparison seam samples through,
            // and where the seam falls — see `crate::split`, whose header
            // carries why a scene pass selects per pixel where a full-screen
            // effect records itself twice.
            //
            // **The column comes from `split::halves`' left rectangle** rather
            // than from a second multiplication here, so the seam module stays
            // the one place a fraction becomes a column: a frame comparing
            // nothing carries the console's own filter in both lanes and a zero
            // column, which is a row of zeroes at the default and therefore the
            // frame every golden was blessed as.
            shadow_filter: {
                let near = shadow::filter().mode();
                let seam = shadow::split_at().and_then(|at| crate::split::halves(extent, at));
                let (far, column) = seam.map_or((near, 0), |(left, _)| {
                    (shadow::shipped_filter().mode(), left.width)
                });
                [near, far, column, 0]
            },
        };
        // Advanced here, between writing the block that reads it and anything
        // else that could look at it: this function runs exactly once per
        // `InstancePool::rotate`, so the camera's history moves on the same
        // frame boundary the instances' does. See the field.
        self.previous_view_projection = Some(view_projection);
        device.write_buffer(self.uniforms[self.frame], 0, &uniforms.to_bytes())?;

        // The extent auto-exposure bins, which is the internal one this
        // function has been working in since `internal_extent` — the histogram
        // reads the scene target, not the caller's window.
        //
        // Written whether or not this frame adds the passes, on every other
        // block's terms: one written only on the frames that measure is stale on
        // the frame a caller first switches the effect on.
        self.auto_exposure
            .begin_frame(device, self.frame, extent, self.exposure_adaptation)?;

        // The tonemap's one number, written here rather than in `add_passes` for
        // every other block's reason: a pass body runs at execute time, and the
        // buffer it reads has to have been written before the frame was
        // submitted.
        device.write_buffer(
            self.tonemap_uniforms[self.frame],
            0,
            &tonemap::TonemapParams {
                exposure: self.exposure,
                curve: self.tonemap_curve,
                // **The switch, and it is this frame's effects rather than the
                // caller's request**: a device that refused the effect draws the
                // frame with the number a caller set, and reading a buffer no
                // pass wrote would be reading whatever the last frame in this
                // slot left there.
                auto_exposure: self.frame_effects.contains(RenderEffects::AUTO_EXPOSURE),
            }
            .to_bytes(),
        )?;

        // `docs/plan/18-render-features.md`'s occlusion block. **The projection
        // alone, not the view-projection**: the occlusion integral asks what is
        // near a surface, and view space is where "near" is isotropic and the eye
        // is at the origin — a world-space reconstruction would put the camera
        // somewhere else every frame and the hemisphere would have to be rotated
        // into it for no gain.
        //
        // `inverse` here rather than a hand-derived unprojection: an infinite
        // reversed-Z perspective and a reversed orthographic box do not share a
        // closed form, and the two matrices this pass needs are then provably
        // each other's inverse rather than two derivations that agree today.
        let projection = camera.projection.matrix(aspect);
        let inv_projection = projection.inverse();
        self.ssao.begin_frame(
            device,
            self.frame,
            ssao::SsaoParams {
                inv_proj: inv_projection.to_cols_array(),
                proj: projection.to_cols_array(),
                // View → world, for the bent direction alone: the gather works
                // in view space and its one output has to reach `mesh.slang`'s
                // world-space ambient term. The same expression the reflection
                // pass's block carries, a few lines below.
                inv_view: camera.view().inverse().to_cols_array(),
                radius: crate::ssao::radius(),
                slices: crate::ssao::slice_count(),
                intensity: crate::ssao::intensity(),
                bent_normals: crate::ssao::bent_normals(),
            },
        )?;
        // The contact march's block: the same two matrices, and the sun in view
        // space.
        //
        // **Rotated here rather than in the shader**, because the shader has no
        // view matrix and no world-space anything: it walks a view-space ray in
        // screen space, so the one rotation the frame needs happens once on the
        // host instead of once per covered pixel. `Mat4::transform_vector3`
        // takes the direction through the rotation and drops the translation,
        // which is what a direction wants.
        //
        // `normalize_or_zero` for `sun_row`'s reason: a caller may hand this a
        // zero direction, and the march reads a zero as "the sun is along the
        // view axis at every pixel" and reports lit — which is the frame a
        // scene with no sun should draw.
        let to_light = camera
            .view()
            .transform_vector3(light.direction.normalize_or_zero())
            .normalize_or_zero();
        self.contact_shadows.begin_frame(
            device,
            self.frame,
            contact_shadows::ContactShadowParams {
                inv_proj: inv_projection.to_cols_array(),
                proj: projection.to_cols_array(),
                to_light: to_light.to_array(),
            },
        )?;
        // The reflection march's block: the same two matrices and nothing else.
        // Its own buffer rather than the pair's — see [`crate::ssr::Ssr`] — and
        // the same pair of `glam` values, so a march and an occlusion sample can
        // never be looking at two different cameras.
        self.ssr.begin_frame(
            device,
            self.frame,
            ssr::SsrParams {
                inv_proj: inv_projection.to_cols_array(),
                proj: projection.to_cols_array(),
                inv_view: camera.view().inverse().to_cols_array(),
                probe_volume: self.probe_volume,
                // How far up the pyramid the march may climb, which is the
                // pyramid this extent has — `add_passes` records the reduction
                // whenever it records the march, so the two never disagree. A
                // frame too small to halve gets zero, and the march walks the
                // prepass at full resolution.
                hiz_levels: crate::hiz::levels_for(extent),
                // The gradient itself rather than the projection the frame
                // block carries: this pass wants the radiance along one
                // direction and `Sky::gradient` is what has it exactly. A
                // renderer nobody called `set_sky` on writes three zero rows,
                // which the march adds to its probe fallback and changes
                // nothing.
                sky: gradient.rows(),
            },
        )?;
        // The background pass's block: the same two inverses `SsrParams` above
        // carries and the same three gradient rows, so a reflection that missed
        // and the sky behind it cannot be looking at two different skies.
        // Written whether or not this frame adds the pass, on the blocks below's
        // terms — a block written only on the frames that draw one is stale on
        // the frame a caller first calls `set_sky`.
        self.sky_pass.begin_frame(
            device,
            self.frame,
            inv_projection.to_cols_array(),
            camera.view().inverse().to_cols_array(),
            &gradient,
        )?;
        // The bloom chain's blocks, one row per step: each step needs the texel
        // size of the image it reads, and the chain's shape is a function of the
        // extent alone. Written whether or not the chain is in this frame, on
        // the two blocks above's terms — a row nobody reads costs sixteen bytes.
        self.bloom.begin_frame(device, self.frame, extent)?;
        // The resolve's block: the reciprocal of the extent and three constants.
        // Written on the chain's terms above — a frame that adds no resolve pays
        // for twenty bytes nobody reads, and a block written only on the frames
        // that use it is a block that is stale on the frame a caller switches
        // the effect on.
        self.fxaa.begin_frame(device, self.frame, extent)?;
        // The higher tier's block: the same extent and its reciprocal, written
        // on the resolve's terms above. Sixteen bytes, and a frame that resolves
        // through the other tier pays for them and reads none.
        self.smaa.begin_frame(device, self.frame, extent)?;
        // The upscale's block: the internal extent and its reciprocal. Written
        // on the two above's terms — a frame at full scale adds no upscale pass
        // and pays for sixteen bytes nobody reads, and a block written only on
        // the frames that use it is stale on the frame a caller moves the knob.
        self.upscale.begin_frame(device, self.frame, extent)?;
        // The atlas viewer's block: where the atlas is letterboxed into this
        // frame, and where each slot's map is inside it. Written on the four
        // above's terms — a frame not drawing the view pays for the write and
        // reads none of it, and a block written only on the frames that draw it
        // is stale on the frame a caller switches the view on.
        //
        // **`atlas_rects` and not a second derivation of it.** These are the
        // rectangles the frame block above carries, so the grid the viewer draws
        // is the one the sampling side reads through; a viewer that worked its
        // own out would be a diagnostic supplying its own evidence.
        self.atlas_viewer.begin_frame(
            device,
            self.frame,
            extent,
            shadow::atlas_extent(),
            atlas_rects,
        )?;

        // `docs/plan/07-ui-debug.md` item 5's immediate-mode buffer: whatever
        // any system appended since the last frame, uploaded and cleared here.
        // **The camera this frame is drawn with**, not a second derivation of
        // it — the ground grid's pass takes the same matrix for the same
        // reason.
        //
        // A frame with nothing appended, or one with the console switch off,
        // uploads nothing and leaves the slot's count at zero, which is what
        // makes `add_frame_passes` record no pass at all. See
        // [`crate::debug_draw`].
        let frames = self.uniforms.len();
        self.debug_draw
            .begin_frame(device, frames, self.frame, view_projection)?;

        // Every element the pool has ever handed out, not its live count: a
        // removed instance leaves a hole and the live ones above it still have
        // to be tested. `InstancePool::slot_count` carries the difference.
        let instance_count = self.instances.slot_count();
        // The camera and the two selection numbers go to the cull/draw-argument
        // pair as well as into the block above, and they are handed over rather
        // than re-derived: `docs/plan/25-lod.md`'s uniform cut runs there, the
        // mesh path's per-cluster descent runs off the block, and a frame that
        // selected detail against one camera while drawing with another is a
        // difference nothing in the frame can see.
        //
        // **The eye is the selection's, which is the camera's unless a caller
        // pinned it** — `set_frozen_selection_eye`, and the whole of what that
        // feature is. It reaches this parameter block and nothing else: the
        // frustum handed over with it is extracted from this frame's own
        // view-projection, and the frame block written above carries
        // `camera.eye`, so a pinned selection changes which cut is chosen and
        // nothing about what is culled, faced or drawn.
        let selection_eye = self.frozen_selection_eye.unwrap_or(camera.eye);
        self.draws.begin_frame(
            device,
            self.frame,
            &Frustum::from_view_projection(view_projection),
            instance_count,
            [selection_eye.x, selection_eye.y, selection_eye.z],
            self.lod_params,
        )?;

        // One cull per cascade and per **occupied** light slot, against that
        // view's own frustum. The orthographic box gives
        // `Frustum::from_view_projection` six real planes — unlike the camera's
        // infinite perspective, whose far plane is degenerate on purpose — so a
        // caster outside the cascade is rejected before it costs a vertex. A
        // spot's perspective box is finite too, and gives six real planes for
        // the same reason.
        //
        // **A point light culls against its sphere rather than against a
        // matrix**, which is topic 18's fourth decision: its six faces have six
        // matrices and one visible set between them, so what they share is the
        // reach of the light. `shadow::point_frustum` is that box.
        //
        // A free slot takes neither a write nor a dispatch: nothing samples
        // through its tiles, and `add_shadow_pass` records no pass for it
        // either, so they keep the reversed-Z clear the pass wrote.
        //
        // **A frame with [`RenderEffects::SHADOWS`] off is every slot free**,
        // and the whole block below is skipped for the same reason — the atlas
        // is cleared and nothing draws into it, so there is no cull to parametrise
        // and no view to write a matrix for. That is what makes the switch cost
        // nothing rather than costing the culls and throwing them away.
        //
        // **The camera as the eye handed to `begin_frame`, not the light**, and
        // the two are deliberately different questions asked of one pass.
        //
        // The block each view writes puts the light at `camera_position` because
        // the amplification stage's *normal cone* test asks which way a cluster
        // faces relative to the viewer, and a shadow map's viewer is the light.
        // Detail is not that question. A directional sun has no position for a
        // distance metric to measure from — the point below is the camera's own
        // eye pushed along the sun's direction, so a "distance to the light"
        // taken from it is a fact about the camera wearing the light's name, and
        // it steps discontinuously from one cascade to the next because
        // `cascades.far` does.
        //
        // What a coarser caster actually costs is a shadow edge displaced by the
        // group's error, and that displacement is **seen by the camera**, at the
        // camera's pixels per unit and the camera's distance. So the budget is
        // denominated in the camera's pixels, and the eye that makes the metric
        // mean that is the camera's. The bias above is then a statement about
        // shadows rather than a side effect of where a light was placed — and it
        // is the same statement for a spot or a point light, whose maps are
        // looked at through the camera's pixels just as a cascade's is.
        // `docs/plan/45-shadows.md`'s static-caching rung, and the whole of the
        // decision it makes: everything below is assembled first and written
        // only if it differs from what the atlas was last *drawn* from. See
        // [`ForwardRenderer::shadow_atlas_record`] for what the comparison
        // covers and what it deliberately does not.
        let shadows = self.frame_effects.contains(RenderEffects::SHADOWS);
        // The cascades select for the camera, so they take the camera's
        // selection eye — pinned along with the colour pass's, or a frozen cut
        // would draw under a shadow silhouette that was still following the
        // reviewer around.
        let eye = [selection_eye.x, selection_eye.y, selection_eye.z];
        let view_block = |view_proj: Mat4, from: Vec3| -> mesh::FrameUniforms {
            // The spread carries the normals view's switch into these blocks too,
            // and nothing reads it: `MeshModules::depth_pipeline` names no
            // fragment stage at all, so the atlas is filled by the geometry
            // stages alone whichever view the colour pass is drawing.
            mesh::FrameUniforms {
                view_proj: view_proj.to_cols_array(),
                camera_position: from.extend(1.0).to_array(),
                // **This view's budgets, not the camera's**, which is the pair
                // its own draw generator selected under —
                // [`shadow_lod_params`](Self::shadow_lod_params). Nothing reads
                // the colour these stages produce, because `depth_pipeline`
                // names no fragment stage; carrying the camera's numbers here
                // would still be a block that says a cascade selected under a
                // budget it did not.
                lod_params: [
                    self.shadow_lod_params[0],
                    self.shadow_lod_params[1],
                    self.shadow_lod_params[2],
                    0.0,
                ],
                // **This view's own matrix, so the interpolant is zero motion**
                // rather than a reprojection through the camera. Nothing reads
                // it: `depthVertexMain` never looks at it, and where the mesh
                // stage draws these tiles instead, the previous clip position it
                // emits dies with the fragment stage `depth_pipeline` does not
                // name. Carrying the camera's matrix would still be a block that
                // says a cascade moved the way the viewer did.
                previous_view_proj: view_proj.to_cols_array(),
                // **Not the console's filter row.** `depth_pipeline` names no
                // fragment stage, so nothing drawing these views reads it — and
                // the block's bytes are the atlas cache's record of what a map
                // was drawn from. Carried, the row made `r_shadow_filter` and
                // `r_shadow_split` redraw every map in the atlas, and made the
                // tests that move them redraw it under the cache tests in this
                // crate's own process. Zeroed, a sampling knob is a sampling
                // knob. `a_moved_shadow_filter_leaves_a_still_atlas_alone` holds
                // it there.
                shadow_filter: [0; 4],
                ..uniforms
            }
        };
        // Which view gets which block, and which cull is parametrised with
        // which frustum — **built before either is written**, because the
        // record below has to be exactly what the atlas would be drawn from
        // rather than a second derivation of it. A frame with shadows off fills
        // neither: nothing draws into the atlas, so the pass's clear is the
        // whole of what it will hold.
        //
        // **Grouped**, since the cadence rung: a group is a cascade or a light
        // slot's whole run of tiles, it is what one cull covers, and it is what
        // [`shadow::Cadence`] schedules. `regions` is the tier each group sits
        // on, the centre its maps are projected from and how far they reach —
        // the three things the schedule and the reset are decided from.
        let mut views: Vec<(usize, usize, mesh::FrameUniforms)> = Vec::with_capacity(SHADOW_VIEWS);
        let mut culls: Vec<(usize, Frustum)> = Vec::with_capacity(SHADOW_CULLS);
        let mut regions: [Option<(usize, Vec3, f32)>; SHADOW_CULLS] = [None; SHADOW_CULLS];
        // One shadowed light slot's per-face matrices and the frustum its one
        // cull runs against, read out of the selection made above.
        //
        // **The one derivation both paths below read.** The atlas's fitting
        // loop takes it, and so does the shadows-off branch that keeps
        // `docs/plan/50-irradiance-probes.md`'s punctual producer fed — so a
        // face drawn into the atlas and the same face drawn into the reflective
        // shadow map cannot be through different matrices.
        let slot_matrices = |held: shadow::Assignment, light: &Light| -> (Vec<Mat4>, Frustum) {
            let faces = (0..shadow::tile_span(light))
                .map(|face| Mat4::from_cols_array(&light_view_proj[held.base + face]))
                .collect();
            let frustum = match light {
                Light::Point(point) => shadow::point_frustum(point),
                // A rectangle holds no slot, for the reason the
                // `light_view_proj` fill above gives, so it cannot reach either
                // caller; its tile's identity matrix is what this would cull
                // against if it did.
                Light::Spot(_) | Light::Rect(_) => Frustum::from_view_projection(
                    Mat4::from_cols_array(&light_view_proj[held.base]),
                ),
            };
            (faces, frustum)
        };
        if shadows {
            for (cascade, region) in regions.iter_mut().take(shadow::CASCADES).enumerate() {
                let view_proj = cascades.view_proj[cascade];
                let centre = camera.eye + direction * cascades.far[cascade];
                views.push((cascade, cascade, view_block(view_proj, centre)));
                culls.push((cascade, Frustum::from_view_projection(view_proj)));
                // **The cascade's own index is its tier**, which is the cadence
                // rung's own words: the near cascade is re-rendered every frame
                // and each one out doubles the period. Its reach is the sphere
                // it was fitted to — see [`shadow::Cascades::far`] — so an eye
                // that has moved further than that is an eye whose new sphere
                // the held map does not reach at all.
                *region = Some((cascade, centre, cascades.far[cascade]));
            }
            for (slot, held) in self.shadow_lights.slots().iter().enumerate() {
                let Some(held) = held else {
                    continue;
                };
                // A selection's indices are into the list it was run over, which
                // is this one — so this is a resolution rather than a check.
                // Skipped rather than asserted all the same, because the
                // alternative to a frame with one shadow missing is no frame at
                // all.
                let Some(light) = self.extra_lights.get(held.light) else {
                    continue;
                };
                let group = shadow_cull(slot);
                let (faces, frustum) = slot_matrices(*held, light);
                for (face, view_proj) in faces.into_iter().enumerate() {
                    views.push((
                        group,
                        shadow_view(slot, face),
                        view_block(view_proj, light.sphere().0),
                    ));
                }
                culls.push((group, frustum));
                // **The tile's own level is the tier**, so `shadow::coverage`
                // decides how often a light's map is redrawn exactly as it
                // decides how large that map is — one scorer, read twice, which
                // is what the priority rung established. The reach is the
                // light's own radius: a light that has moved further than that
                // lights somewhere its held map says nothing about.
                let (centre, reach) = light.sphere();
                regions[group] = Some((held.level, centre, reach));
            }
        }

        // The frame the cadence is keyed to, and the one thing in this whole
        // decision that is not a function of the scene.
        self.shadow_frame = self.shadow_frame.wrapping_add(1);

        // The pass recorded last, if its body ran: what it drew is now what the
        // image holds. Applied here rather than where it was recorded, for
        // [`ForwardRenderer::shadow_pass_ran`]'s reason — a frame whose graph
        // was refused left the image exactly as it was.
        if let Some(pending) = self.shadow_pending.take()
            && self.shadow_pass_ran.load(Ordering::Relaxed) == pending.id
        {
            for (group, held) in pending.groups.iter().enumerate() {
                if let Some(held) = *held {
                    self.shadow_group_held[group] = held;
                }
            }
            if pending.layout.is_some() {
                self.shadow_atlas_layout = pending.layout;
            }
        }

        // Where every map lands this frame. **A frame with shadows off is one
        // more layout rather than a second kind of state**: the atlas holds a
        // clear instead of maps, so its layout is the empty one, and the
        // comparison below is what makes the frame that switches shadows off
        // clear the image once and the frames after it cost nothing.
        let layout: Vec<shadow::TileRect> = (0..shadow::TILES)
            .map(|slot| {
                if shadows {
                    self.shadow_lights.atlas_rect(slot)
                } else {
                    shadow::TileRect::EMPTY
                }
            })
            .collect();
        // **A layout that has moved resets the whole atlas.** A tile left over
        // from a different layout is texels nothing this frame can account for,
        // and the only clear this seam has covers the whole attachment — so such
        // a frame clears everything and redraws every group, with no cadence and
        // no budget deciding otherwise.
        let relaid = self.shadow_atlas_layout.as_ref() != Some(&layout);

        // What each group would be drawn from, and which of them the image is
        // already holding.
        let mut wanted: [Option<shadow::Group>; SHADOW_CULLS] = [None; SHADOW_CULLS];
        for (group, region) in regions.iter().enumerate() {
            let Some((tier, centre, _)) = *region else {
                continue;
            };
            let record = self.shadow_group_record(group, &views, &culls, eye, instance_count);
            if record != self.shadow_group_inputs[group] {
                self.shadow_group_inputs[group] = record;
                self.shadow_group_id[group] = self.shadow_group_id[group].wrapping_add(1);
            }
            let (held_id, held_centre, held_reach) = self.shadow_group_held[group];
            // A skinned frame's vertices are written by a compute pass into the
            // pool, so nothing on the host can tell this group's maps from the
            // last frame's — see [`ForwardRenderer::frame_skins`].
            //
            // **No group is held while the probe updater is on**, and that is a
            // correctness rule rather than a cost: the reflective shadow maps
            // are drawn from these groups' own draws, and the probe table is a
            // ring of `FRAMES_IN_FLIGHT` buffers. A frame that skipped one
            // producer would write *this frame's* slot without it, and the next
            // frame's would have it again — so the room would light and unlight
            // on alternate frames. It is cascade 0's rule extended to the light
            // slots by the punctual producer, which draws through their views
            // exactly as the sun's half draws through cascade 0's.
            //
            // Redrawing a held group costs its tiles and draws exactly the
            // texels they already held, so the atlas is unchanged; what it buys
            // is a map for the gather to read.
            let updater_needs_it = self.probe_update_runs();
            if !relaid
                && !self.frame_skins
                && !updater_needs_it
                && held_id == self.shadow_group_id[group]
            {
                continue;
            }
            wanted[group] = Some(shadow::Group {
                faces: views.iter().filter(|(owner, _, _)| *owner == group).count(),
                tier,
                // The reset, per group: a map whose centre has moved further
                // than its own reach is about somewhere else rather than a
                // frame out of date. A group the image has never held reads as
                // forced too, its reach being zero, which is the right answer
                // for the first frame of all.
                forced: relaid || centre.distance(held_centre) > held_reach,
            });
        }

        // **The reset spends no budget**: a relaid atlas has nothing to hold, so
        // every group it still has is redrawn whatever the console says.
        let cadence = if relaid {
            shadow::Cadence::EVERY_FRAME
        } else {
            self.shadow_cadence_request
                .unwrap_or_else(shadow::Cadence::from_console)
        };
        let redraw = cadence.schedule(self.shadow_frame, &wanted);
        self.shadow_group_redrawn.copy_from_slice(&redraw);
        self.shadow_faces_redrawn = wanted
            .iter()
            .enumerate()
            .filter(|(group, _)| redraw[*group])
            .filter_map(|(_, want)| *want)
            .map(|want| u32::try_from(want.faces).unwrap_or(u32::MAX))
            .sum();
        self.shadow_cadence_reset = wanted.iter().flatten().any(|want| want.forced);
        // **Whether the pass clears the whole attachment or loads it.** Clearing
        // is what shipped and is what a frame keeping nothing should do — every
        // group it draws at all, it redraws. Loading is the cadence's own path,
        // and there each redrawn tile is reset by
        // [`MeshModules::depth_clear_pipeline`] instead.
        self.shadow_atlas_clears = relaid
            || regions
                .iter()
                .enumerate()
                .all(|(group, region)| region.is_none() || redraw[group]);
        // A relaid atlas records its pass even with nothing to draw into it: the
        // clear *is* the frame's answer, and it is what a frame that switched
        // shadows off leaves behind for anything that samples the maps.
        // **A frame drawing without `RenderEffects::SHADOWS` still fits cascade
        // 0 while the probe updater is on.** The reflective shadow map is that
        // cascade's box and its draws, and it is not the atlas: nothing here
        // puts cascade 0 into `views` or `regions`, so no tile is written and
        // the switch goes on removing every shadow in the frame. What it buys is
        // that switching shadows off does not also switch the *bounce* off,
        // which would make the sun's direct term and its indirect one one
        // toggle — and `apps/lantern`'s `every_effect_toggles_and_the_frame_says_so`
        // is what says a floor already in full sun must not move when the
        // shadows go.
        let updater_drives_cascade_zero = !shadows && self.probe_update_runs();
        self.shadow_atlas_cached =
            !relaid && !redraw.iter().any(|drawn| *drawn) && !updater_drives_cascade_zero;
        if self.shadow_atlas_cached {
            // Nothing was fitted, so there is nothing for the map to be drawn
            // from — see [`ForwardRenderer::rsm_cull_ready`].
            self.rsm_cull_ready = false;
            return Ok(());
        }

        self.shadow_pass_id = self.shadow_pass_id.wrapping_add(1);
        self.shadow_pending = Some(ShadowCommit {
            id: self.shadow_pass_id,
            groups: regions
                .iter()
                .enumerate()
                .map(|(group, region)| {
                    let (_, centre, reach) = (*region)?;
                    redraw[group].then_some((self.shadow_group_id[group], centre, reach))
                })
                .collect(),
            layout: self.shadow_atlas_clears.then_some(layout),
        });

        for (group, view, block) in &views {
            if !redraw[*group] {
                continue;
            }
            device.write_buffer(
                self.shadow_uniforms[self.frame][*view],
                0,
                &block.to_bytes(),
            )?;
        }
        for (cull, frustum) in &culls {
            if !redraw[*cull] {
                continue;
            }
            self.shadow_draws[*cull].begin_frame(
                device,
                self.frame,
                frustum,
                instance_count,
                eye,
                self.shadow_lod_params,
            )?;
        }
        // Cascade 0's block and cull on a frame with shadows off, and every
        // shadowed light's beside it, which the two loops above skipped because
        // they are in neither list — see `updater_drives_cascade_zero`. Written
        // straight rather than pushed into `views`/`culls`, because those are
        // what the *atlas* is drawn from and none of this is being drawn into
        // it.
        if updater_drives_cascade_zero {
            let view_proj = cascades.view_proj[0];
            let centre = camera.eye + direction * cascades.far[0];
            device.write_buffer(
                // **Cascade 0's view is view 0.** `shadow_view` numbers a light
                // slot's faces, which start past `shadow::CASCADES` — a cascade
                // is its own index, which is what the loop above pushes.
                self.shadow_uniforms[self.frame][CASCADE_ZERO_VIEW],
                0,
                &view_block(view_proj, centre).to_bytes(),
            )?;
            self.shadow_draws[0].begin_frame(
                device,
                self.frame,
                &Frustum::from_view_projection(view_proj),
                instance_count,
                eye,
                self.shadow_lod_params,
            )?;
            // And the punctual producer's views, on exactly the terms above:
            // the lamp's bounce is drawn through the very faces the atlas would
            // have used, so a frame that fitted neither would switch the
            // *bounce* off with the shadows — which is the one thing the block
            // above exists to prevent, and `apps/lantern`'s frame claim 6 is
            // what would say so.
            for (slot, held) in self.shadow_lights.slots().iter().enumerate() {
                let Some(held) = *held else {
                    continue;
                };
                let Some(light) = self.extra_lights.get(held.light) else {
                    continue;
                };
                let (faces, frustum) = slot_matrices(held, light);
                for (face, view_proj) in faces.into_iter().enumerate() {
                    device.write_buffer(
                        self.shadow_uniforms[self.frame][shadow_view(slot, face)],
                        0,
                        &view_block(view_proj, light.sphere().0).to_bytes(),
                    )?;
                }
                self.shadow_draws[shadow_cull(slot)].begin_frame(
                    device,
                    self.frame,
                    &frustum,
                    instance_count,
                    eye,
                    self.shadow_lod_params,
                )?;
            }
        }
        // Whether the map has a cascade to be drawn from at all: the updater is
        // on, and cascade 0's block and cull were written this frame — by the
        // loops above where shadows are on, or by the branch above where they
        // are off.
        self.rsm_cull_ready = self.probe_update_runs()
            && (updater_drives_cascade_zero || (redraw[0] && regions[0].is_some()));
        // `docs/plan/50-irradiance-probes.md`'s gather, parameterised from the
        // cascade this frame just fitted and the sun it was fitted to. Written
        // here rather than in the pass body: a pass body runs while the frame's
        // commands are being recorded, and a host write to a buffer an earlier
        // submission may still be reading is the hazard the whole ring exists to
        // avoid.
        //
        // **Written whether or not the pass is recorded.** Which slot the gather
        // reads is decided by the frame, and a block left holding an older
        // frame's sun would be read the moment the pass came back.
        //
        // **The producer table is written here too**, off the selection this
        // call has just made: `add_shadow_pass` draws exactly this list and the
        // gather walks exactly this list, and `punctual_faces` is what makes
        // those the same sentence.
        let producers: Vec<PunctualProducer> = self
            .punctual_faces()
            .iter()
            .map(|face| face.producer)
            .collect();
        if let Some(gather) = self.probe_gather.as_ref() {
            gather.begin_frame(
                device,
                self.frame,
                self.sun_color.to_array(),
                rsm::texel_area(self.cascade_reach),
                &producers,
            )?;
        }
        Ok(())
    }

    /// Whether this frame records `docs/plan/50-irradiance-probes.md`'s two
    /// updater passes.
    ///
    /// The scene's own answer and the console's together: the volume decides
    /// whether the feature applies at all — see [`ProbeUpdate`], which is where
    /// the reason it is not an effect bit lives — and `r_probe_bounce` is the
    /// on/off pair a pricing run reads the two passes' cost off with everything
    /// else held still.
    ///
    /// **It is not the whole condition.** The map is cascade 0's own draws, so
    /// the pass is recorded only where that cascade is being drawn too — see
    /// `add_shadow_pass`, which is where the rest of it is.
    fn probe_update_runs(&self) -> bool {
        self.probe_update == ProbeUpdate::EveryFrame && rsm::enabled()
    }

    /// Every punctual shadow face this frame draws into the punctual reflective
    /// shadow map, in the order the pass draws them and the producer table lists
    /// them.
    ///
    /// **One list for both halves of the updater**, and that is the point:
    /// `add_shadow_pass` sets a viewport from a face's
    /// [`tile`](PunctualFace::tile) and the gather reads that same rectangle out
    /// of the matching producer row, so a face the pass drew and a row the
    /// gather walked have to be the same face. Building the two from separate
    /// filters is two things that can drift, and the drift would be a light
    /// bouncing another light's map.
    ///
    /// **The occupied slots this frame redraws**, which is the same set
    /// `add_shadow_pass` gives the atlas: a face's transform is a shadow view's
    /// block and its draws are a shadow cull's, and
    /// [`Self::begin_frame_body`](Self::begin_frame) writes neither for a slot
    /// it is holding.
    ///
    /// # Every occupied tile, and no budget of its own
    ///
    /// The producers are exactly the atlas's occupied faces — up to
    /// [`shadow::LIGHT_SLOTS`] lights and [`shadow::LIGHT_TILES`] tiles between
    /// them — rather than the highest-ranked light or some smaller count.
    ///
    /// **Because the budget already exists and is spent.**
    /// [`shadow::Selection`] has ranked these lights by projected screen
    /// influence and handed out every tile there is; a second budget on top
    /// would rank the same lights again, differently, and produce a light that
    /// casts a shadow and lends no bounce. That is a difference no picture shows
    /// — the same argument [`Self::shadow_lights`] makes about a map's size —
    /// and it is the kind of quiet inconsistency the two halves of one light are
    /// worst placed to have.
    ///
    /// It is affordable at the extent that was chosen for it:
    /// `apps/lantern` lights seven faces, and
    /// [`PUNCTUAL_RSM_SIDE`](crcbl_shaders::probe_gather::PUNCTUAL_RSM_SIDE)
    /// carries the sweep that priced them.
    ///
    /// **Empty where shadows are off**, for that same reason — a punctual face
    /// *is* a shadow view. The sun's half is not, which is what
    /// `updater_drives_cascade_zero` exists for: switching shadows off keeps the
    /// sun's bounce and loses every punctual one, rather than switching the
    /// whole updater off with them.
    fn punctual_faces(&self) -> Vec<PunctualFace> {
        if !self.probe_update_runs() {
            return Vec::new();
        }
        // A frame with the shadows switched off draws no atlas, so no group of
        // it is *redrawn* — and `begin_frame_body`'s `updater_drives_cascade_zero`
        // branch has still written every one of these views and dispatched every
        // one of these culls, for this pass to draw through.
        let updater_drives = !self.frame_effects.contains(RenderEffects::SHADOWS);
        let mut faces = Vec::new();
        for (slot, held) in self.shadow_lights.slots().iter().enumerate() {
            let Some(held) = *held else {
                continue;
            };
            if !updater_drives && !self.shadow_group_redrawn[shadow_cull(slot)] {
                continue;
            }
            let Some(light) = self.extra_lights.get(held.light) else {
                continue;
            };
            // A spot draws face 0 alone and a point light all six, each through
            // its own matrix into its own tile — `shadow::tile_span` is the same
            // function `Selection` allocated the run with, so a face here and a
            // tile there cannot disagree about how many there are.
            for face in 0..shadow::tile_span(light) {
                let tile = rsm::punctual_tile(held.base + face);
                faces.push(PunctualFace {
                    view: shadow_view(slot, face),
                    cull: shadow_cull(slot),
                    tile,
                    producer: producer_row(light, tile),
                });
            }
        }
        faces
    }

    /// Everything one group of the shadow atlas is a function of, as bytes to
    /// compare with the reading the image was last drawn from.
    ///
    /// A **group** is a cascade or a light slot's whole run of tiles — what one
    /// cull covers and what [`shadow::Cadence`] schedules. Per group rather than
    /// per atlas since the cadence rung: a lamp that swings makes its own group's
    /// reading differ and leaves every other group's alone, which is what lets
    /// the frame redraw its tiles and hold the rest.
    ///
    /// # What is in it
    ///
    /// * **The group's view blocks and its cull's frustum**, exactly the values
    ///   [`begin_frame_body`](Self::begin_frame_body) is about to write. Those
    ///   carry the cascade's or the light's matrices, the selection's atlas
    ///   rectangles and this frame's shadow LOD budgets — so a light that moved,
    ///   turned, changed radius or angle, gained or lost a tile, or landed in a
    ///   different rectangle is a different reading, and so is a camera that
    ///   moved, because a cascade is fitted to it.
    /// * **The selection eye and the instance count**, which reach the cull
    ///   beside the frustum and are in no block.
    /// * [`InstancePool::revision`], which is every caster: a transform, a mesh,
    ///   a material, a removal or a skinned re-point all pass through the one
    ///   write that moves it. It is conservative — it moves for writes no map
    ///   could show, and for writes only some other group's map could — and it
    ///   is never still while a byte the shadow pass draws from has changed.
    ///
    /// The bytes are the blocks' own `to_bytes` — the encoding the GPU reads —
    /// rather than the objects' memory, so no padding and no `f32` bit pattern
    /// nobody wrote is in the comparison. Two readings that differ by `-0.0`
    /// against `0.0` compare unequal and redraw, which is the safe direction.
    ///
    /// A skinned frame is **not** in here and is not meant to be: a record that
    /// merely carried "this frame skins" would still match the last skinned
    /// frame's and cache. [`ForwardRenderer::frame_skins`] is a separate veto on
    /// the answer for that reason, and it is where that limit is written down.
    ///
    /// # What is deliberately not in it
    ///
    /// * **The mesh pool.** Its buffers are written by `build_geometry` and
    ///   `residents` alone, both of which run inside
    ///   [`with_scene`](Self::with_scene) before a renderer exists — this type
    ///   exposes no way to upload or free a mesh afterwards. The one runtime
    ///   caller of the suballocator is
    ///   [`reserve_skinned`](Self::reserve_skinned), whose bytes are a skinned
    ///   region's, and those are `frame_skins`' always-redraw case.
    /// * **Whether the frame has shadows at all**, which is the atlas's *layout*
    ///   rather than any group's reading: a frame with shadows off gives every
    ///   group no views and no cull, so none of them reaches this function, and
    ///   `begin_frame_body`'s empty layout is what makes such a frame clear the
    ///   image once and cost nothing after.
    /// * **Materials, samplers and the colour pipeline.** `depth_pipeline`
    ///   names no fragment stage, so nothing the atlas holds is a function of
    ///   them.
    fn shadow_group_record(
        &self,
        group: usize,
        views: &[(usize, usize, mesh::FrameUniforms)],
        culls: &[(usize, Frustum)],
        eye: [f32; 3],
        instance_count: u32,
    ) -> Vec<u8> {
        // A point light's cube is the longest a group gets, so this is one
        // allocation rather than a handful of regrowths. A hint, not a bound.
        let mut record = Vec::with_capacity(shadow::POINT_FACES * mesh::FRAME_UNIFORMS_SIZE);
        record.extend_from_slice(&self.instances.revision().to_le_bytes());
        record.extend_from_slice(&instance_count.to_le_bytes());
        for value in eye {
            record.extend_from_slice(&value.to_le_bytes());
        }
        for (_, view, block) in views.iter().filter(|(owner, _, _)| *owner == group) {
            record.extend_from_slice(&(*view as u32).to_le_bytes());
            record.extend_from_slice(&block.to_bytes());
        }
        for (cull, frustum) in culls.iter().filter(|(cull, _)| *cull == group) {
            record.extend_from_slice(&(*cull as u32).to_le_bytes());
            for plane in frustum.planes {
                for value in plane.to_array() {
                    record.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        record
    }

    /// Puts an object in the scene and returns the handle that names it.
    ///
    /// An instance in the pool **is** an object in the scene: the frame records
    /// a fixed number of draws and the cull pass decides which instances they
    /// walk, so adding one costs a slot in an array and not a command. It is
    /// drawn from the next [`ForwardRenderer::begin_frame`], which is what
    /// uploads it.
    ///
    /// # The index it lands at is not decoration
    ///
    /// `draw_gen.slang` keys `docs/plan/25-lod.md`'s hysteresis state by the
    /// instance's **array index**, at `instance_index * group_stride`, and
    /// `mesh_cluster.slang`'s amplification stage reads the same address. So the
    /// record of which of a DAG's groups an object had expanded belongs to the
    /// *slot*, not to the object — and a slot that [`remove_instance`] freed is
    /// handed to the next `add_instance`, which inherits it. What that costs is
    /// one frame selected against a history that is not this object's, and the
    /// next frame's own judgement replaces it; it is never a wrong picture,
    /// because every group is judged afresh whatever the state said.
    ///
    /// That inheritance is inert while the scene holds **one** instance of a DAG
    /// mesh, which is every frame the engine has drawn so far — one instance's
    /// run has no previous occupant, and a flat mesh's
    /// [`MeshLevels`](level_select::MeshLevels) record names no groups at all, so
    /// nothing indexes the state for it. It stops being inert the moment an
    /// application places a second.
    ///
    /// [`remove_instance`]: ForwardRenderer::remove_instance
    ///
    /// # A DAG mesh needs a device that can choose a level
    ///
    /// One device shape cannot draw a [`Geometry::Dag`] mesh: a mesh stage with
    /// **no** amplification stage in front of it emits every cluster of the
    /// bucket, which for a DAG is every level at once — several overlapping
    /// copies of one surface. This does not refuse it, because it has no
    /// vocabulary for that refusal; a caller placing a DAG asks
    /// [`ForwardRenderer::selects_levels`] first, and places it only where that
    /// says yes. A flat mesh is unaffected on every path.
    ///
    /// # Errors
    ///
    /// [`InstancePoolError::PoolFull`] when the instance pool is full. It never
    /// grows — [`Capacities::instances`](crate::scene::Capacities::instances) is
    /// what a caller sizes it with.
    ///
    /// # Panics
    ///
    /// If [`InstanceDesc::mesh`] or [`InstanceDesc::material`] is past the end of
    /// the description this renderer was built from. Both are indices a caller
    /// wrote against a description it holds, so an out-of-range one is a mistake
    /// in the calling code rather than a condition the frame can be in — and the
    /// alternative is an object silently drawn as some other mesh, in some other
    /// colour.
    pub fn add_instance(
        &mut self,
        desc: &InstanceDesc,
    ) -> Result<InstanceHandle, InstancePoolError> {
        let instance = self.gpu_instance(desc);
        self.instances.insert(&instance)
    }

    /// Rewrites the object `handle` names — its mesh, its material and its
    /// transform, all three.
    ///
    /// A stale handle — one [`ForwardRenderer::remove_instance`] retired — writes
    /// nothing, rather than overwriting whatever took its slot.
    ///
    /// Takes effect at the next [`ForwardRenderer::begin_frame`], which uploads
    /// the element and nothing else: an object that did not move costs no
    /// transfer at all.
    ///
    /// # Panics
    ///
    /// On [`ForwardRenderer::add_instance`]'s terms, for the same reason.
    pub fn set_instance(&mut self, handle: InstanceHandle, desc: &InstanceDesc) {
        let instance = self.gpu_instance(desc);
        self.instances.set(handle, &instance);
    }

    /// Takes the object `handle` names back out of the scene, freeing its slot.
    ///
    /// **A removal is a cleared live bit, not a skipped draw**, because the frame
    /// no longer records a draw per object: `cull.slang` asks whether an instance
    /// is live before it reads anything else, so removing an object and culling
    /// it off screen take the same path out of the frame. An instance left in the
    /// pool would be drawn wherever its transform put it.
    ///
    /// A stale handle removes nothing. Takes effect at the next
    /// [`ForwardRenderer::begin_frame`], on
    /// [`ForwardRenderer::set_instance`]'s terms.
    pub fn remove_instance(&mut self, handle: InstanceHandle) {
        self.instances.remove(handle);
    }

    /// Captures a visibility map for every probe, from the static geometry
    /// standing in the scene right now.
    ///
    /// `docs/plan/50-irradiance-probes.md`'s rung: each probe records how far
    /// away the nearest surface is in every direction, and `mesh.slang`'s
    /// `probe_irradiance` then weighs each of a fragment's eight probes by
    /// whether it can *see* that fragment — so a probe on the far side of a wall
    /// contributes nothing and a room stops lighting the room next door.
    /// This crate's own `probe_capture` module is the pass, `probe_visibility`
    /// is the geometry it draws, and [`crcbl_shaders::probe_visibility`] is the
    /// format.
    ///
    /// # Call it once, after the scene's walls are placed
    ///
    /// **A description carries no instances** — [`SceneDesc`] is meshes,
    /// materials, a page and the probe rows, and the objects arrive afterwards
    /// through [`ForwardRenderer::add_instance`] — so there is nothing for a
    /// capture inside [`ForwardRenderer::with_scene`] to be about. This is the
    /// call that has the room in it, and an application makes it once its static
    /// geometry is placed.
    ///
    /// A caller that never makes it draws exactly the frame this renderer drew
    /// before the maps existed: every group names the one-texel placeholder,
    /// which no surface is further away than, and every probe keeps all of its
    /// weight.
    ///
    /// **It is a capture of geometry and not a bake of light.** Nothing about
    /// the lights is recorded, every one of them still moves, and the rows the
    /// probes carry are untouched.
    ///
    /// # What it sees, and what it does not
    ///
    /// Every instance live in the pool when it is called, at the transform it
    /// then has, rasterised from its mesh's level-0 triangles into six 90°
    /// faces per probe. Objects that move
    /// afterwards keep the occlusion they had here, which is the accepted limit
    /// this technique has everywhere it is used — a probe volume is about the
    /// walls. Recapturing after a wall moves is this call again.
    ///
    /// **The device must be idle.** The previous capture's image is released
    /// here, and a frame in flight may still be reading it.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the capture. The previous maps are kept on the failing
    /// path, so a renderer that has captured once and then failed goes on
    /// drawing with what it had.
    pub fn capture_probe_visibility(
        &mut self,
        device: &dyn Device,
        queue: QueueHandle,
    ) -> Result<(), HalError> {
        let placed: Vec<crate::probe_visibility::Occluder> = self
            .instances
            .live()
            .into_iter()
            .map(|record| crate::probe_visibility::Occluder {
                mesh: record.mesh,
                transform: Mat4::from_cols_array(&record.transform),
            })
            .collect();
        let Some(uploaded) = crate::probe_capture::capture(
            device,
            queue,
            &self.probe_volume,
            &self.probe_occluders,
            &placed,
        )?
        else {
            // No probes, or nothing placed for a map to be about. Leaving the
            // placeholder bound is the honest answer, and it is what a scene
            // with no probes has always read.
            return Ok(());
        };
        // The cached groups name the view this replaces, so they go before it
        // does — a bind group outliving the image it names is a validation
        // error on every backend that checks, and a use-after-free on the ones
        // that do not.
        for slot in &mut self.screen_channel_groups {
            if let Some((_, stale)) = slot.take() {
                device.destroy_bind_group(stale);
            }
        }
        if let Some(previous) = self.probe_visibility.replace(uploaded) {
            previous.destroy(device);
        }
        Ok(())
    }

    /// Resolves a description's mesh and material indices into the table ids the
    /// GPU reads.
    ///
    /// The whole record every time, because [`InstancePool::set`] writes the
    /// whole record: an instance that lost either id would name entry 0 of that
    /// table, which is a mesh and a material rather than an absence.
    fn gpu_instance(&self, desc: &InstanceDesc) -> mesh::GpuInstance {
        mesh::GpuInstance {
            transform: desc.transform.to_cols_array(),
            mesh: self.mesh_ids[desc.mesh],
            material: self.material_ids[desc.material],
            ..mesh::GpuInstance::default()
        }
    }

    // --- skinning: reserving the region, placing the object, ordering the
    // dispatch ---

    /// The vertex pool every mesh here is resident in — what
    /// [`SkinningDesc::vertices`](crate::skinning::SkinningDesc::vertices)
    /// takes.
    ///
    /// The skinning pass reads a bind pose out of this buffer and writes the
    /// deformed vertices back into another run of it, which is what makes a
    /// skinned draw an ordinary draw. It must be *this* pool's buffer: a
    /// [`Skinning`] built against any other writes vertices no draw here can
    /// reach.
    #[must_use]
    pub const fn vertex_buffer(&self) -> BufferHandle {
        self.pool.vertex_buffer()
    }

    /// Where that pool's **attribute** region begins, in words — what
    /// [`SkinningDesc::attribute_base`](crate::skinning::SkinningDesc::attribute_base)
    /// takes.
    ///
    /// The buffer holds two streams, so a pass that writes it needs the
    /// boundary as well as the handle; a [`Skinning`] built with the wrong one
    /// writes a mesh's attributes over another mesh's positions. See
    /// [`MeshPool::attribute_base`](crate::MeshPool::attribute_base).
    #[must_use]
    pub const fn attribute_base(&self) -> u32 {
        self.pool.attribute_base()
    }

    /// Reserves a skinned region for description mesh `mesh`.
    ///
    /// The whole of what a caller needs to skin something, in one value: hand
    /// [`SkinnedMesh::skin_range`] a palette and a set of bindings to get the
    /// frame's [`SkinRange`], hand the result to
    /// [`begin_skinned_frame`](Self::begin_skinned_frame), and place it with
    /// [`add_skinned_instance`](Self::add_skinned_instance). The bases and the
    /// parity never have to be spelled by the caller at all — see
    /// [`crate::skinning::SkinnedMesh`], where the ping-pong lives.
    ///
    /// **The mesh keeps its own entry and the region takes none**, so the same
    /// description mesh can be drawn skinned through this and in its bind pose
    /// through [`add_instance`](Self::add_instance) in the same frame, both
    /// resolving through the one table entry it already had.
    ///
    /// Give it back with [`release_skinned`](Self::release_skinned).
    ///
    /// # Errors
    ///
    /// Whatever [`SkinnedMesh::reserve`] refuses: a vertex pool with no room for
    /// both halves. **Two vertex runs per skinned primitive** is what a scene's
    /// [`Capacities`](crate::scene::Capacities) has to have been sized for.
    ///
    /// # Panics
    ///
    /// If `mesh` is past the end of the description this renderer was built
    /// from, on [`add_instance`](Self::add_instance)'s terms — and if it names a
    /// [`Geometry::Dag`], which cannot be drawn out of a skinned region at all.
    /// A DAG is refused rather than skinned wrongly: its coarser levels are
    /// separate vertex runs no dispatch writes, so a cut that descended past
    /// level 0 would draw whatever the pool happened to be holding there.
    pub fn reserve_skinned(&mut self, mesh: usize) -> Result<SkinnedMesh, MeshPoolError> {
        let source = self.skinnable_meshes[mesh].unwrap_or_else(|| {
            panic!(
                "description mesh {mesh} is a Geometry::Dag, whose coarser levels are vertex \
                 runs no skinning dispatch writes; only a flat mesh can be skinned"
            )
        });
        SkinnedMesh::reserve(&mut self.pool, source)
    }

    /// Gives `skinned`'s region back to the vertex pool.
    ///
    /// The device must not still be drawing it: this records no barrier, on
    /// [`SkinnedMesh::release`]'s terms. Objects placed through it are **not**
    /// removed — [`remove_instance`](Self::remove_instance) is what does that,
    /// and an instance left behind goes on drawing its source mesh's geometry
    /// out of vertices the pool has handed back. Remove the objects first.
    pub fn release_skinned(&mut self, skinned: SkinnedMesh) {
        skinned.release(&mut self.pool);
    }

    /// Places a skinned object in the scene.
    ///
    /// [`add_instance`](Self::add_instance) with a reserved region in place of a
    /// description mesh. The renderer keeps the object in a list of its own and
    /// **re-points it every [`begin_skinned_frame`](Self::begin_skinned_frame)**
    /// at the half of the region that frame's dispatch fills, so the parity is
    /// never something a caller writes.
    ///
    /// A skinned object driven by plain [`begin_frame`](Self::begin_frame) is
    /// therefore left pointing wherever it last was, which after the first frame
    /// is the pose of the frame before — the reason the two entry points are
    /// separate calls rather than a flag.
    ///
    /// # Errors
    ///
    /// [`InstancePoolError::PoolFull`], on
    /// [`add_instance`](Self::add_instance)'s terms.
    ///
    /// # Panics
    ///
    /// If [`SkinnedInstanceDesc::material`] is past the end of the description
    /// this renderer was built from, on
    /// [`add_instance`](Self::add_instance)'s terms.
    pub fn add_skinned_instance(
        &mut self,
        desc: &SkinnedInstanceDesc<'_>,
    ) -> Result<InstanceHandle, InstancePoolError> {
        // Parity zero, and it never reaches the GPU as such: the instance pool
        // uploads by delta at `begin_skinned_frame`, which re-points every entry
        // of the list below before it flushes. What this writes is the rest of
        // the record.
        let instance = self.skinned_gpu_instance(desc, 0);
        let handle = self.instances.insert(&instance)?;
        self.skinned_instances.push(SkinnedInstance {
            handle,
            bases: skinned_bases(desc.mesh),
            pointed: false,
        });
        Ok(handle)
    }

    /// Rewrites the skinned object `handle` names — its region, its material and
    /// its transform.
    ///
    /// [`set_instance`](Self::set_instance) for an object placed by
    /// [`add_skinned_instance`](Self::add_skinned_instance), and the call a
    /// character's transform changes through. A stale handle writes nothing.
    ///
    /// Passing a handle this renderer has never seen skinned adopts it: the
    /// object joins the re-pointed list from here on. That is what lets a caller
    /// swap a bind-pose instance over to a region without removing and replacing
    /// it.
    ///
    /// # Panics
    ///
    /// On [`add_skinned_instance`](Self::add_skinned_instance)'s terms.
    pub fn set_skinned_instance(&mut self, handle: InstanceHandle, desc: &SkinnedInstanceDesc<'_>) {
        let instance = self.skinned_gpu_instance(desc, 0);
        if !self.instances.set(handle, &instance) {
            return;
        }
        let bases = skinned_bases(desc.mesh);
        match self
            .skinned_instances
            .iter_mut()
            .find(|entry| entry.handle == handle)
        {
            // **A different region is a fresh entry.** The half this object has
            // never been drawn out of holds whatever its reservation came with,
            // so the next pointing owes it rest rather than a previous
            // deformation — see [`SkinnedInstance::pointed`]. The pair of bases
            // is what "different" means here: two calls naming one region give
            // the same two runs, so rewriting an object's material or transform
            // is not a change of region.
            Some(entry) => {
                entry.pointed &= entry.bases == bases;
                entry.bases = bases;
            }
            None => self.skinned_instances.push(SkinnedInstance {
                handle,
                bases,
                pointed: false,
            }),
        }
    }

    /// A skinned object's record, at one parity — [`gpu_instance`](Self::gpu_instance)
    /// with the region's base vertex carried in the record and the bit that says
    /// to read it.
    ///
    /// The mesh id is the **source** mesh's, not a second entry's, so the bucket
    /// this instance is scattered into, the levels it selects through and the
    /// box it is culled against are the ones the undeformed mesh already had.
    fn skinned_gpu_instance(
        &self,
        desc: &SkinnedInstanceDesc<'_>,
        parity: u32,
    ) -> mesh::GpuInstance {
        let base_vertex = desc.mesh.region().base(parity);
        mesh::GpuInstance {
            transform: desc.transform.to_cols_array(),
            mesh: desc.mesh.mesh_id(),
            material: self.material_ids[desc.material],
            flags: mesh::GpuInstance::BASE_VERTEX_OVERRIDE,
            base_vertex,
            // At rest, which is what a record this call writes has to be: the
            // object has not been drawn out of this region yet, so the other
            // half holds whatever its reservation came with. A frame that runs
            // [`point_skinned_instances`](Self::point_skinned_instances) is
            // what starts pointing this at the half before.
            previous_base_vertex: base_vertex,
            ..mesh::GpuInstance::default()
        }
    }

    /// Points every skinned object at the half of its region a frame of this
    /// parity fills **and at the half the frame before it filled**, and forgets
    /// the ones whose instance has gone.
    ///
    /// Called between the instance ring's rotation and its upload, which is the
    /// whole reason those two are separate calls — see
    /// [`InstancePool::rotate`](crate::instance_pool::InstancePool::rotate).
    ///
    /// The second base is what a skinned fragment's motion vector is subtracted
    /// against — see
    /// [`GpuInstance::previous_base_vertex`](mesh::GpuInstance::previous_base_vertex).
    /// **A region this frame's `ranges` left out was not skinned this frame**,
    /// so the half this points at as the current one already holds an earlier
    /// frame's pose. That is pre-existing — it is what a caller driving a
    /// skinned object through plain [`begin_frame`](Self::begin_frame) gets, and
    /// it predates this second base — and the previous half is then no worse off
    /// than the current one.
    fn point_skinned_instances(&mut self, parity: u32) {
        let Self {
            instances,
            skinned_instances,
            ..
        } = self;
        skinned_instances.retain_mut(|entry| {
            // `SkinnedRegion::base`'s indexing and `SkinnedRegion::previous_base`'s,
            // applied to the entry's own copy of the pair — see
            // [`SkinnedInstance::bases`].
            let base_vertex = entry.bases[(parity & 1) as usize];
            let previous_base_vertex = if entry.pointed {
                entry.bases[((parity ^ 1) & 1) as usize]
            } else {
                // Nothing has been written into the other half for this object,
                // so it is put at rest — see [`SkinnedInstance::pointed`].
                base_vertex
            };
            entry.pointed = true;
            // The two bases and nothing else: the transform, the material and
            // the mesh the caller last set survive, and so does the transform
            // the record moved *from* — which
            // [`InstancePool::set`](crate::instance_pool::InstancePool::set)
            // would overwrite with this frame's, leaving a skinned object's own
            // travel out of the motion target. See
            // [`InstancePool::set_bases`](crate::instance_pool::InstancePool::set_bases).
            // Its answer is also what says the instance is still there, so an
            // object a caller removed drops out of the list here.
            instances.set_bases(entry.handle, base_vertex, previous_base_vertex)
        });
    }

    /// The lights in the frame **beside the sun**, which
    /// [`begin_frame`](Self::begin_frame) still takes on its own.
    ///
    /// `docs/plan/18-render-features.md`'s light list, minus its first row: the
    /// sun is row 0 of every frame's list and the ones set here follow it, in
    /// the order given. That order is the one a froxel keeps a prefix of when it
    /// runs out of budget — see [`ForwardRenderer::light_capacity`] and the
    /// overflow counter beside it — so it is a caller's lever and not an
    /// accident.
    ///
    /// The sun keeps a parameter of its own because it is the light that owns
    /// the ambient term and the shadow cascades, neither of which a [`Light`]
    /// has. What it stopped being is a special case in the *shader*, which is
    /// what the list bought.
    ///
    /// Takes effect at the next [`begin_frame`](Self::begin_frame), which is
    /// what uploads the rows.
    ///
    /// # Panics
    ///
    /// If there are more lights than [`ForwardRenderer::light_capacity`] leaves
    /// room for once the sun has its row. Refused rather than truncated: a light
    /// missing from the list is missing from every froxel, and **no counter in
    /// the frame would report it** — the overflow counter counts what a froxel's
    /// budget refused, which is a different number.
    pub fn set_lights(&mut self, lights: &[Light]) {
        assert!(
            lights.len() < self.lights.capacity() as usize,
            "{} lights beside the sun, in a list of {}",
            lights.len(),
            self.lights.capacity()
        );
        self.extra_lights.clear();
        self.extra_lights.extend_from_slice(lights);
    }

    /// Rows the light list holds, the sun's included.
    #[must_use]
    pub const fn light_capacity(&self) -> u32 {
        self.lights.capacity()
    }

    /// This frame's froxel grid, as [`begin_frame`](Self::begin_frame) sized it
    /// from the viewport and the camera.
    ///
    /// Exposed for the reason [`lod_params`](Self::lod_params) is: a test that
    /// wants to know which froxel a point was shaded out of has to use the grid
    /// the frame really used, and re-deriving it would be a second copy of the
    /// arithmetic rather than a reading of the first.
    #[must_use]
    pub const fn grid(&self) -> Grid {
        self.grid
    }

    /// `frame`'s froxel grid buffer, for a test copying out what the clustering
    /// pass decided.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn light_grid_buffer(&self, frame: usize) -> BufferHandle {
        self.lights.grid(frame)
    }

    /// The pixel budget the descent compares a group's projected error against.
    ///
    /// One pixel until this is called — see the constant this crate keeps it
    /// in. Larger is coarser: a group
    /// projecting *over* the budget is expanded and its children are drawn, so
    /// raising it stops the descent higher up the DAG.
    ///
    /// Takes effect at the next [`begin_frame`](Self::begin_frame), which is
    /// what writes it into the frame block.
    pub const fn set_lod_error_budget(&mut self, budget: f32) {
        self.lod_error_budget = budget;
    }

    /// The budget this renderer selects under unless
    /// [`set_lod_error_budget`](Self::set_lod_error_budget) says otherwise, and
    /// the ratio the hold budget sits at below whichever one is in force.
    ///
    /// Public because a test that drives a camera across a level boundary has to
    /// know where the boundary is, and where it is depends on both numbers —
    /// re-deriving either in the test would be a second copy to drift.
    pub const LOD_ERROR_BUDGET: f32 = LOD_ERROR_BUDGET;
    /// See [`LOD_ERROR_BUDGET`](Self::LOD_ERROR_BUDGET).
    pub const LOD_HOLD_RATIO: f32 = LOD_HOLD_RATIO;

    /// How far below the budget an already-expanded group is held before it
    /// collapses again — `docs/plan/25-lod.md`'s hysteresis, as a fraction of
    /// the budget.
    ///
    /// [`LOD_HOLD_RATIO`](Self::LOD_HOLD_RATIO) until this is called. **A ratio
    /// of one removes the band**, which makes the two budgets equal and the
    /// previous frame's answer stop mattering — the setting a test uses to show
    /// that the band is what stops a drifting camera flickering, on the same
    /// code path and one number apart.
    ///
    /// Takes effect at the next [`begin_frame`](Self::begin_frame), which is
    /// what writes it into the two params blocks.
    pub const fn set_lod_hold_ratio(&mut self, ratio: f32) {
        self.lod_hold_ratio = ratio;
    }

    /// What the last [`begin_frame`](Self::begin_frame) handed the descent:
    /// pixels per unit at one unit from the eye, and the pixel budget.
    ///
    /// Pixels per unit at one unit from the eye, the budget an unexpanded group
    /// starts expanding over, and the budget an expanded one is held down to.
    ///
    /// What `ClusterDag::expand` takes, so a caller reading
    /// [`cluster_selection`](Self::cluster_selection) back can run the same
    /// frames host-side rather than re-deriving the numbers from the camera and
    /// hoping they agree. `[0.0, 0.0, 0.0]` before the first frame.
    #[must_use]
    pub const fn lod_params(&self) -> [f32; 3] {
        self.lod_params
    }

    /// The same three numbers the **shadow cascades** selected under, which is
    /// [`lod_params`](Self::lod_params) with both budgets multiplied by
    /// [`SHADOW_LOD_BIAS`] — `docs/plan/25-lod.md`'s shadow LOD bias, as the
    /// parameters it actually reaches the GPU as.
    ///
    /// The pixels-per-unit is the camera's, unchanged, because the cascades
    /// select from the camera's eye at the camera's scale; see
    /// [`begin_frame`](Self::begin_frame). `[0.0, 0.0, 0.0]` before the first
    /// frame.
    #[must_use]
    pub const fn shadow_lod_params(&self) -> [f32; 3] {
        self.shadow_lod_params
    }

    /// Where description mesh `mesh`'s clusters are in the cluster pool — every
    /// level of it, as one run — or `None` off the mesh path, where there is no
    /// cluster pool at all.
    ///
    /// The base a reader adds a `(level, cluster)` to in order to index
    /// [`cluster_selection`](Self::cluster_selection): the levels are laid down
    /// finest first and contiguously, so level `d` cluster `c` is at
    /// `base + (clusters of levels below d) + c`. A flat mesh is one level, so
    /// its run is its clusters and `c` indexes it directly.
    ///
    /// # Panics
    ///
    /// If `mesh` is past the end of the description this renderer was built
    /// from, on [`add_instance`](Self::add_instance)'s terms.
    #[must_use]
    pub fn cluster_range(&self, mesh: usize) -> Option<ClusterRange> {
        (!self.mesh_clusters.is_empty()).then(|| self.mesh_clusters[mesh])
    }

    /// The buffer `frame`'s amplification stage recorded its chosen cut into:
    /// one `u32` per resident cluster, `1` where the cluster was drawn.
    ///
    /// `None` where there is no amplification stage, which is every device that
    /// reports no [`Features::TASK_SHADER`] and every non-mesh geometry path.
    ///
    /// **This is `docs/plan/25-lod.md`'s observable and nothing in the frame
    /// reads it.** A frame whose every cluster came from one level is a
    /// plausible picture and matches any golden blessed from it, so a golden
    /// cannot show per-cluster selection happening at all; this can. It is
    /// `TRANSFER_SRC`, and copying it out is the caller's to record.
    ///
    /// [`Features::TASK_SHADER`]: crcbl_hal::Features::TASK_SHADER
    #[must_use]
    pub fn cluster_selection(&self, frame: usize) -> Option<BufferHandle> {
        self.cluster_selection.get(frame).copied()
    }

    /// The same, for `cascade`'s shadow pass: the cut that cascade's
    /// amplification stage chose this frame.
    ///
    /// **The shadow LOD bias' observable.** A budget the cascades merely *were
    /// passed* is a parameter; the cut they reached under it is the behaviour,
    /// and this is the only thing that carries it out of a frame — the colour
    /// pass overwrites [`cluster_selection`](Self::cluster_selection) after the
    /// cascades have run. Indexed and read exactly as that buffer is, through
    /// [`cluster_range`](Self::cluster_range).
    ///
    /// `None` where there is no amplification stage, and where `cascade` is not
    /// one of [`shadow::CASCADES`].
    #[must_use]
    pub fn shadow_selection(&self, frame: usize, cascade: usize) -> Option<BufferHandle> {
        self.shadow_selection
            .get(frame)
            .and_then(|cascades| cascades.get(cascade))
            .copied()
    }

    /// The buckets description mesh `mesh`'s levels draw through, finest first —
    /// element `d` is level `d`'s bucket, and it is empty on the mesh path.
    ///
    /// **This is the uniform cut's observable**, and it is the indirect
    /// arguments rather than a buffer of its own: `draw_gen.slang` scatters an
    /// instance into the bucket for the level it selected, so exactly one of
    /// these buckets comes out of a frame with a non-zero instance count and
    /// which one it is *is* the chosen level. A reader takes
    /// [`DrawGen::args_offset`](crate::draw_gen::DrawGen::args_offset) of each
    /// and looks at
    /// [`DrawIndexedArgs::instance_count`](crcbl_shaders::draw_gen::DrawIndexedArgs::instance_count).
    ///
    /// Empty on [`GeometryPath::MeshShader`], where a mesh is one bucket and the
    /// level is chosen per cluster instead — see
    /// [`cluster_selection`](Self::cluster_selection), which is that path's
    /// observable.
    ///
    /// # Panics
    ///
    /// If `mesh` is past the end of the description this renderer was built
    /// from, on [`add_instance`](Self::add_instance)'s terms.
    #[must_use]
    pub fn level_buckets(&self, mesh: usize) -> &[u32] {
        if self.mesh_level_buckets.is_empty() {
            return &[];
        }
        &self.mesh_level_buckets[mesh]
    }

    /// `frame`'s indirect draw arguments, [`crcbl_shaders::draw_gen::DRAW_ARGS_SIZE`]
    /// bytes per bucket — what a reader of
    /// [`level_buckets`](Self::level_buckets) copies out.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this renderer was built with.
    #[must_use]
    pub fn draw_args(&self, frame: usize) -> BufferHandle {
        self.draws.args(frame)
    }

    /// The most passes [`add_passes`](Self::add_passes) adds to one frame.
    ///
    /// [`DrawGen::MAX_PASSES`] per cull — the camera's, and one per shadow cull
    /// beside it — plus [`LightGrid::MAX_PASSES`] for the froxel grid and the
    /// passes that draw. **A ceiling, not a count**: a frame whose shadow slots
    /// are not all filled runs fewer culls, which is what makes a free tile
    /// free.
    ///
    /// Derived rather than written down, because a number written down is one
    /// that stops matching the frame the day a pass is added — which is exactly
    /// how the samples' hand-picked `8` came to time the first eight passes of a
    /// fourteen-pass frame. A caller sizing
    /// [`PassTimers`](crate::timing::PassTimers) wants
    /// [`MAX_TIMED_PASSES`](crate::timing::MAX_TIMED_PASSES), which adds this to
    /// what the overlay renderers record.
    pub const MAX_PASSES: u32 =
        DrawGen::MAX_PASSES * (1 + SHADOW_CULLS as u32) + LightGrid::MAX_PASSES + RENDER_PASSES;

    /// Adds the forward and tonemap passes to `graph`, rendering into `target`,
    /// and returns the HDR scene target they went through.
    ///
    /// `target` is normally the imported swapchain image. Everything else — the
    /// HDR scene colour, the depth buffer, and every barrier between them — is
    /// the graph's.
    ///
    /// The returned [`ImageId`] is the `Rgba16Float` frame the tonemap read:
    /// the scene colour **with the screen-space reflections already added**, not
    /// the target the forward pass wrote. A caller that wants to add a pass of
    /// its own after the tonemap — a debug overlay, or a readback proving the
    /// HDR range is real — declares a read of it and the graph works out the
    /// transition, exactly as it does for the tonemap.
    ///
    /// The two are the same description and different images, which is the
    /// design's off-switch as data rather than as a branch: a frame with
    /// [`RenderEffects::REFLECTIONS`] off adds neither pass, returns the forward
    /// pass's own id, and is bit-identical.
    ///
    /// # Which passes this adds is [`effects`](Self::effects)
    ///
    /// Resolved by the [`begin_frame`](Self::begin_frame) that opened this frame
    /// and read here — see [`crate::effects`] for the four layers and for what
    /// each toggle removes. Every effect is on by default, which is the frame
    /// every caller drew before the toggles existed.
    ///
    /// A switched-off effect is **fewer passes and one different bound
    /// descriptor**, never a shader permutation: the shadow atlas keeps its
    /// reversed-Z clear and reads as fully lit, the occlusion binding falls back
    /// to the renderer's white 1×1, and the reflection pair simply is not there.
    ///
    /// # The ground grid comes last, and after the tonemap
    ///
    /// [`set_ground_grid`](Self::set_ground_grid) is what puts it in the frame,
    /// and it is off by default. When it is on, its pass is added **after** the
    /// tonemap and writes `target` rather than the HDR scene colour, depth-tested
    /// against the scene depth: the grid is reference chrome, and a grid drawn
    /// before the tonemap would be exposed and tonemapped like geometry, so its
    /// colour would move with how bright the scene is. See [`crate::grid`].
    ///
    /// # `pool` is where the shadow atlas's incoming state comes from
    ///
    /// It must be the same pool the caller is about to
    /// [`compile`](RenderGraph::compile) and
    /// [`execute`](crate::CompiledGraph::execute) against, because the atlas and
    /// its placeholder are imports this renderer owns across frames and the
    /// pool's ledger is the record of what the last executed graph left them in
    /// — [`TransientPool::imported_image_use`], with [`None`] read as
    /// [`ResourceState::Undefined`]. Reading it here rather than carrying a
    /// field of our own is what keeps the declaration and the audit
    /// ([`InitialClaim::Tracked`]) one answer instead of two that drift: a frame
    /// whose `compile` fails is a frame that never ran, and a field advanced at
    /// build time would already have moved on without it.
    pub fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        pool: &TransientPool,
        target: ImageId,
        extent: (u32, u32),
    ) -> ImageId {
        self.add_frame_passes(graph, pool, target, extent, None)
    }

    /// [`add_passes`](Self::add_passes) for a frame that **skins**: the same
    /// frame with `skinning`'s dispatch in front of every pass that draws.
    ///
    /// # This takes the pass rather than its [`BufferId`], and that is the point
    ///
    /// [`Skinning::add_pass`] hands back the id of the vertex pool so that a
    /// mesh pass can declare a read of it and be ordered after the dispatch. The
    /// obvious seam is therefore an id parameter here — and it is the wrong one,
    /// because every way of getting it wrong is silent. A caller can pass the id
    /// of a *different* import, add the skinning pass after this one, or not add
    /// it at all, and in each case the graph orders nothing, the draws read the
    /// region before the compute write has been made visible, and what comes out
    /// is a mesh that flickers between two poses on hardware and looks perfect
    /// under a validation layer.
    ///
    /// So this takes the [`Skinning`] and **adds the pass itself**, first, with
    /// this renderer's own frame slot, and declares the read on the three passes
    /// that pull vertices out of the pool — the shadow atlas, the depth prepass
    /// and the forward pass. There is no order for a caller to get right,
    /// because there is only one call.
    ///
    /// A frame whose [`begin_skinned_frame`](Self::begin_skinned_frame) was
    /// handed no ranges adds no dispatch and no declarations, and is the frame
    /// [`add_passes`](Self::add_passes) would have built.
    ///
    /// # It records one pass more than [`MAX_PASSES`](Self::MAX_PASSES)
    ///
    /// That constant is the bound on [`add_passes`](Self::add_passes) and stays
    /// so; the skinning dispatch is [`Skinning::MAX_PASSES`], and
    /// [`MAX_TIMED_PASSES`](crate::timing::MAX_TIMED_PASSES) — what a caller
    /// sizes [`PassTimers`](crate::PassTimers) with — already sums the two.
    pub fn add_skinned_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        pool: &TransientPool,
        target: ImageId,
        extent: (u32, u32),
        skinning: &Skinning,
    ) -> ImageId {
        self.add_frame_passes(graph, pool, target, extent, Some(skinning))
    }

    /// The body of [`add_passes`](Self::add_passes) and
    /// [`add_skinned_passes`](Self::add_skinned_passes), which differ in one
    /// argument and in nothing else.
    fn add_frame_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        pool: &TransientPool,
        target: ImageId,
        extent: (u32, u32),
        skinning: Option<&Skinning>,
    ) -> ImageId {
        // **From here down `extent` is the extent the frame is *drawn* at**,
        // which is the caller's at a render scale of `1.0` and smaller below it.
        // Every use of it in this function sizes a transient, and every one of
        // those transients belongs to the internal frame; `target` is the only
        // thing here at the caller's extent, and it is an id whose image already
        // knows how big it is. `begin_frame_body` shadows it the same way and
        // for the same reason, so the two halves of a frame cannot disagree.
        //
        // `upscaling` is derived from the two extents rather than from the scale
        // itself, which is what makes it exact: a target small enough that the
        // scaled extent rounds back onto it adds no pass, because there would be
        // nothing for the pass to do.
        let target_extent = extent;
        let extent = self.internal_extent(target_extent);
        let upscaling = extent != target_extent;

        // **The skinning dispatch, before anything else is added.** It imports
        // the vertex pool and hands back that node's id; every pass below that
        // pulls vertices declares a read of it, and the graph is what turns the
        // two declarations into the barrier between the compute write and the
        // draws. `None` is a frame with nothing to skin, which adds no pass and
        // declares nothing — see `Skinning::add_pass`.
        let skinned = skinning.and_then(|skinning| skinning.add_pass(graph, self.frame));

        // The cull dispatch and the draw-argument dispatch, before anything
        // draws. Every barrier between them and the pass below — including the
        // one into `IndirectArgument` — is the graph's, computed from what each
        // pass declares.
        let generated = self
            .draws
            .add_passes(graph, self.frame, self.instances.slot_count());

        // `docs/plan/25-lod.md`'s record of the cut — the colour pass's, and one
        // per cascade beside it. Each is written by exactly one mesh pass, so
        // what the graph orders here is this frame's write against the next
        // frame's use of the same slot. Each arrives in the state the previous
        // frame left it in, which is the one declared as final.
        let import_selection = |graph: &mut RenderGraph<'_>, label: &str, buffer| {
            graph.import_buffer(
                label,
                ImportedBuffer {
                    buffer,
                    initial: ResourceState::ShaderReadWrite,
                    final_state: ResourceState::ShaderReadWrite,
                },
            )
        };
        let selection = self
            .cluster_selection
            .get(self.frame)
            .map(|&buffer| import_selection(graph, "cluster-selection", buffer));
        let tile_selection: Vec<BufferId> = self
            .shadow_selection
            .get(self.frame)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(tile, &buffer)| {
                import_selection(graph, &format!("shadow-selection-{tile}"), buffer)
            })
            .collect();

        // Topic 18's clustering, **after the pair above and before anything
        // draws**. After, because the clearing dispatch is what zeroes the
        // overflow counter this pass adds to, and the two are ordered by the
        // graph out of the one id they both declare — which is why `stats` is
        // the id the draw generator handed back rather than a second import of
        // the same buffer. It has no other dependency on the cull: lights are
        // assigned to froxels, and a froxel is a property of the camera.
        let light_grid =
            self.lights
                .add_pass(graph, self.frame, generated.visible_count_id, self.grid);

        // **This frame's slot of the probe ring**, which is the buffer every
        // one of this frame's bind groups names — see [`crate::probe`] for why
        // there is more than one. Imported in `ShaderRead` and handed back in
        // it: the build's staging copy left it there and no pass in the frame
        // writes it yet.
        let probe_buffer = self.probes.buffer(self.frame);
        let probe_table = graph.import_buffer(
            "probes",
            ImportedBuffer {
                buffer: probe_buffer,
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );

        // The occlusion placeholder, imported once and read by every pass whose
        // bind group names it: the shadow pass's views, the depth prepass's copy
        // of the camera's group, and — on a frame drawing without
        // `RenderEffects::AMBIENT_OCCLUSION` — the forward pass itself. It was
        // uploaded at build, so it is already in `ShaderRead` and the graph has
        // nothing to transition; declaring it is what keeps that true if a later
        // pass wants it in some other state.
        let occlusion_placeholder = graph.import_image(
            "ssao-placeholder",
            ImportedImage {
                image: self.ambient_occlusion_placeholder.image,
                view: self.ambient_occlusion_placeholder.view,
                format: Format::R8Unorm,
                extent: (1, 1),
                initial: ResourceState::ShaderRead,
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );

        // The contact-shadow placeholder, imported beside the occlusion one and
        // on its terms exactly: every group of the mesh layout names this
        // binding, and on a frame drawing without `RenderEffects::CONTACT_SHADOWS`
        // it is what the forward pass binds.
        let contact_placeholder = graph.import_image(
            "contact-shadows-placeholder",
            ImportedImage {
                image: self.contact_shadow_placeholder.image,
                view: self.contact_shadow_placeholder.view,
                format: Format::R8Unorm,
                extent: (1, 1),
                initial: ResourceState::ShaderRead,
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );

        // §3.2's base-colour page, in every bind group of `mesh_layout` and so
        // read by all three of the passes that draw geometry. Like the
        // placeholder above it is already in `ShaderRead` and the graph has
        // nothing to transition on a frame nothing writes it — **the
        // declaration is for the frames something does**. A caller that copies a
        // render-to-texture view into one of its layers imports the same handle,
        // gets this same id back (`RenderGraph::import_image`), and the graph
        // then has an edge to order its write against these reads. Without the
        // declaration there is no such edge: the page is bound out of a
        // descriptor the graph never hears about, and a copy scheduled anywhere
        // but the tail of the frame leaves it in `TransferDst` for the passes
        // that sample it.
        let base_color_page =
            graph.import_image(Self::BASE_COLOR_PAGE_LABEL, self.base_color_page_import());
        // §2's normal page beside it, declared for exactly the reason above: it
        // is in every group of `mesh_layout` too, so a caller copying into one
        // of its layers needs the same edge against these draws.
        let normal_page = graph.import_image(Self::NORMAL_PAGE_LABEL, self.normal_page_import());

        // What this frame draws, resolved by the `begin_frame` that opened it —
        // see [`crate::effects`]. Read once here so every conditional below is
        // about one value rather than about four reads of a field.
        let effects = self.frame_effects;
        // Read here for `effects`' reason and for one more: the passes below
        // borrow `self` field by field, and a `&self` method called between
        // them would borrow the whole of it.
        let draws_sky = self.draws_sky();
        // Whether this frame ends by drawing the shadow atlas over itself — the
        // resolved view rather than the switch, so a caller who left the atlas
        // up and then asked for normals gets normals. Read here for
        // `draws_sky`'s reason exactly: `debug_view` takes `&self`.
        let draws_atlas_view = self.debug_view() == DebugView::ShadowAtlas;

        let pages = MaterialPages {
            base_color: base_color_page,
            normal: normal_page,
        };
        let (shadow_atlas, shadow_draws, shadow_tile_clears, rsm_images) = self.add_shadow_pass(
            graph,
            pool,
            &tile_selection,
            ShadowReads {
                occlusion_placeholder,
                pages,
                skinned,
                probes: probe_table,
            },
        );

        let scene_color =
            graph.create_image("scene-color", TransientImageDesc::scene_color(extent));
        let scene_depth =
            graph.create_image("scene-depth", TransientImageDesc::scene_depth(extent));
        // The occlusion chain's transients, and **every one of them is
        // conditional**: a transient nothing reads or writes is a physical image
        // taken out of the pool for a pass that does not exist. What the forward
        // pass binds when they are absent is the 1×1 placeholder — see the pair
        // below.
        //
        // **Three of the four are at [`crate::ssao::half_extent`]**, which is
        // where the march and the blurs run — see that module's header. Only the
        // reconstruction's target is the scene's own size, because it is the
        // image `mesh.slang` reads at `SV_Position`.
        //
        // The blur's target is requested first, which is the order the two were
        // requested in when only one of them was conditional. The pool hands out
        // physical images in request order, and an AO-on frame has to be the
        // frame it was before this became a chain.
        let occlusion_chain = effects.contains(RenderEffects::AMBIENT_OCCLUSION).then(|| {
            let gathered = crate::ssao::half_extent(extent);
            let blurred = graph.create_image(
                "ssao-blurred",
                TransientImageDesc::ambient_occlusion(gathered),
            );
            let raw = graph.create_image("ssao", TransientImageDesc::ambient_occlusion(gathered));
            // The second blur's target, and requested **last of the small ones**
            // so the two above keep the physical images they had before this one
            // could be asked for — the pool hands them out in request order, and
            // a frame at the default switch has to be the frame it was.
            let again = (self.frame_ssao_blurs > 1).then(|| {
                graph.create_image(
                    "ssao-blurred-2",
                    TransientImageDesc::ambient_occlusion(gathered),
                )
            });
            // The reconstruction's target, and the only one of these at the
            // scene's extent. Requested after the small ones for their reason
            // inverted: it is a different description, so it could not have
            // aliased them whatever the order, and asking for it last leaves
            // every request before it where it was.
            let upsampled = graph.create_image(
                "ssao-upsampled",
                TransientImageDesc::ambient_occlusion(extent),
            );
            crate::ssao::OcclusionImages {
                raw,
                blurred,
                again,
                upsampled,
            }
        });
        // The contact march's one transient, conditional for the occlusion
        // pair's reason: an image nothing reads or writes is a physical image
        // out of the pool for a pass that does not exist. Requested **after**
        // the pair so that a frame with contact shadows off keeps the physical
        // images it had before this rung existed — the pool hands them out in
        // request order.
        let contact_mask = effects.contains(RenderEffects::CONTACT_SHADOWS).then(|| {
            graph.create_image(
                "contact-shadows",
                // The occlusion channel's description, and reused rather than
                // copied: it is the same `R8Unorm` colour attachment at the
                // same extent, and two descriptions of one shape are two
                // things to keep in step. `TransientImageDesc`'s constructor
                // says what the shape is for.
                TransientImageDesc::ambient_occlusion(extent),
            )
        });
        // **Created whatever the reflections are doing**, unlike the pair above.
        // It is one of the forward pass's colour attachments, which is in that
        // pipeline whether or not anything reads what it wrote — see the
        // `clear_color` on it below.
        let reflectivity =
            graph.create_image("reflectivity", TransientImageDesc::reflectivity(extent));
        // **Created whatever else the frame is doing**, on the reflectivity
        // attachment's terms exactly: it is the forward pass's third colour
        // attachment, which is in that pipeline whether or not anything reads
        // what it wrote. Nothing does yet — `docs/plan/49-antialiasing.md`'s
        // TAA is the first pass that will, and until then `DebugView::Motion`
        // is what observes the subtraction.
        let motion = graph.create_image("motion", TransientImageDesc::motion(extent));
        // The march's output and the blur's, and both are the scene target's
        // description exactly. The blur writes the scene colour plus a term, so
        // a narrower image there would tonemap a truncated frame; the march
        // writes a reflection *out of* that same colour, so an eight-bit image
        // here would clip the frame's bright end before the tonemap saw it.
        // Three live requests for one description are three physical images —
        // see `TransientPool::image`.
        let reflected = effects.contains(RenderEffects::REFLECTIONS).then(|| {
            (
                graph.create_image("reflection", TransientImageDesc::scene_color(extent)),
                graph.create_image("scene-reflected", TransientImageDesc::scene_color(extent)),
            )
        });
        // Where the froxel volume's composite writes, conditional on the effect
        // for the reflection pair's reason exactly: an image nobody reads or
        // writes is a physical image taken out of the pool for a pass that does
        // not exist. It is the scene target's description, because it stands in
        // for that image from here on.
        let fogged = effects
            .contains(RenderEffects::VOLUMETRIC_FOG)
            .then(|| graph.create_image("scene-fogged", TransientImageDesc::scene_color(extent)));
        // The Hi-Z pyramid's levels, **conditional on the march** that is the
        // only thing that reads them: an image nobody samples is a physical
        // image out of the pool for a pass that does not exist. Level 0 is the
        // prepass itself and is not in this list.
        //
        // Empty on a target too small to halve, which is a frame the march walks
        // at full resolution — `crate::hiz::levels_for` carries that floor, and
        // `SsrParams::hiz_levels` is what tells the shader about it.
        let pyramid: Vec<ImageId> = if reflected.is_some() {
            (1..=crate::hiz::levels_for(extent))
                .map(|level| {
                    graph.create_image(
                        format!("hiz-{level}"),
                        TransientImageDesc::hiz_level(crate::hiz::level_extent(extent, level)),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        // The bloom chain's levels and the image its composite writes, and
        // **all of them are conditional** on the pair above's terms exactly: a
        // transient nothing reads or writes is a physical image taken out of the
        // pool for a pass that does not exist.
        //
        // `None` covers two cases that are one case downstream — the toggle is
        // off, or the target is too small for even one level of chain (see
        // [`crate::bloom`]) — and in both the tonemap reads whatever it would
        // have read before, which is what makes this effect's off-switch
        // bit-identical.
        //
        // The levels are requested largest first, which is the order the
        // downsample chain writes them in. The composite's target is the scene
        // target's description exactly — it stands in for that image from here
        // on, so a narrower one would tonemap a truncated frame — which makes it
        // a fourth live request for that description on a frame that also
        // reflects, and therefore a fourth distinct physical image out of the
        // pool. See `TransientPool::image`.
        let bloomed = effects
            .contains(RenderEffects::BLOOM)
            .then(|| crate::bloom::mips_for(extent))
            .filter(|levels| *levels > 0)
            .map(|levels| {
                let mips: Vec<ImageId> = (1..=levels)
                    .map(|level| {
                        graph.create_image(
                            format!("bloom-mip-{level}"),
                            TransientImageDesc::bloom_mip(crate::bloom::mip_extent(extent, level)),
                        )
                    })
                    .collect();
                (
                    mips,
                    graph.create_image("bloom-color", TransientImageDesc::scene_color(extent)),
                )
            });
        // Where the tonemap writes, and it is the caller's `target` on every
        // frame that adds no resolve.
        //
        // **This is the one effect that changes the shape of the frame rather
        // than adding a pass to it** — see [`crate::fxaa`]. The resolve reads
        // what the tonemap wrote, so the tonemap has to write something the
        // resolve can sample, and a swapchain image is not that. So with the bit
        // on the tonemap writes a transient of the target's own description and
        // the resolve writes the target; with it off the tonemap writes the
        // target and there is no second image at all, which is what makes this
        // effect's off-switch bit-identical on the three above's terms.
        // Where the last pass of the internal frame writes, and it is the
        // caller's `target` on every frame drawn at full render scale.
        //
        // **This is the second effect in the frame that changes its shape rather
        // than adding a pass to it** — see [`crate::upscale`], and the resolve
        // below for the first. The upscale reads what the internal frame ended
        // with, so that last write has to go somewhere it can sample, and a
        // swapchain image at the caller's own extent is not it. So with the
        // scale below one the chain ends in a transient at the *internal*
        // extent and the upscale writes the target; at full scale there is no
        // second image at all, which is what makes the knob's off position
        // bit-identical.
        let present = if upscaling {
            graph.create_image(
                "render-scale-color",
                TransientImageDesc::display_color(extent, self.target_format),
            )
        } else {
            target
        };
        //
        // **Two tiers share that slot and at most one fills it** — see
        // [`RenderEffects::SMAA`], which is where "never both" is written down.
        // The shape above is the same either way; what differs is what the
        // resolve reads besides the tonemap's image, and SMAA's two working
        // images are declared here beside it rather than inside the pass group,
        // on [`Ssao::add_passes`]'s terms: a frame's images belong to the graph.
        let resolving = effects.intersects(RenderEffects::ANTIALIASING.union(RenderEffects::SMAA));
        let display = if resolving {
            graph.create_image(
                "display-color",
                TransientImageDesc::display_color(extent, self.target_format),
            )
        } else {
            present
        };
        let smaa_images = effects.contains(RenderEffects::SMAA).then(|| {
            (
                graph.create_image("smaa-edges", TransientImageDesc::smaa_edges(extent)),
                graph.create_image("smaa-weights", TransientImageDesc::smaa_weights(extent)),
            )
        });

        let group = self.mesh_groups[self.frame];
        let emit = self.emit;
        // The wireframe twin where a caller switched the view on, and `None`
        // everywhere else — see `set_wireframe`. Written on `ground_grid`'s
        // terms, and the `filter` is what keeps "switched on" from outrunning
        // "built": the field is only `Some` after a build that succeeded.
        //
        // The depth prepass below takes `shadow_pipeline` and is untouched by
        // this, which is the half that makes a wireframe frame legible: the
        // occlusion pair still reads solid depth. **It is also what makes this
        // one variable rather than two**: the colour pass's depth attachment
        // branches on it as well, because a wireframe is not the geometry the
        // prepass drew — see `MeshModules::color_depth_stencil`.
        let wireframe = self.wireframe_pipeline.filter(|_| self.wireframe_on);
        let bucket_draws = BucketDraws {
            pipeline: wireframe.unwrap_or(self.mesh_pipeline),
            layout: self.mesh_pipeline_layout,
            indices: self.pool.index_buffer(),
            emit,
            calls: self
                .bucket_constants
                .iter()
                .enumerate()
                .map(|(bucket, constant_offset)| {
                    let bucket = u32::try_from(bucket)
                        .unwrap_or_else(|_| unreachable!("a fixed table of a few buckets"));
                    (
                        *constant_offset,
                        self.draws.args_offset(bucket),
                        self.draws.count_offset(bucket),
                        self.draws.mesh_args_offset(bucket),
                    )
                })
                .collect(),
        };

        // Every draw this frame records: the shadow pass's, one per bucket in
        // **each** of the depth prepass and the forward pass, and one full-screen
        // triangle per full-screen pass. Assigned before the passes below borrow
        // the fields they need, so it is the count for the frame being built
        // rather than the one before it — and off this frame's resolved effects
        // rather than off a constant, so a switched-off effect's triangle is not
        // counted as submitted.
        //
        // The ground grid is added on top rather than folded into
        // `fullscreen_passes`: it is not a [`RenderEffects`] bit and is not
        // resolved by the toggle order — it is a caller's opt-in, off by
        // default — so what decides it is the field and not this frame's
        // effects.
        self.recorded_fullscreen = fullscreen_passes(effects, extent, upscaling, self.frame_ssao_blurs)
                + u64::from(self.ground_grid().is_some())
                + if draws_sky { SkyPass::PASSES } else { 0 }
                // The atlas viewer, on the ground grid's terms: not an effect
                // bit and not resolved by the toggle order, so what decides it
                // is the debug view and not this frame's effects.
                + if draws_atlas_view {
                    u64::from(AtlasView::PASSES)
                } else {
                    0
                };
        // The debug draw layer's own call, on the ground grid's terms: not an
        // effect bit, and decided by whether this frame's slot has a segment in
        // it. One direct draw of one instance covering every segment appended,
        // which is why it joins the full-screen triangles rather than the
        // indirect draws — `counters` counts both as submitted *and* drawn,
        // because the CPU knows what they cover.
        self.recorded_debug_draw = u64::from(self.debug_draw.records_pass(self.frame));
        // The shadow pass's tile resets, on the same terms: one over-sized
        // triangle of one instance per tile it redrew, and none at all on a
        // frame that cleared the whole attachment — see
        // [`MeshModules::depth_clear_pipeline`].
        self.recorded_tile_clears = shadow_tile_clears;
        self.recorded_draws =
            shadow_draws + 2 * bucket_draws.calls.len() as u64 + self.direct_draws();

        // --- the probe gather ---
        //
        // `docs/plan/50-irradiance-probes.md`'s updater, second half: the map
        // the pass above drew, read into every row of the table. **Here rather
        // than beside that pass** because the write is `&mut` — the group naming
        // the map's transient views is cached across frames, and a group naming a
        // transient cannot be built where the others are; see
        // [`crate::ssao::cached_group`].
        //
        // It is before the depth prepass and the forward pass, which is the
        // ordering that matters: both declare a read of these rows, so the graph
        // barriers this write against them.
        //
        // Which map layer a row is weighed against is the row's own index, so
        // the view is resolved exactly as the drawing passes resolve it — the
        // placeholder until `capture_probe_visibility` has run, and off entirely
        // where `r_probe_visibility` is off. A gather against the placeholder
        // occludes nothing, which is the honest answer for a scene that never
        // captured.
        let probe_visibility_view = match &self.probe_visibility {
            Some(captured) if crate::probe_visibility::enabled() => captured.view,
            _ => self.probe_visibility_placeholder.view,
        };
        let frame = self.frame;
        if let Some((gather, images)) = self.probe_gather.as_mut().zip(rsm_images) {
            gather.add_pass(graph, frame, images, probe_visibility_view, probe_table);
        }

        // --- the depth prepass ---
        //
        // `docs/plan/18-render-features.md`'s prepass, and it is unusually cheap:
        // `shadow_pipeline` is already the depth-only twin of the colour pipeline,
        // built from the same modules and the same layout, so driven with the
        // camera's draws and a copy of the camera's bind group it *is* a scene
        // depth prepass — no new pipeline and no new shader. It shares the
        // cascades' `depthVertexMain` too, which is why the split-stream vertex
        // pool pays for this pass as well as for the atlas.
        //
        // **Stored, unlike the depth the forward pass writes.** This is what the
        // occlusion pass samples, and it is the only reason
        // `TransientImageDesc::scene_depth` carries `SAMPLED`.
        //
        // # The overdraw win, and what it rests on
        //
        // With this depth in the buffer the colour pass tests `GreaterOrEqual`
        // with writes off — `PassBuilder::depth_read` and
        // `DepthStencilState::equal_depth_read_only` — so every hidden fragment's
        // clustered-forward shading goes away. See
        // `MeshModules::color_depth_stencil`.
        //
        // **It rests on `SV_Position.z` invariance between the two pipelines**,
        // which the engine does not decorate for: a fragment the colour pass
        // places a hair farther than this pass did is *rejected*, and that
        // arrives as holes in the frame rather than as an error. Two things make
        // it hold rather than hope. On the mesh path the two passes run the
        // **same** geometry stage — `depth_pipeline`'s doc says so — so there is
        // nothing to diverge. On the vertex path `depthVertexMain` is the same
        // clip position written the same way as `vertexMain`'s, out of the same
        // module compiled in one invocation. The observable is a screenshot:
        // holes are not subtle, and the render-e2e goldens are what would catch
        // them on a rasteriser this machine does not have.
        let depth_group = self.prepass_groups[self.frame];
        let prepass_draws = BucketDraws {
            pipeline: self.shadow_pipeline,
            ..bucket_draws.clone()
        };
        // The prepass's own cluster counter — see
        // [`ForwardRenderer::prepass_stats`]. Imported in the state the last frame
        // on this slot left it in, on `cluster-selection`'s terms exactly: a
        // barrier naming `Undefined` as its source carries no source scope, so it
        // would order this frame's write against nothing.
        let prepass_stats =
            import_selection(graph, "prepass-stats", self.prepass_stats[self.frame]);
        let prepass = graph
            .add_render_pass("depth-prepass")
            .depth(
                scene_depth,
                LoadOp::Clear,
                StoreOp::Store,
                crcbl_hal::ClearValue {
                    depth: crcbl_hal::depth::CLEAR,
                    ..crcbl_hal::ClearValue::default()
                },
            )
            // Both are in this pass's bind group. Nothing samples either — the
            // depth-only pipeline has no fragment stage — but a bound descriptor
            // whose image is in the wrong layout is what
            // `VUID-vkCmdDrawIndexedIndirectCount-imageLayout-00344` names, and
            // the other backends read whatever the last writer left behind.
            .read_image(shadow_atlas)
            .read_image(occlusion_placeholder)
            // And the page, on the same terms: `mesh_layout` names it in every
            // group, this pass's included. Nothing samples it here either, and
            // declaring it is what lets the graph order a copy into a page layer
            // against this pass — see `base_color_page_import`.
            .read_image(base_color_page)
            // And §2's normal page, which is in the same groups for the same
            // reason and is sampled here just as little.
            .read_image(normal_page)
            // And the probe rows, which `mesh_layout` names in this group too
            // and which the depth-only pipeline reads not at all — declared for
            // the colour pass's reason exactly: they are device-local and
            // writable-by-copy so a gather can fill them, and the graph orders a
            // write against a pass only if that pass declared the read.
            .read_buffer(probe_table);
        // `read_draw_sources` declares the *camera's* statistics buffer, because
        // that is the one the arguments came out of; the prepass writes its own
        // instead, so both are declared and the graph barriers both.
        let prepass = read_draw_sources(prepass, &generated, emit)
            .use_buffer(prepass_stats, ResourceState::ShaderReadWrite);
        // The skinned vertices, on the shadow pass's terms. This pass writes the
        // depth the occlusion pair samples and the forward pass tests against,
        // so a prepass reading the region before the dispatch is visible lays
        // down the previous pose's silhouette and the frame is rejected against
        // it.
        let prepass = match skinned {
            Some(vertices) => prepass.read_buffer(vertices),
            None => prepass,
        };
        // The camera's own cut, written here and again by the forward pass with
        // the same camera and the same budget. Shared rather than a buffer of its
        // own — unlike a cascade's, which a *later* pass would overwrite before
        // anything could read it — because the second write is the one that stands
        // and it writes the same words.
        let prepass = match selection {
            Some(selection) if emit.is_mesh() => {
                prepass.use_buffer(selection, ResourceState::ShaderReadWrite)
            }
            _ => prepass,
        };
        prepass.execute(move |ctx| {
            let encoder = ctx.encoder();
            prepass_draws.open(encoder);
            prepass_draws.record(encoder, depth_group, &generated);
        });

        let frame = self.frame;
        // `docs/plan/18-render-features.md`'s occlusion pair, or the one texel
        // that stands for "no occlusion was computed" where it is switched off.
        //
        // # The switched-off arm is the 1×1 placeholder, and the shader is what
        // makes that work
        //
        // Topic 18 sanctions the placeholder — "a renderer-owned 1×1 `R8Unorm`
        // cleared to 1.0, bound when the AO passes are not added" — and it is
        // what this binds. The property it rests on is not free: `mesh.slang`
        // reads this channel with a `Load` at `SV_Position.xy`, and a `Load`
        // outside a texture's extent yields **zero** rather than the nearest
        // texel, so the fetch has to be clamped against the image's own extent
        // or a one-texel image occludes everything but the origin. That clamp is
        // in `mesh.slang` and `crcbl`'s `forward_e2e::depth_probe` is what asks
        // whether it is; a frame drawn without it is black wherever ambient is the whole of
        // the light, on real hardware, with nothing reporting an error.
        //
        // So an AO-off frame records **no occlusion pass at all** and takes no
        // frame-sized image out of the transient pool: no shader permutation, no
        // uniform branch, one pipeline, and a bound value that occludes nothing.
        //
        // The placeholder's other job is unchanged — filling the binding for the
        // two depth-only passes, neither of which has a fragment stage and so
        // neither of which ever samples it.
        let occlusion = match occlusion_chain {
            Some(images) => self
                .ssao
                .add_passes(graph, frame, extent, scene_depth, images),
            None => occlusion_placeholder,
        };
        // `docs/plan/45-shadows.md`'s contact march, or the one texel that
        // stands for "the sun reaches this surface" where it is switched off.
        //
        // The occlusion pair's arms exactly, and the clamp the off-arm rests on
        // is `mesh.slang`'s `contact_at`. What differs is what it costs to get
        // wrong: an unclamped `Load` past this placeholder's one texel would
        // report *total* shadow at every pixel but one, so a frame with the
        // effect off would lose its sun rather than its ambient.
        //
        // Recorded here, after the occlusion pair and before the forward pass,
        // because the forward pass is what binds it — and the prepass it reads
        // has already run.
        let contact = match contact_mask {
            Some(mask) => self
                .contact_shadows
                .add_passes(graph, frame, scene_depth, mask),
            None => contact_placeholder,
        };

        let pass = graph
            .add_render_pass("forward")
            .clear_color(scene_color, SCENE_CLEAR)
            // `mesh.slang`'s second target, and **cleared rather than loaded or
            // discarded**. A pixel no geometry covered has no material, and the
            // pass that will read this marches a ray from whatever it finds
            // there — so the value that has to be in it is the one that says
            // "nothing reflects here", not the last frame's or an undefined
            // one. `NO_REFLECTION` is that value: no `F0`, and fully rough —
            // the alpha is a roughness, so a zero there would read as a
            // mirror. No picture tells the two apart, because a zero `F0`
            // leaves the reflection nothing to scale but Schlick's grazing
            // tail; what the alpha buys is that `ssr.slang` takes its early
            // return on every uncovered pixel instead of marching the sky.
            .clear_color(reflectivity, ssr::NO_REFLECTION)
            // `mesh.slang`'s third target, **cleared to zero** on the
            // reflectivity attachment's terms: a pixel no geometry covered has
            // no motion of its own, and zero is what a consumer reading its
            // history at `uv - motion` needs there — it reads the same pixel
            // back. It is not the *right* answer for the sky, whose motion is
            // the camera's and is owed to the pass that first wants it; it is
            // the answer that reprojects onto itself rather than onto
            // somewhere undefined. `docs/backlog.md` carries what is owed.
            .clear_color(motion, [0.0; 4]);
        // **Loaded and stored rather than cleared, and read-only**, which is the
        // half of the depth prepass that pays for the pass itself:
        // `MeshModules::color_depth_stencil` tests `GreaterOrEqual` with writes
        // off, so a fragment survives only where the prepass already put its
        // surface — and the clustered-forward shading of everything a nearer
        // surface goes on to cover is never run. It is stored because the
        // reflection march is downstream and reads exactly this image; the
        // values are the prepass's, which are this pass's own answer written by
        // the same transform.
        //
        // **The wireframe frame keeps the old shape**, because
        // `PolygonMode::Line` does not reproduce the prepass's depths — see
        // `color_depth_stencil`. It must *clear*: loading the prepass's depth
        // under the default `Greater` test rejects every fragment of the same
        // geometry, and the frame goes black.
        let pass = match wireframe {
            Some(_) => pass.depth(
                scene_depth,
                LoadOp::Clear,
                StoreOp::Store,
                crcbl_hal::ClearValue {
                    depth: crcbl_hal::depth::CLEAR,
                    ..crcbl_hal::ClearValue::default()
                },
            ),
            None => pass.depth_read(scene_depth),
        };
        let pass = pass
            // The occlusion channel this frame's ambient term is scaled by. On
            // an AO-on frame the blur pass wrote it as a colour attachment a
            // moment ago, so this declaration is the barrier into a
            // shader-readable layout; on an AO-off frame it is the imported
            // placeholder, which is in that layout already and has nothing to
            // transition.
            .read_image(occlusion)
            // The contact-shadow channel this frame's sun is scaled by, declared
            // on the occlusion channel's terms exactly: the march wrote it as a
            // colour attachment a moment ago and this is the barrier into a
            // shader-readable layout, or it is the imported placeholder and there
            // is nothing to transition.
            .read_image(contact)
            // **The page this pass's materials actually sample**, and the one
            // pass of the three where that is literally true. Declared so the
            // graph can order a caller's copy into a page layer against these
            // draws — see `base_color_page_import`.
            .read_image(base_color_page)
            // **And the normal page this pass's materials perturb through**,
            // which is the other of the two pages that is literally sampled
            // here — see `normal_page_import`.
            .read_image(normal_page)
            // **The barrier out of the shadow pass's depth attachment.** The
            // atlas is in this pass's bind group at `SHADOW_ATLAS_BINDING`, and
            // without this declaration the graph leaves it in
            // `DepthStencilWrite` — which Vulkan reports as
            // `VUID-vkCmdDrawIndexedIndirectCount-imageLayout-00344` naming the
            // binding, and which every other backend reads as whatever the
            // depth writes left behind.
            .read_image(shadow_atlas)
            // The froxel grid, on the shadow atlas's terms exactly: the
            // clustering pass left it in `ShaderReadWrite` and the fragment
            // stage has it bound, so declaring the read is what moves it — and
            // without the declaration the fragment stage reads a buffer the
            // compute pass may still be writing.
            .read_buffer(light_grid)
            // The probe rows, on the froxel grid's terms and for a hazard that
            // does not exist yet: nothing writes a probe on the GPU today, so
            // this frame's slot is already in `ShaderRead` and the graph has
            // nothing to transition. It is declared anyway, because the rows are
            // device-local and `TRANSFER_DST` precisely so
            // `docs/plan/50-irradiance-probes.md`'s gather can write them — and
            // a pass that binds a buffer without declaring it is a pass the
            // graph cannot barrier the day something does.
            .read_buffer(probe_table);
        // And the skinned vertices, which is the declaration this whole entry
        // point exists for: without it the vertex stage pulls a region the
        // compute dispatch may still be writing, and the graph — which is told
        // about every other hazard in the frame — has not been told about this
        // one.
        let pass = match skinned {
            Some(vertices) => pass.read_buffer(vertices),
            None => pass,
        };
        let pass = read_draw_sources(pass, &generated, emit);
        let pass = match selection {
            // The colour pass's own, which no cascade writes — the cascades
            // record into buffers of their own, so what survives a frame here is
            // the camera's cut and what survives there is each cascade's. The
            // depth prepass above writes this one too, with the same camera and
            // the same budget, and is ordered before this pass by the graph.
            Some(selection) if emit.is_mesh() => {
                pass.use_buffer(selection, ResourceState::ShaderReadWrite)
            }
            _ => pass,
        };

        // The camera's group rebuilt against the two screen-space channels the
        // graph just realised, cached against both views and the probe
        // visibility maps — the shape the tonemap group below has, and for the
        // same reason: a graph transient's view is not known until execute time.
        // Three entries of the stored list differ; see
        // `ForwardRenderer::mesh_group_entries`.
        //
        //
        // The rebuild is unconditional, so there is one shape of forward pass
        // rather than two: the group is cached against the views it was built
        // from, so an AO-off frame naming the placeholder's view and an AO-on
        // frame naming the blur's target are the same code and one cache miss
        // apiece when a toggle moves. **The key is both views**, because a group
        // naming two transients is stale as soon as either one moves.
        let entries = self.mesh_group_entries[self.frame].clone();
        let mesh_layout = self.mesh_layout;
        // The captured probe-visibility maps, or the one-texel placeholder when
        // nothing has been captured or the console switch is off. It rides
        // through the same cache as the two channels beside it — it is not a
        // graph transient, but it does change between frames when the switch
        // moves, and a group cached against a view it no longer names is the
        // failure that cache exists to stop.
        let probe_visibility_view = match &self.probe_visibility {
            Some(captured) if crate::probe_visibility::enabled() => captured.view,
            _ => self.probe_visibility_placeholder.view,
        };
        let cached_mesh = &mut self.screen_channel_groups[self.frame];
        pass.execute(move |ctx| {
            let view = ctx.image_view(occlusion);
            let contact_view = ctx.image_view(contact);
            let device = ctx.device();
            let group = cached_group(
                cached_mesh,
                device,
                &[
                    (AMBIENT_OCCLUSION_BINDING, view),
                    (CONTACT_SHADOW_BINDING, contact_view),
                    (PROBE_VISIBILITY_BINDING, probe_visibility_view),
                ],
                "mesh frame",
                mesh_layout,
                entries,
            )
            // Falling back to the group built at `build` rather than dropping
            // the frame, and **the fallback costs the occlusion and the contact
            // shadow and nothing else**: that group names the two 1×1 white
            // placeholders, which `mesh.slang` clamps its `Load`s into and reads
            // as "nothing occludes" and "the sun reaches here". A descriptor
            // failure therefore draws the frame this scene would have drawn with
            // both effects switched off, rather than one with no ambient term
            // and no sun in it.
            .unwrap_or(group);
            let encoder = ctx.encoder();
            bucket_draws.open(encoder);
            bucket_draws.record(encoder, group, &generated);
        });

        // --- the background ---
        //
        // **After the forward pass and before everything that reads the scene
        // colour**, which is what makes the sky the background of the frame the
        // reflection composite, the bloom chain and the tonemap all work on
        // rather than a colour added at the end.
        //
        // It does not disturb the reflection itself. The march reads the scene
        // colour only at a crossing it found in the depth prepass, and the far
        // plane has no surface to cross to — `ssr.slang` returns before it
        // marches on a pixel whose depth is the clear value — so nothing this
        // pass writes is ever tapped as a reflected colour. A ray that leaves
        // the frame still falls back to the analytic gradient rather than to
        // these texels, which is the same sky evaluated exactly.
        //
        // Conditional on the sky and not on [`RenderEffects`], on the ground
        // grid's terms: see [`crate::sky_pass`] for why the off position has to
        // be no pass at all.
        if draws_sky {
            self.sky_pass
                .add_pass(graph, frame, scene_color, scene_depth);
        }

        // `docs/plan/51-volumetrics.md`'s froxel volume, and it composites over
        // the sky as well as over the geometry — a pixel at the far plane is a
        // whole column of air, which is exactly what makes a distant horizon
        // read as distant.
        //
        // **Before the reflection march**, where `mesh.slang`'s closed form also
        // ran: the march reads the scene colour as reflected radiance, and fog
        // it can see is fog the surface it bounced off could see. The reflection
        // the blur adds afterwards is still unfogged, which is the same gap the
        // analytic path has and `docs/backlog.md` carries.
        let scene_color = match fogged {
            Some(composited) => {
                self.volumetric.add_passes(
                    graph,
                    frame,
                    self.grid,
                    VolumetricImages {
                        depth: scene_depth,
                        color: scene_color,
                        composited,
                        shadow_atlas,
                    },
                    light_grid,
                );
                composited
            }
            None => scene_color,
        };

        // `docs/plan/18-render-features.md`'s reflection march and its blur, and
        // **the second of them is the composite**: the march reads the scene
        // colour, the depth prepass and the reflectivity attachment and writes
        // the reflection alone, and the blur filters that and adds it to the
        // scene colour — so everything below this line works on `tonemapped`
        // rather than on `scene_color`. A frame that does not add the pair hands
        // `scene_color` on and is bit-identical, which is this effect's whole
        // off-switch.
        let tonemapped = match reflected {
            Some((reflection, composited)) => {
                // The pyramid first: the march climbs it, so every level has to
                // be written before the pass that reads it is recorded. Skipped
                // outright on a frame whose extent has no levels, which
                // `Hiz::add_passes` would record zero passes for anyway.
                self.hiz
                    .add_passes(graph, frame, extent, scene_depth, &pyramid);
                self.ssr.add_passes(
                    graph,
                    frame,
                    SsrImages {
                        depth: scene_depth,
                        color: scene_color,
                        reflectivity,
                        reflection,
                        composited,
                        pyramid: crate::hiz::level_slots(scene_depth, &pyramid),
                    },
                    probe_buffer,
                    probe_table,
                    // The same view the forward pass's own group named, so the
                    // reflection's probe fallback is weighed by the maps the
                    // diffuse gather was weighed by — or by the placeholder,
                    // when there are none and every probe keeps all of its
                    // weight.
                    probe_visibility_view,
                );
                composited
            }
            None => scene_color,
        };

        // `docs/plan/18-render-features.md`'s bloom chain, and it slots in
        // exactly where the reflection composite left off: it reads whatever the
        // tonemap was about to read, writes a new full-resolution image, and the
        // tonemap reads that instead. A frame that does not add it hands the
        // image on untouched and is bit-identical, which is this effect's whole
        // off-switch — and the group the tonemap builds below is cached against
        // its source view, so a toggle costs one cache miss and nothing else.
        let tonemapped = match &bloomed {
            Some((mips, composited)) => {
                self.bloom
                    .add_passes(graph, frame, extent, tonemapped, mips, *composited);
                *composited
            }
            None => tonemapped,
        };

        // `docs/plan/43-render-standards.md` §6's auto-exposure, and it reads
        // the image the tonemap is about to read: the frame with the medium, the
        // reflection and the chain already in it, which is the picture a viewer
        // sees and therefore the one to expose for. Binning `scene_color`
        // instead would expose the frame for a picture nobody looks at.
        //
        // Read first because the passes borrow the ring for the rest of this
        // function, and the tonemap below needs the handle out of it.
        let measured = self.auto_exposure.measured(frame);
        if self.frame_effects.contains(RenderEffects::AUTO_EXPOSURE) {
            self.auto_exposure
                .add_passes(graph, frame, extent, tonemapped);
        }

        // --- the debug draw layer ---
        //
        // **Before the tonemap, into the HDR image the tonemap is about to
        // read**, which is `docs/plan/18-render-features.md`'s interaction rule:
        // "Debug overlays (debug draw, gizmos) render pre-tonemap in HDR
        // (they're in the world) except UI-space panels." A segment is therefore
        // exposed and tonemapped like the geometry it annotates, which is the
        // opposite of the ground grid below — and [`crate::grid`]'s header and
        // [`crate::debug_draw`]'s carry the two halves of that argument.
        //
        // **And after everything that reads the scene colour.** The reflection
        // march, the bloom chain and the auto-exposure histogram have all
        // already been recorded against this image, so a debug line does not
        // reflect in a surface, does not bloom, and does not move the exposure
        // the scene is metered at. Each of those would be an overlay changing
        // the picture it is drawn over, which is the one thing an overlay must
        // not do.
        //
        // Nothing here is conditional on [`RenderEffects`], on the ground
        // grid's terms: a frame that appended no segment is the frame this
        // renderer recorded before [`crate::debug_draw`] existed — no pass, no
        // pipeline, no block.
        self.debug_draw
            .add_pass(graph, frame, tonemapped, scene_depth);

        // The tonemap group names a *graph-owned* view, so it can only be built
        // once the graph has realised one. It is cached against the view handle
        // and therefore rebuilt only on a resize.
        let sampler = self.sampler;
        let layout = self.tonemap_layout;
        let pipeline_layout = self.tonemap_pipeline_layout;
        let tonemap_pipeline = self.tonemap_pipeline;
        let exposure_block = self.tonemap_uniforms[self.frame];
        let cached = &mut self.tonemap_groups[self.frame];

        graph
            .add_render_pass("tonemap")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
            .color(
                display,
                LoadOp::DontCare,
                StoreOp::Store,
                crcbl_hal::ClearValue::default(),
            )
            // **The reflection pass's output where there is one, and the forward
            // pass's where there is not.** The two are the same description and
            // different images, and tonemapping the first one on a frame that
            // reflected would compile, draw a picture, and silently be the frame
            // without reflections in it.
            .read_image(tonemapped)
            .execute(move |ctx| {
                let view = ctx.image_view(tonemapped);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::Sampler(sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(exposure_block),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(measured),
                    },
                ];
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(0, view)],
                    "tonemap scene",
                    layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(tonemap_pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                // Three vertices, no geometry bound, no vertex buffer anywhere.
                encoder.draw(0..3, 0..1);
            });

        // --- the ground grid ---
        //
        // **After the tonemap, into the target the tonemap just wrote**, and
        // that placement is the decision rather than an accident of ordering.
        // The grid is reference chrome, not scene content: drawn into
        // `scene_color` it would be exposed and tonemapped like geometry, so its
        // colour would shift with how bright the scene happens to be — and a
        // grid whose lines change with the exposure is no longer a reference.
        // Blender draws its overlays the same way, in display space after the
        // render.
        //
        // It still takes `scene_depth`, read-only, so geometry in front of the
        // ground occludes it. That the depth survives this far is not luck: the
        // forward pass stores it (`StoreOp::Store`) because the reflection march
        // reads it, and the graph moves it from whatever state that left it in
        // into `DepthStencilRead` for this pass.
        //
        // Nothing here is conditional on [`RenderEffects`]: the grid is a
        // caller's opt-in, and a frame that never asked for one is the frame
        // this renderer recorded before [`crate::grid`] existed — no pass, no
        // pipeline, no block.
        if let Some(grid) = self.ground_grid.as_ref()
            && self.ground_grid_on
        {
            let view_proj = self.camera_view_proj;
            grid.add_pass(
                graph,
                frame,
                display,
                scene_depth,
                view_proj,
                // The one inversion in the frame, and it is here rather than in
                // the pass: `begin_frame` has no reason to compute it for a grid
                // that is usually off.
                view_proj.inverse(),
            );
        }

        // --- the shadow atlas viewer ---
        //
        // **After the grid and before the resolve, and it replaces what both of
        // them were about.** `docs/plan/sample/18-sundial.md`'s atlas viewer:
        // the `D32Float` image the shadow pass filled, drawn over the finished
        // frame so that which slot holds which map is something a reviewer can
        // look at. [`crate::atlas_view`] carries why it draws here — in display
        // space, after the operator — rather than into the scene colour.
        //
        // After the grid because a grid over a readout is noise; before the
        // resolve because there is nothing to resolve, and nothing to resolve
        // *with*: [`Self::resolved_effects`] takes both antialiasing tiers off
        // for every debug view, so `display` and `present` are one image on
        // every frame this branch runs on.
        //
        // Nothing here is conditional on [`RenderEffects`], on the ground
        // grid's terms: a frame that resolved any other view is the frame this
        // renderer recorded before this module existed — no pass, no pipeline,
        // no block read.
        if draws_atlas_view {
            self.atlas_viewer
                .add_pass(graph, frame, shadow_atlas, display);
        }

        // --- the antialiasing resolve ---
        //
        // **After the grid, and that is the same decision the grid's placement
        // was.** The grid is a field of thin high-contrast lines, which is the
        // thing an edge filter exists for; drawing it into the target *after*
        // the resolve would leave it the one aliased element in an antialiased
        // frame. The UI goes the other way and is composited onto `target` by
        // the caller after this, so its glyphs are never filtered — topic 18
        // refuses to antialias text in as many words.
        //
        // `display` is `present` when neither antialiasing bit is on, and this
        // is the branch that makes that true: no pass, no second image, and the
        // frame the tonemap wrote is already where the frame ends.
        //
        // **Which tier fills the slot is decided here, once.** SMAA is the
        // higher one and takes it whenever its bit is on, whatever the FXAA bit
        // says — the two are never both recorded, which is what
        // [`RenderEffects::SMAA`] means by a tier that is off being a frame with
        // fewer passes rather than a shader branch. Everything the paragraph
        // above says about the grid, the UI and the ordering holds for either.
        if display != present {
            if let Some((edges, weights)) = smaa_images {
                self.smaa
                    .add_passes(graph, frame, display, edges, weights, present);
            } else {
                self.fxaa.add_pass(graph, frame, display, present);
            }
        }

        // --- the render-scale upscale ---
        //
        // **After the resolve, and that ordering is not interchangeable.**
        // Either tier filters the edges the renderer actually drew; run the
        // other way round
        // it would be filtering an interpolation of them, which is both more
        // expensive — the resolve would run at the target's extent rather than
        // the internal one — and worse, because the edge it is looking for has
        // already been spread across several target texels.
        //
        // The UI goes the other way and is composited onto `target` by the
        // caller after this, at native resolution. That is the whole reason a
        // render-scale knob is usable: the 3D frame gets cheap and the text does
        // not get soft.
        if present != target {
            self.upscale.add_pass(graph, frame, present, target);
        }

        // **Last, and that is the point.** Three passes add to the statistics
        // buffer — the cull dispatch, the light grid's overflow counter and the
        // amplification stage inside the forward pass — so a copy scheduled any
        // earlier would take a total the frame had not finished writing. The
        // graph puts it in `TransferSrc` for this and back into the state the
        // next frame on this slot imports it in; there is not a barrier written
        // here.
        if let Some(stats) = self.cull_stats.as_mut() {
            stats.add_copy_pass(graph, generated.visible_count_id);
        }

        tonemapped
    }

    /// Adds the cull dispatches and the depth-only pass that fill the shadow
    /// atlas, and returns the atlas as the graph knows it.
    ///
    /// Every tile is one viewport of one render pass. A pass per tile would be
    /// one clear of the same image and one more barrier per tile, and the graph
    /// would have to be told each of them only touches part of it — where one
    /// pass with a viewport per tile is what a shadow *atlas* is for in the first
    /// place.
    ///
    /// **Only the occupied tiles get a cull and a viewport.** The cascades
    /// always are; a light tile is occupied when
    /// [`shadow::Selection`] gave it to a light. A free one keeps the
    /// reversed-Z clear the pass wrote, which is `0.0` — "nothing stored, as far
    /// away as depth goes" — so anything that did sample it would come back
    /// fully lit rather than fully shadowed.
    ///
    /// # With [`RenderEffects::SHADOWS`] off, every tile is a free one
    ///
    /// That is the whole switch, and it is the same mechanism a free tile
    /// already had rather than a second one: no cull is dispatched and no
    /// viewport is drawn, the pass records its clear and nothing else, and every
    /// comparison against the atlas comes back fully lit. The pass itself stays
    /// on the frame that switches it off — it is what *writes* that clear, and a
    /// frame that skipped it would leave the atlas holding the last frame that
    /// did draw into it, or undefined memory on the first frame of all.
    ///
    /// # A cached atlas records nothing at all
    ///
    /// `docs/plan/45-shadows.md`'s static-caching rung. When
    /// [`ForwardRenderer::shadow_atlas_cached`] says the image already holds
    /// what this frame would draw, this adds **no cull and no pass**: the atlas
    /// is imported so the passes that sample it have an edge to the resource,
    /// and it keeps the contents an earlier frame left in it. That is why the
    /// image is a persistent one with a tracked ledger entry rather than a
    /// transient — nothing else in the frame can alias it, so what it holds is
    /// what the last pass to write it wrote.
    ///
    /// # Per group, and what pays for the tiles it keeps
    ///
    /// `docs/plan/45-shadows.md`'s cadence rung. A **group** — a cascade, or a
    /// light slot's whole run of tiles — is redrawn or held on its own, and
    /// [`ForwardRenderer::shadow_group_redrawn`] is what says which. A frame
    /// that holds nothing clears the whole attachment, which is the recording
    /// that shipped before the rung; a frame that holds something **loads** the
    /// attachment instead and resets each tile it redraws with
    /// [`MeshModules::depth_clear_pipeline`], which is the portable statement of
    /// a clear bounded to one tile.
    ///
    /// The attachment's own [`LoadOp::Clear`] cannot serve there: it covers
    /// every tile including the ones being kept, and the region-bounded
    /// alternatives are not portable — Metal's load action and WebGPU's
    /// `loadOp` have no render area to bound them, so a clear that covered part
    /// of the image on Vulkan and all of it on Metal would be a held tile that
    /// survives on one backend and is erased on another.
    fn add_shadow_pass(
        &self,
        graph: &mut RenderGraph<'_>,
        pool: &TransientPool,
        selection: &[BufferId],
        reads: ShadowReads,
    ) -> (ImageId, u64, u64, Option<RsmImages>) {
        let shadows = self.frame_effects.contains(RenderEffects::SHADOWS);
        let (atlas_width, atlas_height) = shadow::atlas_extent();
        let atlas = graph.import_image(
            "shadow-atlas",
            ImportedImage {
                image: self.shadow_atlas,
                view: self.shadow_atlas_view,
                format: Format::D32Float,
                extent: (atlas_width, atlas_height),
                // What the previous executed frame left it in — `Undefined` on
                // the first, which is what makes the graph give it a layout at
                // all.
                initial: imported_state(pool, self.shadow_atlas),
                // The renderer keeps this image across frames, so the pool's
                // ledger is the record of what happened to it and the line above
                // is that record read back rather than a second copy of it.
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );
        // The cached frame's whole answer: the image above and nothing that
        // touches it. It is already in `ShaderRead` — the frame that drew it
        // handed it back that way — so the graph has no transition to make, and
        // the passes that sample it read the maps an earlier frame left there.
        if self.shadow_atlas_cached {
            // **And no reflective shadow map either**, which cannot happen while
            // the updater is on: `begin_frame` keeps cascade 0 off the hold in
            // exactly that case, so a frame reaching here has nothing to draw
            // the map from. See `probe_update_runs`.
            return (atlas, 0, 0, None);
        }
        let placeholder = graph.import_image(
            "shadow-placeholder",
            ImportedImage {
                image: self.shadow_placeholder,
                view: self.shadow_placeholder_view,
                format: Format::D32Float,
                extent: (1, 1),
                // **Its own ledger entry, not the atlas's.** The two move
                // together today — both are imported by this function on every
                // frame and both are handed back in `ShaderRead` — so one lookup
                // would answer both, and that is exactly the coincidence a
                // second declared state here stops depending on.
                initial: imported_state(pool, self.shadow_placeholder),
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );

        // Which light slots have a light this frame, resolved once: a slot's
        // cull is dispatched and its faces are drawn together or not at all, and
        // `begin_frame` filled that generator's parameters under exactly this
        // condition.
        //
        // **And only the ones this frame redraws**, since the cadence rung: a
        // held slot keeps the texels it has, so recording its cull and its
        // draws would be the same picture at the full price.
        let occupied: Vec<(usize, shadow::Assignment, &Light)> = self
            .shadow_lights
            .slots()
            .iter()
            .enumerate()
            .filter(|_| shadows)
            .filter(|(slot, _)| self.shadow_group_redrawn[shadow_cull(*slot)])
            .filter_map(|(slot, held)| {
                let held = (*held)?;
                Some((slot, held, self.extra_lights.get(held.light)?))
            })
            .collect();

        // One cull dispatch per cascade and per occupied slot, before the pass
        // that draws from them — **not one per tile**, which is topic 18's
        // fourth decision: a point light's six faces draw one visible set.
        let cascades: Vec<usize> = if shadows {
            (0..shadow::CASCADES)
                .filter(|cascade| self.shadow_group_redrawn[*cascade])
                .collect()
        } else {
            Vec::new()
        };
        // **Cascade 0's cull, on a frame that is not drawing it into the
        // atlas.** `begin_frame` fits it whenever the updater is on, shadows or
        // no shadows — see `rsm_cull_ready` — so this is the case where the map
        // is drawn and the atlas keeps its clear. Chained **last**, because the
        // occupied slots' entries are indexed by `cascades.len() + index` a few
        // lines below and an insertion before them would hand a light's draws to
        // the wrong viewport.
        let rsm_only_cull = self.rsm_cull_ready && !cascades.contains(&0);
        // The punctual producer's faces, which are also the light slots'
        // atlas views — see `punctual_faces`, the one list both halves of the
        // updater walk.
        let punctual = self.punctual_faces();
        // And their culls on a frame that is not drawing the atlas, which
        // `occupied` is empty on for the same reason `cascades` is. Deduplicated
        // because a point light's six faces draw one visible set, and chained
        // **after** the entries `views` indexes positionally below.
        let punctual_only_culls: Vec<usize> = if shadows {
            Vec::new()
        } else {
            punctual
                .iter()
                .map(|face| face.cull)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        let generated: Vec<(usize, GeneratedDraws)> = cascades
            .iter()
            .copied()
            .chain(occupied.iter().map(|(slot, _, _)| shadow_cull(*slot)))
            .chain(rsm_only_cull.then_some(0))
            .chain(punctual_only_culls)
            .map(|cull| {
                (
                    cull,
                    self.shadow_draws[cull].add_passes(
                        graph,
                        self.frame,
                        self.instances.slot_count(),
                    ),
                )
            })
            .collect();

        // What the pass body records, in order: which view's bind group, which
        // rectangle of the atlas its viewport covers, and which of the culls
        // above it draws from. Paired rather than positional, because the free
        // slots are missing from every one of these lists and a bare index into
        // one would hand a light's draws to the wrong viewport the moment one
        // slot is free and a later one is not.
        //
        // **The rectangle rather than the slot**, resolved here: the body runs
        // in a closure that outlives this borrow, and asking the selection for
        // it there would be a second reading of an allocation that has to be the
        // one the frame block was written from.
        let mut views: Vec<(usize, shadow::TileRect, usize)> = cascades
            .iter()
            .enumerate()
            .map(|(position, cascade)| {
                (*cascade, self.shadow_lights.atlas_rect(*cascade), position)
            })
            .collect();
        for (index, (slot, held, light)) in occupied.iter().enumerate() {
            // A spot draws face 0 alone; a point light draws all six, each
            // through its own matrix into its own tile. `shadow::tile_span` is
            // the same function `Selection` allocated the run with, so a face
            // here and a tile there cannot disagree about how many there are.
            for face in 0..shadow::tile_span(light) {
                views.push((
                    shadow_view(*slot, face),
                    self.shadow_lights
                        .atlas_rect(shadow::light_tile(held.base + face)),
                    cascades.len() + index,
                ));
            }
        }

        let mut pass = graph
            .add_render_pass("shadow")
            // **Stored, unlike the scene's depth.** This is the one depth buffer
            // in the engine that something downstream reads, which is what
            // `PassBuilder::clear_depth`'s docs said would need a `StoreOp` the
            // day a prepass existed.
            .depth(
                atlas,
                // **Loaded on a frame that is keeping a tile**, which is the
                // cadence rung's whole seam: a clear here covers the image, so
                // the tiles this frame *does* redraw are reset one at a time by
                // the pipeline below instead. A frame keeping nothing clears,
                // which is what shipped.
                if self.shadow_atlas_clears {
                    LoadOp::Clear
                } else {
                    LoadOp::Load
                },
                StoreOp::Store,
                crcbl_hal::ClearValue {
                    depth: crcbl_hal::depth::CLEAR,
                    ..crcbl_hal::ClearValue::default()
                },
            )
            // Declared so the graph gives it a shader-read layout: it is in
            // every cascade's bind group, standing in for the atlas this pass is
            // writing. See `ForwardRenderer::shadow_placeholder`.
            .read_image(placeholder)
            // Likewise, and it is in every one of those groups too — see
            // `ForwardRenderer::ambient_occlusion_placeholder`. Nothing samples
            // it here either: the depth-only pipeline has no fragment stage.
            .read_image(reads.occlusion_placeholder)
            // And §3.2's page, which `mesh_layout` names in every group — a
            // cascade's included. See `ForwardRenderer::base_color_page_import`
            // for what the declaration buys a caller that writes the page.
            .read_image(reads.pages.base_color)
            // And §2's normal page beside it, in those same groups and sampled
            // by no cascade — see `ForwardRenderer::normal_page_import`.
            .read_image(reads.pages.normal)
            // And the probe rows, which are in those groups too and which no
            // cascade reads — declared on the placeholder's and the pages'
            // terms, plus one of their own: the rows are device-local and a copy
            // destination so `docs/plan/50-irradiance-probes.md`'s gather can
            // write them, and a pass that binds a buffer it has not declared is
            // one the graph cannot order that write against.
            .read_buffer(reads.probes);
        // Each tile's mesh pass records the cut it descended to, into a buffer
        // of its own — see `ForwardRenderer::shadow_selection`. Empty where
        // there is no amplification stage to descend anything.
        //
        // Declared for every tile rather than for the occupied ones: the state
        // is what the buffer is *in*, which does not depend on whether this
        // frame happened to write it, and a free tile's buffer still has to
        // arrive in the state the next frame's import claims.
        for &buffer in selection {
            pass = pass.use_buffer(buffer, ResourceState::ShaderReadWrite);
        }
        // The skinned vertices this pass may pull, and the reason it is declared
        // here as well as on the colour passes: a cascade draws the same
        // geometry out of the same pool, so a shadow that fell before the
        // dispatch was visible would be cast by the previous pose — a shadow
        // that does not match its caster, on a frame whose picture is otherwise
        // right.
        if let Some(vertices) = reads.skinned {
            pass = pass.read_buffer(vertices);
        }
        for (_, draws) in &generated {
            pass = read_draw_sources(pass, draws, self.emit);
        }

        let groups = self.shadow_groups[self.frame].clone();
        // **What the reflective shadow map is drawn from**, read out here rather
        // than reconstructed by the caller: cascade 0's bind group — whose
        // uniform block already holds that cascade's `view_proj`, which is what
        // "reusing cascade 0's matrix" means — and the `GeneratedDraws` its cull
        // produced a moment ago. Both are `Copy`, so recording a second pass
        // from them costs no second cull and no second matrix.
        //
        // [`None`] on a frame that is not drawing cascade 0 at all: shadows off,
        // or a `r_shadow_cadence` past one that held the group anyway. The rows
        // then keep what the last gather wrote — see [`ProbeUpdate::EveryFrame`],
        // which says so.
        let cascade_zero: Option<(BindGroupHandle, GeneratedDraws)> = self
            .rsm_cull_ready
            .then(|| {
                generated
                    .iter()
                    .find(|(cull, _)| *cull == 0)
                    .map(|(_, draws)| (groups[CASCADE_ZERO_VIEW], *draws))
            })
            .flatten();
        // **What the punctual reflective shadow map is drawn from**, resolved
        // here for `cascade_zero`'s reason and on its terms: each face's own
        // bind group — whose uniform block already holds that face's
        // `view_proj` — and the `GeneratedDraws` its slot's cull produced a
        // moment ago. Both are `Copy`, so the second pass costs no second cull
        // and no second matrix.
        //
        // Found by cull id rather than by position, so this list and the atlas's
        // cannot be knocked out of step by a slot the other one skipped.
        let punctual_views: Vec<(BindGroupHandle, shadow::TileRect, GeneratedDraws)> = punctual
            .iter()
            .filter_map(|face| {
                let (_, draws) = generated.iter().find(|(cull, _)| *cull == face.cull)?;
                Some((groups[face.view], face.tile, *draws))
            })
            .collect();
        // The culls those faces draw from, **once each**: a point light's six
        // faces draw one visible set, so declaring a face's sources would
        // declare the same buffers six times in one pass.
        let punctual_sources: Vec<GeneratedDraws> = punctual
            .iter()
            .map(|face| face.cull)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter_map(|cull| {
                let (_, draws) = generated.iter().find(|(candidate, _)| *candidate == cull)?;
                Some(*draws)
            })
            .collect();
        let bucket_draws = BucketDraws {
            pipeline: self.shadow_pipeline,
            layout: self.mesh_pipeline_layout,
            indices: self.pool.index_buffer(),
            emit: self.emit,
            calls: self
                .bucket_constants
                .iter()
                .enumerate()
                .map(|(bucket, constant_offset)| {
                    let bucket = u32::try_from(bucket)
                        .unwrap_or_else(|_| unreachable!("a fixed table of a few buckets"));
                    (
                        *constant_offset,
                        self.draws.args_offset(bucket),
                        self.draws.count_offset(bucket),
                        self.draws.mesh_args_offset(bucket),
                    )
                })
                .collect(),
        };
        // Kept back for the reflective shadow map below, which is the same
        // per-bucket call list under a different pipeline — the depth prepass
        // plays exactly this trick with the camera's. Cloned before the pass
        // body takes the original.
        let bucket_calls = bucket_draws.clone();

        // Counted off the two loops the body below runs, before it takes them:
        // one call per bucket per occupied view. `ForwardRenderer::counters` is
        // what reports it, and reading it back off the same `Vec`s is what makes
        // it move when the tile allocation does.
        let recorded = (views.len() * bucket_draws.calls.len()) as u64;
        // And the tile resets beside them, which are direct draws of one
        // instance rather than indirect ones: a frame that cleared the whole
        // attachment records none, and a frame that loaded it records one per
        // tile it redraws.
        let clears = if self.shadow_atlas_clears {
            0
        } else {
            views.len() as u64
        };

        // What the image will hold once this body has run, carried into it so
        // that *recording* the pass is not what claims the atlas was drawn — see
        // [`ForwardRenderer::shadow_atlas_drawn`].
        let ran = Arc::clone(&self.shadow_pass_ran);
        let pass_id = self.shadow_pass_id;
        // The tiles this pass redraws have to be reset before anything is drawn
        // into them, and on a frame that loaded the attachment there is nothing
        // else that will — see [`MeshModules::depth_clear_pipeline`]. A frame
        // that cleared has already had every tile reset by the attachment, so
        // this is `None` and the recording is the one that shipped.
        let clear = (!self.shadow_atlas_clears).then_some(self.shadow_clear_pipeline);
        // **The reset draw has to bind a group, and only a browser says so.**
        // `MeshModules::depth_clear_pipeline` builds this pipeline from
        // `mesh_pipeline_layout`, so its layout declares group 0 whether or not
        // the one triangle's vertex shader reads a byte of it — and WebGPU
        // refuses a draw with any group the *layout* declares left unset, where
        // Vulkan and the others only care about what the shader dereferences. An
        // unbound reset therefore fails no native run and rejects the whole
        // browser frame: `No bind group set at group index 0` arrives when the
        // encoder is finished, so every pass recorded after it is thrown away
        // with it and the canvas stays at the clear colour.
        //
        // Any bucket's offset does: the block is what the mesh draws index and
        // this draw reads none of it, so what matters is that the offset is one
        // the layout accepts rather than which bucket it names.
        let clear_layout = bucket_draws.layout;
        let clear_offset = bucket_draws
            .calls
            .first()
            .map_or(0, |(constant_offset, ..)| *constant_offset);
        pass.execute(move |ctx| {
            ran.store(pass_id, Ordering::Relaxed);
            let encoder = ctx.encoder();
            if let Some(clear) = clear {
                encoder.bind_graphics_pipeline(clear);
                for (view, tile, _) in &views {
                    set_shadow_tile(encoder, *tile);
                    encoder.bind_group(0, groups[*view], &[clear_offset], clear_layout);
                    encoder.draw(0..TILE_CLEAR_VERTICES, 0..1);
                }
            }
            bucket_draws.open(encoder);
            for (view, tile, cull) in &views {
                set_shadow_tile(encoder, *tile);
                bucket_draws.record(encoder, groups[*view], &generated[*cull].1);
            }
        });

        // --- the reflective shadow map ---
        //
        // `docs/plan/50-irradiance-probes.md`'s updater, first half, recorded
        // here because this is where cascade 0's bind group and its
        // already-generated draws are. **A pass of its own at its own extent**,
        // and [`crate::rsm`]'s header argues why it cannot ride the atlas above:
        // one colour attachment there spans the whole 3072² image for data only
        // one 768² tile is read from, and it would give all fourteen light tiles
        // a fragment stage.
        //
        // No viewport is set: the graph opens a pass with a full-target viewport
        // and scissor already, and every attachment here is the map's own square.
        let Some((cascade_group, cascade_draws)) = cascade_zero else {
            return (atlas, recorded, clears, None);
        };
        let extent = rsm::extent();
        let sun = RsmTargets {
            albedo: graph.create_image("rsm-albedo", rsm::albedo_target(extent)),
            normal: graph.create_image("rsm-normal", rsm::normal_target(extent)),
            world: graph.create_image("rsm-world", rsm::world_target(extent)),
        };
        // Its own depth, cleared and discarded: it resolves which surface a
        // texel of the map holds and nothing reads it afterwards.
        let rsm_depth = graph.create_image("rsm-depth", rsm::depth_target(extent));
        let mut rsm = graph
            .add_render_pass("rsm")
            // **Cleared, and the world target's clear is load-bearing**: `w` is
            // the coverage flag `RsmOutput::world` writes, so a texel the pass
            // draws nothing into reads back zero there and the gather skips it.
            // Without the clear the world origin would stand in for every part
            // of the cascade the geometry does not cover.
            .clear_color(sun.albedo, [0.0; 4])
            .clear_color(sun.normal, [0.0; 4])
            .clear_color(sun.world, [0.0; 4])
            .clear_depth(rsm_depth)
            // The same declarations the cascade above makes, for the same
            // reason: every group of the mesh layout names these, and this pass
            // binds one of those groups. Unlike the cascade, this one has a
            // fragment stage — so the pages really are sampled here.
            .read_image(placeholder)
            .read_image(reads.occlusion_placeholder)
            .read_image(reads.pages.base_color)
            .read_image(reads.pages.normal)
            .read_buffer(reads.probes);
        if let Some(vertices) = reads.skinned {
            rsm = rsm.read_buffer(vertices);
        }
        // Cascade 0's cluster-selection buffer, which the mesh path's stages
        // write — declared on the atlas pass's terms, and only for the one view
        // this pass draws.
        if let Some(&buffer) = selection.get(CASCADE_ZERO_VIEW) {
            rsm = rsm.use_buffer(buffer, ResourceState::ShaderReadWrite);
        }
        rsm = read_draw_sources(rsm, &cascade_draws, self.emit);
        let rsm_draws = BucketDraws {
            pipeline: self.rsm_pipeline,
            ..bucket_calls.clone()
        };
        let rsm_recorded = rsm_draws.calls.len() as u64;
        rsm.execute(move |ctx| {
            let encoder = ctx.encoder();
            rsm_draws.open(encoder);
            rsm_draws.record(encoder, cascade_group, &cascade_draws);
        });

        // --- the punctual reflective shadow map ---
        //
        // `docs/plan/50-irradiance-probes.md`'s punctual producer: the same
        // pipeline and the same per-bucket call list as the pass above, under
        // one viewport per shadowed light face instead of one whole map. A spot
        // is a tile and a point light is six, which is `shadow::tile_span` — so
        // this is the atlas pass's own view loop drawn into a second, much
        // smaller set of targets.
        //
        // **Recorded whether or not the frame lights a punctual face**, unlike
        // the map above. The clear is a legitimate answer — a frame with no
        // shadowed punctual light has no punctual bounce, and the coverage flag
        // in `w` says so at every texel — and a pass that came and went with the
        // lights would be a pass list that changes shape with the scene, which
        // is what `RENDER_PASSES` and every timing reader are written against.
        let punctual_extent = rsm::punctual_extent();
        let punctual_targets = RsmTargets {
            albedo: graph.create_image("rsm-punctual-albedo", rsm::albedo_target(punctual_extent)),
            normal: graph.create_image("rsm-punctual-normal", rsm::normal_target(punctual_extent)),
            world: graph.create_image("rsm-punctual-world", rsm::world_target(punctual_extent)),
        };
        let punctual_depth =
            graph.create_image("rsm-punctual-depth", rsm::depth_target(punctual_extent));
        let mut punctual_pass = graph
            .add_render_pass("rsm-punctual")
            // Cleared on the map above's terms, and the world target's clear is
            // load-bearing for its reason as well as for one of this map's own:
            // the tiles the cascades would hold are drawn into by nothing at
            // all, and a coverage flag of zero is what keeps the gather out of
            // them.
            .clear_color(punctual_targets.albedo, [0.0; 4])
            .clear_color(punctual_targets.normal, [0.0; 4])
            .clear_color(punctual_targets.world, [0.0; 4])
            .clear_depth(punctual_depth)
            .read_image(placeholder)
            .read_image(reads.occlusion_placeholder)
            .read_image(reads.pages.base_color)
            .read_image(reads.pages.normal)
            .read_buffer(reads.probes);
        if let Some(vertices) = reads.skinned {
            punctual_pass = punctual_pass.read_buffer(vertices);
        }
        // Each face's own cluster-selection buffer, which the mesh path's stages
        // write — declared on the atlas pass's terms, and only for the views
        // this pass draws.
        for face in &punctual {
            if let Some(&buffer) = selection.get(face.view) {
                punctual_pass = punctual_pass.use_buffer(buffer, ResourceState::ShaderReadWrite);
            }
        }
        for draws in &punctual_sources {
            punctual_pass = read_draw_sources(punctual_pass, draws, self.emit);
        }
        let punctual_draws = BucketDraws {
            pipeline: self.rsm_pipeline,
            ..bucket_calls
        };
        let punctual_recorded = (punctual_views.len() * punctual_draws.calls.len()) as u64;
        punctual_pass.execute(move |ctx| {
            let encoder = ctx.encoder();
            punctual_draws.open(encoder);
            for (group, tile, draws) in &punctual_views {
                set_shadow_tile(encoder, *tile);
                punctual_draws.record(encoder, *group, draws);
            }
        });

        let images = RsmImages {
            sun,
            punctual: punctual_targets,
        };
        (
            atlas,
            recorded + rsm_recorded + punctual_recorded,
            clears,
            Some(images),
        )
    }

    /// Which lights hold the atlas's light tiles this frame, and where each
    /// map was laid out.
    ///
    /// **The observable `docs/plan/45-shadows.md`'s priority rung otherwise has
    /// none for.** A map's *size* leaves no trace in the picture that a golden
    /// can hold anyone to: a light demoted to a quarter of a cell draws a
    /// slightly softer shadow, which is a perfectly plausible frame and one a
    /// re-bless accepts. [`Selection::atlas_rect`](crate::shadow::Selection)
    /// off this is what says which level the allocator actually handed out, and
    /// `crates/crcbl/tests/mesh_e2e/shadow_tiles.rs` is what reads it.
    ///
    /// Named for the selection's own subject rather than for its type, because
    /// [`Self::shadow_selection`] above is a *buffer* — the cut a cascade
    /// descended to — and two accessors of one name is a caller reaching for
    /// the wrong one.
    ///
    /// It is this frame's answer, written by [`Self::begin_frame`]: before the
    /// first frame every slot is free.
    /// Whether this frame took the shadow atlas as an earlier frame left it,
    /// rather than drawing it again.
    ///
    /// `docs/plan/45-shadows.md`'s static-caching rung, read back. True means
    /// this frame recorded **no** shadow cull and no shadow pass, and every map
    /// sampled through [`Self::shadow_lights`]'s rectangles is the one the last
    /// frame that did draw put there — so a caller comparing two frames' pass
    /// lists, draw counts or GPU timings has to know which of them redrew.
    ///
    /// Decided by [`Self::begin_frame`] out of everything each group of the
    /// atlas is a function of — see `ForwardRenderer::shadow_group_record` —
    /// and it answers `false` before the first frame, because no frame has been
    /// opened and so none of them has held anything.
    ///
    /// **Whole-atlas, and since the cadence rung a frame can be neither.** A
    /// frame that redraws one map and holds another answers `false` here and
    /// `false` from [`Self::shadow_slot_redrawn`] for the maps it held; the two
    /// together are what say which.
    #[must_use]
    pub const fn shadow_atlas_cached(&self) -> bool {
        self.shadow_atlas_cached
    }

    /// Whether this frame redrew cascade `cascade`'s map, rather than keeping
    /// the one an earlier frame put there.
    ///
    /// `docs/plan/45-shadows.md`'s cadence rung, read back. **The observable
    /// that rung otherwise has none for**: a held map draws a shadow lagging its
    /// caster by a frame or two, which is a perfectly plausible picture and one
    /// a golden accepts — so nothing but this distinguishes a cadence that is
    /// working from one that is not running at all.
    ///
    /// `false` for a `cascade` past [`shadow::CASCADES`] and before the first
    /// frame, which is the same answer for the same reason: no map was drawn
    /// there.
    #[must_use]
    pub fn shadow_cascade_redrawn(&self, cascade: usize) -> bool {
        cascade < shadow::CASCADES && self.shadow_group_redrawn[cascade]
    }

    /// Whether this frame redrew the maps of light slot `slot` —
    /// [`shadow::Selection::slots`]' index — rather than keeping the ones an
    /// earlier frame put there.
    ///
    /// [`Self::shadow_cascade_redrawn`]'s answer for the light region, and a
    /// point light's [`shadow::POINT_FACES`] faces answer together: they cull
    /// once and draw one visible set, so they are redrawn or held as one.
    #[must_use]
    pub fn shadow_slot_redrawn(&self, slot: usize) -> bool {
        slot < shadow::LIGHT_SLOTS && self.shadow_group_redrawn[shadow_cull(slot)]
    }

    /// How many tiles of the atlas this frame redrew.
    ///
    /// What [`shadow::r_shadow_faces`] is spent in, read back — so a test can
    /// say the budget bound rather than that it was configured. A cascade costs
    /// one and a point light's cube costs [`shadow::POINT_FACES`].
    #[must_use]
    pub const fn shadow_faces_redrawn(&self) -> u32 {
        self.shadow_faces_redrawn
    }

    /// Pins the shadow cadence for this renderer, or hands it back to the
    /// console.
    ///
    /// [`Self::set_effect_request`]'s shape at this knob:
    /// [`shadow::r_shadow_cadence`] and [`shadow::r_shadow_faces`] are
    /// process-global, so they are the *default* rather than the only answer —
    /// an application drawing two views at two qualities, or a test that needs a
    /// frame nothing else in the process can move, pins its own.
    ///
    /// [`None`] hands it back, which is what a renderer starts at.
    ///
    /// Read once per [`Self::begin_frame`], on `ForwardRenderer::frame_effects`'
    /// terms: this call decides it and `add_passes` acts on it.
    pub const fn set_shadow_cadence(&mut self, cadence: Option<shadow::Cadence>) {
        self.shadow_cadence_request = cadence;
    }

    /// Whether this frame ignored the cadence for at least one map because the
    /// region that map covers had moved out from under it.
    ///
    /// `docs/plan/45-shadows.md`'s "a moving light or camera cut resets it",
    /// made precise: a map is *reset* rather than merely out of date when the
    /// centre it is projected from has moved further than the map's own reach —
    /// a light further than its own radius, or a cascade's eye further than the
    /// sphere that cascade was fitted to. Holding such a map is not a shadow
    /// that lags; it is a shadow of somewhere else.
    ///
    /// A frame whose atlas was **relaid** — a light gained, lost or resized a
    /// tile, or shadows were switched on or off — answers `true` as well, and
    /// that frame redraws every map it has: the only clear this seam can make
    /// covers the whole image, so there is nothing left to hold.
    #[must_use]
    pub const fn shadow_cadence_reset(&self) -> bool {
        self.shadow_cadence_reset
    }

    #[must_use]
    pub const fn shadow_lights(&self) -> &shadow::Selection {
        &self.shadow_lights
    }

    /// The shadow atlas, for a caller that wants to read it back.
    ///
    /// **The observable this whole slice can otherwise hide behind.** A shadow
    /// pass that renders nothing leaves every texel at
    /// [`depth::CLEAR`](crcbl_hal::depth::CLEAR) and produces a frame in which
    /// everything is lit — which is a perfectly plausible picture and matches
    /// any golden blessed from it. Copying this image back and finding a texel
    /// that is not the clear value is what distinguishes the two.
    ///
    /// It is created with
    /// [`ImageUsage::TRANSFER_SRC`](crcbl_hal::ImageUsage::TRANSFER_SRC) for
    /// exactly that, and its extent is [`shadow::atlas_extent`].
    #[must_use]
    pub const fn shadow_atlas(&self) -> crcbl_hal::ImageHandle {
        self.shadow_atlas
    }

    /// The base-colour page every material row samples through
    /// [`GpuMaterial::base_color_texture`](crcbl_shaders::mesh::GpuMaterial::base_color_texture),
    /// as the image and the view [`crate::texture`] uploaded.
    ///
    /// **What a render-to-texture view copies into.** A second camera drawing
    /// into an image is only half of a monitor: the surface that *shows* it
    /// samples a page layer, and nothing above this crate can name the image
    /// that layer lives in.
    ///
    /// A caller that wants to *write* it wants
    /// [`base_color_page_import`](Self::base_color_page_import) instead: the
    /// graph can only order a copy against the draws that sample the page while
    /// both are declared, and the two declarations have to agree field for
    /// field. This one is the raw handles, for a caller that binds or reads them
    /// outside a graph.
    ///
    /// It is created with
    /// [`ImageUsage::TRANSFER_DST`](crcbl_hal::ImageUsage::TRANSFER_DST),
    /// because the upload that filled it is a copy, so a per-frame copy into it
    /// needs no new usage flag. Its extent is
    /// [`PageDesc::extent`](crate::scene::PageDesc::extent) squared and it has
    /// one layer per [`SceneDesc::page`](crate::scene::SceneDesc::page) layer;
    /// the description is the caller's, so the caller already knows both.
    #[must_use]
    pub const fn base_color_page(&self) -> UploadedTexture {
        self.base_color_page
    }

    /// What [`add_passes`](Self::add_passes) names the base-colour page in the
    /// graph.
    ///
    /// [`RenderGraph::import_image`] keeps the label of whichever import ran
    /// first, so a caller importing the page too passes this rather than a
    /// string of its own — otherwise every barrier record and
    /// [`dump`](crate::CompiledGraph::dump) line would name the page differently
    /// depending on which subsystem got there first.
    pub const BASE_COLOR_PAGE_LABEL: &'static str = "base-colour-page";

    /// The base-colour page **as this renderer declares it to a graph**.
    ///
    /// [`add_passes`](Self::add_passes) imports exactly this value under
    /// [`BASE_COLOR_PAGE_LABEL`](Self::BASE_COLOR_PAGE_LABEL) and declares a read
    /// of it on every pass whose bind group names the page, so a caller that
    /// writes the page — a render-to-texture view copied into one layer — gets
    /// the graph to order its write against those draws.
    ///
    /// **A caller that writes it imports this value rather than one of its
    /// own.** [`RenderGraph::import_image`] hands the second importer the id the
    /// first got, which is what puts the write and the reads on one state
    /// tracker, and it does that only while the two declarations agree in every
    /// field — a disagreement is [`GraphError::ImportDeclarationConflict`] at
    /// [`compile`](RenderGraph::compile). Reading the declaration off the
    /// renderer that owns the page is what makes them one answer instead of two
    /// constants that have to be kept equal by hand.
    ///
    /// [`InitialClaim::Tracked`] with [`ResourceState::ShaderRead`] both ways:
    /// [`crate::texture`] leaves an uploaded page in that state, and the
    /// trailing barrier the `final_state` asks for puts it back there at the end
    /// of every frame — so the pool's ledger and this declaration are the same
    /// answer, which is what `Tracked` has the graph check.
    ///
    /// [`GraphError::ImportDeclarationConflict`]: crate::graph::GraphError::ImportDeclarationConflict
    #[must_use]
    pub const fn base_color_page_import(&self) -> ImportedImage {
        ImportedImage {
            image: self.base_color_page.image,
            view: self.base_color_page.view,
            format: BASE_COLOR_PAGE_FORMAT,
            extent: self.base_color_page_extent,
            initial: ResourceState::ShaderRead,
            claim: InitialClaim::Tracked,
            final_state: ResourceState::ShaderRead,
        }
    }

    /// `docs/plan/43-render-standards.md` §2's **normal** page, the raw handles.
    ///
    /// [`base_color_page`](Self::base_color_page)'s counterpart, on every one of
    /// its terms: one `D2Array` image, one layer per
    /// [`PageDesc::normal_layers`](crate::scene::PageDesc::normal_layers) entry,
    /// the same extent, and created with
    /// [`ImageUsage::TRANSFER_DST`](crcbl_hal::ImageUsage::TRANSFER_DST) so a
    /// per-frame copy into a layer needs no new usage flag. What it is *not* is
    /// the same format: it is `Rgba8Unorm`, where the base-colour page is
    /// `Rgba8UnormSrgb`, and `docs/plan/44-lighting.md`'s rung 2 is where that
    /// is argued.
    #[must_use]
    pub const fn normal_page(&self) -> UploadedTexture {
        self.normal_page
    }

    /// What [`add_passes`](Self::add_passes) names the normal page in the graph,
    /// on [`BASE_COLOR_PAGE_LABEL`](Self::BASE_COLOR_PAGE_LABEL)'s terms.
    pub const NORMAL_PAGE_LABEL: &'static str = "normal-page";

    /// The normal page **as this renderer declares it to a graph**, on
    /// [`base_color_page_import`](Self::base_color_page_import)'s terms exactly
    /// — including that a caller writing a layer imports *this* value rather
    /// than one of its own, so the write and the draws land on one state
    /// tracker.
    #[must_use]
    pub const fn normal_page_import(&self) -> ImportedImage {
        ImportedImage {
            image: self.normal_page.image,
            view: self.normal_page.view,
            format: NORMAL_PAGE_FORMAT,
            extent: self.base_color_page_extent,
            initial: ResourceState::ShaderRead,
            claim: InitialClaim::Tracked,
            final_state: ResourceState::ShaderRead,
        }
    }

    /// The model matrix for a cube spinning at `seconds` into the run.
    ///
    /// Two axes at incommensurable rates, so the animation never repeats and
    /// every face comes into view — which is what makes "the cube is spinning"
    /// observable rather than a claim about a matrix.
    #[must_use]
    pub fn spin(seconds: f32) -> Mat4 {
        Mat4::from_quat(
            Quat::from_axis_angle(Vec3::Y, seconds * 0.9)
                * Quat::from_axis_angle(Vec3::X, seconds * 0.55),
        )
    }

    /// The format the tonemap pass renders into.
    #[must_use]
    pub const fn target_format(&self) -> Format {
        self.target_format
    }

    /// Switches the infinite ground grid on with `style`, or off with [`None`].
    ///
    /// **Off by default.** Every sample and every golden image predates the
    /// grid, so a renderer nobody has called this on records exactly the passes
    /// it always did — see [`crate::grid`] for what the pass is and
    /// [`add_passes`](Self::add_passes) for where in the frame it lands.
    ///
    /// The first `Some` builds the pipeline and the uniform ring; later calls
    /// only replace the style, and a `None` leaves both built. Releasing them
    /// here would mean releasing blocks the frames in flight may still be
    /// reading, so they are released by [`destroy`](Self::destroy) alone — the
    /// one call that already requires an idle device.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the grid's pipeline or its blocks could not be created.
    /// Nothing is left switched on when that happens: the grid stays as it was.
    pub fn set_ground_grid(
        &mut self,
        device: &dyn Device,
        style: Option<GridStyle>,
    ) -> Result<(), HalError> {
        let Some(style) = style else {
            self.ground_grid_on = false;
            return Ok(());
        };
        let grid = match self.ground_grid.as_mut() {
            Some(grid) => grid,
            None => {
                // Built for the tonemap's target and the scene depth, because
                // those are the two attachments the pass is given — see
                // `add_passes`. Both are pipeline state, so a renderer whose
                // target format changed would need a new `Grid`; nothing in this
                // crate changes it after `build`, and `target_format` is kept
                // for exactly that reason.
                let grid = GroundGrid::new(
                    device,
                    self.uniforms.len(),
                    self.target_format,
                    SCENE_DEPTH_FORMAT,
                )?;
                self.ground_grid.insert(grid)
            }
        };
        grid.set_style(style);
        self.ground_grid_on = true;
        Ok(())
    }

    /// The debug draw layer's immediate-mode buffer, to append to.
    ///
    /// `docs/plan/07-ui-debug.md` item 5: any system appends lines, boxes,
    /// spheres and frusta during the frame, and
    /// [`begin_frame`](Self::begin_frame) uploads and clears what is there — so
    /// a segment lives exactly the frame it was appended in and nothing has to
    /// be un-appended. See [`crate::debug_draw`] for where the pass lands and
    /// for the console switch that has to be on before anything is drawn.
    ///
    /// **Appending is free while the layer is off**, but building the geometry
    /// is the caller's: a system whose overlay costs something to compute asks
    /// [`DebugDraw::enabled`] first.
    pub fn debug_draw(&mut self) -> &mut DebugDraw {
        &mut self.debug_draw
    }

    /// How the ground grid is drawn, or [`None`] where it is switched off.
    ///
    /// The read-back half of [`set_ground_grid`](Self::set_ground_grid), and the
    /// observable a test asking "is the grid on" wants: a renderer that built
    /// the grid and then switched it off answers [`None`], because what it draws
    /// is what the question is about.
    #[must_use]
    pub fn ground_grid(&self) -> Option<&GridStyle> {
        self.ground_grid
            .as_ref()
            .filter(|_| self.ground_grid_on)
            .map(GroundGrid::style)
    }

    /// Whether this frame adds [`crate::sky_pass`]'s background pass.
    ///
    /// [`Sky::NONE`] is the value a renderer nobody called
    /// [`set_sky`](Self::set_sky) on holds, and it is checked here rather than
    /// left to the shader for the reason [`crate::sky_pass`] gives: a black
    /// gradient drawn over [`SCENE_CLEAR`] would *change* those frames, where
    /// adding no pass leaves them exactly as they were.
    fn draws_sky(&self) -> bool {
        self.sky != Sky::NONE
    }

    /// Sets the multiplier the tonemap pass applies before its clamp, clamped
    /// into [`EXPOSURE_MIN`]`..=`[`EXPOSURE_MAX`].
    ///
    /// **The default is [`crcbl_shaders::tonemap::DEFAULT_EXPOSURE`]**, which is
    /// the value `tonemap.slang` held as a compile-time constant until this
    /// became a runtime one — so a renderer nobody calls this on draws the frame
    /// it always drew, and every golden image is untouched.
    ///
    /// It takes effect on the next
    /// [`begin_frame`](Self::begin_frame), which is what writes the block: a
    /// frame already recorded reads the value it was recorded with.
    ///
    /// **The clamp is here rather than at the call site** so that no caller can
    /// reach a picture it cannot get back from, and so that a control which
    /// steps by reading this back and setting a multiple of it — which is what
    /// [`exposure`](Self::exposure) is for — cannot wind up past the end and
    /// then need as many presses to come back.
    pub const fn set_exposure(&mut self, exposure: f32) {
        // `max` then `min` rather than `f32::clamp`: `clamp` propagates a NaN,
        // and a NaN reaching the block is a frame of NaN pixels with no key to
        // press to get out of it. `f32::max` returns the bound instead, so a NaN
        // lands on `EXPOSURE_MIN` — dark, and one press from being bright again.
        self.exposure = exposure.max(EXPOSURE_MIN).min(EXPOSURE_MAX);
    }

    /// How far auto-exposure may travel toward its measurement in one frame.
    ///
    /// **[`None`] is the whole distance**, which is what a renderer nobody
    /// calls this on does and what the pass did before adaptation existed — so
    /// no golden moved when this arrived. A view that wants the roll asks, and
    /// asks *every frame*, because [`ExposureAdaptation`] carries that frame's
    /// own delta: this crate has no clock, and the frame time lives where the
    /// frame loop is.
    ///
    /// It takes effect on the next [`begin_frame`](Self::begin_frame), which is
    /// what writes the block, and it does nothing at all on a frame that did not
    /// ask for [`RenderEffects::AUTO_EXPOSURE`] — the passes that read it are
    /// the ones that bit adds.
    pub const fn set_exposure_adaptation(&mut self, adaptation: Option<ExposureAdaptation>) {
        self.exposure_adaptation = adaptation;
    }

    /// What [`set_exposure_adaptation`](Self::set_exposure_adaptation) was last
    /// handed.
    #[must_use]
    pub const fn exposure_adaptation(&self) -> Option<ExposureAdaptation> {
        self.exposure_adaptation
    }

    /// Which operator the tonemap pass runs on the exposed scene colour.
    ///
    /// **Off by default, and that is the whole of why nothing re-blessed when
    /// the curve landed.** `tonemap.slang`'s clamp is the identity on `0..=1`,
    /// so display-referred content — every 2D sample in this tree — reaches the
    /// swapchain exactly; a filmic curve applied to it would move colours
    /// somebody chose. A 3D view is the one that wants the roll-off, and asks.
    ///
    /// It takes effect on the next [`begin_frame`](Self::begin_frame), for
    /// [`set_exposure`](Self::set_exposure)'s reason: the block is written
    /// there, and a frame already recorded reads the value it was recorded
    /// with.
    ///
    /// **There is nothing to refuse and no `supports_` probe beside it.** The
    /// selector is a lane of a uniform block read by a branch every device
    /// already runs, so unlike the wireframe there is no second pipeline for a
    /// device to decline to build.
    pub const fn set_tonemap_curve(&mut self, curve: tonemap::TonemapCurve) {
        self.tonemap_curve = curve;
    }

    /// The operator in force, on [`exposure`](Self::exposure)'s terms: the
    /// value the next frame will be drawn with.
    #[must_use]
    pub const fn tonemap_curve(&self) -> tonemap::TonemapCurve {
        self.tonemap_curve
    }

    /// How large the internal render target is as a fraction of the extent
    /// [`begin_frame`](Self::begin_frame) is handed, clamped to
    /// `MIN_RENDER_SCALE..=1.0`.
    ///
    /// **This is the largest performance knob this renderer has.** Every pass
    /// before the upscale costs what its extent costs, so a scale of `0.7` is
    /// roughly half the shading work of `1.0`; the UI is composited onto the
    /// target afterwards at native resolution, so text stays sharp while the 3D
    /// frame gets cheap. `docs/plan/43-render-standards.md` is where that trade
    /// is written down.
    ///
    /// **`1.0` is not a special case with a fast path, it is the absence of the
    /// feature.** At full scale the internal extent *is* the caller's, the post
    /// chain writes the target directly, and no upscale pass is recorded — which
    /// is what makes a renderer nobody has called this on draw the frame it drew
    /// before the knob existed, bit for bit.
    ///
    /// **Above `1.0` is clamped away rather than supported.** Supersampling
    /// wants a box or Lanczos reduction and `upscale.slang` is a Catmull-Rom
    /// *reconstruction*; running it as a minification filter would alias, which
    /// is the opposite of what a caller asking for it wants. A non-finite scale
    /// is clamped to `1.0` for the same reason a `NaN` extent would be refused:
    /// the failure it produces is a zero-sized target somewhere downstream.
    ///
    /// Takes effect on the next [`begin_frame`](Self::begin_frame), on
    /// [`set_exposure`](Self::set_exposure)'s terms exactly — a frame already
    /// recorded is drawn at the scale it was recorded with.
    pub fn set_render_scale(&mut self, scale: f32) {
        self.render_scale = if scale.is_finite() {
            scale.clamp(MIN_RENDER_SCALE, 1.0)
        } else {
            1.0
        };
    }

    /// The render scale in force, after the clamp
    /// [`set_render_scale`](Self::set_render_scale) applied.
    ///
    /// The read-back half, on [`exposure`](Self::exposure)'s terms.
    #[must_use]
    pub const fn render_scale(&self) -> f32 {
        self.render_scale
    }

    /// The anisotropy the base-colour page is sampled with, in force from the
    /// next [`begin_frame`](Self::begin_frame).
    ///
    /// `docs/plan/43-render-standards.md`'s filtering rung, the player's half:
    /// [`anisotropy_for`](Self::anisotropy_for) is what a renderer nobody has
    /// called this on samples with, and this is what a settings row moves it
    /// to. Clamped to `1.0..=max_sampler_anisotropy` where the device was
    /// granted [`Features::SAMPLER_ANISOTROPY`] and to exactly `1.0` where it
    /// was not — the row can turn the filter off or push it to the device's
    /// ceiling, and nothing it asks reaches a `create_sampler` that would
    /// refuse it. A value that is not a number is the default, on
    /// [`set_render_scale`](Self::set_render_scale)'s terms.
    ///
    /// **A sampler is a resource a bind group holds by handle**, so a new
    /// anisotropy is a new sampler, and every group of the mesh layout has to
    /// be rebuilt to name it. That happens per frame slot, at the slot's own
    /// `begin_frame`, which is the moment the ring guarantees the slot's
    /// previous submission has retired — the guarantee every host write in
    /// that call already leans on — so a frame in flight goes on naming the
    /// sampler it was recorded with, and the replaced sampler is destroyed once
    /// no slot names it. This call creates the sampler and nothing else, and
    /// asking for the anisotropy already in force creates nothing.
    ///
    /// # Errors
    ///
    /// `create_sampler`'s, and the renderer is unchanged — the sampler in force
    /// stays in force.
    pub fn set_anisotropy(&mut self, device: &dyn Device, anisotropy: f32) -> Result<(), HalError> {
        let caps = device.caps();
        let ceiling = if caps.features.contains(Features::SAMPLER_ANISOTROPY) {
            caps.limits.max_sampler_anisotropy
        } else {
            1.0
        };
        let wanted = if anisotropy.is_finite() {
            anisotropy.clamp(1.0, ceiling)
        } else {
            Self::anisotropy_for(&caps)
        };
        // Bit-equal rather than within an epsilon: both sides came out of the
        // same clamp, and a row that lands on the value in force must not
        // rebuild every group for it.
        if wanted.to_bits() == self.anisotropy.to_bits() {
            return Ok(());
        }
        let sampler = Self::create_page_sampler(device, wanted)?;
        self.retired_page_samplers
            .push(std::mem::replace(&mut self.base_color_sampler, sampler));
        self.anisotropy = wanted;
        Ok(())
    }

    /// The anisotropy in force, after the clamp
    /// [`set_anisotropy`](Self::set_anisotropy) applied.
    ///
    /// The read-back half, on [`render_scale`](Self::render_scale)'s terms.
    #[must_use]
    pub const fn anisotropy(&self) -> f32 {
        self.anisotropy
    }

    /// The anisotropy the base-colour page's sampler is created with on a
    /// device of `caps`: [`DEFAULT_ANISOTROPY`] clamped to
    /// [`Limits::max_sampler_anisotropy`](crcbl_hal::Limits) where
    /// [`Features::SAMPLER_ANISOTROPY`] was granted, and `1.0` where it was
    /// not.
    ///
    /// **Granted, not supported.** The feature is optional at every open site
    /// — [`Features::GPU_DRIVEN`] does not carry it — and a device opened
    /// without asking refuses a sampler above one even on hardware that has
    /// it, which is the shape every backend's `create_sampler` documents. So
    /// the question is what this device was opened *with*, and a caller that
    /// wants the plan's default has to name the feature beside the others it
    /// asks for. `crcbl-webgpu` withholds the feature and reports a ceiling of
    /// one, so a browser samples isotropically until `docs/backlog.md`'s
    /// ceiling decision is taken.
    #[must_use]
    pub fn anisotropy_for(caps: &DeviceCaps) -> f32 {
        if caps.features.contains(Features::SAMPLER_ANISOTROPY) {
            DEFAULT_ANISOTROPY.min(caps.limits.max_sampler_anisotropy)
        } else {
            1.0
        }
    }

    /// The base-colour page's sampler at `anisotropy`, which the caller has
    /// already held to the device — [`anisotropy_for`](Self::anisotropy_for)
    /// at build, [`set_anisotropy`](Self::set_anisotropy)'s clamp after.
    ///
    /// **Trilinear over the whole chain, anisotropic where the device is** —
    /// `docs/plan/43-render-standards.md`'s filtering rung: a minified surface
    /// reads the level its footprint matches instead of shimmering through
    /// level 0, a magnified one blends four texels instead of stepping between
    /// them, and a surface at a grazing angle keeps its detail along the long
    /// axis of its footprint rather than blurring to the short one. A second
    /// sampler object rather than sharing the tonemap's, so a capture names
    /// each for what it filters.
    ///
    /// **`Repeat`, not `ClampToEdge`**, and that is what `mesh.slang`'s
    /// `TILING_PHYSICAL` needs: a physical UV runs to `world_extent /
    /// tile_metres`, past `0..1` on any face wider than one tile, and only a
    /// wrapping address mode makes the page tile across it rather than
    /// smearing its edge row. `TILING_AUTHORED` is unaffected — its UV is the
    /// vertex's, which the meshes author inside `0..1`, so a wrapped and a
    /// clamped read return the same nearest texel for it.
    fn create_page_sampler(
        device: &dyn Device,
        anisotropy: f32,
    ) -> Result<SamplerHandle, HalError> {
        device.create_sampler(&SamplerDesc {
            label: Some("material base colour"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Linear,
            address_mode: [SamplerAddressMode::Repeat; 3],
            anisotropy,
            ..SamplerDesc::default()
        })
    }

    /// Moves this frame's slot onto [`ForwardRenderer::base_color_sampler`],
    /// where [`set_anisotropy`](Self::set_anisotropy) replaced it since the
    /// slot last drew.
    ///
    /// Every group of the mesh layout the slot holds — the camera's, the depth
    /// prepass's and each shadow view's — is rebuilt from the entries it was
    /// built from with [`PAGE_SAMPLER_BINDING`] rewritten, and the occlusion
    /// cache is dropped, since it was built from the camera's entries and
    /// names the old sampler too. **Every replacement is created before any
    /// group is destroyed**, so a refusal leaves the slot whole on the sampler
    /// it had and the next `begin_frame` tries again. A replaced sampler is
    /// destroyed here the moment no slot names it.
    ///
    /// Called from `begin_frame_body` once the ring has rotated: that is the
    /// point at which this slot's previous submission has retired, which is
    /// what makes destroying its groups sound.
    fn adopt_page_sampler(&mut self, device: &dyn Device) -> Result<(), HalError> {
        let frame = self.frame;
        let sampler = self.base_color_sampler;
        if self.slot_page_samplers[frame] == sampler {
            return Ok(());
        }
        let layout = self.mesh_layout;
        let mut fresh = Vec::with_capacity(2 + self.shadow_groups[frame].len());
        let lists = std::iter::once((&mut self.mesh_group_entries[frame], "mesh frame"))
            .chain(std::iter::once((
                &mut self.prepass_group_entries[frame],
                "depth prepass",
            )))
            .chain(
                self.shadow_group_entries[frame]
                    .iter_mut()
                    .map(|entries| (entries, "shadow view")),
            );
        for (entries, label) in lists {
            match rebuilt_with_sampler(device, layout, sampler, entries, label) {
                Ok(group) => fresh.push(group),
                Err(error) => {
                    for group in fresh {
                        device.destroy_bind_group(group);
                    }
                    return Err(error);
                }
            }
        }
        let mut fresh = fresh.into_iter();
        let mut swap = |slot: &mut BindGroupHandle| {
            let stale = std::mem::replace(
                slot,
                fresh
                    .next()
                    .unwrap_or_else(|| unreachable!("one fresh group per entry list")),
            );
            device.destroy_bind_group(stale);
        };
        swap(&mut self.mesh_groups[frame]);
        swap(&mut self.prepass_groups[frame]);
        for group in &mut self.shadow_groups[frame] {
            swap(group);
        }
        if let Some((_, stale)) = self.screen_channel_groups[frame].take() {
            device.destroy_bind_group(stale);
        }
        self.slot_page_samplers[frame] = sampler;
        let named = &self.slot_page_samplers;
        self.retired_page_samplers.retain(|retired| {
            if named.contains(retired) {
                true
            } else {
                device.destroy_sampler(*retired);
                false
            }
        });
        Ok(())
    }

    /// The extent a frame handed `target` is actually drawn at.
    ///
    /// Rounds rather than truncates, and floors at one texel in each dimension:
    /// a zero here would reach a transient image description, and an image of
    /// zero extent is a device error rather than a small frame. A caller who
    /// minimised the window already hands a zero extent through, and that path
    /// is unchanged — `max(1)` on a zero is one either way.
    fn internal_extent(&self, target: (u32, u32)) -> (u32, u32) {
        if self.render_scale >= 1.0 {
            return target;
        }
        let scale = f64::from(self.render_scale);
        (
            ((f64::from(target.0) * scale).round() as u32).max(1),
            ((f64::from(target.1) * scale).round() as u32).max(1),
        )
    }

    /// The exposure in force, after the clamp
    /// [`set_exposure`](Self::set_exposure) applied.
    ///
    /// The read-back half, on [`ground_grid`](Self::ground_grid)'s terms: what a
    /// caller wants is the number the frame is drawn with, not the one it asked
    /// for — which is also what makes a multiplicative step land on the end of
    /// the range rather than run past it.
    #[must_use]
    pub const fn exposure(&self) -> f32 {
        self.exposure
    }

    /// Whether `device` can draw the wireframe view at all.
    ///
    /// The question [`set_wireframe`](Self::set_wireframe) answers with an
    /// error, asked **before** anything is switched on — so an application can
    /// tell its user that this device has no wireframe rather than binding a key
    /// that fails, and can say so once at start-up instead of on every press.
    /// WebGPU has no line fill mode at all, so the browser is the case this
    /// exists for and not a hypothetical.
    ///
    /// An associated function rather than a method: it is a fact about the
    /// device, and a caller wants it before it has decided anything.
    #[must_use]
    pub fn supports_wireframe(device: &dyn Device) -> bool {
        device.caps().supports(Features::POLYGON_MODE_LINE)
    }

    /// Draws the scene's triangles as lines, or goes back to filling them.
    ///
    /// **Off by default.** Every sample and every golden image predates the
    /// view, so a renderer nobody has called this on records exactly the frame
    /// it always did.
    ///
    /// # What it is, and what it is not
    ///
    /// The **colour pass** alone changes fill mode. The depth prepass and the
    /// shadow cascades keep drawing filled triangles, because they are what the
    /// occlusion pair and the shadow lookups read and depth drawn as lines is
    /// depth with holes in it. So a wireframe frame is the same geometry, the
    /// same shading and the same lights, rasterised as edges: what was inside a
    /// triangle becomes the clear colour, and what was on its edge stays lit.
    ///
    /// # It is built on the first `true`
    ///
    /// The pipeline is compiled here rather than beside the filled one at
    /// [`with_scene`](Self::with_scene), and that is the trade this
    /// call makes: one pipeline compile on the first press — a hitch, in a view
    /// the user just asked for — against a second compile in the start-up of
    /// every sample, golden run and headless test that never asks for one. Later
    /// calls only flip the switch, and a `false` leaves the pipeline built, for
    /// [`set_ground_grid`](Self::set_ground_grid)'s reason: releasing something
    /// the frames in flight may still be recording against is what
    /// [`destroy`](Self::destroy) — which already requires an idle device — is
    /// for.
    ///
    /// # Errors
    ///
    /// [`HalError::UnsupportedFeatures`] naming
    /// [`Features::POLYGON_MODE_LINE`](crcbl_hal::Features::POLYGON_MODE_LINE)
    /// when `on` is `true` on a device that has not got it — ask
    /// [`supports_wireframe`](Self::supports_wireframe) first. Refused rather
    /// than quietly filled: a caller who asked for a wireframe and got a solid
    /// frame back with no error has no way to tell that from a wireframe of a
    /// solid-looking model.
    ///
    /// [`HalError`] from the pipeline otherwise. **Nothing is left switched on
    /// when either happens**: the view stays as it was.
    pub fn set_wireframe(&mut self, device: &dyn Device, on: bool) -> Result<(), HalError> {
        if !on {
            self.wireframe_on = false;
            return Ok(());
        }
        if self.wireframe_pipeline.is_none() {
            if !Self::supports_wireframe(device) {
                return Err(HalError::UnsupportedFeatures {
                    missing: Features::POLYGON_MODE_LINE,
                });
            }
            let modules = MeshModules::new(device, self.emit, self.culls_clusters)?;
            let pipeline = modules.color_pipeline(
                device,
                self.mesh_pipeline_layout,
                PolygonMode::Line,
                "wireframe",
            );
            modules.destroy(device);
            self.wireframe_pipeline = Some(pipeline?);
        }
        self.wireframe_on = true;
        Ok(())
    }

    /// Whether the colour pass draws lines instead of filled triangles.
    ///
    /// The read-back half of [`set_wireframe`](Self::set_wireframe), and the
    /// observable a test asking "is this frame a wireframe" wants: a renderer
    /// that built the pipeline and then switched it off answers `false`, because
    /// what it draws is what the question is about.
    #[must_use]
    pub const fn wireframe(&self) -> bool {
        self.wireframe_on
    }

    /// Draws each surface's **world-space** normal instead of shading it, so a
    /// modeller can see whether the normals a document carries are sane.
    ///
    /// **Off by default**, on [`set_wireframe`](Self::set_wireframe)'s terms: it
    /// is a tool's view of a document, and every sample and every golden image
    /// predates it. It takes effect on the next
    /// [`begin_frame`](Self::begin_frame), which is what writes the block.
    ///
    /// # What the colours mean
    ///
    /// The conventional encoding, `n * 0.5 + 0.5` into RGB: +X reads red, +Y
    /// green, +Z blue, and a face whose winding was inverted reads the
    /// **complement** of the colour it should have. A missing or degenerate
    /// normal reads as whatever the vertex data actually holds rather than as the
    /// plausible shading a light would have given it, which is the point.
    ///
    /// # World space, not view space
    ///
    /// The two diagnose different things and this one is world space, so a face
    /// keeps its colour while the camera orbits: "is this face inverted" is then
    /// something a modeller *sees*, instead of inferring it from a picture that
    /// re-colours whenever they move. View-space normals answer the other
    /// question — is this normal smooth, is this seam split — by re-colouring on
    /// purpose, and they are not a thing this block could express anyway: it
    /// carries `view_proj` and no separate view matrix, so there is no rotation in
    /// the shader to take a normal into eye space with.
    ///
    /// # It builds nothing and adds no pass
    ///
    /// Unlike the wireframe, which needs a second pipeline and so a device that
    /// can build one: this is one lane of the frame's uniform block — see
    /// [`FrameUniforms::NORMALS_VIEW_ON`] — read by a branch in the fragment
    /// stage every device already runs. There is nothing to refuse and nothing to
    /// release, which is why it cannot fail and there is no `supports_` probe
    /// beside it.
    ///
    /// The tonemap still applies: at
    /// [`DEFAULT_EXPOSURE`](crcbl_shaders::tonemap::DEFAULT_EXPOSURE) its operator
    /// is the identity on `0..=1` and the encoded normal reaches the swapchain
    /// exactly, and at any other exposure the picture is scaled per channel — so
    /// which channel dominates a face never changes, but a caller reading exact
    /// values wants [`set_exposure`](Self::set_exposure) left alone.
    ///
    /// [`FrameUniforms::NORMALS_VIEW_ON`]: crcbl_shaders::mesh::FrameUniforms::NORMALS_VIEW_ON
    pub const fn set_normals_view(&mut self, on: bool) {
        self.normals_view = on;
    }

    /// Sets the exponential height fog the colour pass composites over every
    /// shaded surface.
    ///
    /// `docs/plan/43-render-standards.md` §4's cheapest large win: distance
    /// reads as distance, a valley fills while a hilltop stays clear, and it
    /// costs four numbers in a block every pipeline already binds.
    ///
    /// # It adds no pass and cannot fail
    ///
    /// On [`set_normals_view`](Self::set_normals_view)'s terms exactly — two
    /// rows of the frame's uniform block, read by arithmetic in the fragment
    /// stage every device already runs. There is nothing to build, so there is
    /// nothing to refuse and no `supports_` probe beside it.
    ///
    /// # Off is exactly off
    ///
    /// [`Fog::NONE`] is the default and a zero density is an *exact* identity,
    /// not an approximate one: the optical depth is zero, its transmittance is
    /// exactly one, and `mesh.slang` composites as `lit * t + fog * (1 - t)`
    /// rather than as a `lerp`, which at `t == 1` would return
    /// `fog + (lit - fog)` and lose the low bits of an HDR radiance far from
    /// the fog colour. So this feature moves no frame it is not switched on
    /// for.
    ///
    /// # What it does not fog
    ///
    /// The screen-space reflections `ssr_blur.slang` adds **after** this pass,
    /// which arrive unfogged onto an already-fogged surface — see
    /// `docs/backlog.md`. Fog is applied where the radiance is finished, and
    /// the reflection composite is one pass later.
    pub const fn set_fog(&mut self, fog: Fog) {
        self.fog = fog;
    }

    /// Lights the scene with a gradient sky, on top of whatever ambient and
    /// irradiance grid it already has.
    ///
    /// `docs/plan/43-render-standards.md` §8's rung. [`Sky`] is three
    /// radiances — zenith, horizon, ground — and what reaches a surface is that
    /// gradient projected onto the L1 basis the probe grid already uses, so a
    /// surface facing up receives the sky and one facing down the ground's
    /// bounce. Three colours, no extra pass.
    ///
    /// # It adds no pass and cannot fail
    ///
    /// On [`set_fog`](Self::set_fog)'s terms exactly — three rows of the
    /// frame's uniform block and three dot products in a fragment stage every
    /// device already runs. Nothing to build, so nothing to refuse and no
    /// `supports_` probe beside it.
    ///
    /// # Off is exactly off
    ///
    /// [`Sky::NONE`] is the default and a black gradient projects to every
    /// coefficient zero — each is a product with a zero factor — so the
    /// fragment stage adds `max(0, 0)` and the ambient sum is the one it was
    /// before this existed, bit for bit.
    ///
    /// # What it does not do
    ///
    /// **Draw anything.** The background is still the scene target's clear
    /// colour, and a scene lit by a sky it cannot see is what this rung leaves
    /// behind until the pass that draws the gradient lands. The environment
    /// `ssr.slang` falls back to on a missed ray is still the probe grid's, so
    /// a mirror under a sky still reflects whatever the probes hold.
    pub const fn set_sky(&mut self, sky: Sky) {
        self.sky = sky;
    }

    /// Draws each cluster tinted by the DAG level it was decimated to, instead
    /// of shading it.
    ///
    /// **The picture cluster LOD's whole claim rests on**: one mesh spanning
    /// several levels across its own surface is a statement a test can assert
    /// and nobody can see, and this is what makes it visible. The mesh path
    /// tints per cluster; the two indirect paths select one level per instance
    /// and have no per-cluster level at all, so they draw one flat grey — which
    /// is the comparison rather than a shortfall.
    ///
    /// Costs what [`set_normals_view`](Self::set_normals_view) costs: one lane
    /// of the frame's uniform block, read by a branch every device already runs.
    /// Nothing to build, nothing to refuse, nothing to release.
    ///
    /// **Wins over the normals view when both are on**, because the two share a
    /// lane and the fragment stage tests this one first.
    pub const fn set_lod_view(&mut self, on: bool) {
        self.lod_view = on;
    }

    /// Whether the colour pass tints clusters by their DAG level.
    ///
    /// **What was asked for, not what is drawn.** The three debug views share
    /// one lane and resolve in one order, so a renderer with this and
    /// [`set_heatmap`](Self::set_heatmap) both on draws the heatmap and still
    /// answers `true` here — the caller's setting is what a caller's toggle has
    /// to read back, and [`debug_view`](Self::debug_view) is what says which one
    /// the frame actually took.
    #[must_use]
    pub const fn lod_view(&self) -> bool {
        self.lod_view
    }

    /// Shades each cluster by the **projected screen-space error** the LOD
    /// selection judged it on, instead of shading it.
    ///
    /// The LOD tint's sibling, and the other half of `docs/plan/25-lod.md`'s
    /// pair: [`set_lod_view`](Self::set_lod_view) answers "which level am I
    /// looking at", and this answers "how close to the budget is this, and where
    /// is the selection about to switch". The number is the one
    /// `draw_gen.slang`'s `group_is_expanded` compared against the budget — a
    /// cluster's *producing* group's error, projected from this frame's eye at
    /// this frame's pixels per unit.
    ///
    /// # What the colours mean
    ///
    /// A ramp that climbs in luminance from a cold floor to white, with a hue
    /// break at each of the two budgets: the hold budget where the hysteresis
    /// band starts, and the expand budget at the top, which is white. So the two
    /// budgets draw themselves as contour lines across the surface, and
    /// "brighter is closer to switching" reads even in a greyscale screenshot.
    /// `mesh_cluster.slang`'s `HEAT_UNDER_LOW` carries the ramp in full, and
    /// `crcbl_shaders`' `the_heatmap_ramp_climbs_in_luminance` is what holds it
    /// to being readable as an ordering.
    ///
    /// A cluster nothing simplified — the original surface, at the bottom of the
    /// DAG — costs exactly zero and takes the ramp's floor.
    ///
    /// # Mesh path only, and that is the capability rather than a shortfall
    ///
    /// A per-cluster error exists only where selection is per cluster.
    /// [`GeometryPath::IndirectCount`] and [`GeometryPath::IndirectPerBatch`]
    /// choose one level per *instance*, so there is no per-cluster number for
    /// them to shade by and `mesh.slang`'s vertex stage writes the same flat grey
    /// the LOD tint gets. Standing the two beside each other is the comparison.
    ///
    /// # It builds nothing and adds no pass
    ///
    /// One lane of the frame's uniform block, on
    /// [`set_normals_view`](Self::set_normals_view)'s terms exactly — plus one
    /// storage-buffer read in the mesh stage, which the mesh path's layout
    /// already carries. Nothing to build, nothing to refuse, nothing to release.
    ///
    /// **Wins over both the LOD tint and the normals view when several are on**;
    /// see [`debug_view`](Self::debug_view), which is that order stated once.
    ///
    /// [`GeometryPath::IndirectCount`]: crcbl_hal::GeometryPath::IndirectCount
    /// [`GeometryPath::IndirectPerBatch`]: crcbl_hal::GeometryPath::IndirectPerBatch
    pub const fn set_heatmap(&mut self, on: bool) {
        self.heatmap = on;
    }

    /// Whether the colour pass shades clusters by their projected error.
    ///
    /// **What was asked for, not what is drawn**, on
    /// [`lod_view`](Self::lod_view)'s terms.
    #[must_use]
    pub const fn heatmap(&self) -> bool {
        self.heatmap
    }

    /// Draws the ambient-occlusion channel alone, as grey, instead of shading.
    ///
    /// One is white and fully occluded is black — the buffer the forward pass
    /// multiplies its ambient term by, shown as itself. **This is how the AO
    /// ladder in `docs/plan/46-ambient-occlusion.md` is compared at all:** a
    /// composited frame shows occlusion times albedo times ambient, and a
    /// difference in the first is not separable from a difference in the other
    /// two by looking at the result.
    ///
    /// # What it draws with occlusion switched off
    ///
    /// White, everywhere. A frame without
    /// [`RenderEffects::AMBIENT_OCCLUSION`] binds a 1×1 white image in place of
    /// a computed channel, and this view shows that image rather than pretending
    /// to a channel nothing computed — "no occlusion was computed" and "nothing
    /// occludes here" are the same value to the shading, and the view does not
    /// get to disagree with the shading about it.
    ///
    /// # It builds nothing and adds no pass
    ///
    /// One lane of the frame's uniform block, on
    /// [`set_normals_view`](Self::set_normals_view)'s terms exactly. Unlike the
    /// tint and the heatmap it needs nothing from the geometry stage, so it
    /// draws on every [`GeometryPath`] rather than on
    /// the mesh-shader one alone.
    ///
    /// **Wins over every other view when several are on**; see
    /// [`debug_view`](Self::debug_view), which is that order stated once.
    pub const fn set_occlusion_view(&mut self, on: bool) {
        self.occlusion_view = on;
    }

    /// Whether the colour pass draws the occlusion channel instead of shading.
    ///
    /// **What was asked for, not what is drawn**, on
    /// [`lod_view`](Self::lod_view)'s terms.
    #[must_use]
    pub const fn occlusion_view(&self) -> bool {
        self.occlusion_view
    }

    /// Draws each fragment's motion vector instead of shading it.
    ///
    /// The two components are stretched by
    /// [`MOTION_VIEW_SCALE`](crcbl_shaders::mesh::MOTION_VIEW_SCALE) and centred
    /// on grey, into red and green, with blue left at zero. A pixel at rest is
    /// grey in both channels to within one half-float step —
    /// [`MOTION_VIEW_SCALE`](crcbl_shaders::mesh::MOTION_VIEW_SCALE) says why
    /// not exactly — and a surface travelling right and down the screen
    /// brightens both.
    ///
    /// # It is the motion-vector target's only reader today
    ///
    /// The forward pass writes a motion vector into a third colour attachment on
    /// every frame — see [`TransientImageDesc::motion`] — and no pass reads it
    /// yet; `docs/plan/49-antialiasing.md`'s TAA is the first that will. This
    /// view is therefore what says the subtraction behind it is the right one,
    /// which is why it exists ahead of a consumer.
    ///
    /// # It builds nothing and adds no pass
    ///
    /// One lane of the frame's uniform block, on
    /// [`set_normals_view`](Self::set_normals_view)'s terms exactly, and like
    /// the occlusion view it needs nothing from the geometry stage that a
    /// shaded frame does not already produce — so it draws on every
    /// [`GeometryPath`].
    ///
    /// **Wins over every other view when several are on**; see
    /// [`debug_view`](Self::debug_view), which is that order stated once.
    pub const fn set_motion_view(&mut self, on: bool) {
        self.motion_view = on;
    }

    /// Whether the colour pass draws the motion vector instead of shading.
    ///
    /// **What was asked for, not what is drawn**, on
    /// [`lod_view`](Self::lod_view)'s terms.
    #[must_use]
    pub const fn motion_view(&self) -> bool {
        self.motion_view
    }

    /// Draws the occlusion channel's **bent direction** instead of shading.
    ///
    /// The three channels beside the visibility scalar, passed through as the
    /// colour: the conventional `n * 0.5 + 0.5`, so +X reads red, +Y green and
    /// +Z blue, which is the mapping
    /// [`set_normals_view`](Self::set_normals_view) already uses for a surface
    /// normal. A fragment the occlusion pass had no direction for reads as the
    /// mid grey the zero sentinel encodes to.
    ///
    /// # What it is for
    ///
    /// The bent direction is what the ambient term is sampled *along*, and a
    /// composited frame cannot show it: ambient times albedo times a
    /// direction-dependent irradiance is one colour, and a direction that is
    /// subtly wrong looks like a lighting choice. This draws the direction
    /// alone, which is how a corner that leans the wrong way out of its own
    /// wall is a thing a reviewer sees rather than infers.
    ///
    /// # What it draws with occlusion switched off
    ///
    /// Mid grey, everywhere. A frame without
    /// [`RenderEffects::AMBIENT_OCCLUSION`] binds a 1×1 placeholder holding
    /// [`BENT_NORMAL_NONE`](crcbl_shaders::ssao::BENT_NORMAL_NONE) in those
    /// three channels, and this view shows that image rather than pretending to
    /// a direction nothing computed — which is the same answer the shading
    /// takes, since `mesh.slang` reads that sentinel as "use the shading
    /// normal".
    ///
    /// # It builds nothing and adds no pass
    ///
    /// One lane of the frame's uniform block, on
    /// [`set_normals_view`](Self::set_normals_view)'s terms exactly, and like
    /// the occlusion view it needs nothing from the geometry stage — so it
    /// draws on every [`GeometryPath`].
    ///
    /// **Wins over every other view when several are on**; see
    /// [`debug_view`](Self::debug_view), which is that order stated once.
    pub const fn set_bent_normal_view(&mut self, on: bool) {
        self.bent_normal_view = on;
    }

    /// Whether the colour pass draws the bent direction instead of shading.
    ///
    /// **What was asked for, not what is drawn**, on
    /// [`lod_view`](Self::lod_view)'s terms.
    #[must_use]
    pub const fn bent_normal_view(&self) -> bool {
        self.bent_normal_view
    }

    /// Tints the shaded picture by the **cascade** each sun-lit fragment's
    /// shadow was sampled from.
    ///
    /// `docs/plan/45-shadows.md`'s eighth decision made the cascade switch a
    /// band rather than a step, and this is the picture that band is judged in:
    /// each fragment's shading multiplied by
    /// [`CASCADE_TINTS`](crcbl_shaders::mesh::CASCADE_TINTS) of the cascade it
    /// selected, and inside the band by the *blend* of the two — the same weight
    /// the visibility itself was mixed with. So the boundary reads as a
    /// gradient, and a seam where two cascades disagree shows up as a step in
    /// the gradient rather than as a ring somebody has to already suspect.
    ///
    /// # It keeps the shaded picture, which is why it loses to every other view
    ///
    /// Every other view replaces the frame; this one multiplies it, so the geometry
    /// stays readable underneath and the shadow the cascades produced is still
    /// in the picture being coloured. That is also what puts it last in
    /// [`debug_view`](Self::debug_view)'s order: with a replacing view on there
    /// is no shading left for a tint to reach.
    ///
    /// # What it draws where no cascade covers a fragment
    ///
    /// Nothing at all —
    /// [`CASCADE_TINT_NONE`](crcbl_shaders::mesh::CASCADE_TINT_NONE) is white.
    /// A surface facing away from the sun and one past the last cascade's reach
    /// are both that, because neither was sampled through a cascade, and a
    /// frame drawn without [`RenderEffects::SHADOWS`](crate::RenderEffects::SHADOWS) is
    /// white everywhere for the same reason. Colouring those would be inventing
    /// a cascade for a pixel no map covers.
    ///
    /// # It builds nothing and adds no pass
    ///
    /// One lane of the frame's uniform block, on
    /// [`set_normals_view`](Self::set_normals_view)'s terms exactly, and like
    /// the occlusion view it needs nothing from the geometry stage — so it draws
    /// on every [`GeometryPath`].
    pub const fn set_cascade_view(&mut self, on: bool) {
        self.cascade_view = on;
    }

    /// Whether the colour pass tints the picture by the sun's cascades.
    ///
    /// **What was asked for, not what is drawn**, on
    /// [`lod_view`](Self::lod_view)'s terms — and here that gap is the usual
    /// case rather than the corner one: this view loses to every other one, so a
    /// renderer with any of them also on answers `true` here and draws that one.
    #[must_use]
    pub const fn cascade_view(&self) -> bool {
        self.cascade_view
    }

    /// Draws the **shadow atlas itself** over the finished frame: the
    /// `D32Float` image `crate::shadow` budgets, letterboxed at its own aspect,
    /// each texel's stored depth as a grey and a border round every occupied
    /// slot.
    ///
    /// `docs/plan/sample/18-sundial.md`'s atlas viewer, and the answer to a
    /// question no other view can be asked: which slot holds which map, and
    /// which slots hold nothing. A tile that was never rendered into, or
    /// rendered at the wrong viewport, lights a scene *fully* — so the frame it
    /// produces is plausible and every golden blessed from it passes. Here it is
    /// a rectangle that is visibly empty.
    ///
    /// # It is a pass, and it is the only debug view that is
    ///
    /// Every other view rides in one lane of the frame's uniform block and is a
    /// branch in `mesh.slang`, because every other view is a function of the
    /// fragment being shaded. The atlas is not: it is one image the whole frame
    /// shares. So this switch adds `crate::atlas_view`'s full-screen pass to
    /// the frame instead — after the tonemap, in display space, for that
    /// module's reason — and the frame block is byte-identical either way.
    ///
    /// # Where it resolves, and what it costs when it is off
    ///
    /// Below every replacing view and above
    /// [`set_cascade_view`](Self::set_cascade_view)'s tint — see
    /// [`debug_view`](Self::debug_view), which is that order stated once. A
    /// frame that resolved anything else records **no pass**, no pipeline bind
    /// and no block read, which is what makes the off position bit-identical on
    /// `crate::sky_pass`'s terms.
    pub const fn set_atlas_view(&mut self, on: bool) {
        self.atlas_view = on;
    }

    /// Whether the frame draws the shadow atlas over the picture.
    ///
    /// **What was asked for, not what is drawn**, on
    /// [`lod_view`](Self::lod_view)'s terms: a renderer with a replacing view
    /// also on answers `true` here and draws that one.
    #[must_use]
    pub const fn atlas_view(&self) -> bool {
        self.atlas_view
    }

    /// Pins the eye `docs/plan/25-lod.md`'s selection is projected from, so the
    /// cut stops following the camera.
    ///
    /// [`None`] is the default and means "the camera's own eye", which is what
    /// every frame before this existed did. `Some(eye)` selects detail for a
    /// viewer standing at `eye` while the frame is still drawn, culled and lit
    /// from wherever the camera actually is.
    ///
    /// # What it is for
    ///
    /// Per-cluster LOD is only checkable from somewhere other than the point
    /// that chose it. Looked at from the selecting eye, a cut that is far too
    /// coarse and a cut that is exactly right produce the same silhouette —
    /// that is what a screen-space error budget *means* — so the one viewpoint
    /// where a wrong cut cannot be seen is the viewpoint every unfrozen frame is
    /// judged from. Pin the eye and fly away from it, and the boundaries between
    /// levels become geometry a reviewer can walk up to and look at edge-on.
    ///
    /// # Only the selection freezes
    ///
    /// The frame has three places a viewer position is read, and this moves
    /// exactly one of them:
    ///
    /// * **the selection** — `draw_gen.slang`'s `select_level`, which reads
    ///   `DrawGenParams::camera_position` and is the only thing that decides the
    ///   cut. It writes the group hysteresis state that the mesh path's
    ///   amplification stage then descends, so freezing this one field freezes
    ///   the cut on the per-cluster path and the two indirect paths alike. This
    ///   is what moves.
    /// * **the frustum culls** — `cull.slang`'s instance test and
    ///   `mesh_cluster.slang`'s per-cluster one, whose planes are extracted from
    ///   the frame's own view-projection and never read a camera position at
    ///   all. Unaffected, and it has to be: a frame culled against a frustum the
    ///   reviewer has flown out of would draw nothing.
    /// * **the normal cone** — `mesh_cluster.slang`'s `cluster_survives`, which
    ///   reads [`FrameUniforms::camera_position`] and asks which way a cluster
    ///   faces *relative to the viewer*. That question is about the eye the
    ///   picture is drawn from, not the eye it was selected for; frozen, a
    ///   reviewer who flew round to the other side of the face would have every
    ///   cluster now facing them rejected for facing away from a viewpoint
    ///   nobody is standing at. It stays live.
    ///
    /// The split is not new. `DrawGenParams::camera_position` and
    /// [`FrameUniforms::camera_position`] are already separate fields carrying
    /// different values on a shadow cascade — that one holds the sun while this
    /// one stays the camera, because detail is denominated in the camera's
    /// pixels and facing is asked about the light. This freezes the same seam
    /// from the other side, and needs no new binding, no new shader input and no
    /// shader change of any kind.
    ///
    /// # It freezes the cascades' selection too
    ///
    /// [`begin_frame`](Self::begin_frame) hands the same eye to every shadow
    /// cascade's draw generator, because a cascade selects detail for the
    /// *camera* — see [`SHADOW_LOD_BIAS`]. A frozen camera cut over a live
    /// shadow cut would be a shadow silhouette sliding under a fixed one, which
    /// is a difference in the picture that belongs to neither.
    ///
    /// # The heatmap reads the live eye, and says so
    ///
    /// [`set_heatmap`](Self::set_heatmap) shades a cluster by its producing
    /// group's projected error, computed in the mesh stage from
    /// [`FrameUniforms::camera_position`] — the eye that stays live. So under a
    /// frozen selection the two overlays answer different questions: the cut is
    /// the one chosen at the pinned eye, and the heat is what the error would be
    /// if it were chosen from here now. That is readable — flying towards the
    /// face brightens the clusters the frozen cut is not refining, which is the
    /// selection's sensitivity made visible — but it does mean the ramp's
    /// contours stop coinciding with the level boundaries while frozen.
    /// [`set_lod_view`](Self::set_lod_view) is the overlay that composes
    /// unconditionally: a cluster's level is a property of the cut and of
    /// nothing else.
    ///
    /// Carrying the pinned eye into the mesh stage as well would need a second
    /// position in the frame block, which is a new shader input in every backend
    /// for one overlay's benefit; `docs/backlog.md` holds that trade-off.
    ///
    /// [`FrameUniforms::camera_position`]: crcbl_shaders::mesh::FrameUniforms::camera_position
    pub const fn set_frozen_selection_eye(&mut self, eye: Option<Vec3>) {
        self.frozen_selection_eye = eye;
    }

    /// Where the selection is pinned, or [`None`] while it follows the camera.
    ///
    /// The read-back half of
    /// [`set_frozen_selection_eye`](Self::set_frozen_selection_eye), and what a
    /// debug panel says "frozen at" out of — "frozen" without the position is a
    /// row nobody can act on.
    #[must_use]
    pub const fn frozen_selection_eye(&self) -> Option<Vec3> {
        self.frozen_selection_eye
    }

    /// Which of the debug views the next frame will actually draw.
    ///
    /// **The one place the precedence is decided**, and every other statement of
    /// it in this crate reads it back from here. The switches ride in one float
    /// lane of the frame block — see
    /// [`FrameUniforms::HEATMAP_VIEW_ON`](crcbl_shaders::mesh::FrameUniforms::HEATMAP_VIEW_ON)
    /// — and the shaders test the outermost threshold first, so a sentinel for
    /// an outer view clears every threshold below it. Resolving here rather than
    /// letting each caller guess is what stops "which view is on" being answered
    /// differently by a menu row, a debug panel and the picture.
    #[must_use]
    pub const fn debug_view(&self) -> DebugView {
        if self.bent_normal_view {
            DebugView::BentNormal
        } else if self.motion_view {
            DebugView::Motion
        } else if self.occlusion_view {
            DebugView::AmbientOcclusion
        } else if self.heatmap {
            DebugView::Heatmap
        } else if self.lod_view {
            DebugView::LodTint
        } else if self.normals_view {
            DebugView::Normals
        } else if self.atlas_view {
            // **Below every view above it, and above the tint below.**
            //
            // Below, because every branch before this one is a question about
            // the *picture* — a cluster's error, a surface's normal, the
            // occlusion channel — and this one is not a picture of the scene at
            // all. A caller who left the atlas up and then asked for normals has
            // asked to look at the frame again, and the frame is what the branch
            // above draws; the atlas is one keystroke away and is not lost by
            // being deferred to.
            //
            // Above, because the cascade tint multiplies the shaded picture and
            // this replaces it: a tint on a frame the atlas overwrote would be a
            // tint on the atlas, which is a colour standing for nothing.
            DebugView::ShadowAtlas
        } else if self.cascade_view {
            // **Innermost, and the only view that sits below the others rather
            // than above them.** Every branch before this one draws something
            // *instead of* the shaded picture and returns before the shading; a
            // cascade tint is a multiply on that shading, so there is nothing
            // for it to modulate while one of them is showing and a caller with
            // both on has asked for the picture the outer one draws. The lane
            // says the same thing from the other end — the cascade sentinel is
            // negative, so it clears none of the shaders' thresholds — and
            // `crcbl_shaders`'
            // `the_cascade_view_threshold_lies_below_every_replacing_view` is
            // what holds that half.
            DebugView::Cascades
        } else {
            DebugView::Shaded
        }
    }

    /// [`debug_view`](Self::debug_view) as the value the frame block's lane
    /// carries.
    const fn debug_view_lane(&self) -> f32 {
        match self.debug_view() {
            DebugView::BentNormal => mesh::FrameUniforms::BENT_NORMAL_VIEW_ON,
            DebugView::Motion => mesh::FrameUniforms::MOTION_VIEW_ON,
            DebugView::AmbientOcclusion => mesh::FrameUniforms::OCCLUSION_VIEW_ON,
            DebugView::Heatmap => mesh::FrameUniforms::HEATMAP_VIEW_ON,
            DebugView::LodTint => mesh::FrameUniforms::LOD_VIEW_ON,
            DebugView::Normals => mesh::FrameUniforms::NORMALS_VIEW_ON,
            DebugView::Cascades => mesh::FrameUniforms::CASCADE_VIEW_ON,
            // **The off sentinel, and it is not an omission.** This view is a
            // pass rather than a branch in `mesh.slang` — see
            // [`crate::atlas_view`] — so the colour pass draws exactly the
            // shaded picture it always drew and the pass afterwards replaces it.
            // A lane of its own would be a threshold no shader tests.
            DebugView::ShadowAtlas | DebugView::Shaded => mesh::FrameUniforms::NORMALS_VIEW_OFF,
        }
    }

    /// Whether the colour pass draws world-space normals instead of shading.
    ///
    /// The read-back half of [`set_normals_view`](Self::set_normals_view), on
    /// [`wireframe`](Self::wireframe)'s terms: what it draws is what the question
    /// is about.
    #[must_use]
    pub const fn normals_view(&self) -> bool {
        self.normals_view
    }

    /// The cull and draw-argument passes, and the buffers they produce.
    ///
    /// What a caller reads the culling statistics out of — topic 03 §3.6's
    /// delayed readback ring is [`DrawGen::visible_count`] — and what
    /// `crcbl-vk`'s end-to-end suite copies back to check the generated
    /// arguments against the draws this pass used to record itself.
    #[must_use]
    pub const fn draws(&self) -> &DrawGen {
        &self.draws
    }

    /// `frame`'s auto-exposure measurement: the bins the histogram pass filled
    /// and the exposure the reduce wrote out of them.
    ///
    /// What `crcbl`'s `mesh_e2e` copies back to check the histogram bin by bin
    /// against `crcbl_shaders::exposure`, on [`draws`](Self::draws)' terms — see
    /// [`ExposureBuffers`]. Both hold whatever the last frame that measured left
    /// there, so a frame drawn without
    /// [`RenderEffects::AUTO_EXPOSURE`](crate::RenderEffects::AUTO_EXPOSURE)
    /// hands back the frame before it rather than nothing.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this renderer was built with.
    #[must_use]
    pub fn exposure_buffers(&self, frame: usize) -> ExposureBuffers {
        self.auto_exposure.buffers(frame)
    }

    /// `frame`'s froxel column: the buffers `docs/plan/51-volumetrics.md`'s
    /// three passes write, and the block they read.
    ///
    /// What `crcbl`'s `mesh_e2e` copies back to check the column froxel by
    /// froxel against `crcbl_shaders::volumetric`, on
    /// [`draws`](Self::draws)' terms — see [`FroxelBuffers`].
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this renderer was built with.
    #[must_use]
    pub fn froxel_buffers(&self, frame: usize) -> FroxelBuffers {
        self.volumetric.buffers(frame)
    }

    /// The three layers a caller supplies to the toggle resolution order — see
    /// [`crate::effects`].
    ///
    /// Read back rather than only written, so a caller changing one layer does
    /// not have to remember the other two: take this, edit the field it owns,
    /// hand it back to [`set_effect_request`](Self::set_effect_request).
    #[must_use]
    pub const fn effect_request(&self) -> EffectRequest {
        self.effect_request
    }

    /// Replaces all three requested layers, and resolves them at the next
    /// [`begin_frame`](Self::begin_frame).
    ///
    /// **The frame in flight does not move.** `begin_frame` freezes what it
    /// resolved and [`add_passes`](Self::add_passes) records that, because the
    /// two halves of a frame have to agree about which culls were parametrised:
    /// `begin_frame` skips a shadow cull's parameter write when shadows are off
    /// and `add_passes` skips its dispatch, and a request landing between the two
    /// would dispatch a cull against numbers nothing zeroed.
    pub const fn set_effect_request(&mut self, request: EffectRequest) {
        self.effect_request = request;
    }

    /// What the frame the last [`begin_frame`](Self::begin_frame) opened draws.
    ///
    /// The resolved set, not a layer: every effect in it has survived the
    /// camera's stack, the player's quality clamp, any programmatic override and
    /// the device — which is the one question a test asking "did this frame have
    /// shadows in it" wants answered.
    ///
    /// [`RenderEffects::all`] before the first frame, which is what a renderer
    /// nobody has changed would draw.
    #[must_use]
    pub const fn effects(&self) -> RenderEffects {
        self.frame_effects
    }

    /// What a frame begun **now** would draw: the whole order applied to the
    /// current request.
    ///
    /// [`effects`](Self::effects)' question one frame earlier, and the one a
    /// caller reporting its own configuration wants — a debug panel built at
    /// startup has no frame behind it yet, and printing what the last one drew
    /// would print a default.
    ///
    /// **A debug view takes the antialiasing resolve off** — either tier of it
    /// — whatever the four layers said. Those views are readouts rather than
    /// pictures — a pixel's colour *is* the cluster's projected error or its
    /// DAG level, read back against a legend — and a filter that blends two
    /// clusters' shades invents a ramp position no cluster occupies.
    /// `apps/quarry`'s heatmap and LOD tests are the consumers that count a
    /// frame's distinct colours.
    #[must_use]
    pub fn resolved_effects(&self) -> RenderEffects {
        let resolved = self.effect_request.resolve(self.device_effects);
        if matches!(self.debug_view(), DebugView::Shaded) {
            resolved
        } else {
            // **Both tiers**, because the slot is what a readout cannot have
            // rather than one filter in particular.
            resolved.difference(RenderEffects::ANTIALIASING.union(RenderEffects::SMAA))
        }
    }

    /// Which effects this **device** permits, which is the fourth layer and
    /// clamps last.
    ///
    /// Every one of them today — see [`ForwardRenderer::device_effects`], which
    /// is where that is argued per effect rather than asserted.
    #[must_use]
    pub const fn device_effects(&self) -> RenderEffects {
        self.device_effects
    }

    /// Which [`GeometryPath`] this renderer was **built for** — not what the
    /// device reports, but what it actually built.
    ///
    /// The two are the same by construction and that is the point of asking the
    /// renderer rather than the device: [`GeometryPath::MeshShader`] here means
    /// `build` created a mesh pipeline out of `mesh_cluster.slang` and its
    /// cluster buffers, and created **no** raster pipeline for the pass to fall
    /// back to. So a frame this renderer drew came out of a mesh stage, and a
    /// silent degradation is not a state it has — which is the claim a golden
    /// image on its own could never make, because a fall-through draws the same
    /// picture.
    #[must_use]
    pub const fn geometry_path(&self) -> GeometryPath {
        match self.emit {
            EmitTail::Mesh => GeometryPath::MeshShader,
            EmitTail::Count => GeometryPath::IndirectCount,
            EmitTail::PerBatch => GeometryPath::IndirectPerBatch,
        }
    }

    /// Whether this renderer **built** §3.5's per-cluster cull — an
    /// amplification stage in front of the mesh stage, rejecting a surviving
    /// instance's back-facing and off-screen clusters.
    ///
    /// `false` on the two indirect tails, which draw index ranges and have no
    /// stage to put it in, and on a device with `Features::MESH_SHADER` and no
    /// `Features::TASK_SHADER` — a real and supported state, not a degradation:
    /// that device draws every cluster of every surviving instance and the
    /// picture is the same one.
    ///
    /// The instance cull runs on every path either way, and is unaffected by
    /// this.
    ///
    /// It is asked of the renderer rather than of the device for
    /// [`ForwardRenderer::geometry_path`]'s reason: this reports what was
    /// built, so `true` means
    /// [`DrawGen::visible_count`](crate::draw_gen::DrawGen::visible_count)'s
    /// cluster word is a number the frame produced rather than the zero the
    /// clearing pass left.
    #[must_use]
    pub const fn culls_clusters(&self) -> bool {
        self.culls_clusters
    }

    /// Whether this renderer can choose **which level** of a
    /// [`Geometry::Dag`] mesh an object is drawn at, and therefore whether
    /// [`ForwardRenderer::add_instance`] may be given one at all.
    ///
    /// `docs/plan/25-lod.md`'s selection happens in the amplification stage where
    /// there is one, and in the cull pass where the tail is indirect and a level
    /// is an ordinary index range. **One device shape has neither**: a mesh stage
    /// with [`Features::MESH_SHADER`](crcbl_hal::Features::MESH_SHADER) and no
    /// [`Features::TASK_SHADER`](crcbl_hal::Features::TASK_SHADER) draws through
    /// `mesh_cluster.slang`'s un-amplified `meshMain`, which emits every cluster
    /// of the bucket — and a DAG's bucket is every level end to end, so the frame
    /// is several overlapping copies of one surface. That is a real and supported
    /// device state rather than a degradation, so this reports it instead of
    /// refusing to build.
    ///
    /// A [`Geometry::Flat`] mesh is one level and is unaffected: its clusters
    /// carry [`ClusterSelect::ALWAYS`] and every path draws it from every camera,
    /// which is why this is asked only before placing a DAG.
    ///
    /// Asked of the renderer rather than of the device for
    /// [`ForwardRenderer::culls_clusters`]' reason: it reports what was built.
    ///
    /// [`ClusterSelect::ALWAYS`]: crcbl_shaders::cluster_select::ClusterSelect::ALWAYS
    #[must_use]
    pub const fn selects_levels(&self) -> bool {
        !self.emit.is_mesh() || self.culls_clusters
    }

    /// Which frame-in-flight slot the last [`ForwardRenderer::begin_frame`]
    /// rotated to, which is the slot every per-frame buffer is indexed by.
    #[must_use]
    pub const fn frame(&self) -> usize {
        self.frame
    }

    /// What the last [`add_passes`](Self::add_passes) recorded, and what it
    /// cannot know.
    ///
    /// Zero draws until a frame has been built: this reports what was recorded,
    /// not what a frame would record.
    ///
    /// # The triangles are [`None`] here, on every geometry path
    ///
    /// Every draw this renderer records except the full-screen passes' and the
    /// debug draw layer's is **indirect**:
    /// the instance count and the index range live in the argument buffer
    /// `draw_gen.slang` wrote, so the CPU records the call and learns nothing
    /// about what it covered. That is true of
    /// [`GeometryPath::MeshShader`] and of
    /// both indirect tails alike — the three differ in which call is recorded,
    /// not in who decides the counts — so
    /// [`FrameCounters::triangles`](crate::counters::FrameCounters::triangles)
    /// is [`None`] whatever the device supports. Nothing counts a triangle on
    /// the GPU either, and a cluster count times a nominal triangles-per-cluster
    /// would read as authoritative and be neither.
    ///
    /// # The instances drawn come back off the GPU, a few frames late
    ///
    /// [`FrameCounters::drawn`](crate::counters::FrameCounters::drawn) is
    /// `cull.slang`'s survivor count, off [`crate::cull_stats`]'s delayed ring,
    /// plus the frame's direct draws — the ones here whose instance the CPU
    /// knows about, and the ones that are also in
    /// [`instances`](crate::counters::FrameCounters::instances). So the pair is
    /// comparable: the same quantity submitted and kept.
    ///
    /// **The camera's cull alone.** The shadow culls test other frustums and
    /// their survivors are counted into buffers of their own; adding them here
    /// would give a "drawn" larger than "submitted" on any frame with a shadow
    /// in it.
    ///
    /// It is [`None`] — the row says `indirect` — for the first few frames of a
    /// renderer's life, while the ring has not come round, and for good on a
    /// device that would not give out a readback.
    /// [`cull_frame`](crate::counters::FrameCounters::cull_frame) is which frame
    /// the number is about whenever there is one.
    ///
    /// [`FrameCounters::instances`](crate::counters::FrameCounters::instances)
    /// is known and is the plan's "submitted" half: the live instances
    /// [`add_passes`](Self::add_passes) hands the cull dispatches, plus the
    /// frame's direct draws — every full-screen pass's triangle and the debug
    /// draw layer's line list.
    #[must_use]
    pub fn counters(&self) -> FrameCounters {
        if self.recorded_draws == 0 {
            return FrameCounters::default();
        }
        let stats = self.cull_stats();
        let direct = self.direct_draws();
        FrameCounters {
            draws: self.recorded_draws,
            instances: self.instances.len() as u64 + direct,
            // Each direct draw is submitted and drawn, and none of them is in
            // the cull's count — they are draws of one instance, added on both
            // sides so the two halves of the row measure the same thing. Off the
            // frame that *was* recorded rather than off a constant, so a frame
            // with an effect switched off does not count a triangle it never
            // submitted.
            drawn: stats.map(|stats| stats.instances + direct),
            triangles: None,
            // The survivors alone: the panel row is "clusters drawn", and the
            // frustum and cone rejection counts beside them are a different
            // question — `CullStats::clusters` carries all three for a caller
            // that wants the split, which `apps/quarry`'s own panel row is.
            clusters: stats
                .and_then(|stats| stats.clusters)
                .map(|cull| cull.survivors),
            cull_frame: stats.map(|stats| stats.frame),
        }
    }

    /// The draws this renderer records **directly** — the ones whose instance
    /// the CPU knows about, as opposed to the indirect calls whose counts live
    /// in a buffer a shader wrote.
    ///
    /// Every full-screen pass's over-sized triangle, plus the debug draw
    /// layer's line list on a frame that has one. One instance each, which is
    /// why [`counters`](Self::counters) adds this to both halves of its
    /// submitted/drawn pair.
    fn direct_draws(&self) -> u64 {
        self.recorded_fullscreen * FULLSCREEN_DRAWS
            + self.recorded_debug_draw
            + self.recorded_tile_clears
    }

    /// What the camera's cull kept, on the frame the ring has reached.
    ///
    /// Topic 03 §3.6's one permitted readback, and [`None`] until it has come
    /// round — see [`crate::cull_stats`] for the latency and for what makes it
    /// free of any wait. [`counters`](Self::counters) is where this reaches the
    /// panel; it is exposed on its own because the frame number and the cluster
    /// word are both things a test asserts about directly.
    #[must_use]
    pub fn cull_stats(&self) -> Option<crate::cull_stats::CullStats> {
        self.cull_stats.as_ref().and_then(CullStatsRing::latest)
    }

    /// Describes the imported swapchain image the frame ends in.
    ///
    /// A helper rather than something every caller writes, because getting
    /// `initial`/`final_state` wrong is exactly the hand-written barrier this
    /// slice removed: an acquired image's contents are undefined, and the
    /// compositor may only be handed one in [`ResourceState::Present`].
    #[must_use]
    pub fn present_target(
        image: crcbl_hal::ImageHandle,
        view: ImageViewHandle,
        format: Format,
        extent: (u32, u32),
    ) -> ImportedImage {
        ImportedImage {
            image,
            view,
            format,
            extent,
            initial: ResourceState::Undefined,
            // The acquire's semaphore, not a barrier, is what orders this frame
            // after the last use of the image — which is what makes `Undefined`
            // honest here and what makes it uncheckable. See `InitialClaim`.
            claim: InitialClaim::Acquired,
            final_state: ResourceState::Present,
        }
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        if let Some(stats) = self.cull_stats {
            stats.destroy(device);
        }
        for (_, group) in self.tonemap_groups.into_iter().flatten() {
            device.destroy_bind_group(group);
        }
        for buffer in self.tonemap_uniforms {
            device.destroy_buffer(buffer);
        }
        device.destroy_sampler(self.sampler);
        device.destroy_graphics_pipeline(self.tonemap_pipeline);
        device.destroy_pipeline_layout(self.tonemap_pipeline_layout);
        device.destroy_bind_group_layout(self.tonemap_layout);

        device.destroy_graphics_pipeline(self.shadow_pipeline);
        device.destroy_graphics_pipeline(self.rsm_pipeline);
        device.destroy_graphics_pipeline(self.shadow_clear_pipeline);
        for groups in self.shadow_groups {
            for group in groups {
                device.destroy_bind_group(group);
            }
        }
        for buffers in self.shadow_uniforms {
            for buffer in buffers {
                device.destroy_buffer(buffer);
            }
        }
        for buffers in self.shadow_selection {
            for buffer in buffers {
                device.destroy_buffer(buffer);
            }
        }
        for draws in self.shadow_draws {
            draws.destroy(device);
        }
        device.destroy_sampler(self.shadow_sampler);
        device.destroy_image_view(self.shadow_atlas_view);
        device.destroy_image(self.shadow_atlas);
        device.destroy_image_view(self.shadow_placeholder_view);
        device.destroy_image(self.shadow_placeholder);

        if let Some(grid) = self.ground_grid {
            grid.destroy(device);
        }
        // Nothing to release on a renderer nobody drew a segment through — see
        // [`crate::debug_draw`], whose pipeline is built on first use.
        self.debug_draw.destroy(device);
        self.atlas_viewer.destroy(device);
        self.sky_pass.destroy(device);
        self.upscale.destroy(device);
        self.smaa.destroy(device);
        self.fxaa.destroy(device);
        self.bloom.destroy(device);
        self.auto_exposure.destroy(device);
        self.volumetric.destroy(device);
        self.ssr.destroy(device);
        self.hiz.destroy(device);
        self.contact_shadows.destroy(device);
        self.ssao.destroy(device);
        self.ambient_occlusion_placeholder.destroy(device);
        self.contact_shadow_placeholder.destroy(device);
        self.probe_visibility_placeholder.destroy(device);
        if let Some(captured) = self.probe_visibility {
            captured.destroy(device);
        }
        self.specular_dfg.destroy(device);
        self.ltc_table.destroy(device);
        self.normal_page.destroy(device);

        device.destroy_graphics_pipeline(self.mesh_pipeline);
        // The one place the wireframe twin is released — `set_wireframe(.., false)`
        // deliberately is not, because this call is the one that requires an
        // idle device.
        if let Some(pipeline) = self.wireframe_pipeline {
            device.destroy_graphics_pipeline(pipeline);
        }
        device.destroy_pipeline_layout(self.mesh_pipeline_layout);
        for group in self
            .mesh_groups
            .into_iter()
            .chain(self.prepass_groups)
            .chain(
                self.screen_channel_groups
                    .into_iter()
                    .flatten()
                    .map(|(_, group)| group),
            )
        {
            device.destroy_bind_group(group);
        }
        for buffer in self.prepass_stats {
            device.destroy_buffer(buffer);
        }
        device.destroy_bind_group_layout(self.mesh_layout);
        self.base_color_page.destroy(device);
        device.destroy_sampler(self.base_color_sampler);
        for sampler in self.retired_page_samplers {
            device.destroy_sampler(sampler);
        }
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        for buffer in self.cluster_selection {
            device.destroy_buffer(buffer);
        }
        device.destroy_buffer(self.draw_constants);
        if let Some(clusters) = self.clusters {
            clusters.destroy(device);
        }
        // Before the draw generator, because its bind groups name that
        // generator's statistics buffers: the overflow counter is a word of
        // them.
        self.lights.destroy(device);
        self.draws.destroy(device);
        // Before the probe table, because its bind groups name that table's
        // rows — the same ordering the light grid is under above.
        if let Some(gather) = self.probe_gather {
            gather.destroy(device);
        }
        self.probes.destroy(device);
        self.materials.destroy(device);
        self.instances.destroy(device);
        self.pool.destroy(device);
    }
}

/// What the shadow pass a frame recorded would leave the image holding, held
/// until the frame after can say whether its body ran.
///
/// `docs/plan/45-shadows.md`'s static-caching rung, per group: recording a pass
/// is not drawing one, so nothing here is believed until
/// [`ForwardRenderer::shadow_pass_ran`] carries this [`ShadowCommit::id`] back
/// out of the graph.
#[derive(Clone, Debug)]
struct ShadowCommit {
    /// The [`ForwardRenderer::shadow_pass_id`] the body publishes.
    id: u64,
    /// `[group]`: what the group's entry of
    /// [`ForwardRenderer::shadow_group_held`] becomes for every group the pass
    /// redraws, and [`None`] for the groups it holds.
    groups: Vec<Option<(u64, Vec3, f32)>>,
    /// The layout the pass draws under, where it clears the whole attachment —
    /// and [`None`] where it loads, because a loading pass leaves every tile it
    /// does not redraw exactly as it found it.
    layout: Option<Vec<shadow::TileRect>>,
}

/// The shader modules a mesh pipeline is built from, with its entry points
/// already resolved.
///
/// **A type rather than a run of locals in `build`** because there are now two
/// callers: `build`, which makes the colour pipeline and its depth-only twin,
/// and [`ForwardRenderer::set_wireframe`], which makes the colour pipeline again
/// in [`PolygonMode::Line`] long after `build` released its modules. The
/// alternative was a second copy of a descriptor that names two modules, their
/// entry points, the colour targets and a depth state — the pair that drifts.
///
/// Short-lived: created, used to build pipelines, and [`MeshModules::destroy`]ed
/// in the same function. A module is not needed once the pipeline exists.
struct MeshModules {
    /// `mesh.slang`'s module — the vertex stage on the indirect path, and the
    /// fragment stage on both.
    mesh: ShaderModuleHandle,
    /// `vertexMain`, unused on the mesh-shader path.
    vertex: &'static str,
    /// `depthVertexMain`, which [`MeshModules::depth_pipeline`] takes in
    /// [`MeshModules::vertex`]'s place: the same transform off stream 0 of the
    /// vertex pool and no attribute region at all. Unused on the mesh-shader
    /// path, where the geometry stage is `mesh_cluster.slang`'s either way.
    depth_vertex: &'static str,
    /// `depthClearVertexMain`, which [`MeshModules::depth_clear_pipeline`]
    /// builds the tile clear from: three corners off `SV_VertexID` and no input
    /// at all, so it resets one tile of the atlas without reading a vertex.
    ///
    /// **The same stage on every path**, unlike the two above: a device with a
    /// mesh stage still has a vertex stage, and a clear that produced no
    /// geometry is a primitive, not a cluster.
    depth_clear_vertex: &'static str,
    fragment: &'static str,
    /// `rsmFragmentMain`, which [`MeshModules::rsm_pipeline`] takes in
    /// [`MeshModules::fragment`]'s place: the same geometry stage, three targets
    /// describing the surface rather than shading it.
    ///
    /// **The reason the shaded stage above is looked up by name.** A module with
    /// two fragment entry points has no unambiguous one, which is what `entry`
    /// answers `None` for.
    rsm_fragment: &'static str,
    /// `mesh_cluster.slang`'s, on the mesh-shader path alone.
    cluster: Option<ClusterStages>,
}

/// The geometry half of a mesh-shader pipeline: `mesh_cluster.slang`'s module
/// and the stages taken out of it.
struct ClusterStages {
    /// The module the mesh stage is built from, and the one the task stage is,
    /// which are **two modules rather than one**.
    ///
    /// `mesh_cluster.slang` compiles to a combined module carrying every entry
    /// point and to one module per stage, and these are the per-stage ones. A
    /// module holding a task entry point and its mesh partner carries two
    /// `TaskPayloadWorkgroupEXT` variables — the task writes one, the mesh stage
    /// reads the other — and an NVIDIA driver then delivers a payload of zeroes,
    /// which draws cluster 0 of instance slot 0 of every bucket and nothing
    /// else. See `crcbl_shaders::EntryPoint`.
    mesh_module: ShaderModuleHandle,
    /// The task stage's own module, on the same terms. [`None`] exactly when
    /// [`task`](Self::task) is.
    task_module: Option<ShaderModuleHandle>,
    /// Which of the module's two mesh entry points this path builds — the
    /// amplified one where the device has a task stage to feed it.
    mesh: &'static str,
    /// §3.5's per-cluster cull, where the device has the stage to run it in.
    /// [`None`] is not a degradation: `meshMain` draws the same picture out of
    /// the same clusters, having rejected none of them, which is what a device
    /// with [`Features::MESH_SHADER`](crcbl_hal::Features::MESH_SHADER) and no
    /// [`Features::TASK_SHADER`](crcbl_hal::Features::TASK_SHADER) gets.
    task: Option<&'static str>,
}

impl MeshModules {
    /// **Three targets, one fragment stage.** `mesh.slang`'s `FragmentOutput`
    /// writes the shaded colour, then `docs/plan/18-render-features.md`'s
    /// reflectivity channel, then `docs/plan/43-render-standards.md` §9's motion
    /// vector — and both pipeline shapes name that same entry point, so each
    /// target is one more element of this array and not a second pipeline, a
    /// second entry point or a new interpolant. The refusal the AO section
    /// records was about the depth prepass, which has no fragment stage and no
    /// colour target at all; it does not reach this array.
    ///
    /// Element order is `SV_Target` order, and each format is the one the
    /// matching `TransientImageDesc` creates: a pipeline built for a format the
    /// attachment is not in is a validation error on WebGPU and a silent
    /// reinterpretation elsewhere.
    const COLOR_TARGETS: [ColorTargetState; 3] = [
        ColorTargetState::opaque(Format::Rgba16Float),
        ColorTargetState::opaque(Format::Rgba8Unorm),
        ColorTargetState::opaque(MOTION_FORMAT),
    ];

    /// **Three targets again, and a different three.** `mesh.slang`'s
    /// `RsmOutput` writes the surface's diffuse albedo, its world normal encoded
    /// `n * 0.5 + 0.5`, and its world position with a coverage flag in `w` —
    /// which is what `docs/plan/50-irradiance-probes.md`'s gather needs of a
    /// texel and nothing more.
    ///
    /// Element order is `SV_Target` order and each format is the one the
    /// matching description in [`crate::rsm`] creates: a pipeline built for a
    /// format the attachment is not in is a validation error on WebGPU and a
    /// silent reinterpretation elsewhere. `the_targets_carry_the_formats_the_shader_writes`
    /// in that module is the other half of the pair.
    const RSM_TARGETS: [ColorTargetState; 3] = [
        ColorTargetState::opaque(Format::Rgba16Float),
        ColorTargetState::opaque(Format::Rgba8Unorm),
        ColorTargetState::opaque(Format::Rgba32Float),
    ];

    /// Invocations per mesh workgroup, which is `mesh_cluster.slang`'s
    /// `[numthreads(THREADS, 1, 1)]` — and `THREADS` is
    /// `MAX_CLUSTER_VERTICES`, one lane per vertex a cluster can hold.
    ///
    /// Taken from the crate that owns the shader source rather than written
    /// out, as
    /// [`MeshPipelineDesc::mesh_workgroup_size`](crcbl_hal::MeshPipelineDesc::mesh_workgroup_size)
    /// requires: only Metal reads this field, so a literal that disagreed with
    /// the shader would launch the wrong number of threads on one backend and
    /// nowhere else.
    const MESH_WORKGROUP_SIZE: [u32; 3] =
        [crcbl_shaders::meshlet::MAX_CLUSTER_VERTICES as u32, 1, 1];

    /// Invocations per task workgroup: `taskMain` is `[numthreads(1, 1, 1)]`,
    /// one invocation per cluster group, which is what lets the payload be a
    /// plain local. See `mesh_cluster.slang`.
    const TASK_WORKGROUP_SIZE: [u32; 3] = [1, 1, 1];

    /// Loads whichever modules `emit` needs and resolves their entry points.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] if the committed manifest names no such
    /// entry point, and [`HalError`] from either module. A failure after the
    /// first module was created releases it rather than leaking it.
    fn new(device: &dyn Device, emit: EmitTail, culls_clusters: bool) -> Result<Self, HalError> {
        // Entry points resolved before the module exists: a manifest that
        // disagreed with the SPIR-V would otherwise fail inside the descriptor
        // literal, with the module already created and nothing holding it.
        //
        // **The vertex stage is named rather than looked up by stage**, for
        // `mesh_cluster.slang`'s reason below: since the depth-only stage landed
        // this module has two vertex entry points, and a stage lookup answers
        // `None` for two matches. Which of the two a pipeline names is the whole
        // of what the split-stream vertex pool bought.
        let vertex = named_entry(&MESH, "vertexMain", Stage::Vertex)?;
        let depth_vertex = named_entry(&MESH, "depthVertexMain", Stage::Vertex)?;
        let depth_clear_vertex = named_entry(&MESH, "depthClearVertexMain", Stage::Vertex)?;
        // **Named rather than looked up by stage, on the vertex entry points'
        // terms exactly.** Since `docs/plan/50-irradiance-probes.md`'s updater
        // landed, this module has *two* fragment entry points — the shaded one
        // and `rsmFragmentMain` — and `entry` answers `None` for two matches, so
        // a stage lookup here refuses the build outright with "exposes no
        // unambiguous Fragment entry point".
        let fragment = named_entry(&MESH, "fragmentMain", Stage::Fragment)?;
        let rsm_fragment = named_entry(&MESH, "rsmFragmentMain", Stage::Fragment)?;
        // **Named rather than looked up by stage**, because the module has two
        // mesh entry points and a stage lookup would refuse an ambiguous one —
        // see `named_entry`. Which of the two this names is the whole of the
        // amplification decision.
        let cluster_stages = emit
            .is_mesh()
            .then(|| {
                let mesh = named_entry(
                    &MESH_CLUSTER,
                    if culls_clusters {
                        "amplifiedMeshMain"
                    } else {
                        "meshMain"
                    },
                    Stage::Mesh,
                )?;
                let task = culls_clusters
                    .then(|| named_entry(&MESH_CLUSTER, "taskMain", Stage::Task))
                    .transpose()?;
                Ok::<_, HalError>((mesh, task))
            })
            .transpose()?;

        let mesh = device.create_shader_module(&ShaderModuleDesc {
            label: Some("mesh.slang"),
            spirv: MESH.spirv(),
            wgsl: MESH.wgsl(),
            msl: MESH.msl(),
            // **Both containers, and still one module.** A DXIL container holds
            // exactly one entry point, so this is a list where its three
            // neighbours are single artifacts; the backend that consumes it
            // picks the container named by the stage it is building. Nothing
            // here branches on a backend, and the three that ignore DXIL see
            // the same module they always did.
            dxil: &MESH.dxil_containers(),
        })?;
        let cluster = match cluster_stages {
            None => None,
            Some((cluster_mesh, task)) => {
                // **The mesh stage's module is `mesh_cluster.slang`; the
                // fragment stage's is still `mesh.slang`'s.** A pipeline takes a
                // module per stage, so the shading — Lambert, the GGX lobe, the
                // material row, the page sample — is the same code both paths
                // run rather than a copy that agrees today.
                // `mesh_cluster.slang`'s header carries the argument, and its
                // `VertexOutput` is what the two agree through.
                // **One module per stage, each carrying that stage's own
                // SPIR-V.** Only the SPIR-V differs between them: MSL is one
                // artifact per source whatever the stage, and a DXIL container
                // is already per entry point, so both are handed over unchanged
                // and the backends that read them see what they always did.
                let stage_module = |entry_point: &str| {
                    device.create_shader_module(&ShaderModuleDesc {
                        label: Some("mesh_cluster.slang"),
                        // The combined module where this stage has none of its
                        // own, which is every stage of every other shader.
                        spirv: MESH_CLUSTER
                            .spirv_for(entry_point)
                            .unwrap_or_else(|| MESH_CLUSTER.spirv()),
                        // `None`, and it is the whole reason this is a second
                        // file: WGSL cannot express a mesh stage at all.
                        wgsl: MESH_CLUSTER.wgsl(),
                        msl: MESH_CLUSTER.msl(),
                        dxil: &MESH_CLUSTER.dxil_containers(),
                    })
                };
                let mesh_module = match stage_module(cluster_mesh) {
                    Ok(module) => module,
                    Err(error) => {
                        device.destroy_shader_module(mesh);
                        return Err(error);
                    }
                };
                let task_module = match task.map(stage_module).transpose() {
                    Ok(module) => module,
                    Err(error) => {
                        device.destroy_shader_module(mesh);
                        device.destroy_shader_module(mesh_module);
                        return Err(error);
                    }
                };
                Some(ClusterStages {
                    mesh_module,
                    task_module,
                    mesh: cluster_mesh,
                    task,
                })
            }
        };
        Ok(Self {
            mesh,
            vertex,
            depth_vertex,
            depth_clear_vertex,
            fragment,
            rsm_fragment,
            cluster,
        })
    }

    /// The rasteriser state both pipeline shapes share, because the only thing
    /// that differs between them is which stage produces the geometry.
    ///
    /// Back-face culling is on from the first mesh. The cube's winding is
    /// asserted by `crcbl-shaders`' own tests, so a face that vanished would be a
    /// *test* failure rather than a debugging session — and a mesh drawn without
    /// culling would let a winding mistake survive into P7's geometry pool. The
    /// mesh stage emits its corner triples in the index buffer's own order, so
    /// the winding it produces is the same one.
    fn primitive(polygon_mode: PolygonMode) -> PrimitiveState {
        PrimitiveState {
            cull_mode: CullMode::Back,
            polygon_mode,
            ..PrimitiveState::default()
        }
    }

    /// Milestone 3's depth test, and the seam's default is already reversed-Z:
    /// `Greater` against `D32Float`, writes on. The clear value that agrees with
    /// it comes from the graph (`PassBuilder::clear_depth`), and the projection
    /// matrix that agrees with **both** comes from [`crate::camera`].
    fn depth_stencil() -> Option<DepthStencilState> {
        Some(DepthStencilState::default())
    }

    /// The colour pass's depth state, which is not the same shape as
    /// [`MeshModules::depth_stencil`] because the depth prepass has already
    /// drawn this geometry.
    ///
    /// **Filled: [`DepthStencilState::equal_depth_read_only`], so the shading is
    /// paid for once per pixel.** The prepass runs the same draws through
    /// `depthVertexMain` — or, on the mesh path, through the very same geometry
    /// stage — so the depth already in the attachment is this pass's own answer,
    /// and a `GreaterOrEqual` test with writes off keeps exactly the fragments
    /// that would have won the write. What it removes is the clustered-forward
    /// shading of every fragment a nearer one goes on to cover.
    ///
    /// **Lines: the writing default, because a wireframe is not the geometry the
    /// prepass drew.** [`PolygonMode::Line`] rasterises the triangle's edges,
    /// and an edge pixel's depth is interpolated along the line rather than
    /// across the face, so testing it for equality against a filled prepass
    /// rejects most of the wireframe. That frame keeps the pass shape this pass
    /// had before the prepass existed — see the colour pass's depth attachment,
    /// which branches on the same condition.
    fn color_depth_stencil(polygon_mode: PolygonMode) -> Option<DepthStencilState> {
        match polygon_mode {
            PolygonMode::Fill => Some(DepthStencilState::equal_depth_read_only(Format::D32Float)),
            _ => Self::depth_stencil(),
        }
    }

    /// The colour pass's pipeline, filled or wireframe.
    ///
    /// `label` names the caller — `"forward"` for the frame's own, `"wireframe"`
    /// for [`ForwardRenderer::set_wireframe`]'s — so a capture tells the two
    /// apart.
    fn color_pipeline(
        &self,
        device: &dyn Device,
        layout: PipelineLayoutHandle,
        polygon_mode: PolygonMode,
        label: &str,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let primitive = Self::primitive(polygon_mode);
        let depth_stencil = Self::color_depth_stencil(polygon_mode);
        let fragment = Some(ShaderEntry {
            module: self.mesh,
            entry_point: self.fragment,
        });
        match self.cluster.as_ref() {
            Some(cluster) => device.create_mesh_pipeline(&MeshPipelineDesc {
                label: Some(&format!("{label} mesh cluster")),
                layout,
                task: cluster
                    .task
                    .zip(cluster.task_module)
                    .map(|(entry_point, module)| ShaderEntry {
                        module,
                        entry_point,
                    }),
                task_workgroup_size: Self::TASK_WORKGROUP_SIZE,
                mesh: ShaderEntry {
                    module: cluster.mesh_module,
                    entry_point: cluster.mesh,
                },
                mesh_workgroup_size: Self::MESH_WORKGROUP_SIZE,
                fragment,
                primitive,
                depth_stencil,
                multisample: MultisampleState::default(),
                color_targets: &Self::COLOR_TARGETS,
            }),
            None => device.create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some(&format!("{label} mesh")),
                layout,
                vertex: ShaderEntry {
                    module: self.mesh,
                    entry_point: self.vertex,
                },
                fragment,
                primitive,
                depth_stencil,
                multisample: MultisampleState::default(),
                color_targets: &Self::COLOR_TARGETS,
            }),
        }
    }

    /// The depth-only twin of [`MeshModules::color_pipeline`], built out of the
    /// same modules and the same layout: no fragment stage, no colour target,
    /// and a vertex stage that fetches stream 0 of the vertex pool alone.
    ///
    /// **The transform is identical and the fetch is not, which is the point.**
    /// Topic 18 asks for a shadow pass "identical on every `GeometryPath` —
    /// depth pass plus whatever emit tail the device selected", and what makes
    /// that work is a frame block whose `view_proj` is the cascade's matrix
    /// rather than a second transform path: a cascade that disagreed with the
    /// colour pass about where a vertex is would produce shadows that do not
    /// line up with their casters, which is indistinguishable from a bias
    /// problem. `depthVertexMain` is that same clip position written the same
    /// way, with everything the discarded varyings needed — the attribute
    /// region and the previous frame's position — left unread;
    /// `docs/plan/43-render-standards.md` §2 split the pool for exactly this
    /// pass, and this is where it is spent.
    ///
    /// **The mesh-shader path still runs the colour pipeline's geometry
    /// stage.** `mesh_cluster.slang` emits one `VertexOutput`, so a depth-only
    /// mesh stage is a second entry point in that module and its own rung; a
    /// device with a mesh stage goes on reading whole vertices here.
    ///
    /// Always [`PolygonMode::Fill`]: it fills the shadow atlas and the depth
    /// prepass, and depth drawn as lines is depth with holes in it — which the
    /// occlusion pair would read as geometry that is not there.
    fn depth_pipeline(
        &self,
        device: &dyn Device,
        layout: PipelineLayoutHandle,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let primitive = Self::primitive(PolygonMode::Fill);
        match self.cluster.as_ref() {
            Some(cluster) => device.create_mesh_pipeline(&MeshPipelineDesc {
                label: Some("shadow cascade mesh cluster"),
                layout,
                task: cluster
                    .task
                    .zip(cluster.task_module)
                    .map(|(entry_point, module)| ShaderEntry {
                        module,
                        entry_point,
                    }),
                task_workgroup_size: Self::TASK_WORKGROUP_SIZE,
                mesh: ShaderEntry {
                    module: cluster.mesh_module,
                    entry_point: cluster.mesh,
                },
                mesh_workgroup_size: Self::MESH_WORKGROUP_SIZE,
                fragment: None,
                primitive,
                depth_stencil: Self::depth_stencil(),
                multisample: MultisampleState::default(),
                color_targets: &[],
            }),
            None => device.create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("shadow cascade"),
                layout,
                vertex: ShaderEntry {
                    module: self.mesh,
                    entry_point: self.depth_vertex,
                },
                fragment: None,
                primitive,
                depth_stencil: Self::depth_stencil(),
                multisample: MultisampleState::default(),
                color_targets: &[],
            }),
        }
    }

    /// The reflective shadow map's pipeline: the colour pipeline's geometry
    /// stage, `rsmFragmentMain` in the shaded stage's place, and
    /// [`MeshModules::RSM_TARGETS`] in [`MeshModules::COLOR_TARGETS`]'s.
    ///
    /// `docs/plan/50-irradiance-probes.md`'s updater draws the sun's near
    /// cascade a second time through this — see [`crate::rsm`], which owns the
    /// attachments, and `ForwardRenderer::add_shadow_pass`, which records the
    /// pass out of that cascade's own bind group and its own already-generated
    /// draws.
    ///
    /// **[`MeshModules::vertex`] and not [`MeshModules::depth_vertex`]**, unlike
    /// the cascade this rides beside: a fragment stage that writes a world
    /// position, a normal and an albedo needs the varyings `depthVertexMain`
    /// exists to leave unread. On the mesh path it is the very same geometry
    /// stage the colour pass runs, for the reason `depth_pipeline` gives.
    ///
    /// **Its own depth, written**, rather than the colour pass's read-only
    /// equality test: nothing has drawn this view's depth before it, and the
    /// pass is what decides which surface a texel of the map holds.
    fn rsm_pipeline(
        &self,
        device: &dyn Device,
        layout: PipelineLayoutHandle,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let primitive = Self::primitive(PolygonMode::Fill);
        let fragment = Some(ShaderEntry {
            module: self.mesh,
            entry_point: self.rsm_fragment,
        });
        match self.cluster.as_ref() {
            Some(cluster) => device.create_mesh_pipeline(&MeshPipelineDesc {
                label: Some("rsm mesh cluster"),
                layout,
                task: cluster
                    .task
                    .zip(cluster.task_module)
                    .map(|(entry_point, module)| ShaderEntry {
                        module,
                        entry_point,
                    }),
                task_workgroup_size: Self::TASK_WORKGROUP_SIZE,
                mesh: ShaderEntry {
                    module: cluster.mesh_module,
                    entry_point: cluster.mesh,
                },
                mesh_workgroup_size: Self::MESH_WORKGROUP_SIZE,
                fragment,
                primitive,
                depth_stencil: Self::depth_stencil(),
                multisample: MultisampleState::default(),
                color_targets: &Self::RSM_TARGETS,
            }),
            None => device.create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("rsm"),
                layout,
                vertex: ShaderEntry {
                    module: self.mesh,
                    entry_point: self.vertex,
                },
                fragment,
                primitive,
                depth_stencil: Self::depth_stencil(),
                multisample: MultisampleState::default(),
                color_targets: &Self::RSM_TARGETS,
            }),
        }
    }

    /// The pipeline that resets one tile of the shadow atlas, so a pass can keep
    /// some tiles and redraw others.
    ///
    /// `docs/plan/45-shadows.md`'s cadence rung needs a tile cleared on its own,
    /// and the attachment's [`LoadOp`] cannot do it — a clear there covers the
    /// whole image, and the region-bounded forms are not portable. This is the
    /// portable statement of the same thing: `mesh.slang`'s
    /// `depthClearVertexMain` covers the viewport the shadow pass has already
    /// set over the tile, and the depth state below writes
    /// [`depth::CLEAR`](crcbl_hal::depth) over whatever was there.
    ///
    /// **[`CompareOp::Always`] and writes on**, which is the pair that makes it a
    /// clear rather than a draw: the tile holds the previous frame's depths, and
    /// under the engine's [`CompareOp::Greater`] a clear value of zero would
    /// fail against every one of them and leave the tile exactly as it was.
    ///
    /// **[`CullMode::None`]**, unlike [`MeshModules::primitive`]: the triangle
    /// is generated from `SV_VertexID` rather than wound by an artist, so a
    /// culling rule meant for scene geometry would decide whether the clear
    /// happens at all.
    ///
    /// **A vertex pipeline on every path**, including the mesh-shader one: a
    /// device with a mesh stage still has a vertex stage, and the clear has no
    /// clusters to descend.
    fn depth_clear_pipeline(
        &self,
        device: &dyn Device,
        layout: PipelineLayoutHandle,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("shadow tile clear"),
            // The mesh layout, so binding this pipeline in the middle of the
            // shadow pass does not disturb the bind group the draws around it
            // are recorded with. The stage reads none of it.
            layout,
            vertex: ShaderEntry {
                module: self.mesh,
                entry_point: self.depth_clear_vertex,
            },
            fragment: None,
            primitive: PrimitiveState {
                cull_mode: CullMode::None,
                polygon_mode: PolygonMode::Fill,
                ..PrimitiveState::default()
            },
            depth_stencil: Some(DepthStencilState {
                depth_compare: CompareOp::Always,
                ..DepthStencilState::default()
            }),
            multisample: MultisampleState::default(),
            color_targets: &[],
        })
    }

    /// Releases every module. The pipelines built from them stay valid.
    fn destroy(self, device: &dyn Device) {
        device.destroy_shader_module(self.mesh);
        if let Some(cluster) = self.cluster {
            device.destroy_shader_module(cluster.mesh_module);
            if let Some(task) = cluster.task_module {
                device.destroy_shader_module(task);
            }
        }
    }
}

/// The entry point for `stage`, or an error naming the shader.
///
/// Unreachable in practice — `crcbl-shaders`' own tests assert both modules
/// expose both stages — but the alternative is an `expect` in engine code.
fn entry(shader: &crcbl_shaders::Shader, stage: Stage) -> Result<&'static str, HalError> {
    shader.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no unambiguous {stage:?} entry point; the committed SPIR-V and its \
             manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix",
            shader.name()
        ))
    })
}

/// The entry point called `name`, checked to be `stage`'s.
///
/// [`entry`] cannot serve `mesh_cluster.slang`: it has **two** mesh entry
/// points — one behind the amplification stage and one without it — and a
/// lookup by stage answers `None` for two matches rather than picking one, for
/// the good reason that picking one would draw the wrong geometry.
///
/// The stage is still checked rather than taken on trust: a name that resolved
/// to the wrong stage would be a pipeline built with an amplification stage in
/// its mesh slot, and the failure would arrive as a driver error rather than as
/// this sentence.
fn named_entry(
    shader: &crcbl_shaders::Shader,
    name: &'static str,
    stage: Stage,
) -> Result<&'static str, HalError> {
    let found = shader
        .entry_points()
        .iter()
        .find(|entry| entry.name() == name && entry.stage() == stage);
    found.map(crcbl_shaders::EntryPoint::name).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no {stage:?} entry point called `{name}`; the committed SPIR-V and \
             its manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix",
            shader.name()
        ))
    })
}

/// `entries` with [`PAGE_SAMPLER_BINDING`] rewritten to `sampler`, as a new
/// group of `layout` — [`ForwardRenderer::adopt_page_sampler`]'s one step.
///
/// The rewrite is in place, so a list whose group the device refused is
/// already right for the retry.
fn rebuilt_with_sampler(
    device: &dyn Device,
    layout: BindGroupLayoutHandle,
    sampler: SamplerHandle,
    entries: &mut [BindGroupEntry],
    label: &str,
) -> Result<BindGroupHandle, HalError> {
    let slot = entries
        .iter_mut()
        .find(|entry| entry.binding == PAGE_SAMPLER_BINDING)
        .unwrap_or_else(|| unreachable!("{label}: every mesh-layout group names the page sampler"));
    slot.resource = BindingResource::Sampler(sampler);
    device.create_bind_group(&BindGroupDesc {
        label: Some(label),
        layout,
        entries,
        variable_count: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::EffectOverride;
    use crate::scene::{
        DEMO_CUBE, DEMO_DUNES, DEMO_OPEN_BOX, DEMO_PYRAMID, DEMO_TEXTURED, DEMO_TINTED,
        DEMO_UNTINTED,
    };
    use crcbl_hal::null::{Event, NullInstance, ObjectKind, Recorder};
    use crcbl_hal::{DeviceDesc, Features, Instance, QueueKind};

    fn open() -> (Recorder, Box<dyn Device>, QueueHandle) {
        open_with(DeviceDesc::for_adapter(crcbl_hal::AdapterId(0)).optional_features)
    }

    /// [`open`] asking for `optional_features` instead of
    /// [`DeviceDesc::for_adapter`]'s.
    ///
    /// The null preset *has* more than the default request asks for, and a
    /// granted feature set is the intersection of the two — so this is how a test
    /// reaches a device with something the ordinary one lacks. What it exists for
    /// is [`Features::POLYGON_MODE_LINE`]: with the default request, `open`'s
    /// device has no line fill mode, which is the other half of the wireframe
    /// pair of tests below.
    fn open_with(optional_features: Features) -> (Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                optional_features,
                ..DeviceDesc::for_adapter(adapter.id)
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (recorder, device, queue)
    }

    /// Puts one of [`scene::demo`]'s meshes in the frame the way a caller does.
    ///
    /// **Insertion order is what the caller decides**, and it is load-bearing:
    /// the slot an object lands in is `docs/plan/25-lod.md`'s hysteresis key. So
    /// every test below places its objects in the order the frame used to hold
    /// them — the cube first, wherever there is one.
    fn place_demo(
        renderer: &mut ForwardRenderer,
        mesh: usize,
        material: usize,
        transform: Mat4,
    ) -> InstanceHandle {
        renderer
            .add_instance(&InstanceDesc {
                mesh,
                material,
                transform,
            })
            .expect("a pool of thousands has room for a handful of objects")
    }

    /// [`scene::demo`]'s cube, placed **first** so it takes the pool slot it has
    /// always had.
    ///
    /// `begin_frame` used to write this object for every caller; the tests below
    /// that are about a frame with something in it place it themselves now.
    fn place_cube(renderer: &mut ForwardRenderer, transform: Mat4) -> InstanceHandle {
        place_demo(renderer, DEMO_CUBE, DEMO_UNTINTED, transform)
    }

    /// The whole renderer builds and tears down against a backend with no
    /// driver, which is what makes it testable on every CI leg.
    #[test]
    fn the_forward_renderer_builds_and_leaks_nothing() {
        let (recorder, device, queue) = open();
        let before = recorder.total_live_objects();
        let renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
            .expect("the null backend accepts every descriptor");
        assert_eq!(renderer.target_format(), Format::Rgba8UnormSrgb);
        assert!(recorder.total_live_objects() > before);
        renderer.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "every object created must be destroyed"
        );
        recorder.assert_valid();
    }

    /// The uniform ring really rotates, or a spinning cube is a
    /// read-after-write hazard across submissions.
    #[test]
    fn the_uniform_ring_rotates() {
        let (_, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();

        let mut seen = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(device.as_ref(), &camera, &light, (64, 48))
                .expect("write");
            seen.push(renderer.uniforms[renderer.frame]);
        }
        assert_ne!(
            seen[0], seen[1],
            "consecutive frames must not share a buffer"
        );
        assert_eq!(seen[0], seen[FRAMES_IN_FLIGHT], "and the ring must wrap");
        renderer.destroy(device.as_ref());
    }

    /// **The tonemap's block is a ring too, and it carries the exposure the
    /// caller set.**
    ///
    /// The bytes are the observable rather than the field, because the field is
    /// what a renderer that never wired the block up would also have: the claim
    /// is that the number reaches the buffer `tonemap.slang` reads, and only the
    /// null recorder's write log can say so.
    ///
    /// The default is checked first, and it is the check that says every golden
    /// image is unmoved — a renderer nobody has called
    /// [`ForwardRenderer::set_exposure`] on writes the value the shader's
    /// `EXPOSURE` constant held.
    #[test]
    fn the_tonemap_block_holds_the_exposure_and_rotates_with_the_frame() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();

        renderer
            .begin_frame(device.as_ref(), &camera, &light, (64, 48))
            .expect("write");
        assert_eq!(
            recorder
                .buffer_bytes(renderer.tonemap_uniforms[renderer.frame])
                .expect("begin_frame wrote the block"),
            crcbl_shaders::tonemap::TonemapParams {
                exposure: crcbl_shaders::tonemap::DEFAULT_EXPOSURE,
                curve: crcbl_shaders::tonemap::TonemapCurve::Clamp,
                auto_exposure: false,
            }
            .to_bytes(),
            "an untouched renderer must write the exposure the constant used to hold, \
             under the operator that constant is the identity for",
        );

        // A value that is neither the default nor a bound, so a block left at
        // either would fail.
        renderer.set_exposure(3.5);
        let mut slots = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(device.as_ref(), &camera, &light, (64, 48))
                .expect("write");
            slots.push(renderer.tonemap_uniforms[renderer.frame]);
            assert_eq!(
                recorder
                    .buffer_bytes(renderer.tonemap_uniforms[renderer.frame])
                    .expect("begin_frame wrote the block"),
                crcbl_shaders::tonemap::TonemapParams {
                    exposure: 3.5,
                    curve: crcbl_shaders::tonemap::TonemapCurve::Clamp,
                    auto_exposure: false,
                }
                .to_bytes(),
                "the frame's own block must carry the exposure in force",
            );
        }
        assert_ne!(
            slots[0], slots[1],
            "consecutive frames must not share a block"
        );
        assert_eq!(slots[0], slots[FRAMES_IN_FLIGHT], "and the ring must wrap");

        renderer.destroy(device.as_ref());
    }

    /// **The curve reaches the block, and the default one does not move it.**
    ///
    /// The selector is a lane of the same ring the exposure rides in, and a lane
    /// nothing wrote would read as
    /// [`TonemapCurve::Clamp`](crcbl_shaders::tonemap::TonemapCurve::Clamp)
    /// whatever a caller asked for — a setter that compiles, a frame that looks
    /// right, and a curve that never runs. So the check is that setting it
    /// changes the bytes, on every frame of the ring.
    #[test]
    fn the_tonemap_block_carries_the_curve_a_caller_selected() {
        use crcbl_shaders::tonemap::TonemapCurve;

        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();

        assert_eq!(
            renderer.tonemap_curve(),
            TonemapCurve::Clamp,
            "a renderer nobody configured must run the operator the goldens were blessed under",
        );

        renderer.set_tonemap_curve(TonemapCurve::Aces);
        assert_eq!(renderer.tonemap_curve(), TonemapCurve::Aces);
        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(device.as_ref(), &camera, &light, (64, 48))
                .expect("write");
            let written = recorder
                .buffer_bytes(renderer.tonemap_uniforms[renderer.frame])
                .expect("begin_frame wrote the block");
            assert_eq!(
                written,
                crcbl_shaders::tonemap::TonemapParams {
                    exposure: crcbl_shaders::tonemap::DEFAULT_EXPOSURE,
                    curve: TonemapCurve::Aces,
                    auto_exposure: false,
                }
                .to_bytes(),
                "every frame of the ring must carry the selected curve",
            );
            assert_ne!(
                written,
                crcbl_shaders::tonemap::TonemapParams::default().to_bytes(),
                "and it must differ from the block a default renderer writes",
            );
        }

        renderer.destroy(device.as_ref());
    }

    /// **The exposure is clamped where it is set**, so no caller can reach a
    /// frame it cannot get back from.
    ///
    /// Each case is a different way of leaving the range, and the NaN one is the
    /// reason [`ForwardRenderer::set_exposure`] does not use `f32::clamp`: that
    /// propagates a NaN into the block, which is a frame of NaN pixels with no
    /// key to press to escape it.
    #[test]
    fn the_exposure_setter_clamps_to_a_range_a_caller_can_come_back_from() {
        let (_, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        assert!(
            (renderer.exposure() - crcbl_shaders::tonemap::DEFAULT_EXPOSURE).abs() < f32::EPSILON,
            "the default is {}",
            renderer.exposure(),
        );

        for (asked, expected) in [
            (2.0, 2.0),
            (EXPOSURE_MAX * 4.0, EXPOSURE_MAX),
            (f32::INFINITY, EXPOSURE_MAX),
            (EXPOSURE_MIN / 4.0, EXPOSURE_MIN),
            (0.0, EXPOSURE_MIN),
            (-8.0, EXPOSURE_MIN),
            (f32::NEG_INFINITY, EXPOSURE_MIN),
            (f32::NAN, EXPOSURE_MIN),
        ] {
            renderer.set_exposure(asked);
            assert!(
                (renderer.exposure() - expected).abs() < f32::EPSILON,
                "{asked} was clamped to {}, not {expected}",
                renderer.exposure(),
            );
        }

        renderer.destroy(device.as_ref());
    }

    /// **The debug views resolve in one order, however they are set.**
    ///
    /// Every one of them rides in one float lane and the shaders test the
    /// outermost threshold first, so a renderer with two switches on draws
    /// exactly one picture — and which one has to be a rule rather than
    /// whichever branch a caller's setter happened to run last. Every
    /// combination of the switches is walked, because a precedence written as an
    /// `if` chain is right for most of them by accident: a chain in the wrong
    /// order agrees with this one on every combination where at most one switch
    /// is set.
    ///
    /// **The two innermost views are the exception the walk is worth most for.**
    /// [`DebugView::Cascades`] keeps the shaded picture rather than replacing
    /// it, so it resolves *last* and its sentinel runs the other way down the
    /// lane; [`DebugView::ShadowAtlas`] resolves just above it and carries the
    /// *off* sentinel, because it is a pass rather than a branch. Every
    /// combination where either is set beside a replacing view is a combination
    /// an order written any other way answers differently, and there is no
    /// single-switch case that could tell them apart.
    ///
    /// [`ForwardRenderer::debug_view`] is asserted beside the lane, so the value
    /// a panel reads back and the value the shader branches on cannot drift —
    /// they are the same function.
    #[test]
    fn the_debug_views_resolve_in_one_order_however_they_are_set() {
        // `w` of the second `float4` of the block — the lane
        // `the_normals_view_moves_the_ambients_last_lane_and_no_other_byte`
        // spells out, and for its reason.
        const AMBIENT_W: usize = 64 + 16 + 12;

        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();

        for bent in [false, true] {
            for motion in [false, true] {
                for occlusion in [false, true] {
                    for heatmap in [false, true] {
                        for lod in [false, true] {
                            for normals in [false, true] {
                                for (atlas, cascades) in
                                    [false, true].into_iter().flat_map(|atlas| {
                                        [false, true].map(|cascades| (atlas, cascades))
                                    })
                                {
                                    renderer.set_bent_normal_view(bent);
                                    renderer.set_motion_view(motion);
                                    renderer.set_occlusion_view(occlusion);
                                    renderer.set_heatmap(heatmap);
                                    renderer.set_lod_view(lod);
                                    renderer.set_normals_view(normals);
                                    renderer.set_atlas_view(atlas);
                                    renderer.set_cascade_view(cascades);
                                    let set = format!(
                                        "bent={bent} motion={motion} occlusion={occlusion} \
                                 heatmap={heatmap} lod={lod} normals={normals} \
                                 atlas={atlas} cascades={cascades}"
                                    );
                                    // Each switch reads back what it was set to, whatever
                                    // the others are: a caller's toggle is about the
                                    // caller's setting, and only `debug_view` is about the
                                    // picture.
                                    assert_eq!(renderer.bent_normal_view(), bent);
                                    assert_eq!(renderer.motion_view(), motion);
                                    assert_eq!(renderer.occlusion_view(), occlusion);
                                    assert_eq!(renderer.heatmap(), heatmap);
                                    assert_eq!(renderer.lod_view(), lod);
                                    assert_eq!(renderer.normals_view(), normals);
                                    assert_eq!(renderer.atlas_view(), atlas);
                                    assert_eq!(renderer.cascade_view(), cascades);

                                    let expected = if bent {
                                        DebugView::BentNormal
                                    } else if motion {
                                        DebugView::Motion
                                    } else if occlusion {
                                        DebugView::AmbientOcclusion
                                    } else if heatmap {
                                        DebugView::Heatmap
                                    } else if lod {
                                        DebugView::LodTint
                                    } else if normals {
                                        DebugView::Normals
                                    } else if atlas {
                                        // **Below every replacing view above and
                                        // above the tint below**, which is the
                                        // half of this order no single-switch
                                        // case reaches: with a replacing view
                                        // also set the frame is that view's, and
                                        // with the tint also set the frame is the
                                        // atlas.
                                        DebugView::ShadowAtlas
                                    } else if cascades {
                                        // **Last, which is the half of this order a
                                        // chain in any other arrangement would get
                                        // right by accident.** The cascade tint is a
                                        // multiply on the shaded picture, so every
                                        // combination that also names a replacing
                                        // view has to resolve to *that* view — and
                                        // those are exactly the combinations an
                                        // `if cascades` written one line higher
                                        // would answer differently.
                                        DebugView::Cascades
                                    } else {
                                        DebugView::Shaded
                                    };
                                    assert_eq!(renderer.debug_view(), expected, "{set}");

                                    renderer
                                        .begin_frame(device.as_ref(), &camera, &light, (64, 48))
                                        .expect("write");
                                    let block = recorder
                                        .buffer_bytes(renderer.uniforms[renderer.frame])
                                        .expect("begin_frame wrote the block");
                                    let sentinel = match expected {
                                        DebugView::BentNormal => {
                                            mesh::FrameUniforms::BENT_NORMAL_VIEW_ON
                                        }
                                        DebugView::Motion => mesh::FrameUniforms::MOTION_VIEW_ON,
                                        DebugView::AmbientOcclusion => {
                                            mesh::FrameUniforms::OCCLUSION_VIEW_ON
                                        }
                                        DebugView::Heatmap => mesh::FrameUniforms::HEATMAP_VIEW_ON,
                                        DebugView::LodTint => mesh::FrameUniforms::LOD_VIEW_ON,
                                        DebugView::Normals => mesh::FrameUniforms::NORMALS_VIEW_ON,
                                        DebugView::Cascades => mesh::FrameUniforms::CASCADE_VIEW_ON,
                                        // **The atlas view carries the *off*
                                        // sentinel**, which is what makes this
                                        // assertion say something rather than
                                        // restate `debug_view_lane`: the colour
                                        // pass draws the shaded picture and
                                        // `crate::atlas_view`'s pass replaces
                                        // it, so a lane of its own here would be
                                        // a threshold no shader tests.
                                        DebugView::ShadowAtlas | DebugView::Shaded => {
                                            mesh::FrameUniforms::NORMALS_VIEW_OFF
                                        }
                                    };
                                    assert_eq!(
                                        &block[AMBIENT_W..AMBIENT_W + 4],
                                        &sentinel.to_le_bytes(),
                                        "{set} resolved to {expected:?}, and the lane says otherwise"
                                    );
                                    // And the resolve comes off for every one of them:
                                    // these frames are read back as data, so their colours
                                    // have to stay the ones the shader wrote — see
                                    // `resolved_effects`.
                                    assert_eq!(
                                        renderer.effects().contains(RenderEffects::ANTIALIASING),
                                        expected == DebugView::Shaded,
                                        "{set}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        renderer.destroy(device.as_ref());
    }

    /// **The atlas viewer records one pass, and only on the frame it is the view
    /// showing.**
    ///
    /// The mechanism no other check in this file can see. Every other debug view
    /// is a lane of the frame block, so
    /// [`the_debug_views_resolve_in_one_order_however_they_are_set`] observes it
    /// by reading four bytes; this one is a *pass*, and "the pass was recorded"
    /// is a claim about the command stream and about nothing else. A switch that
    /// resolved correctly and recorded nothing would satisfy every other
    /// assertion in this module and draw the frame unchanged.
    ///
    /// Three arms, and the third is the one that makes the first two mean
    /// something:
    ///
    /// * off — no pass at all, which is what makes the off position
    ///   bit-identical for every golden in this workspace;
    /// * on — the pass, with the full-screen triangle in it;
    /// * on **beside a replacing view** — no pass, because
    ///   [`ForwardRenderer::debug_view`] resolved to that view and the atlas is
    ///   not what this frame draws. That is the half a `self.atlas_view` read at
    ///   the recording site would get wrong while passing both arms above.
    ///
    /// [`the_debug_views_resolve_in_one_order_however_they_are_set`]: fn@the_debug_views_resolve_in_one_order_however_they_are_set
    #[test]
    fn the_atlas_viewer_records_its_pass_only_when_it_is_the_view_showing() {
        use crcbl_hal::null::Command;

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let (camera, sun) = (Camera::default(), DirectionalLight::default());

        // How many times the viewer's pass was opened since `from`, which is
        // where the previous arm's commands ended.
        let opened = |from: usize| {
            recorder.commands()[from..]
                .iter()
                .filter(|command| {
                    matches!(
                        command.opens_pass(),
                        Some((_, Some(recorded))) if recorded == crate::atlas_view::LABEL
                    )
                })
                .count()
        };

        let shaded = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
        assert_eq!(
            opened(0),
            0,
            "a frame drawing the shaded picture recorded the atlas viewer's pass"
        );
        let after_shaded = recorder.commands().len();
        shaded.release(device);

        renderer.set_atlas_view(true);
        assert_eq!(renderer.debug_view(), DebugView::ShadowAtlas);
        let viewing = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
        assert_eq!(
            opened(after_shaded),
            1,
            "the atlas viewer is on and its pass was not recorded exactly once"
        );
        // And what it recorded is the over-sized triangle and nothing else: a
        // pass that bound a pipeline and drew no vertices would open above and
        // leave the frame the tonemap wrote.
        let drawn: Vec<(u32, u32)> = commands_in_pass(&recorder, crate::atlas_view::LABEL)
            .into_iter()
            .filter_map(|command| match command {
                Command::Draw {
                    vertices,
                    instances,
                } => Some((
                    vertices.end - vertices.start,
                    instances.end - instances.start,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(drawn, vec![(crate::atlas_view::FULLSCREEN_VERTICES, 1)]);
        let after_viewing = recorder.commands().len();
        viewing.release(device);

        // The switch left on, and a replacing view asked for over the top of it.
        renderer.set_normals_view(true);
        assert_eq!(
            renderer.debug_view(),
            DebugView::Normals,
            "the atlas view does not lose to a replacing view, so the arm below asks nothing"
        );
        let normals = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
        assert_eq!(
            opened(after_viewing),
            0,
            "the frame resolved to the normals view and still recorded the atlas viewer's \
             pass over the top of it"
        );
        normals.release(device);

        renderer.destroy(device);
    }

    /// **The heatmap moves the ambient's last lane and no other byte**, and
    /// switching it off puts the block back.
    ///
    /// The normals view's assertion below, for the outermost of the three: the
    /// overlay costs one lane, so every golden image ever blessed is untouched
    /// by the feature existing. The `lod_params` row the heatmap reads is
    /// written every frame whether it is on or not — it is the numbers the
    /// selection used — so this compares two frames of the *same* camera and
    /// budget, where that row is equal on both sides and any byte outside the
    /// lane is a real difference.
    #[test]
    fn the_heatmap_moves_the_ambients_last_lane_and_no_other_byte() {
        const AMBIENT_W: usize = 64 + 16 + 12;

        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();
        let block = |renderer: &mut ForwardRenderer| -> Vec<u8> {
            renderer
                .begin_frame(device.as_ref(), &camera, &light, (64, 48))
                .expect("write");
            recorder
                .buffer_bytes(renderer.uniforms[renderer.frame])
                .expect("begin_frame wrote the block")
        };

        assert!(
            !renderer.heatmap(),
            "the heatmap has to be off in a renderer nobody has asked",
        );
        let shaded = block(&mut renderer);
        assert_eq!(
            &shaded[AMBIENT_W..AMBIENT_W + 4],
            &mesh::FrameUniforms::NORMALS_VIEW_OFF.to_le_bytes(),
            "an untouched renderer must write the value every golden image was blessed with",
        );

        renderer.set_heatmap(true);
        let heatmap = block(&mut renderer);
        assert_eq!(
            &heatmap[AMBIENT_W..AMBIENT_W + 4],
            &mesh::FrameUniforms::HEATMAP_VIEW_ON.to_le_bytes(),
            "the switch has to reach the lane the mesh stage branches on",
        );
        let outside: Vec<usize> = shaded
            .iter()
            .zip(heatmap.iter())
            .enumerate()
            .filter(|(at, (was, now))| was != now && !(AMBIENT_W..AMBIENT_W + 4).contains(at))
            .map(|(at, _)| at)
            .collect();
        assert!(
            outside.is_empty(),
            "the frame block changed at {outside:?}, which is outside the heatmap's own lane",
        );

        renderer.set_heatmap(false);
        assert_eq!(
            block(&mut renderer),
            shaded,
            "switching the view off has to put the block back, not just stop reporting it",
        );

        renderer.destroy(device.as_ref());
    }

    /// **The normals view moves one lane of the frame block and nothing else.**
    ///
    /// The mechanism, at the level a null device can observe it: which bytes of
    /// the buffer `mesh.slang` reads changed. A field on the renderer would say
    /// only that a flag moved; the block is what the shader actually branches on.
    ///
    /// Four claims, and each rules out a different way of passing wrongly:
    ///
    /// * an untouched renderer writes [`FrameUniforms::NORMALS_VIEW_OFF`] — which
    ///   is what makes every golden image untouched by this feature;
    /// * switching on writes [`FrameUniforms::NORMALS_VIEW_ON`] into the lane the
    ///   shader reads, at the offset `std140` puts it at rather than at one this
    ///   test picked;
    /// * **those four bytes are the only ones that moved**, so the switch did not
    ///   come at the cost of the ambient colour beside it or of any matrix;
    /// * and switching off puts the block back exactly, so the view is a toggle
    ///   rather than a one-way door.
    ///
    /// [`FrameUniforms::NORMALS_VIEW_OFF`]: crcbl_shaders::mesh::FrameUniforms::NORMALS_VIEW_OFF
    /// [`FrameUniforms::NORMALS_VIEW_ON`]: crcbl_shaders::mesh::FrameUniforms::NORMALS_VIEW_ON
    #[test]
    fn the_normals_view_moves_the_ambients_last_lane_and_no_other_byte() {
        // `w` of the second `float4` of the block: one `float4x4`, one `float4`,
        // then three floats of ambient colour. Spelled as the sum rather than as
        // `92` so that a member arriving before it is a compile-time question
        // about `FrameUniforms`' own layout and not a silent read of a matrix.
        const AMBIENT_W: usize = 64 + 16 + 12;

        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();
        let block = |renderer: &mut ForwardRenderer| -> Vec<u8> {
            renderer
                .begin_frame(device.as_ref(), &camera, &light, (64, 48))
                .expect("write");
            recorder
                .buffer_bytes(renderer.uniforms[renderer.frame])
                .expect("begin_frame wrote the block")
        };

        assert!(
            !renderer.normals_view(),
            "the normals view has to be off in a renderer nobody has asked",
        );
        let shaded = block(&mut renderer);
        assert_eq!(
            &shaded[AMBIENT_W..AMBIENT_W + 4],
            &mesh::FrameUniforms::NORMALS_VIEW_OFF.to_le_bytes(),
            "an untouched renderer must write the value every golden image was blessed with",
        );

        renderer.set_normals_view(true);
        assert!(renderer.normals_view());
        let normals = block(&mut renderer);
        assert_eq!(
            &normals[AMBIENT_W..AMBIENT_W + 4],
            &mesh::FrameUniforms::NORMALS_VIEW_ON.to_le_bytes(),
            "the switch has to reach the lane the fragment stage branches on",
        );
        // The bytes that moved, and they all have to be inside the lane — not
        // *all* of the lane, because `0.0f` and `1.0f` share their two low bytes
        // and an equality against the whole range would be a test about IEEE 754
        // rather than about the block.
        let outside: Vec<usize> = shaded
            .iter()
            .zip(normals.iter())
            .enumerate()
            .filter(|(at, (was, now))| was != now && !(AMBIENT_W..AMBIENT_W + 4).contains(at))
            .map(|(at, _)| at)
            .collect();
        assert!(
            outside.is_empty(),
            "the frame block changed at {outside:?}, which is outside the normals view's own lane",
        );

        renderer.set_normals_view(false);
        assert!(!renderer.normals_view());
        assert_eq!(
            block(&mut renderer),
            shaded,
            "switching the view off has to put the block back, not just stop reporting it",
        );

        renderer.destroy(device.as_ref());
    }

    /// **A frame uploads the instances a caller moved, and a frame nobody moved
    /// anything in uploads nothing at all.**
    ///
    /// The second half is the stronger claim and it is the one that needed the
    /// cube to become an ordinary instance: `begin_frame` used to take the
    /// cube's transform as an argument and write it into the pool every frame,
    /// so that slot was dirty in every ring slot forever and a motionless scene
    /// paid a spinning one's transfer. The bytes were identical, so no golden
    /// image could ever have shown it — the recorder's write log is the only
    /// place the difference is visible.
    ///
    /// The pyramid is here so that "only the one that moved" is a claim about a
    /// choice: with one instance in the array, uploading it and uploading
    /// everything are the same event.
    #[test]
    fn only_the_instance_that_moved_is_uploaded() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let instance_buffers = renderer.instances.buffers().to_vec();
        let cube = place_cube(&mut renderer, Mat4::IDENTITY);
        place_demo(
            &mut renderer,
            DEMO_PYRAMID,
            DEMO_UNTINTED,
            Mat4::from_translation(Vec3::X),
        );

        // Both instances are dirty in *every* slot when they are inserted, so
        // the first frames upload both however well delta upload works. The ring
        // has to be walked right through before an upload is evidence about the
        // delta rather than about initialisation.
        let frame = |renderer: &mut ForwardRenderer| {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 48),
                )
                .expect("write");
        };
        for _ in 0..FRAMES_IN_FLIGHT {
            frame(&mut renderer);
        }
        let instance_writes = |from: usize| -> Vec<(u64, usize)> {
            recorder
                .events()
                .into_iter()
                .skip(from)
                .filter_map(|event| match event {
                    crcbl_hal::null::Event::BufferWritten {
                        buffer,
                        offset,
                        len,
                    } if instance_buffers.contains(&buffer) => Some((offset, len)),
                    _ => None,
                })
                .collect()
        };

        let before = recorder.events().len();
        frame(&mut renderer);
        assert_eq!(
            instance_writes(before),
            [],
            "a frame in which nothing moved must upload no instance bytes at all"
        );

        let before = recorder.events().len();
        let model = ForwardRenderer::spin(1.25);
        renderer.set_instance(
            cube,
            &InstanceDesc {
                mesh: DEMO_CUBE,
                material: DEMO_UNTINTED,
                transform: model,
            },
        );
        frame(&mut renderer);
        assert_eq!(
            renderer
                .instances
                .get(cube)
                .expect("the cube is live")
                .transform,
            model.to_cols_array(),
            "the model matrix must land in the instance, not the uniform block"
        );
        let cube_at = u64::from(renderer.instances.index(cube).expect("the cube is live"))
            * crcbl_shaders::mesh::INSTANCE_STRIDE as u64;
        assert_eq!(
            instance_writes(before),
            [(cube_at, crcbl_shaders::mesh::INSTANCE_STRIDE)],
            "a steady-state frame must upload exactly the one instance that changed"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **An object takes one pool slot, a rewrite keeps it, and a removal gives
    /// it back for the next object to reuse.**
    ///
    /// The three-call bookkeeping every caller of the instance API depends on,
    /// checked on the pool rather than on the recorder. A `set_instance` that
    /// inserted instead would take a fresh slot per call — the pool exhausted in
    /// a few thousand frames of a caller that writes a transform per frame, which
    /// is what every sample does — and every slot it abandoned would stay
    /// **live**, so the frame would draw a copy of the object at each transform
    /// it had ever been given. Neither is something a golden image would show
    /// until the pool ran out.
    ///
    /// `only_the_instance_that_moved_is_uploaded` is the delta half of this and
    /// reads the recorder; this is the bookkeeping half and reads the pool, which
    /// is where an abandoned slot would still be counted.
    ///
    /// Run over every mesh of [`scene::demo`], because the DAG is the one whose
    /// slot carries `docs/plan/25-lod.md`'s hysteresis state — the reuse below is
    /// exactly what [`ForwardRenderer::add_instance`] documents as inheriting a
    /// previous occupant's expanded groups.
    #[test]
    fn an_object_takes_one_slot_and_a_removal_gives_it_back() {
        for mesh in [DEMO_CUBE, DEMO_PYRAMID, DEMO_OPEN_BOX, DEMO_DUNES] {
            let (recorder, device, queue) = open();
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("built");
            // The null backend takes an indirect tail, so the DAG is placeable
            // here; the arm that cannot select a level refuses before an
            // application ever calls this.
            assert!(
                renderer.selects_levels(),
                "an indirect tail chooses a level for every mesh, DAG included"
            );
            // What the pool holds before anything is placed, which is nothing: a
            // renderer places no object of its own any more.
            let resident = renderer.instances.len();
            assert_eq!(resident, 0, "a fresh renderer holds no objects");

            let first = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
            let handle = place_demo(&mut renderer, mesh, DEMO_UNTINTED, first);
            assert_eq!(
                renderer.instances.len(),
                resident + 1,
                "mesh {mesh}: add_instance takes one slot"
            );

            // The rewrite is the whole test: the same handle, the same slot
            // count, and the new transform in it.
            let second = Mat4::from_translation(Vec3::new(-4.0, 5.0, -6.0));
            renderer.set_instance(
                handle,
                &InstanceDesc {
                    mesh,
                    material: DEMO_UNTINTED,
                    transform: second,
                },
            );
            assert_eq!(
                renderer.instances.len(),
                resident + 1,
                "mesh {mesh}: set_instance must rewrite its instance, not insert a second"
            );
            assert_eq!(
                renderer.instances.slot_count(),
                u32::try_from(resident + 1).expect("a handful of instances"),
                "mesh {mesh}: set_instance must not have reached past the slot it holds"
            );
            assert_eq!(
                renderer
                    .instances
                    .get(handle)
                    .unwrap_or_else(|| panic!("mesh {mesh}: the instance is live"))
                    .transform,
                second.to_cols_array(),
                "mesh {mesh}: set_instance must have written the second transform"
            );

            // And a removal gives the slot back, so a caller toggling an object
            // is not a leak either.
            renderer.remove_instance(handle);
            assert_eq!(
                renderer.instances.len(),
                resident,
                "mesh {mesh}: remove_instance must free the slot rather than hide the object"
            );
            place_demo(&mut renderer, mesh, DEMO_UNTINTED, first);
            assert_eq!(
                renderer.instances.slot_count(),
                u32::try_from(resident + 1).expect("a handful of instances"),
                "mesh {mesh}: the object after a removal must reuse the freed slot"
            );

            renderer.destroy(device.as_ref());
            recorder.assert_valid();
        }
    }

    /// **The description resolves to the ids the renderer used to hand itself**:
    /// one table entry per description mesh, in description order, with the DAG
    /// occupying one per level after them — and every level's vertex base
    /// measured from level 0's, which is zero.
    ///
    /// Upload order is what decides a mesh id, and `cull.slang` reads a bounding
    /// box out of the entry the *instance* names. So a description walked in
    /// another order, or a DAG whose levels landed among the flat meshes, would
    /// leave every instance naming a mesh that is not its own: the cube culled
    /// against the pyramid's box, the dunes patch drawn as an open box. Every
    /// one of those is a frame that still looks like a frame.
    ///
    /// The bases are asserted against the table bytes rather than against
    /// `ResidentMesh`, because the table is what the GPU reads.
    #[test]
    fn the_description_resolves_to_the_ids_it_was_written_in() {
        let (recorder, device, queue) = open();
        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");

        assert_eq!(
            renderer.mesh_ids,
            [0, 1, 2, 3],
            "the four description meshes take the first four table entries, in order"
        );

        // The DAG's coarser levels follow level 0, so a cut that spans them
        // reads entries the flat meshes never occupied.
        let scene = scene::demo();
        let Geometry::Dag { dag, .. } = &scene.meshes[DEMO_DUNES].geometry else {
            panic!("the demo description's fourth mesh is the DAG");
        };
        let level_ids: Vec<u32> = (0..dag.levels.len())
            .map(|level| renderer.mesh_ids[DEMO_DUNES] + level as u32)
            .collect();
        assert!(
            level_ids
                .iter()
                .all(|&id| id > renderer.mesh_ids[DEMO_OPEN_BOX]),
            "every dunes level is past the flat residents: {level_ids:?}"
        );

        // **Level 0 is at the pool's own first vertex**, which is what makes
        // every `ClusterSelect::vertex_base` a non-negative offset from it — the
        // sum the mesh stage forms with the instance's own base.
        let entries: Vec<crcbl_shaders::mesh::GpuMesh> = level_ids
            .iter()
            .map(|&id| mesh_entry(&recorder, &renderer, id))
            .collect();
        let level_zero = entries[0].base_vertex;
        assert_eq!(
            level_zero,
            mesh_entry(&recorder, &renderer, renderer.mesh_ids[DEMO_OPEN_BOX]).base_vertex
                + crcbl_shaders::mesh::OPEN_BOX_VERTEX_COUNT as u32,
            "the DAG starts where the last flat resident ends"
        );
        let bases: Vec<u32> = entries
            .iter()
            .map(|entry| {
                entry
                    .base_vertex
                    .checked_sub(level_zero)
                    .expect("a level below level 0 would wrap the offset a cluster carries")
            })
            .collect();
        assert_eq!(
            bases[0], 0,
            "level 0 is the base every other is measured from"
        );
        assert!(
            bases.windows(2).all(|pair| pair[0] < pair[1]),
            "each level starts past the one below it: {bases:?}"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A description that is not [`scene::demo`]: five meshes and two DAGs.**
    ///
    /// [`ForwardRenderer::with_scene`] is written for any mesh count and any
    /// number of DAGs, and until this nothing had put either through it — every
    /// golden image and every end-to-end run is `scene::demo()`, whose four
    /// meshes hold one DAG between them.
    ///
    /// What only a second DAG exercises is the **concatenation**: `level_groups`
    /// is every DAG's groups laid end to end and a DAG reaches its own run
    /// through the `first_group` offset handed to `ClusterDag::selection_records`,
    /// which is zero for the first one and therefore invisible while there is
    /// only one. A second DAG whose records still named the first's groups would
    /// descend a hierarchy belonging to another surface — a cut that is a
    /// plausible picture and matches no assertion the CPU makes about the frame.
    ///
    /// On the mesh path, because that is where the offset is *used*: an indirect
    /// tail takes a uniform cut and builds no [`ClusterSelect`] records at all.
    ///
    /// [`ClusterSelect`]: crcbl_shaders::cluster_select::ClusterSelect
    #[test]
    fn a_second_dag_reaches_its_own_groups_and_not_the_first_s() {
        use crcbl_shaders::cluster_select::{CLUSTER_SELECT_STRIDE, ClusterSelect};

        let recorder = Recorder::new();
        let (device, queue) = open_mesh_path(&recorder, Features::TASK_SHADER);

        // The engine has one DAG, so a second one is a second copy of it. That
        // is the whole point: two DAGs whose *groups* are alike is the case
        // where reaching the wrong run still produces a cut, and so the case a
        // frame comparison could never catch.
        let mut scene = scene::demo();
        let mut again = scene.meshes[DEMO_DUNES].clone();
        again.label = "dunes again".into();
        scene.meshes.push(again);
        // Room for the second copy. Doubling rather than measuring, because the
        // number that matters is that the description carries its own sizes at
        // all — see `Capacities`, which is the caller's to choose.
        scene.capacities.vertices *= 2;
        scene.capacities.indices *= 2;
        let second_dag = scene.meshes.len() - 1;

        let Geometry::Dag { dag, .. } = &scene.meshes[DEMO_DUNES].geometry else {
            panic!("the demo description's fourth mesh is the DAG");
        };
        let levels = dag.levels.len();
        let groups = dag.level_groups().len();
        assert!(groups > 0, "a DAG with no groups would select nothing");

        let mut renderer =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect("a five-mesh description with two DAGs is one this can make resident");

        // Every level of both is its own table entry, laid down in description
        // order, so the second DAG's level 0 is a whole hierarchy past the
        // first's.
        assert_eq!(
            renderer.mesh_ids.len(),
            scene.meshes.len(),
            "one id per description mesh, whatever the count"
        );
        assert_eq!(
            renderer.mesh_ids[second_dag],
            renderer.mesh_ids[DEMO_DUNES] + levels as u32,
            "the second DAG's level 0 starts past every level of the first"
        );

        // **The hysteresis state is per instance per resident group**, and
        // `group_stride` is what says how many groups that is. One DAG made this
        // number a single hierarchy's; two is what tells a stride that sums from
        // one that took the first answer it found.
        assert_eq!(
            renderer.draws.group_stride(),
            u32::try_from(groups * 2).expect("a few dozen groups per DAG"),
            "every resident DAG's groups are in the stride, not just the first's"
        );

        // The records themselves: which group each cluster produces and which
        // contains it, as the amplification stage reads them.
        let clusters = renderer
            .clusters
            .as_ref()
            .expect("the mesh path builds a cluster pool");
        let selection = recorder
            .buffer_bytes(clusters.selection())
            .expect("the selection records are live");
        // Entries are laid down in description order, a flat mesh taking one and
        // a DAG one per level — so the second DAG's start a hierarchy past the
        // first's, which start past the three flat residents.
        let first_entry = DEMO_DUNES;
        let record = |entry: usize| -> Vec<ClusterSelect> {
            let range = clusters.range(entry).expect("one range per entry");
            (range.base..range.base + range.count)
                .map(|cluster| {
                    let at = cluster as usize * CLUSTER_SELECT_STRIDE;
                    ClusterSelect::from_bytes(
                        selection[at..at + CLUSTER_SELECT_STRIDE]
                            .try_into()
                            .expect("one whole record"),
                    )
                })
                .collect()
        };
        // The named group of every record that has one, per DAG — and the count,
        // because a comparison over an empty set is a check that cannot fail.
        let named = |base: usize| -> Vec<u32> {
            (base..base + levels)
                .flat_map(record)
                .flat_map(|select| {
                    let producer = (select.flags & ClusterSelect::HAS_PRODUCER != 0)
                        .then_some(select.producer_group);
                    let container = (select.flags & ClusterSelect::HAS_CONTAINER != 0)
                        .then_some(select.container_group);
                    producer.into_iter().chain(container)
                })
                .collect()
        };
        let boundary = u32::try_from(groups).expect("a few dozen groups per DAG");
        let first = named(first_entry);
        let second = named(first_entry + levels);
        assert!(
            !first.is_empty() && first.len() == second.len(),
            "the two copies must name the same number of groups: {} against {}",
            first.len(),
            second.len()
        );
        assert!(
            first.iter().all(|&group| group < boundary),
            "the first DAG's records stay inside its own run: {first:?}"
        );
        assert!(
            second
                .iter()
                .all(|&group| (boundary..boundary * 2).contains(&group)),
            "the second DAG's records must name the groups past the first's, and \
             `first_group` is the only thing that puts them there: {second:?}"
        );

        // And both are placeable through the runtime API, which is what a
        // description of a shape no `set_*` method names is *for*.
        let place = |renderer: &mut ForwardRenderer, mesh: usize, x: f32| {
            renderer
                .add_instance(&InstanceDesc {
                    mesh,
                    material: DEMO_UNTINTED,
                    transform: Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
                })
                .expect("the pool has room")
        };
        let near = place(&mut renderer, DEMO_DUNES, -64.0);
        let far = place(&mut renderer, second_dag, 64.0);
        let index = |handle| {
            renderer
                .instances
                .index(handle)
                .expect("a just-added instance is live")
        };
        assert_ne!(
            index(near),
            index(far),
            "two instances take two slots, and the index is the hysteresis key"
        );
        assert_eq!(
            renderer.instances.get(far).expect("live").mesh,
            renderer.mesh_ids[second_dag],
            "an InstanceDesc's mesh index resolves through the description, not through \
             a table id the caller cannot know"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A description the renderer cannot make resident is refused before the
    /// first device object exists**, so a rejection leaks nothing.
    ///
    /// `build` hands the geometry pool and the material table to the rollback
    /// and then creates a dozen objects with `?` between them. A check that ran
    /// part way down that list would be a new early exit on the wrong side of a
    /// handover, and the pool would go with it — which is exactly the leak
    /// `Rollback` exists to prevent and which
    /// `the_forward_renderer_builds_and_leaks_nothing` cannot see, because it
    /// only walks the happy path.
    ///
    /// Each arm below is a *different* refusal, so a check that stopped running
    /// would show up as one arm leaking rather than as all of them — and each
    /// names a fragment of the message it must be refused *with*, or an arm
    /// would pass on some other check's answer.
    #[test]
    fn a_refused_description_creates_nothing_at_all() {
        /// One way of writing a description the renderer cannot build, and the
        /// words that say it was this refusal rather than another.
        type Break = (&'static str, fn(&mut SceneDesc<'static>), &'static str);

        let cases: [Break; 7] = [
            (
                "a row naming a layer the page has not got",
                |scene| {
                    scene.materials[DEMO_TEXTURED].base_color_texture = 7;
                },
                "material row 2 samples page layer 7",
            ),
            // The one page refusal a caller can actually spell: `push_layer`
            // takes bytes it cannot measure against the extent.
            (
                "a page layer of the wrong length",
                |scene| {
                    scene.page = scene::PageDesc::opaque_white(scene::PAGE_EXTENT);
                    scene.page.push_layer(vec![0x00; 4]);
                    scene.materials[DEMO_TEXTURED].base_color_texture = scene::CHECKER_LAYER;
                },
                "page layer 1 carries 4 bytes",
            ),
            (
                "a DAG level with no vertices",
                |scene| {
                    let Geometry::Dag { levels, .. } = &mut scene.meshes[DEMO_DUNES].geometry
                    else {
                        panic!("the demo description's fourth mesh is the DAG");
                    };
                    levels.pop();
                },
                "vertex array(s) for a DAG of",
            ),
            // The four capacities, each of which is a pool a caller sized and
            // a description outgrew. Written as a capacity the description is
            // too large for rather than as a description too large for the
            // default, because that is the shape of the mistake: the numbers on
            // `Capacities` are the ones an application picks.
            (
                "more vertices than the pool holds",
                |scene| {
                    scene.capacities.vertices = 1;
                },
                "the vertex pool holds 1 and the description needs",
            ),
            (
                "more indices than the pool holds",
                |scene| {
                    scene.capacities.indices = 1;
                },
                "the index pool holds 1 and the description needs",
            ),
            (
                "more meshes than the table holds",
                |scene| {
                    scene.capacities.meshes = 1;
                },
                "the mesh table holds 1 and the description needs",
            ),
            (
                "more material rows than the table holds",
                |scene| {
                    scene.capacities.materials = 2;
                },
                "the material table holds 2 and the description needs 3",
            ),
        ];

        for (what, break_it, says) in cases {
            let (recorder, device, queue) = open();
            let before = recorder.total_live_objects();
            let mut scene = scene::demo();
            break_it(&mut scene);
            let error =
                ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                    .expect_err(&format!("{what} must be refused"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what} must be refused as an invalid descriptor, not as {error:?}"
            );
            assert!(
                error.to_string().contains(says),
                "{what} must be refused with a message saying so, and this one says {error}"
            );
            assert_eq!(
                recorder.total_live_objects(),
                before,
                "{what} was refused after something had been created, and the rollback \
                 is the only thing that could have released it"
            );
            recorder.assert_valid();
        }

        // **And the refusal that is not free**, which is the half of the
        // handover every arm above walks past: `check_scene` settles what the
        // whole description says, and what one mesh's *bytes* say is the pool's,
        // which only sees them one upload at a time. Vertex bytes that are not a
        // whole number of vertices reach `MeshPool::upload` — so the third of
        // the four meshes is refused with the pool created, two device-local
        // buffers in it and the first two meshes already staged into them.
        // `build_geometry` is what releases that, and it can only do so because
        // the pool is not the rollback's until it has returned.
        let (recorder, device, queue) = open();
        let before = recorder.total_live_objects();
        // So the count below is this build's and not `open`'s.
        recorder.clear();
        let mut scene = scene::demo();
        let Geometry::Flat { vertices, .. } = &mut scene.meshes[DEMO_OPEN_BOX].geometry else {
            panic!("the demo description's third mesh is flat");
        };
        vertices.to_mut().push(0);
        let error =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect_err("a mesh of part of a vertex must be refused");
        assert!(
            error.to_string().contains("is not a whole number of"),
            "the pool's own refusal reaches the caller with its numbers, and this \
             one says {error}"
        );
        assert!(
            recorder
                .events()
                .iter()
                .any(|event| matches!(event, crcbl_hal::null::Event::Created { .. })),
            "this arm is evidence about the rollback only if the refusal came after \
             something had been created"
        );
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "a mesh refused part way through the list left the geometry pool behind, \
             which is exactly what `build_geometry` being self-cleaning is for"
        );
        recorder.assert_valid();
    }

    /// **The one capacity a description cannot be checked against, and the one
    /// an application is expected to handle while it runs.**
    ///
    /// Every other number on [`scene::Capacities`] sizes something the
    /// description fills once, so `check_scene` refuses an over-large one before
    /// a device object exists. Objects are placed at any point in a renderer's
    /// life, so the only honest answer for that one is the error
    /// [`ForwardRenderer::add_instance`] returns — and it returns
    /// [`InstancePoolError`] rather than a [`HalError`], so the capacity and
    /// what is in it survive to the caller that chose them.
    ///
    /// It also gives [`Capacities::instances`](scene::Capacities::instances) a
    /// value of its own rather than the default, which is what makes the pool
    /// fillable at all: a renderer that ignored the number the description
    /// carries and reserved the default would draw every frame in the tree
    /// identically and refuse nothing here.
    #[test]
    fn a_full_instance_pool_reaches_the_application_that_sized_it() {
        let (recorder, device, queue) = open();
        let mut scene = scene::demo();
        scene.capacities.instances = 2;
        let mut renderer =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect("a scene with room for two objects builds");
        let place = |renderer: &mut ForwardRenderer| {
            renderer.add_instance(&InstanceDesc {
                mesh: DEMO_CUBE,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            })
        };
        place(&mut renderer).expect("the first of two");
        place(&mut renderer).expect("the second of two");
        let error = place(&mut renderer).expect_err("a third object has no slot to go in");
        assert!(
            matches!(
                error,
                InstancePoolError::PoolFull {
                    capacity: 2,
                    in_use: 2
                }
            ),
            "a full pool must reach the caller as a full pool, with the capacity it \
             chose; this arrived as {error}"
        );
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **One mesh and one material row is a scene, and it draws.**
    ///
    /// The floor is gone. `with_scene` used to refuse anything shorter than
    /// [`scene::demo`] — four meshes and three rows — because the five `set_*`
    /// demo wrappers named those entries by position, and the renderer's own
    /// `dunes_clusters` and `dunes_level_buckets` indexed the description at
    /// `DEMO_DUNES` besides. So an application's own description was not merely
    /// refused: with the check removed it would have panicked out of `build`.
    ///
    /// A frame is recorded rather than only a build, because that is where the
    /// positional indexing was: the bucket table, the cluster ranges and the
    /// draw-argument blocks are all one-per-description-mesh, and a renderer that
    /// still assumed four would come apart here rather than at `with_scene`.
    #[test]
    fn a_description_smaller_than_the_demo_is_a_scene() {
        use std::borrow::Cow;

        let scene = SceneDesc {
            meshes: vec![scene::MeshDesc {
                label: Cow::Borrowed("the only mesh"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(crcbl_shaders::mesh::cube_vertex_bytes()),
                    uv_range: crcbl_shaders::mesh::demo_uv_range(),
                    indices: Cow::Owned(crcbl_shaders::mesh::cube_indices()),
                    clusters: crcbl_shaders::meshlet::cube_clusters(),
                    flags: 0,
                },
            }],
            materials: vec![mesh::GpuMaterial::UNTINTED],
            page: scene::PageDesc::opaque_white(1),
            probes: scene::ProbeGrid::default(),
            capacities: scene::Capacities::default(),
        };
        assert!(
            scene.meshes.len() < scene::demo().meshes.len()
                && scene.materials.len() < scene::demo().materials.len(),
            "this description must be smaller than the demo's in both, or it says nothing \
             about the floor"
        );

        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect("one mesh and one row is a description the renderer can hold");
        assert_eq!(
            renderer.mesh_ids,
            [0],
            "the one description mesh takes the one table entry"
        );
        assert_eq!(
            renderer.bucket_constants.len(),
            1,
            "one bucket per description mesh, and there is one"
        );
        renderer
            .add_instance(&InstanceDesc {
                mesh: 0,
                material: 0,
                transform: Mat4::IDENTITY,
            })
            .expect("an empty pool has room for the only object");
        let recorded = frame(device.as_ref(), &mut renderer, queue);
        assert_eq!(
            commands_in_pass(&recorder, "forward")
                .into_iter()
                .filter(|command| matches!(
                    command,
                    crcbl_hal::null::Command::DrawIndexedIndirectCount(_)
                ))
                .count(),
            1,
            "a one-mesh scene records the one bucket's indirect call"
        );
        recorded.finish(device.as_ref(), renderer);
    }

    /// **A description that exactly fills what it reserved is built, not
    /// refused.**
    ///
    /// The other side of the capacity check, and the side nothing else in the
    /// tree can fail on: every other scene reserves far more than it holds, so a
    /// comparison written `>=` instead of `>` would pass the whole suite and
    /// refuse only the application that had sized its pools exactly right.
    ///
    /// The two capacities asserted here are the two a caller can size without
    /// re-deriving anything the renderer knows: one mesh table entry per
    /// [`Geometry::levels`], which is what that method is documented to answer,
    /// and one row per material. The four rows share one comparison, so they
    /// stand or fall together.
    #[test]
    fn a_description_that_exactly_fits_its_capacities_is_built() {
        let (recorder, device, queue) = open();
        let mut scene = scene::demo();
        scene.capacities.meshes = scene
            .meshes
            .iter()
            .map(|desc| u32::try_from(desc.geometry.levels()).expect("a few levels"))
            .sum();
        scene.capacities.materials =
            u32::try_from(scene.materials.len()).expect("a few material rows");
        let renderer =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect("a description that fits exactly is not too large for itself");
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **The level tables cover every mesh-table slot an instance can name**,
    /// not only the slots the description filled.
    ///
    /// `draw_gen.slang`'s `mesh_levels_of` reads `gen.mesh_levels_at + mesh *
    /// MESH_LEVELS_WORDS` with no bound, and says in its own doc comment that
    /// "every entry an instance can name is filled". An instance's mesh id is a
    /// bare table index, so a table sized to the description alone makes that
    /// read land in whatever the packer put next for any slot the description
    /// left empty.
    ///
    /// Growing the capacity by a known number of slots grows the tables buffer
    /// by exactly what those slots occupy: one `MeshLevels` record and one
    /// `level_meshes` word each. The arithmetic is the assertion, so nothing
    /// here is a number to keep in step by hand.
    #[test]
    fn the_level_tables_cover_every_slot_the_mesh_table_has() {
        const EXTRA: u32 = 4;

        let sized = |extra: u32| {
            let (recorder, device, queue) = open();
            let mut scene = scene::demo();
            scene.capacities.meshes += extra;
            let renderer =
                ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                    .expect("a description with room to spare");
            let size = recorder
                .buffer_bytes(renderer.draws.tables())
                .expect("the table buffer is live")
                .len();
            renderer.destroy(device.as_ref());
            recorder.assert_valid();
            size
        };

        let grown = sized(EXTRA) - sized(0);
        let want = EXTRA as usize * (crcbl_shaders::level_select::MESH_LEVELS_STRIDE + 4);
        assert_eq!(
            grown, want,
            "{EXTRA} more table slots must add {EXTRA} more selection records and {EXTRA} more \
             level words, and added {grown} bytes"
        );
    }

    /// The property the whole arrangement exists for: the pool's second
    /// resident is **not** at base vertex zero, so a frame containing it is a
    /// frame that fails if the base is lost between the pool and the shader.
    ///
    /// Read out of the **mesh table** rather than out of a renderer field,
    /// because the table is where a generated draw's index range comes from:
    /// `draw_gen.slang` builds every argument structure out of these bytes and
    /// nothing on the CPU is consulted.
    ///
    /// Without this the pass is back where it started — one mesh, base 0, and a
    /// base-vertex bug that no picture can show.
    #[test]
    fn the_second_mesh_lands_past_the_first() {
        let (recorder, device, queue) = open();
        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let cube = mesh_entry(&recorder, &renderer, renderer.mesh_ids[DEMO_CUBE]);
        let pyramid = mesh_entry(&recorder, &renderer, renderer.mesh_ids[DEMO_PYRAMID]);

        assert_eq!(cube.base_vertex, 0, "the cube is first");
        assert_eq!(
            pyramid.base_vertex as usize,
            crcbl_shaders::mesh::CUBE_VERTEX_COUNT,
            "the pyramid must start where the cube ends, or the pools are not \
             suballocating at all"
        );
        assert_eq!(
            pyramid.index_count as usize,
            crcbl_shaders::mesh::PYRAMID_INDEX_COUNT,
            "and the entry carries the whole range, which is what the draw-argument \
             pass builds its arguments out of"
        );
        // And the two buckets read different constant blocks, or both would walk
        // the same run of instances.
        assert_ne!(
            renderer.bucket_constants[DEMO_CUBE], renderer.bucket_constants[DEMO_PYRAMID],
            "each bucket needs a block of its own"
        );
        renderer.destroy(device.as_ref());
    }

    /// **Each bucket's block points at that bucket's own run**, and the runs do
    /// not overlap.
    ///
    /// This is what replaced "the block names an instance": a draw covers
    /// however many instances survived culling, so what it can say for itself is
    /// only where its slice of the survivors begins. Two buckets sharing a base
    /// would draw each other's objects — with the *right* geometry, because the
    /// index range comes from the arguments, which is exactly the kind of
    /// plausible wrong picture a golden image struggles with.
    ///
    /// Read out of the bytes that reached the device rather than out of the
    /// renderer's own fields, because the failure is a block written wrong and
    /// the field would still be right.
    #[test]
    fn each_bucket_walks_its_own_run_of_survivors() {
        let (recorder, device, queue) = open();
        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let blocks = recorder
            .buffer_bytes(renderer.draw_constants)
            .expect("the blocks are live");
        let base_at = |offset: u32| {
            let at = offset as usize;
            u32::from_le_bytes(blocks[at..at + 4].try_into().expect("four bytes"))
        };

        let cube = base_at(renderer.bucket_constants[DEMO_CUBE]);
        let pyramid = base_at(renderer.bucket_constants[DEMO_PYRAMID]);
        // **Not zero**, and that is the point since the runs came to share a
        // buffer with `cull.slang`'s survivor list: the first bucket's run starts
        // where that list ends. A base of zero would have the first bucket walk
        // the survivors instead of its own run — the same instances in a
        // different order, which draws a plausible picture.
        assert_eq!(
            cube,
            renderer.draws.visible_capacity(),
            "the first bucket's run starts past the survivor list"
        );
        assert_eq!(
            pyramid,
            renderer.draws.visible_capacity() * 2,
            "and the second's starts a whole run later — the stride is the \
             capacity, so a bucket that filled up still cannot reach the next"
        );
        assert_eq!(
            cube,
            renderer.draws.bucket_base(DEMO_CUBE as u32),
            "the block carries what `DrawGen::bucket_base` says, because a reader \
             copying a run back uses that accessor and the shader uses this block"
        );
        renderer.destroy(device.as_ref());
    }

    /// **The two pyramids differ in exactly one field, and it is the material
    /// id — which names a row holding a different colour and a different
    /// roughness.**
    ///
    /// The whole of what §3.2's table buys, stated as the two things that have
    /// to be true at once. Placed at the *same* transform, so "differ only in
    /// material" is literal rather than nearly: same mesh id, same matrix, same
    /// flags, and a `GpuInstance` comparison that would fail on any other
    /// field. And the ids they carry name rows the device actually holds —
    /// asserted on the bytes in the buffer, because a table the renderer agrees
    /// with itself about is not a table a shader can read.
    ///
    /// The two rows are asserted to differ, or the pair would be evidence of
    /// nothing: two instances naming two rows of the same colour draw the same
    /// picture whether or not either id was ever used.
    #[test]
    fn the_two_pyramids_differ_only_in_their_material() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let at = Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0));
        let plain_handle = place_demo(&mut renderer, DEMO_PYRAMID, DEMO_UNTINTED, at);
        let tinted_handle = place_demo(&mut renderer, DEMO_PYRAMID, DEMO_TINTED, at);

        let instance = |handle: InstanceHandle| {
            renderer
                .instances
                .get(handle)
                .expect("the instance was inserted and is live")
        };
        let plain = instance(plain_handle);
        let tinted = instance(tinted_handle);
        assert_ne!(
            plain.material, tinted.material,
            "the two pyramids must not share a material row"
        );
        assert_eq!(
            mesh::GpuInstance {
                material: tinted.material,
                ..plain
            },
            tinted,
            "the two instances must differ in the material id and in nothing else"
        );

        let bytes = recorder
            .buffer_bytes(renderer.materials.buffer())
            .expect("the table is live");
        let row = |index: u32| {
            let at = index as usize * crcbl_shaders::mesh::MATERIAL_STRIDE;
            mesh::GpuMaterial::from_bytes(
                bytes[at..at + crcbl_shaders::mesh::MATERIAL_STRIDE]
                    .try_into()
                    .expect("one row"),
            )
        };
        assert_eq!(
            row(plain.material),
            mesh::GpuMaterial::UNTINTED,
            "the plain pyramid's row must be the factor that changes nothing"
        );
        assert_eq!(
            row(tinted.material),
            mesh::GpuMaterial {
                base_color: scene::PYRAMID_TINT,
                roughness: scene::PYRAMID_ROUGHNESS,
                ..mesh::GpuMaterial::UNTINTED
            },
            "the tinted pyramid's row must be the tint and the tighter lobe"
        );
        // The shading half of that row, on its own: a roughness equal to the
        // neutral row's would leave `crcbl`'s render e2e comparing one lobe
        // against itself, and that test would go green on a shader that had
        // never read the column.
        assert_ne!(
            row(plain.material).roughness,
            row(tinted.material).roughness,
            "the two rows must differ in roughness, or the highlight the render e2e measures \
             is not attributable to the material"
        );
        assert_ne!(
            row(plain.material),
            row(tinted.material),
            "two rows holding the same colour would make the pair prove nothing"
        );
        // And the tint is the *only* thing that differs between them: a row that
        // also changed its page layer would make this pair evidence about two
        // columns at once, which is what the third pyramid exists to avoid.
        assert_eq!(
            row(plain.material).base_color_texture,
            row(tinted.material).base_color_texture,
            "the factor pair must sample the same page layer"
        );

        // And the second pyramid leaves the frame the way the first does: the
        // instance is removed, not a draw skipped.
        renderer.remove_instance(tinted_handle);
        assert!(
            renderer.instances.get(tinted_handle).is_none(),
            "the instance must be given back, or an object nobody asked for stays in the scene"
        );

        renderer.destroy(device.as_ref());
    }

    /// **The same claim one column along: the plain and textured pyramids
    /// differ in exactly one field, and the rows it names differ in exactly one
    /// column — the base-colour page layer.**
    ///
    /// [`the_two_pyramids_differ_only_in_their_material`]'s argument for §3.2's
    /// *texture indices* rather than its *factors*. The factors of the two rows
    /// are asserted **equal** here, which is the half that makes the pair
    /// evidence about the texture at all: a textured row that also carried a
    /// tint would draw a different picture for a reason the frame could not
    /// distinguish from the tinted pyramid's.
    ///
    /// The layer numbers are asserted against the page the renderer actually
    /// uploaded, so a row naming a layer that does not exist is caught here
    /// rather than as an out-of-range sample on a device.
    #[test]
    fn the_textured_pyramid_differs_only_in_its_page_layer() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let at = Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0));
        let plain_handle = place_demo(&mut renderer, DEMO_PYRAMID, DEMO_UNTINTED, at);
        let textured_handle = place_demo(&mut renderer, DEMO_PYRAMID, DEMO_TEXTURED, at);

        let instance = |handle: InstanceHandle| {
            renderer
                .instances
                .get(handle)
                .expect("the instance was inserted and is live")
        };
        let plain = instance(plain_handle);
        let textured = instance(textured_handle);
        assert_ne!(
            plain.material, textured.material,
            "the two pyramids must not share a material row"
        );
        assert_eq!(
            mesh::GpuInstance {
                material: textured.material,
                ..plain
            },
            textured,
            "the two instances must differ in the material id and in nothing else"
        );

        let bytes = recorder
            .buffer_bytes(renderer.materials.buffer())
            .expect("the table is live");
        let row = |index: u32| {
            let at = index as usize * crcbl_shaders::mesh::MATERIAL_STRIDE;
            mesh::GpuMaterial::from_bytes(
                bytes[at..at + crcbl_shaders::mesh::MATERIAL_STRIDE]
                    .try_into()
                    .expect("one row"),
            )
        };
        assert_eq!(
            row(plain.material).base_color,
            row(textured.material).base_color,
            "the texture pair must share a factor, or the pair is evidence about both columns"
        );
        assert_eq!(
            row(plain.material).base_color_texture,
            scene::PageDesc::UNTEXTURED_LAYER
        );
        assert_eq!(
            row(textured.material).base_color_texture,
            scene::CHECKER_LAYER
        );
        assert_ne!(
            row(plain.material),
            row(textured.material),
            "two rows naming the same layer would make the pair prove nothing"
        );
        // The layers those numbers name are layers the page has, and they hold
        // different texels — a page whose two layers were the same image would
        // pass every assertion above and draw one picture.
        let page = scene::demo().page;
        assert_ne!(
            page.layers()[scene::PageDesc::UNTEXTURED_LAYER as usize],
            page.layers()[scene::CHECKER_LAYER as usize]
        );
        for layer in [scene::PageDesc::UNTEXTURED_LAYER, scene::CHECKER_LAYER] {
            assert!(
                (layer as usize) < page.layers().len(),
                "layer {layer} is past the end of a {}-layer page, which is an out-of-range \
                 sample nothing below the seam would report",
                page.layers().len()
            );
        }

        renderer.remove_instance(textured_handle);
        assert!(
            renderer.instances.get(textured_handle).is_none(),
            "the instance must be given back, or an object nobody asked for stays in the scene"
        );

        renderer.destroy(device.as_ref());
    }

    /// **A page and a material table that are the caller's own, not
    /// [`scene::demo`]'s**: a different extent, more layers than the demo page
    /// holds, and rows past the three the demo writes.
    ///
    /// Every other scene in the tree is `scene::demo()` — with a mesh appended,
    /// at most — so until this nothing had put an app-supplied *layer* or an
    /// app-supplied *row* through [`ForwardRenderer::with_scene`] at all. A
    /// build that uploaded the demo's two layers and stopped, or that inserted
    /// its three rows and stopped, would leave every golden byte-identical and
    /// pass every other assertion in this module: the demo scene cannot tell a
    /// table filled from the description from one filled up to the description's
    /// third row.
    ///
    /// Read off the recorder rather than off the description, because the
    /// description is the input. The copies are what reached the image and the
    /// table buffer is what the shader indexes, and neither is a restatement of
    /// what was handed in.
    #[test]
    fn an_app_page_and_table_reach_the_device_whole() {
        use crcbl_hal::null::Command;

        /// Not [`scene::PAGE_EXTENT`], so an extent that had been compiled in
        /// rather than read off the page shows up as a copy of the wrong size.
        const APP_EXTENT: u32 = 4;
        let layer_bytes = APP_EXTENT as usize * APP_EXTENT as usize * 4;

        // Layers past the white one, each flat and each a value of its own: a
        // build that uploaded one layer twice, or that stopped short, is a
        // different sequence of copies.
        let mut page = scene::PageDesc::opaque_white(APP_EXTENT);
        let app_layers: Vec<u32> = [0x11_u8, 0x22, 0x33]
            .into_iter()
            .map(|value| page.push_layer(vec![value; layer_bytes]))
            .collect();

        let mut scene = scene::demo();
        scene.page = page;
        // One row per appended layer, and each with a factor of its own as well,
        // so a row that landed at another row's id is not a row that happens to
        // look like it.
        for (nth, &layer) in app_layers.iter().enumerate() {
            let shade = 0.25 * (nth as f32 + 1.0);
            scene.materials.push(mesh::GpuMaterial {
                base_color: [shade, shade, shade, 1.0],
                base_color_texture: layer,
                ..mesh::GpuMaterial::UNTINTED
            });
        }
        assert!(
            scene.materials.len() > scene::demo().materials.len()
                && scene.page.layers().len() > scene::demo().page.layers().len(),
            "the description under test must exceed the demo's in both, or this is a \
             demo test wearing another name"
        );

        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect("a page and a table of the caller's own are ones this can make resident");

        // The page, layer by layer. Filtered by image because the build uploads
        // the ambient-occlusion placeholder through the same path.
        let copies: Vec<crcbl_hal::BufferImageCopy> = recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::CopyBufferToImage(copy)
                    if copy.image == renderer.base_color_page.image =>
                {
                    Some(copy)
                }
                _ => None,
            })
            .collect();
        // Every layer goes up with its chain — three levels for this extent —
        // so the copies are layer-major, level-minor, each at its level's own
        // extent.
        let levels = crcbl_hal::Extent3d::d2(APP_EXTENT, APP_EXTENT)
            .full_mip_levels(crcbl_hal::ImageType::D2);
        assert_eq!(levels, 3, "a 4² page has three levels down to one texel");
        let layer_count = u32::try_from(scene.page.layers().len()).expect("a page of a few layers");
        assert_eq!(
            copies
                .iter()
                .map(|copy| (
                    copy.image_subresource.base_layer,
                    copy.image_subresource.mip
                ))
                .collect::<Vec<(u32, u32)>>(),
            (0..layer_count)
                .flat_map(|layer| (0..levels).map(move |level| (layer, level)))
                .collect::<Vec<(u32, u32)>>(),
            "every level of every layer the description carries must be copied into the page, \
             in order"
        );
        assert!(
            copies.iter().all(|copy| {
                let side = crate::mip::level_extent(APP_EXTENT, copy.image_subresource.mip);
                copy.image_extent == crcbl_hal::Extent3d::d2(side, side)
            }),
            "the page is the extent the caller wrote, not the demo's, halved per level: \
             {copies:?}"
        );

        // The table, row by row, out of the buffer `mesh.slang` indexes.
        let bytes = recorder
            .buffer_bytes(renderer.materials.buffer())
            .expect("the table is live");
        let row = |id: u32| {
            let at = id as usize * crcbl_shaders::mesh::MATERIAL_STRIDE;
            mesh::GpuMaterial::from_bytes(
                bytes[at..at + crcbl_shaders::mesh::MATERIAL_STRIDE]
                    .try_into()
                    .expect("one row"),
            )
        };
        assert_eq!(
            renderer.material_ids.len(),
            scene.materials.len(),
            "one id per description row, whatever the count"
        );
        for (index, wrote) in scene.materials.iter().enumerate() {
            assert_eq!(
                &row(renderer.material_ids[index]),
                wrote,
                "description row {index} must be the row its id names"
            );
        }

        // And a caller can place an object through a row only its own
        // description has, which is the whole point of the rows being the
        // caller's.
        let last = scene.materials.len() - 1;
        let handle = renderer
            .add_instance(&InstanceDesc {
                mesh: DEMO_CUBE,
                material: last,
                transform: Mat4::IDENTITY,
            })
            .expect("an empty pool of thousands has room for one object");
        assert_eq!(
            renderer
                .instances
                .get(handle)
                .expect("a just-added instance is live")
                .material,
            renderer.material_ids[last],
            "an InstanceDesc's material index resolves through the description, not \
             through a table id the caller cannot know"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **The frame records two compute dispatches and exactly one indirect draw
    /// per bucket — whatever is in the scene.**
    ///
    /// The property topic 03 opens with, checked on the recorded command stream:
    /// adding the pyramid adds an instance and changes no command. A pass that
    /// had kept a draw per object would record one command here and two there,
    /// which is what this used to assert and what the indirect path replaced.
    ///
    /// It also pins the arguments each call names: bucket `n` reads the `n`-th
    /// argument structure and the `n`-th count, at the stride every API fixed.
    /// Two calls pointing at one structure is a frame that draws one object
    /// twice.
    #[test]
    fn the_frame_records_one_indirect_call_per_bucket_whatever_the_scene_holds() {
        use crcbl_hal::null::Command;

        for pyramid in [None, Some(Mat4::from_translation(Vec3::X))] {
            let (recorder, device, queue) = open();
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("built");
            // The cube in both scenes, so the difference between them is the
            // pyramid alone. It also keeps both frames non-empty, and an empty
            // one would be a weaker comparison than this claims: `cull`'s
            // dispatch covers no workgroups when the pool is empty, and the pass
            // records nothing rather than a dispatch of zero — see
            // [`DrawGen::add_passes`].
            place_cube(&mut renderer, Mat4::IDENTITY);
            if let Some(at) = pyramid {
                place_demo(&mut renderer, DEMO_PYRAMID, DEMO_UNTINTED, at);
            }
            let frame = frame(device.as_ref(), &mut renderer, queue);

            let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u32;
            let expected: Vec<(u32, u64, u64)> = (0..renderer.bucket_constants.len())
                .map(|bucket| {
                    (
                        renderer.bucket_constants[bucket],
                        bucket as u64 * u64::from(stride),
                        bucket as u64 * 4,
                    )
                })
                .collect();

            // The dynamic offset last bound before each draw, paired with the
            // offsets that draw read its arguments and its count from.
            let mut offset = None;
            let mut seen: Vec<(u32, u64, u64)> = Vec::new();
            let mut dispatches = 0;
            for command in recorder.commands() {
                if matches!(command, Command::Dispatch { .. }) {
                    dispatches += 1;
                }
            }
            for command in commands_in_pass(&recorder, "forward") {
                match command {
                    Command::BindGroup {
                        slot: 0,
                        dynamic_offsets,
                        ..
                    } => offset = dynamic_offsets.first().copied(),
                    Command::DrawIndexedIndirectCount(draw) => {
                        assert_eq!(draw.stride, stride, "sizeof(VkDrawIndexedIndirectCommand)");
                        assert_eq!(
                            draw.max_draw_count, 1,
                            "one argument structure per bucket, so one is the ceiling"
                        );
                        seen.push((
                            offset.expect("a draw is preceded by its constant block"),
                            draw.args_offset,
                            draw.count_offset,
                        ));
                    }
                    Command::DrawIndexed { .. } => {
                        panic!("the forward pass must not record a CPU-counted draw any more")
                    }
                    _ => {}
                }
            }
            assert_eq!(
                dispatches,
                3 * (1 + shadow::CASCADES) + 1,
                "the clearing pass, the cull pass and the draw-argument pass, in front of \
                 the draws — once for the camera and once per shadow cascade — plus topic \
                 18's one clustering dispatch, which is the camera's alone because a \
                 cascade shades nothing"
            );
            assert_eq!(
                seen, expected,
                "with pyramid = {pyramid:?}, the pass must record one indirect call per \
                 bucket, each pointing at its own arguments, its own count and its own \
                 block"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// **The two `GeometryPath` arms record different calls and nothing else
    /// differs.**
    ///
    /// A device with a GPU-side count takes it from GPU memory; one without —
    /// Metal, whose API has multi-draw-indirect and no count buffer — reads each
    /// bucket's one argument structure unconditionally. Same buckets, same
    /// arguments, same block: the tail is the only thing that moves, which is
    /// what `docs/plan/03-gpu-driven-rendering.md` means by "the lesser path is
    /// a constraint on data layout, not a separate renderer".
    ///
    /// Both presets are run here rather than only the one this machine has,
    /// because the arm a device never selects is the arm nothing tests.
    #[test]
    fn each_geometry_path_records_its_own_indirect_call() {
        use crcbl_hal::null::Command;

        for (preset, path, counted) in [
            (
                NullInstance::gpu_driven(),
                GeometryPath::IndirectCount,
                true,
            ),
            (
                NullInstance::portable(),
                GeometryPath::IndirectPerBatch,
                false,
            ),
        ] {
            let recorder = Recorder::new();
            let instance = preset.with_recorder(recorder.clone());
            let adapter = instance.adapters().remove(0);
            assert_eq!(
                adapter.caps.geometry_path(),
                path,
                "the preset must select the path this arm is about"
            );
            // `DeviceDesc::for_adapter` requires a timeline semaphore, which the
            // portable preset does not have; the forward pass does not need one
            // either, so this asks for what it uses. The optional set matters:
            // a device is opened with the features it *asked* for, and the path
            // is derived from those rather than from the adapter's.
            let device = instance
                .create_device(&DeviceDesc {
                    label: None,
                    adapter: adapter.id,
                    required_features: Features::COMPUTE,
                    optional_features: Features::GPU_DRIVEN,
                    compatible_surface: None,
                })
                .expect("the null backend always opens");
            assert_eq!(
                device.caps().geometry_path(),
                path,
                "and the opened device still selects it"
            );
            let queue = device.queue(QueueKind::Graphics).expect("always present");
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("built");
            let frame = frame(device.as_ref(), &mut renderer, queue);

            let mut counted_calls = 0;
            let mut per_batch_calls = 0;
            for command in commands_in_pass(&recorder, "forward") {
                match command {
                    Command::DrawIndexedIndirectCount(_) => counted_calls += 1,
                    Command::DrawIndexedIndirect(draw) => {
                        assert_eq!(
                            draw.draw_count, 1,
                            "one argument structure per bucket, read unconditionally"
                        );
                        per_batch_calls += 1;
                    }
                    _ => {}
                }
            }
            let buckets = renderer.bucket_constants.len();
            let (expected_counted, expected_per_batch) =
                if counted { (buckets, 0) } else { (0, buckets) };
            assert_eq!(
                (counted_calls, per_batch_calls),
                (expected_counted, expected_per_batch),
                "{path:?} must record one call of its own kind per bucket and none of the other"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// Serialises the two checks that move
    /// [`r_debug_draw`](crate::debug_draw::r_debug_draw), and puts it back
    /// afterwards.
    ///
    /// A [`ConVar`](crcbl_console::ConVar) is process-global by design, and
    /// `cargo test` runs a crate's tests as threads of *one* process: two checks
    /// that move the switch are then two writers to one cell, which shows up as
    /// a flake rather than as a failure anybody can read.
    /// `crcbl::debug_view::for_test` is the same shape one crate up and there
    /// for the same reason; this one is smaller because nothing outside these
    /// two tests ever writes the cell.
    fn debug_draw_switch() -> DebugDrawSwitch {
        let guard = DEBUG_DRAW_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let switch = DebugDrawSwitch { _guard: guard };
        switch.set(false);
        switch
    }

    /// What [`debug_draw_switch`] hands back: the switch is this thread's until
    /// it drops, and it goes back off when it does.
    struct DebugDrawSwitch {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl DebugDrawSwitch {
        /// Moves the switch.
        fn set(&self, on: bool) {
            crate::debug_draw::r_debug_draw
                .set(&crcbl_console::Value::Bool(on))
                .expect("`r_debug_draw` is a writable bool");
        }
    }

    impl Drop for DebugDrawSwitch {
        fn drop(&mut self) {
            // Off at both ends, so a check that panicked mid-way does not hand
            // the next holder a switched-on layer.
            self.set(false);
        }
    }

    /// [`debug_draw_switch`]'s lock.
    ///
    /// **First of the two**, when a test takes this and [`ssao_blur_switch`]
    /// both — see that function for what the opposite order cost.
    static DEBUG_DRAW_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Takes `r_ssao_blur_passes` for this thread and puts it back at its
    /// default when the guard drops.
    ///
    /// [`debug_draw_switch`]'s shape exactly, for its reason: a plain
    /// `cargo test` run shares one process between tests, and a variable this
    /// one left moved is a frame the next test did not ask for.
    ///
    /// **Every test whose assertion depends on the shape of a frame takes this,
    /// not only the one that moves the count.** The variable is a process
    /// global and `cargo test` runs this module's tests on threads that share
    /// it, so a frame built while
    /// [`a_second_occlusion_blur_is_a_pass_the_frame_records`] holds two blurs
    /// carries an `ssao-blur-2` pass, a full-screen triangle and a pipeline bind
    /// that the test building it never asked for. Taking the guard both
    /// serialises against that test and pins the count for the frames this one
    /// builds.
    ///
    /// Two shapes need it, and the second is easy to miss. A test comparing a
    /// frame against a **literal pass list** breaks whenever the count is two.
    /// A test comparing **two frames it built itself** — a difference in draws,
    /// an equality between pass lists, a "one pass longer" claim — breaks only
    /// when the count *moves between them*, which is a narrower window and why
    /// this reproduced at roughly one run in eight rather than every run.
    ///
    /// Both were established by experiment on 2026-09-01 rather than by
    /// reading: pinning the count at two reddens the first shape, and making
    /// `crate::ssao::blur_passes` alternate reddens the second.
    ///
    /// **A test taking this and [`debug_draw_switch`] takes that one first.**
    /// These are two plain mutexes with no ordering between them, so two tests
    /// acquiring both in opposite orders deadlock — which is not a hazard this
    /// comment is guessing at: adding the guard to the two debug-draw tests
    /// above their existing one did exactly that on 2026-09-01, and turned a
    /// 0.2 second suite into one that had not finished in ten minutes. It is
    /// invisible in a single-threaded run, where the same suite passes in a
    /// second, so `--test-threads=1` is not a check for it.
    ///
    /// [`a_second_occlusion_blur_is_a_pass_the_frame_records`]: fn@a_second_occlusion_blur_is_a_pass_the_frame_records
    fn ssao_blur_switch() -> SsaoBlurSwitch {
        let guard = SSAO_BLUR_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let switch = SsaoBlurSwitch { _guard: guard };
        switch.set(1);
        switch
    }

    /// What [`ssao_blur_switch`] hands back.
    struct SsaoBlurSwitch {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl SsaoBlurSwitch {
        /// Asks for `passes` of them.
        fn set(&self, passes: i64) {
            crate::ssao::r_ssao_blur_passes
                .set(&crcbl_console::Value::Int(passes))
                .expect("`r_ssao_blur_passes` is a writable int in range");
        }
    }

    impl Drop for SsaoBlurSwitch {
        fn drop(&mut self) {
            // Back to the shipping count at both ends, so a check that panicked
            // mid-way does not hand the next holder a three-pass frame.
            self.set(1);
        }
    }

    /// [`ssao_blur_switch`]'s lock.
    static SSAO_BLUR_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Takes `r_ssao_split` for this thread and puts it back when the guard
    /// drops.
    ///
    /// [`ssao_blur_switch`]'s shape exactly, for its reason. The seam is what
    /// makes the occlusion chain record its second march, so a test about the
    /// widest frame a renderer can produce has to hold this as well as the blur
    /// count — see `crate::ssao`'s `r_ssao_split`.
    fn ssao_split_switch() -> SsaoSplitSwitch {
        let guard = SSAO_SPLIT_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let switch = SsaoSplitSwitch { _guard: guard };
        switch.set(0.0);
        switch
    }

    /// What [`ssao_split_switch`] hands back.
    struct SsaoSplitSwitch {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl SsaoSplitSwitch {
        /// Asks for a seam at `at`, or for none at zero.
        fn set(&self, at: f32) {
            crate::ssao::r_ssao_split
                .set(&crcbl_console::Value::Float(at))
                .expect("`r_ssao_split` is a writable float in range");
        }
    }

    impl Drop for SsaoSplitSwitch {
        fn drop(&mut self) {
            // Back to the default at both ends, so a check that panicked
            // mid-way does not hand the next holder a comparing frame.
            self.set(0.0);
        }
    }

    /// [`ssao_split_switch`]'s lock.
    static SSAO_SPLIT_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Takes `r_shadow_filter` and `r_shadow_split` for this thread and puts
    /// them both back at their defaults when the guard drops.
    ///
    /// [`ssao_split_switch`]'s shape, with two variables under one lock because
    /// they are read into one row of one block — `crate::shadow::FILTER_SWITCH`
    /// is where that is argued, and why the lock lives there rather than beside
    /// this function: `crate::shadow`'s own tests move the same two.
    fn shadow_filter_switch() -> ShadowFilterSwitch {
        let guard = crate::shadow::FILTER_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let switch = ShadowFilterSwitch { _guard: guard };
        switch.reset();
        switch
    }

    /// What [`shadow_filter_switch`] hands back.
    struct ShadowFilterSwitch {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl ShadowFilterSwitch {
        /// Asks for `filter` on the near side of the seam.
        fn filter(&self, filter: shadow::Filter) {
            crate::shadow::r_shadow_filter
                .set(&crcbl_console::Value::Enum(filter.label()))
                .expect("`r_shadow_filter` is a writable enum in the set");
        }

        /// Asks for a seam at `at`, or for none at zero.
        fn seam(&self, at: f32) {
            crate::shadow::r_shadow_split
                .set(&crcbl_console::Value::Float(at))
                .expect("`r_shadow_split` is a writable float in range");
        }

        /// Back to the frame every golden was blessed as.
        fn reset(&self) {
            self.filter(shadow::shipped_filter());
            self.seam(0.0);
        }
    }

    impl Drop for ShadowFilterSwitch {
        fn drop(&mut self) {
            // Back to the defaults at both ends, so a check that panicked
            // mid-way does not hand the next holder a comparing frame.
            self.reset();
        }
    }

    /// Takes `r_ssao_slices` for this thread and puts it back at its default
    /// when the guard drops.
    ///
    /// [`ssao_blur_switch`]'s shape exactly, for its reason: a plain
    /// `cargo test` run shares one process between tests, and a variable this
    /// one left moved is a frame the next test did not ask for.
    fn ssao_slice_switch() -> SsaoSliceSwitch {
        let guard = SSAO_SLICE_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let switch = SsaoSliceSwitch { _guard: guard };
        switch.set(i64::from(ssao::SLICE_COUNT_DEFAULT));
        switch
    }

    /// What [`ssao_slice_switch`] hands back.
    struct SsaoSliceSwitch {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl SsaoSliceSwitch {
        /// Asks each pixel for `slices` of them.
        fn set(&self, slices: i64) {
            crate::ssao::r_ssao_slices
                .set(&crcbl_console::Value::Int(slices))
                .expect("`r_ssao_slices` is a writable int in range");
        }
    }

    impl Drop for SsaoSliceSwitch {
        fn drop(&mut self) {
            // Back to the shipping count at both ends, so a check that panicked
            // mid-way does not hand the next holder a four-slice frame — which
            // is a frame every golden in this workspace was blessed without.
            self.set(i64::from(ssao::SLICE_COUNT_DEFAULT));
        }
    }

    /// [`ssao_slice_switch`]'s lock.
    static SSAO_SLICE_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Takes `r_ssao_technique` for this thread and puts it back at its default
    /// when the guard drops.
    ///
    /// [`ssao_blur_switch`]'s shape exactly, for its reason — and this one moves
    /// a **pipeline** rather than a lane of a uniform block, so a frame that
    /// picked it up from another thread's test gathers through a shader that
    /// test never selected.
    ///
    /// **Its lock lives in `crate::ssao`** rather than beside this function, and
    /// it is the one switch here whose does: `ssao`'s own
    /// `the_seams_other_side_is_the_technique_that_ships` moves the same
    /// variable, and a second mutex would serialise neither against the other.
    /// See `crate::ssao::TECHNIQUE_SWITCH` for where it sits in the order.
    fn ssao_technique_switch() -> SsaoTechniqueSwitch {
        let guard = crate::ssao::TECHNIQUE_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let switch = SsaoTechniqueSwitch { _guard: guard };
        switch.set(shipped_technique_name());
        switch
    }

    /// The technique name the chain ships, and the one
    /// [`ssao_technique_switch`] restores.
    ///
    /// **Read off the variable rather than written down**, which is
    /// [`the_comparison_seam_marches_twice_over_columns_that_tile_the_gather`]'s
    /// rule about the slice count and for its reason: which value ships is a
    /// thing that moves, and a test naming the wrong one would be comparing the
    /// shipped configuration against itself.
    ///
    /// [`the_comparison_seam_marches_twice_over_columns_that_tile_the_gather`]: fn@the_comparison_seam_marches_twice_over_columns_that_tile_the_gather
    fn shipped_technique_name() -> &'static str {
        match crate::ssao::r_ssao_technique.default() {
            crcbl_console::Value::Enum(name) => name,
            other => panic!("`r_ssao_technique` defaults to {other:?}, not to a technique name"),
        }
    }

    /// A name from the variable's own set that is **not** the one it ships.
    ///
    /// [`shipped_technique_name`]'s clause from the other side: the tests below
    /// need a technique to move the console to, and picking it out of the
    /// declared set is what keeps them from naming one the variable does not
    /// offer — or, if a third arrived, from silently going on comparing the
    /// same two.
    fn other_technique_name() -> &'static str {
        let crcbl_console::Kind::Enum(names) = crate::ssao::r_ssao_technique.kind() else {
            panic!("`r_ssao_technique` is an enum variable");
        };
        let ships = shipped_technique_name();
        names
            .iter()
            .copied()
            .find(|name| *name != ships)
            .expect("`r_ssao_technique` offers a technique other than the one it ships")
    }

    /// What [`ssao_technique_switch`] hands back.
    struct SsaoTechniqueSwitch {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl SsaoTechniqueSwitch {
        /// Asks for the gather `name` selects.
        fn set(&self, name: &'static str) {
            crate::ssao::r_ssao_technique
                .set(&crcbl_console::Value::Enum(name))
                .expect("`r_ssao_technique` is a writable enum in the set");
        }
    }

    impl Drop for SsaoTechniqueSwitch {
        fn drop(&mut self) {
            // Back to what ships at both ends, so a check that panicked mid-way
            // does not hand the next holder a frame gathered through the cheap
            // tier — which is a frame no golden in this workspace was blessed
            // on.
            self.set(shipped_technique_name());
        }
    }

    /// Every draw the recorded stream holds, whichever call recorded it.
    ///
    /// Not scoped to a pass, unlike [`commands_in_pass`]: what
    /// [`ForwardRenderer::counters`] claims is a count over the *whole* frame, so
    /// the thing to compare it against is the whole frame.
    fn recorded_draws(recorder: &Recorder) -> usize {
        use crcbl_hal::null::Command;

        recorder
            .commands()
            .into_iter()
            .filter(|command| {
                matches!(
                    command,
                    Command::Draw { .. }
                        | Command::DrawIndexed { .. }
                        | Command::DrawIndirect(_)
                        | Command::DrawIndexedIndirect(_)
                        | Command::DrawIndirectCount(_)
                        | Command::DrawIndexedIndirectCount(_)
                        | Command::DrawMeshTasks { .. }
                        | Command::DrawMeshTasksIndirect(_)
                )
            })
            .count()
    }

    /// **The draw count is what the frame recorded, and the two GPU-side
    /// counters are `None` rather than a guess.**
    ///
    /// Both halves matter and both are checked here against something that
    /// moves. The draws are compared with the recorded stream, so a count that
    /// forgot the shadow pass's share or the tonemap's triangle fails. The
    /// instances are read before and after a pyramid is put in the pool, so a
    /// counter wired to a constant — or to the pool's *capacity*, which does not
    /// move — fails too.
    ///
    /// `drawn` and `triangles` are asserted [`None`] **on the frames this test
    /// runs**, which are inside [`CULL_STATS_FLOOR`]: nothing has come
    /// back off the GPU yet, and a `drawn` that was `Some` here would be a
    /// number invented before any readback landed. That is the assertion that
    /// catches the tempting version of this method: an instance count taken from
    /// the pool and reported as "drawn" would read as a culling win of exactly
    /// zero on every frame, which is the number the whole cull exists to change.
    /// What happens once the ring *has* come round is the test below.
    #[test]
    fn the_forward_counters_are_the_recorded_draws_and_admit_what_they_cannot_know() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");

        // Before any frame: nothing recorded, and said as a known zero.
        assert_eq!(renderer.counters(), FrameCounters::default());

        let first = frame(device.as_ref(), &mut renderer, queue);
        let counters = renderer.counters();
        assert_eq!(
            counters.draws,
            recorded_draws(&recorder) as u64,
            "the counter and the frame's recorded draws disagree",
        );
        // Derived from what the frame resolved rather than from a constant, so
        // this is still the every-effect frame's count and would follow a frame
        // that switched one off.
        let fullscreen = fullscreen_passes(
            renderer.effects(),
            TEST_EXTENT,
            false,
            renderer.frame_ssao_blurs,
        ) * FULLSCREEN_DRAWS;
        assert!(
            counters.draws > fullscreen,
            "a frame that recorded only its full-screen passes drew no scene",
        );
        assert_eq!(
            counters.instances,
            renderer.instances.len() as u64 + fullscreen,
        );
        assert_eq!(
            counters.drawn, None,
            "the ring has not come round, so there is no survivor count yet",
        );
        assert_eq!(counters.cull_frame, None, "and no frame to stamp it with");
        assert_eq!(counters.triangles, None);
        first.release(device.as_ref());

        // A second scene: one more instance in the pool, and the counter moves
        // with it. The draws do not — one call per bucket whatever the scene
        // holds is what §3.3 is about, and this is where that shows.
        place_demo(&mut renderer, DEMO_PYRAMID, DEMO_UNTINTED, Mat4::IDENTITY);
        recorder.clear();
        let second = frame(device.as_ref(), &mut renderer, queue);
        let richer = renderer.counters();
        assert_eq!(
            richer.instances,
            counters.instances + 1,
            "an instance added to the pool must move the counter",
        );
        assert_eq!(
            richer.draws, counters.draws,
            "the recorded call count is independent of what the scene holds",
        );
        assert_eq!(richer.draws, recorded_draws(&recorder) as u64);

        second.finish(device.as_ref(), renderer);
    }

    /// **Once the ring has come round, `drawn` is a number and it says which
    /// frame it is from.**
    ///
    /// The other half of the test above: `indirect` while the readback is in
    /// flight, a count afterwards, and a `cull_frame` naming a frame several
    /// behind the one just recorded. The null backend executes no copy, so the
    /// *value* here is the full-screen passes' own draws and nothing else — what the cull
    /// really kept is asserted against a real GPU, in the umbrella crate's
    /// `tests/draw_gen_e2e/`, where a scene is culled on purpose.
    ///
    /// The frame number is the assertion with teeth here: a ring that read the
    /// slot it had just written would report the frame it is on, and a
    /// `cull_frame` left at `None` would put a latent number on the panel with
    /// nothing to say how old it is.
    #[test]
    fn the_culling_counters_arrive_a_few_frames_late_and_say_which_frame() {
        let (_recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");

        let mut frames = Vec::new();
        for _ in 0..CULL_STATS_FLOOR {
            frames.push(frame(device.as_ref(), &mut renderer, queue));
            assert_eq!(
                renderer.counters().drawn,
                None,
                "no readback has even been polled yet, so there is nothing back",
            );
        }

        frames.push(frame(device.as_ref(), &mut renderer, queue));
        let counters = renderer.counters();
        assert_eq!(
            counters.drawn,
            Some(
                fullscreen_passes(
                    renderer.effects(),
                    TEST_EXTENT,
                    false,
                    renderer.frame_ssao_blurs,
                ) * FULLSCREEN_DRAWS,
            ),
            "the survivor count the null backend produced is zero, plus one instance per \
             full-screen pass — which is on both sides of the row",
        );
        assert_eq!(
            counters.cull_frame,
            Some(1),
            "the report is the frame whose copy answered, not the frame just recorded",
        );
        assert_eq!(renderer.cull_stats().expect("a readback answered").frame, 1,);

        for frame in frames {
            frame.release(device.as_ref());
        }
        renderer.destroy(device.as_ref());
    }

    /// The frames a culling counter cannot possibly have arrived in.
    ///
    /// Two: the copy is recorded on one frame and the readback for it requested
    /// on the next, so the earliest frame that can poll anything is the third.
    /// A device that answers its first poll — the null backend, and every native
    /// one — reports there; a browser answers later, which is why this is a
    /// floor rather than a latency. See [`crate::cull_stats`].
    const CULL_STATS_FLOOR: usize = 2;

    /// **A light that takes a shadow tile moves the draw count**, because the
    /// shadow pass records one call per bucket per occupied view.
    ///
    /// The observable a `recorded_draws` comparison alone would miss: a
    /// `counters` that counted only the colour pass and the tonemap agrees with
    /// nothing here, and one that hard-coded the cascades agrees with the first
    /// frame and not the second.
    #[test]
    fn a_light_that_takes_a_shadow_tile_moves_the_draw_count() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");

        let dark = frame(device.as_ref(), &mut renderer, queue);
        let without = renderer.counters().draws;
        assert_eq!(without, recorded_draws(&recorder) as u64);
        dark.release(device.as_ref());

        renderer.set_lights(&[shadowable_spot(0.0)]);
        recorder.clear();
        let lit = frame(device.as_ref(), &mut renderer, queue);
        let with = renderer.counters().draws;
        assert!(
            with > without,
            "a shadow-casting light adds a view and therefore draws: {with} against {without}",
        );
        assert_eq!(
            with,
            recorded_draws(&recorder) as u64,
            "and the counter still matches the stream",
        );
        lit.finish(device.as_ref(), renderer);
    }

    /// Opens a null device offering `optional` on top of the mesh path, and
    /// asserts it really selected that path.
    ///
    /// Asked for rather than merely reported, because a device grants what it
    /// enabled: leaving mesh shaders out of the optional set opens a device on
    /// an indirect tail and tests nothing.
    fn open_mesh_path(recorder: &Recorder, optional: Features) -> (Box<dyn Device>, QueueHandle) {
        let caps = crcbl_hal::DeviceCaps {
            features: Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
            limits: crcbl_hal::Limits::desktop(),
        };
        let instance = NullInstance::new(caps).with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        assert_eq!(
            adapter.caps.geometry_path(),
            GeometryPath::MeshShader,
            "a device with mesh shaders selects them, which is the case this is about"
        );
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                optional_features: Features::GPU_DRIVEN | Features::MESH_SHADER | optional,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        assert_eq!(
            device.caps().geometry_path(),
            GeometryPath::MeshShader,
            "and the opened device still selects it"
        );
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    /// **The mesh layout declares the same binding numbers with and without a
    /// task stage, and that is a correctness property on Metal.**
    ///
    /// Slang's Metal target ignores `[[vk::binding]]` and gives each resource
    /// the next index in its stage's flat argument table, in the order
    /// `mesh_cluster.slang` declares them — so `msl/mesh_cluster.metal` puts
    /// `cluster_select` (binding 17) at `buffer(13)` and every buffer above it
    /// at a fixed index. `crcbl-mtl` reaches the same numbers by counting the
    /// same-table entries of the layout **below** each binding, which agrees
    /// only while the layout declares everything the shader does.
    ///
    /// It did not. Bindings 13, 14, 18 and 19 were gated on
    /// [`ForwardRenderer::culls_clusters`], so a device with
    /// `Features::MESH_SHADER` and no `Features::TASK_SHADER` built a layout
    /// missing four buffers below 17 — placing it at `buffer(11)` while the MSL
    /// read `buffer(13)`, and shifting every binding above it by two. Nothing
    /// on any other backend can see that: Vulkan and D3D12 read the binding
    /// number, and WebGPU never takes this path.
    ///
    /// **The two material pages are created with different formats, and the
    /// base-colour one is the sRGB one.**
    ///
    /// `docs/plan/44-lighting.md`'s rung 2 calls this the classic PBR bug and
    /// says why it survives review: a base-colour texel is an sRGB-encoded
    /// *colour*, which is what glTF defines it as, and a normal texel is a
    /// *number* — so decoding a normal through the sRGB curve is wrong by a
    /// gamma and looks merely like a surface bumpier than the author drew.
    /// Nothing in a frame reports it, no golden comparison can name it, and the
    /// two constants sit a dozen lines apart with the wrong one compiling.
    ///
    /// So this is the whole guard, and it is three assertions rather than one:
    /// that the pair differ, that each is the one it should be, and — the half
    /// that would otherwise be a check that cannot fail — that the format each
    /// page is *created* with is the format its `ImportedImage` declares. A
    /// declaration naming a format the image is not in picks the wrong aspects
    /// in a barrier.
    #[test]
    fn the_two_page_formats_differ() {
        assert_eq!(BASE_COLOR_PAGE_FORMAT, Format::Rgba8UnormSrgb);
        assert_eq!(NORMAL_PAGE_FORMAT, Format::Rgba8Unorm);
        assert_ne!(
            BASE_COLOR_PAGE_FORMAT, NORMAL_PAGE_FORMAT,
            "a normal page created sRGB decodes every texel through a gamma it never had"
        );

        let (recorder, device, queue) = open();
        let renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
            .expect("the forward renderer builds on the null backend");
        assert_eq!(
            renderer.base_color_page_import().format,
            BASE_COLOR_PAGE_FORMAT
        );
        assert_eq!(renderer.normal_page_import().format, NORMAL_PAGE_FORMAT);
        // And the two really are two images, not one bound twice — which is the
        // other way a page could be "linear" and still be sampled through the
        // sRGB one. The labels come off the recorder rather than off the
        // handles, so this is evidence about what the device was asked to
        // create.
        assert_ne!(
            renderer.normal_page().image,
            renderer.base_color_page().image
        );
        assert_ne!(renderer.normal_page().view, renderer.base_color_page().view);
        let images: Vec<String> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Created {
                    kind: ObjectKind::Image,
                    label: Some(label),
                } => Some(label),
                _ => None,
            })
            .collect();
        for page in ["material base colour", "material normals"] {
            assert!(
                images.iter().any(|label| label == page),
                "no image was created for the {page} page, and it is {images:?} that were"
            );
        }
        renderer.destroy(device.as_ref());
    }

    /// **What turns it red.** Putting either `if emit.is_mesh()` in the layout
    /// back to `if culls_clusters`: the no-task arm then declares 17 fewer than
    /// two buffers below where the amplified arm does, and the sets stop
    /// matching.
    #[test]
    fn the_mesh_layout_declares_the_same_bindings_with_and_without_a_task_stage() {
        let mut declared: Vec<Vec<(u32, BindingKind)>> = Vec::new();
        for optional in [Features::TASK_SHADER, Features::empty()] {
            let recorder = Recorder::new();
            let (device, queue) = open_mesh_path(&recorder, optional);
            let renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("the forward renderer builds on both device shapes");
            assert_eq!(
                renderer.culls_clusters(),
                optional.contains(Features::TASK_SHADER),
                "the two arms must actually differ, or this compares a device with itself"
            );
            let (label, entries) = recorder
                .bind_group_layouts_created()
                .into_iter()
                .find(|(label, _)| label.as_deref() == Some(MESH_LAYOUT_LABEL))
                .expect("the mesh pass declares its layout by name");
            assert_eq!(label.as_deref(), Some(MESH_LAYOUT_LABEL));
            declared.push(
                entries
                    .iter()
                    .map(|entry| (entry.binding, entry.kind))
                    .collect(),
            );
        }

        let [amplified, plain] = declared
            .try_into()
            .unwrap_or_else(|_| unreachable!("two arms were run"));
        assert_eq!(
            amplified, plain,
            "the mesh layout's bindings changed with the task stage, which moves every Metal \
             argument-table index above the difference"
        );
        // Not a vacuous comparison: the four bindings the gate used to hide are
        // the ones that have to be there, and 17 is the one whose index the
        // committed MSL pins.
        for binding in [13, 14, 17, 18, 19] {
            assert!(
                plain.iter().any(|(number, _)| *number == binding),
                "binding {binding} is declared by mesh_cluster.slang and must be in the layout"
            );
        }
        // **And no gaps**, which is the other half of what makes `crcbl-mtl`'s
        // count agree with Slang's declaration order. Counting the same-table
        // entries below a binding yields the index Slang assigned only while
        // the layout is the shader's whole declaration set, contiguous from
        // zero; a gap means some resource the module declares is missing, and
        // every index above the gap is off by as many as are missing.
        let numbers: Vec<u32> = plain.iter().map(|(binding, _)| *binding).collect();
        assert_eq!(
            numbers,
            (0..numbers.len() as u32).collect::<Vec<u32>>(),
            "the mesh layout must declare every binding mesh.slang and mesh_cluster.slang do, \
             ascending from zero with no gaps"
        );
    }

    /// **`Features::TASK_SHADER` is what decides whether §3.5's per-cluster cull
    /// is built, and a device without it still draws.**
    ///
    /// The two capabilities are separate, and the adapter above offers both — so
    /// asking for one and not the other is a device state a real driver has and
    /// this can reach. Neither arm is a skip:
    ///
    /// * With the task stage, the renderer builds the amplification stage and
    ///   [`ForwardRenderer::culls_clusters`] says so. The null backend refuses a
    ///   task stage on a device without the flag, so an arm that computed this
    ///   wrongly would fail to build rather than pass quietly.
    /// * Without it, the renderer builds `meshMain` — the un-amplified entry
    ///   point — and draws every cluster of every surviving instance. That is
    ///   the path that existed before this slice and it is unchanged, dispatch
    ///   extents included.
    #[test]
    fn the_task_stage_is_built_only_where_the_device_has_one() {
        use crcbl_hal::null::Command;

        for (optional, expected) in [(Features::TASK_SHADER, true), (Features::empty(), false)] {
            let recorder = Recorder::new();
            let (device, queue) = open_mesh_path(&recorder, optional);
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("the forward renderer builds on both device shapes");
            assert_eq!(
                device.caps().supports(Features::TASK_SHADER),
                expected,
                "the device must actually differ between the arms, or both test the same thing"
            );
            assert_eq!(
                renderer.culls_clusters(),
                expected,
                "the amplification stage is built exactly where the device has one"
            );
            assert_eq!(
                renderer.geometry_path(),
                GeometryPath::MeshShader,
                "both arms are the mesh path; only the stage in front of it differs"
            );

            let frame = frame(device.as_ref(), &mut renderer, queue);
            // After the frame, because the buffer named is the slot the frame
            // rotated to — asking before it names the previous slot's ring
            // entry and the comparison is against a buffer nothing drew from.
            let expected = mesh_dispatch_calls(&renderer);

            // **The recorded stream is the same either way**, which is what
            // makes the cull a rejection inside the existing shape rather than
            // a second one: one indirect dispatch per bucket, reading that
            // bucket's own arguments, and the amplification stage is what turns
            // a group into no work.
            let dispatched: Vec<DrawIndirect> = commands_in_pass(&recorder, "forward")
                .into_iter()
                .filter_map(|command| match command {
                    Command::DrawMeshTasksIndirect(draw) => Some(draw),
                    _ => None,
                })
                .collect();
            assert_eq!(
                dispatched, expected,
                "one indirect dispatch per bucket on both arms, each reading its own \
                 argument structure"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// The [`DrawIndirect`] the mesh path must record for each bucket, in bucket
    /// order.
    ///
    /// Built from the buffer the draw-argument pass writes and the stride the
    /// APIs fixed, so a test comparing against it is asserting that the call
    /// reads *this frame's* arguments at *this bucket's* offset — the two ways
    /// an indirect dispatch goes wrong while still dispatching something.
    ///
    /// **The offsets are multiplied out here rather than asked of
    /// [`DrawGen::mesh_args_offset`]**, which is the difference between a
    /// comparison and a tautology: an offset function that returned zero for
    /// every bucket would agree with itself, and three calls onto one structure
    /// is a frame that draws one bucket's geometry three times.
    fn mesh_dispatch_calls(renderer: &ForwardRenderer) -> Vec<DrawIndirect> {
        let stride = crcbl_shaders::draw_gen::MESH_ARGS_SIZE as u32;
        // The extents share a buffer with the one draw count per bucket that
        // precedes them, so every offset is that far in. Written out from the
        // bucket count here rather than taken from `DrawGen::mesh_args_offset`,
        // for the reason above: a base of zero is exactly the mistake this
        // comparison exists to catch, and it would agree with itself.
        let counts_bytes = renderer.bucket_constants.len() as u64 * 4;
        (0..renderer.bucket_constants.len())
            .map(|bucket| DrawIndirect {
                args: renderer.draws.mesh_args(renderer.frame),
                offset: counts_bytes + bucket as u64 * u64::from(stride),
                draw_count: 1,
                stride,
            })
            .collect()
    }

    /// **A device selecting [`GeometryPath::MeshShader`] draws through a mesh
    /// pipeline, and records no indirect draw and no index-buffer bind at all.**
    ///
    /// The three halves are one claim. That it dispatched says the path is
    /// wired; that it recorded *no* `DrawIndexedIndirect` of either kind says it
    /// did not quietly fall through to a tail that draws the same picture; and
    /// that it bound no index buffer says the geometry really came out of the
    /// cluster records rather than out of the index pool. Until 2026-08 this
    /// device degraded and logged that it had, which is exactly the shape —
    /// "not implemented" arriving as a passing frame — the three together rule
    /// out.
    ///
    /// **The extents are not here to be checked, and that is the point.** They
    /// are three words the draw-argument pass writes, so what this can pin is
    /// the call reading *this* bucket's structure out of *this* frame's buffer
    /// — and the CPU-side half of the x extent, the per-bucket cluster table
    /// the host uploaded, which the null backend keeps the bytes of. What the
    /// GPU then makes of it needs a GPU: `crcbl-vk`'s
    /// `the_mesh_dispatch_extent_is_the_culled_instance_count` reads the
    /// arguments back.
    #[test]
    fn a_mesh_shader_device_draws_through_a_mesh_pipeline() {
        use crcbl_hal::null::Command;

        let recorder = Recorder::new();
        let caps = crcbl_hal::DeviceCaps {
            features: Features::GPU_DRIVEN | Features::MESH_SHADER,
            limits: crcbl_hal::Limits::desktop(),
        };
        let instance = NullInstance::new(caps).with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        assert_eq!(
            adapter.caps.geometry_path(),
            GeometryPath::MeshShader,
            "a device with mesh shaders selects them, which is the case this is about"
        );
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                // Asked for, because a device reports what it enabled: leaving
                // mesh shaders out of the optional set would open a device that
                // selects an indirect path outright and test nothing.
                optional_features: Features::GPU_DRIVEN | Features::MESH_SHADER,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        assert_eq!(
            device.caps().geometry_path(),
            GeometryPath::MeshShader,
            "and the opened device still selects it"
        );
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        assert_eq!(
            renderer.geometry_path(),
            GeometryPath::MeshShader,
            "the renderer must have *built* the mesh path, not merely been offered it"
        );
        let frame = frame(device.as_ref(), &mut renderer, queue);
        let expected = mesh_dispatch_calls(&renderer);
        // The table is written once at build, into a host-visible buffer the
        // null backend keeps the bytes of — so it is readable whether or not a
        // shader ever ran. It shares that buffer with the four other tables the
        // draw-argument pass reads, so the region has to be cut out at the
        // offset the renderer told the shader about rather than at zero.
        let table_bytes = recorder
            .buffer_bytes(renderer.draws.tables())
            .expect("the table buffer is one of this recorder's buffers");
        let clusters_at = renderer.draws.table_offsets().bucket_clusters_at as usize * 4;
        let clusters: Vec<u32> = table_bytes[clusters_at..]
            .chunks_exact(4)
            .take(renderer.bucket_constants.len())
            .map(|word| u32::from_le_bytes(word.try_into().expect("a four-byte chunk")))
            .collect();

        let mut dispatched = Vec::new();
        let mut index_binds = 0;
        for command in commands_in_pass(&recorder, "forward") {
            match command {
                Command::DrawMeshTasksIndirect(draw) => dispatched.push(draw),
                Command::DrawMeshTasks { .. } => panic!(
                    "the mesh path recorded a CPU-sized dispatch, which is the extent \
                     the instance cull is supposed to have taken over"
                ),
                Command::DrawIndexedIndirectCount(_) | Command::DrawIndexedIndirect(_) => {
                    panic!(
                        "the mesh path recorded an indirect draw, which is the silent \
                         fall-through this test exists to rule out"
                    )
                }
                Command::BindIndexBuffer { .. } => index_binds += 1,
                _ => {}
            }
        }
        assert_eq!(
            dispatched, expected,
            "one indirect dispatch per bucket, each reading its own argument structure \
             out of this frame's buffer"
        );
        assert_eq!(
            clusters.len(),
            renderer.bucket_constants.len(),
            "one x extent per bucket: {clusters:?}"
        );
        assert!(
            clusters.iter().all(|&count| count > 0),
            "a bucket with no clusters dispatches nothing, whatever the arguments say: \
             {clusters:?}"
        );
        // **The x extents are not all the same number**, which is what makes
        // the table a statement about each bucket's own mesh. Two
        // single-cluster meshes would let a table hard-coded to one cluster
        // pass; the open box is five, one per face.
        assert_eq!(
            clusters[DEMO_OPEN_BOX] as usize,
            crcbl_shaders::mesh::OPEN_BOX_FACES.len(),
            "the open box dispatches one workgroup per face, and that is the only \
             resident whose cluster count is not one: {clusters:?}"
        );
        assert_eq!(
            index_binds, 0,
            "a mesh pipeline has no index buffer; binding one would mean the geometry \
             came from the index pool after all"
        );

        frame.finish(device.as_ref(), renderer);
    }

    /// **The cull pass is told the camera the frame is drawn with**, and how
    /// many array elements to walk.
    ///
    /// Both halves fail silently: a frustum from a different aspect ratio culls
    /// geometry that is on screen, and an instance count short of the pool's
    /// high-water mark stops testing the instances above a freed slot. Neither
    /// is visible in a picture until something disappears from an edge.
    #[test]
    fn the_cull_pass_gets_this_frame_s_camera_and_every_occupied_slot() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        // The cube first and the pyramid above it, because the second half of
        // this test frees the lower slot and asks what the walk still covers.
        let cube = place_cube(&mut renderer, Mat4::IDENTITY);
        place_demo(
            &mut renderer,
            DEMO_PYRAMID,
            DEMO_UNTINTED,
            Mat4::from_translation(Vec3::X),
        );

        let camera = Camera {
            eye: Vec3::new(1.6, 1.2, 2.2),
            ..Camera::default()
        };
        let extent = (256u32, 192u32);
        renderer
            .begin_frame(
                device.as_ref(),
                &camera,
                &DirectionalLight::default(),
                extent,
            )
            .expect("write");

        let aspect = extent.0 as f32 / extent.1 as f32;
        let expected = crate::cull::Frustum::from_view_projection(camera.view_projection(aspect));
        let written = recorder
            .buffer_bytes(renderer.draws.cull_params(renderer.frame))
            .expect("this frame's cull parameters are live");
        assert_eq!(
            &written[..crcbl_shaders::cull::PARAMS_SIZE],
            &crcbl_shaders::cull::Params {
                planes: expected.planes.map(|plane| plane.to_array()),
                instance_count: 2,
                capacity: scene::Capacities::default().instances,
            }
            .to_bytes()[..],
            "the cull block must carry this frame's own frustum, the pool's whole \
             occupied range, and the visible list's capacity"
        );

        // And the high-water mark is what is written, not the live count: remove
        // the cube and the pyramid above it still has to be tested.
        renderer.remove_instance(cube);
        renderer
            .begin_frame(
                device.as_ref(),
                &camera,
                &DirectionalLight::default(),
                extent,
            )
            .expect("write");
        let written = recorder
            .buffer_bytes(renderer.draws.cull_params(renderer.frame))
            .expect("live");
        assert_eq!(
            u32::from_le_bytes(written[96..100].try_into().expect("four bytes")),
            2,
            "one live instance, two occupied slots — a walk of one would never \
             reach the pyramid"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Every counter both compute passes add to is zeroed by a pass of its
    /// own, before either of them runs.**
    ///
    /// The two shaders only ever increment, so a counter carrying the previous
    /// frame's total makes a bucket's argument structure claim more instances
    /// than its run holds — which draws whatever the run's stale tail happens to
    /// name.
    ///
    /// # This once read the bytes back, and no longer can
    ///
    /// It used to poison every counter with `0xAB` and assert `begin_frame`
    /// had written zeroes over them — the poison being the whole test, because
    /// the null backend runs no shader and a counter nothing wrote reads as the
    /// zero it was created with. The counters are device-local now, so nothing
    /// on the host writes them and [`Recorder::buffer_bytes`] holds nothing for
    /// them: the zero is a dispatch inside the frame, and a backend that runs no
    /// shader cannot observe it at all.
    ///
    /// So the poison moved to where a shader actually runs — `crcbl-vk`'s
    /// `draw_gen` end-to-end fills them with a sentinel and reads back the
    /// generated arguments — and what is checkable *here* is the schedule: that
    /// the frame contains the clearing pass, that it writes exactly those
    /// buffers, and that the graph orders the two accumulating passes after it.
    /// Each of the counts below goes red if a buffer is dropped from the
    /// clearing pass, and the pass list goes red if the pass leaves the frame.
    #[test]
    fn every_frame_starts_from_zeroed_counters() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device.as_ref());
        // Twice round the ring, because a slot cleared only on its first use is
        // exactly the failure this is about.
        for round in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 48),
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            let pool = crate::TransientPool::new();
            renderer.add_passes(&mut graph, &pool, target, (64, 48));
            let compiled = graph.compile(&pool).expect("a legal frame");

            let clear = &compiled.passes()[0];
            assert_eq!(
                clear.label(),
                "clear-counters",
                "round {round}: the zero has to be written before anything adds to it"
            );
            let barriers = &clear.barriers().buffers;
            assert_eq!(
                barriers.len(),
                3,
                "round {round}: the survivor count, the draw arguments, and the buffer \
                 holding both the draw counts and the mesh-dispatch arguments, and \
                 nothing else: {barriers:?}"
            );
            assert!(
                barriers
                    .iter()
                    .all(|barrier| barrier.to == ResourceState::ShaderReadWrite),
                "round {round}: every one of them is written by this pass: {barriers:?}"
            );
            // The ones the frame leaves as indirect arguments are the two a
            // driver reads; the survivor count rests in a shader read. That
            // split is what names them without a handle to compare.
            assert_eq!(
                barriers
                    .iter()
                    .filter(|barrier| barrier.from == ResourceState::IndirectArgument)
                    .count(),
                2,
                "round {round}: out of the state a draw read them in: {barriers:?}"
            );

            let after = |label: &str| {
                compiled
                    .passes()
                    .iter()
                    .find(|pass| pass.label() == label)
                    .unwrap_or_else(|| panic!("round {round}: no `{label}` pass"))
                    .barriers()
                    .buffers
                    .iter()
                    .filter(|barrier| {
                        barrier.from == ResourceState::ShaderReadWrite
                            && barrier.to == ResourceState::ShaderReadWrite
                    })
                    .count()
            };
            assert_eq!(
                after("cull"),
                1,
                "round {round}: the survivor count reaches the cull pass's atomic behind a \
                 barrier on the write that zeroed it"
            );
            assert_eq!(
                after("draw-args"),
                4,
                "round {round}: and so do the draw arguments and the buffer holding the \
                 draw counts beside the mesh-dispatch arguments — plus the survivor list, \
                 which the cull pass has just written and this pass reads through the same \
                 descriptor it scatters the runs into, and `docs/plan/25-lod.md`'s \
                 hysteresis state, which the clearing pass does *not* zero and which is \
                 behind the same kind of barrier for the opposite reason: it is the one \
                 buffer here carrying a value out of the previous frame, so what it needs \
                 ordering against is that frame rather than this one's clear"
            );

            // The compiled graph borrows the renderer's pass bodies, so it has
            // to go before the next frame borrows the renderer again.
            drop(compiled);
        }
        renderer.destroy(device.as_ref());
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The graph transitions the generated arguments into
    /// [`ResourceState::IndirectArgument`] before the draws read them**, and the
    /// runs into a shader read.
    ///
    /// The seam calls this the single most important barrier in a GPU-driven
    /// frame and says its absence produces "sometimes nothing draws" — which is
    /// exactly the failure a golden image on one driver would not catch. It is
    /// asserted on the compiled graph, which is pure and needs no device.
    #[test]
    fn the_graph_barriers_the_arguments_into_place_before_the_draws() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        renderer
            .begin_frame(
                device.as_ref(),
                &Camera::default(),
                &DirectionalLight::default(),
                (64, 48),
            )
            .expect("write");

        let imported = swapchain_image(device.as_ref());
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        let pool = crate::TransientPool::new();
        renderer.add_passes(&mut graph, &pool, target, (64, 48));
        let compiled = graph.compile(&pool).expect("a legal frame");

        let passes: Vec<String> = compiled
            .passes()
            .iter()
            .map(|pass| pass.label().to_string())
            .collect();
        // The camera's compute triple and topic 18's clustering dispatch, then
        // one triple per shadow cascade, then the depth-only pass they feed and
        // the colour pass that samples it.
        //
        // **The clustering pass is after the camera's clearing dispatch**, and
        // that is the ordering the overflow counter depends on: the clearing
        // pass zeroes the statistics word this one adds to, and both declare
        // the same buffer, so the graph is what puts the barrier between them.
        let mut expected: Vec<String> = Vec::new();
        for cascade in 0..=shadow::CASCADES {
            expected.extend(
                ["clear-counters", "cull", "draw-args"]
                    .into_iter()
                    .map(str::to_string),
            );
            if cascade == 0 {
                expected.push("light-cluster".to_string());
            }
        }
        // And the culling-statistics copy **last**, after every pass that adds
        // to the buffer it reads — the cull dispatch, the clustering pass and
        // the amplification stage inside the colour pass.
        expected.extend(
            [
                "shadow",
                "depth-prepass",
                "ssao",
                "ssao-blur",
                // The reconstruction that ends the occlusion chain, between the
                // blur it reads and the pass that binds what it wrote — see
                // [`crate::ssao`]'s header on why the first two run smaller than
                // the frame and this one does not.
                "ssao-upsample",
                "forward",
                // The pyramid the march climbs, between the pass that wrote
                // level 0 and the pass that reads the chain. Two levels at this
                // extent, written out for the reason `crate::bloom`'s chain is
                // in the toggle test below: a table generated from
                // `crate::hiz::levels_for` could not fail.
                "hiz-1",
                "hiz-2",
                "ssr",
                "ssr-blur",
                "tonemap",
                "fxaa",
                "cull-stats-readback",
            ]
            .into_iter()
            .map(str::to_string),
        );
        assert_eq!(
            passes, expected,
            "each cull's three compute passes come first, and in that order"
        );

        // **The depth prepass, not the colour pass**, and that is what says the
        // prepass really is the first thing to draw from the camera's cull: it is
        // where the draw buffers leave the compute pass's writes. The colour pass
        // below then needs none of these, because they are already in the state it
        // wants — which is the assertion after it.
        let prepass = compiled
            .passes()
            .iter()
            .find(|pass| pass.label() == "depth-prepass")
            .expect("the pass list above");
        let into_indirect = prepass
            .barriers()
            .buffers
            .iter()
            .filter(|barrier| barrier.to == ResourceState::IndirectArgument)
            .count();
        assert_eq!(
            into_indirect,
            2,
            "the arguments and the count, out of the compute pass's writes and into \
             the state a draw reads them in: {:?}",
            prepass.barriers().buffers
        );
        assert!(
            prepass.barriers().buffers.iter().any(|barrier| {
                barrier.from == ResourceState::ShaderReadWrite
                    && barrier.to == ResourceState::ShaderRead
            }),
            "and the runs, which the vertex stage reads: {:?}",
            prepass.barriers().buffers
        );

        let forward = compiled
            .passes()
            .iter()
            .find(|pass| pass.label() == "forward")
            .expect("the pass list above");
        assert!(
            forward
                .barriers()
                .buffers
                .iter()
                .all(|barrier| barrier.to != ResourceState::IndirectArgument),
            "the prepass draws from the same arguments, so the colour pass finds them \
             already in the state it wants — a barrier here would mean something between \
             the two put them back: {:?}",
            forward.barriers().buffers
        );
        // What the colour pass *does* still have to be given: the froxel grid,
        // which no earlier pass reads.
        assert!(
            forward.barriers().buffers.iter().any(|barrier| {
                barrier.from == ResourceState::ShaderReadWrite
                    && barrier.to == ResourceState::ShaderRead
            }),
            "and the froxel grid, which its fragment stage reads: {:?}",
            forward.barriers().buffers
        );

        let draw_args = compiled
            .passes()
            .iter()
            .find(|pass| pass.label() == "draw-args")
            .expect("the pass list above");
        assert!(
            draw_args.barriers().buffers.iter().any(|barrier| {
                barrier.from == ResourceState::ShaderReadWrite
                    && barrier.to == ResourceState::ShaderRead
            }),
            "and the visible list is ordered after the cull pass wrote it: {:?}",
            draw_args.barriers().buffers
        );

        // The compiled graph borrows the renderer's pass bodies, so it has to go
        // before the renderer does.
        drop(compiled);
        renderer.destroy(device.as_ref());
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// One more frame than the uniform ring is deep, so at least one lap both
    /// reuses a slot and follows a lap that already sampled the atlas.
    const SHADOW_LAPS: usize = FRAMES_IN_FLIGHT + 1;

    /// **Every frame's first barrier on the shadow atlas names what the previous
    /// frame left it in**, and the same for its placeholder.
    ///
    /// # Why this cannot be read off one frame
    ///
    /// The atlas is neither a transient nor a swapchain image. The pool never
    /// hands one out and has no description to size it from, and no acquire
    /// semaphore sits between one frame's use of it and the next — so
    /// [`ForwardRenderer::add_passes`] *declares* it, and the declaration is what
    /// makes the transition true.
    ///
    /// What it declares is [`imported_state`], the pool's record of what the
    /// last graph to *execute* left the image in, so [`InitialClaim::Tracked`]
    /// can only ever find the two agreeing for this one import — deliberately,
    /// because a declaration and an audit reading one value cannot drift apart
    /// the way a renderer-held copy of it could. The audit still has teeth
    /// elsewhere in this frame: `ssao-placeholder` and `probes` declare a
    /// constant, and `the_guard_still_catches_an_engine_import_that_lies` is
    /// what shows it fires on those.
    ///
    /// So neither mechanism says anything about the barriers on either side of
    /// the declaration — whether the frame that ran actually reached the state
    /// the ledger now claims, and whether the next frame's first barrier comes
    /// out of it — which is what the stream below is for.
    ///
    /// Declaring [`ResourceState::Undefined`] on a frame after the first is the
    /// failure this catches. It expands to `srcStageMask = NONE,
    /// srcAccessMask = NONE` (`crcbl_vk::conv::state_masks`), so the transition
    /// into the depth-only pass's attachment state orders against nothing — while
    /// the previous frame's colour pass is still sampling the very same image.
    /// Vulkan reports it as `SYNC-HAZARD-WRITE-AFTER-READ` with
    /// `read_barriers: 0`; every other backend resolves it however the driver
    /// likes.
    ///
    /// # Why the command stream is the observable
    ///
    /// Nothing above the seam can see a wrong declaration: the frame still
    /// renders, the golden still matches, and no state is ever read back. So this
    /// replays the recorded stream with a tracker, which is the same check a
    /// validation layer performs and needs no GPU at all — the shape
    /// `crcbl::screenshot`'s
    /// `every_readback_barrier_declares_the_state_the_image_is_actually_in` uses.
    /// One difference: `Undefined` is **not** waved through after the first
    /// touch. It is true for a swapchain image because the acquire's semaphore
    /// makes it true, and there is no such semaphore here.
    #[test]
    fn the_shadow_atlas_enters_each_frame_in_the_state_the_last_one_left_it() {
        use crcbl_hal::null::Command;

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let atlas = renderer.shadow_atlas();
        let placeholder = renderer.shadow_placeholder;
        let mut pool = crate::TransientPool::new();
        let imported = swapchain_image(device);
        let mut recorded = Vec::with_capacity(SHADOW_LAPS);

        for lap in 0..SHADOW_LAPS {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    // **A different sun each lap**, so every lap redraws the
                    // atlas: this is a test about the barrier a frame that
                    // writes the image declares, and a lap that cached would
                    // record no pass and no transition at all. See
                    // [`moving_sun`].
                    &moving_sun(u32::try_from(lap).expect("a handful of laps")),
                    (64, 48),
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, &pool, target, (64, 48));
            let compiled = graph.compile(&pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("shadow lap"),
                queue,
            });
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("the graph executed");
            recorded.push(encoder.finish().expect("recording succeeded"));
        }

        // The two images the renderer owns across frames. A transient's state is
        // the pool's business and the fake swapchain image's is the acquire's, so
        // neither belongs in here.
        let mut tracked = [
            (atlas, ResourceState::Undefined),
            (placeholder, ResourceState::Undefined),
        ];
        let mut atlas_writes = 0usize;
        let mut named = 0usize;
        for command in recorder.commands() {
            let Command::Barrier { images, .. } = command else {
                continue;
            };
            for barrier in images {
                let Some((_, state)) = tracked
                    .iter_mut()
                    .find(|(handle, _)| *handle == barrier.image)
                else {
                    continue;
                };
                assert_eq!(
                    barrier.from, *state,
                    "a barrier declared {:?} -> {:?} on an image the frame before it left in \
                     {:?}. `Undefined` carries no source scope, so a transition declaring it \
                     orders against nothing and the previous frame's sampled read is still in \
                     flight underneath it.",
                    barrier.from, barrier.to, *state
                );
                if barrier.image == atlas && barrier.to == ResourceState::DepthStencilWrite {
                    atlas_writes += 1;
                }
                *state = barrier.to;
                named += 1;
            }
        }

        // Both halves of the loop above are silent on an empty stream, so the
        // shape it walked is asserted rather than assumed: one transition into
        // the depth-only pass's attachment state per lap, the last one included.
        assert_eq!(
            atlas_writes, SHADOW_LAPS,
            "{named} barrier(s) named the atlas or its placeholder across {SHADOW_LAPS} laps, \
             {atlas_writes} of them taking the atlas into DepthStencilWrite — every lap moved \
             the sun, so every lap redraws the atlas, and anything else means the loop above \
             asserted on a stream that does not contain the transition it is about"
        );
        let (_, ended) = tracked[0];
        assert_eq!(
            ended,
            ResourceState::ShaderRead,
            "the import promises to hand the atlas back in the state the next frame declares"
        );

        renderer.destroy(device);
        for commands in recorded {
            device.destroy_command_buffer(commands);
        }
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// A renderer with a shadowed spot beside the sun's cascades and a caster
    /// for both to draw, which is the smallest scene the atlas cache has
    /// anything to say about: without a light tile the only maps are the
    /// cascades, and without a caster every tile is a clear.
    fn shadow_cache_scene(
        device: &dyn Device,
        queue: QueueHandle,
    ) -> (ForwardRenderer, InstanceHandle) {
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        // Pinned, on `set_shadow_cadence`'s own terms: the console's two limits
        // are process-global and `the_console_moves_both_limits` holds them at
        // a four-frame hold and three faces a frame for the length of its
        // check. A cache test reading the console inside that window reports
        // "did not settle" — seen in the wild on 2026-09-04, and reproduced by
        // holding both limits at those values inside the arm's own process,
        // which the pin makes harmless.
        renderer.set_shadow_cadence(Some(shadow::Cadence::EVERY_FRAME));
        let cube = place_cube(&mut renderer, Mat4::IDENTITY);
        renderer.set_lights(&[shadowable_spot(-1.0)]);
        (renderer, cube)
    }

    /// Draws one frame and answers whether it drew the shadow atlas, **read two
    /// ways that have to agree**.
    ///
    /// [`ForwardRenderer::shadow_atlas_cached`] is the renderer's own claim and
    /// the recorded stream is what the device was actually told; either on its
    /// own would be satisfied by a renderer that reported one thing and did the
    /// other, which is precisely the failure a cache has. `seen` is the cursor
    /// into the cumulative recorder, so each call reads its own frame.
    fn drew_the_atlas(
        recorder: &Recorder,
        seen: &mut usize,
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        queue: QueueHandle,
        camera: &Camera,
        sun: &DirectionalLight,
    ) -> bool {
        let drawn = frame_seen_from(device, renderer, queue, camera, sun);
        let claimed = renderer.shadow_atlas_cached();
        let commands = recorder.commands();
        let opened = commands[*seen..].iter().any(|command| {
            command
                .opens_pass()
                .is_some_and(|(_, label)| label == Some("shadow"))
        });
        *seen = commands.len();
        drawn.release(device);
        assert_eq!(
            opened,
            !claimed,
            "the renderer reports cached = {claimed} and its frame {} a pass labelled `shadow`",
            if opened { "opened" } else { "opened no" }
        );
        opened
    }

    /// **A frame over a scene nothing moved in does not draw the shadow atlas
    /// again**, and what it saves is the whole of the atlas's work.
    ///
    /// `docs/plan/45-shadows.md`'s static-caching rung, and the observable it
    /// otherwise has none for: the map a cached frame samples is byte for byte
    /// the map the frame before it sampled, so no golden image and no readback
    /// can tell the two frames apart. The recorded command stream can — and the
    /// dispatch count is the half that says the *culls* went with the pass,
    /// rather than the frame still paying for the draws it decided not to
    /// record.
    #[test]
    fn a_frame_over_a_still_scene_does_not_draw_the_shadow_atlas_again() {
        use crcbl_hal::null::Command;

        // The blur count is a process-global console variable, and the frame's
        // draw count below includes the full-screen triangle each blur records
        // — so a concurrent test holding `r_ssao_blur_passes` at two would move
        // it under this one. Held rather than merely read, which is the
        // convention that variable's own test set.
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let camera = Camera::default();
        let sun = DirectionalLight::default();

        let mut seen = 0usize;
        let dispatches = |recorder: &Recorder, from: usize| {
            recorder.commands()[from..]
                .iter()
                .filter(|command| matches!(command, Command::Dispatch { .. }))
                .count()
        };

        let at = seen;
        assert!(
            drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "the first frame of all has to draw the atlas: nothing has put anything in it"
        );
        let first = dispatches(&recorder, at);
        let drawing_draws = renderer.counters().draws;
        let buckets = renderer.bucket_constants.len() as u64;
        assert!(
            renderer.shadow_lights().base_of(0).is_some(),
            "the spot must hold a run for the count below to include a light slot's cull"
        );
        // The camera's own triple and topic 18's clustering dispatch, plus a
        // triple per cascade and one for the spot's slot.
        assert_eq!(
            first,
            DrawGen::MAX_PASSES as usize * (2 + shadow::CASCADES) + 1,
            "the drawing frame did not record the culls this test is about skipping"
        );

        for round in 0..3 {
            let at = seen;
            assert!(
                !drew_the_atlas(
                    &recorder,
                    &mut seen,
                    device,
                    &mut renderer,
                    queue,
                    &camera,
                    &sun
                ),
                "round {round} redrew an atlas nothing had changed"
            );
            assert_eq!(
                dispatches(&recorder, at),
                DrawGen::MAX_PASSES as usize + 1,
                "round {round} kept a shadow cull it had no pass to feed"
            );
            assert_eq!(
                drawing_draws - renderer.counters().draws,
                (shadow::CASCADES + 1) as u64 * buckets,
                "round {round}: the frame's own draw count did not lose exactly the atlas's \
                 calls — one per bucket per cascade, and one per bucket for the spot's tile"
            );
        }

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// A cadence that holds every map one rung down for a frame, with the whole
    /// atlas's worth of tiles to spend.
    ///
    /// Pinned through [`ForwardRenderer::set_shadow_cadence`] rather than set on
    /// the console: [`shadow::r_shadow_cadence`] is process-global and the
    /// shadow pass is in every frame's pass list, so a test that moved it would
    /// change the frame every other test in this module draws.
    const HOLD_TWO: shadow::Cadence = shadow::Cadence {
        hold: 2,
        faces: shadow::TILES,
    };

    /// How the shadow pass loaded its depth attachment, over the commands from
    /// `from` on, and [`None`] where the frame recorded no such pass.
    ///
    /// **The observable the tile clear otherwise has none for.** A frame that
    /// kept a tile and a frame that redrew every one of them sample maps that
    /// may be identical, so nothing in the picture separates them; this is the
    /// declaration that says which the device was told.
    fn shadow_pass_depth_load(recorder: &Recorder, from: usize) -> Option<LoadOp> {
        use crcbl_hal::null::Command;

        recorder.commands()[from..].iter().find_map(|command| {
            let Command::BeginRenderPass {
                label,
                depth_stencil_attachment,
                ..
            } = command
            else {
                return None;
            };
            if label.as_deref() != Some("shadow") {
                return None;
            }
            depth_stencil_attachment
                .as_ref()
                .map(|attachment| attachment.depth_load)
        })
    }

    /// Direct draws recorded inside the shadow pass, over the commands from
    /// `from` on — which is one per tile the pass reset itself, the atlas's
    /// geometry being drawn indirectly.
    fn tile_clears_in_the_shadow_pass(recorder: &Recorder, from: usize) -> usize {
        use crcbl_hal::null::Command;

        let mut inside = false;
        let mut clears = 0;
        for command in &recorder.commands()[from..] {
            if let Some((_, label)) = command.opens_pass() {
                inside = label == Some("shadow");
                continue;
            }
            if matches!(command, Command::EndRenderPass | Command::EndComputePass) {
                inside = false;
                continue;
            }
            if inside && matches!(command, Command::Draw { .. }) {
                clears += 1;
            }
        }
        clears
    }

    /// **The far cascade is redrawn every second frame and the near one every
    /// frame**, on a scene where the eye moves and so every map is out of date.
    ///
    /// `docs/plan/45-shadows.md`'s cadence rung, and the observable it otherwise
    /// has none for: a held map draws a shadow one frame behind its caster,
    /// which is a plausible picture and one a golden accepts.
    ///
    /// The eye moves by a fraction of a cascade's own reach, so no frame is a
    /// reset — that is asserted rather than assumed, because a reset would make
    /// every frame redraw everything and this test would pass by having
    /// switched the cadence off.
    #[test]
    fn the_far_cascade_is_held_on_the_frames_between_its_turns() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let sun = DirectionalLight::default();
        renderer.set_shadow_cadence(Some(HOLD_TWO));

        const FRAMES: usize = 8;
        let mut near = 0;
        let mut far = 0;
        for step in 0..FRAMES {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a handful of frames, and the step is what is wanted"
            )]
            let camera = Camera {
                eye: Camera::default().eye + Vec3::X * 0.05 * step as f32,
                ..Camera::default()
            };
            let drawn = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
            assert_eq!(
                renderer.shadow_cadence_reset(),
                step == 0,
                "frame {step}: only the first frame has an atlas to lay out, and a reset \
                 after it would redraw everything and leave nothing for the cadence to hold"
            );
            near += usize::from(renderer.shadow_cascade_redrawn(0));
            far += usize::from(renderer.shadow_cascade_redrawn(1));
            drawn.release(device);
        }
        assert_eq!(
            near, FRAMES,
            "the near cascade is fitted to the eye and is on the top rung: every frame"
        );
        assert_eq!(
            far,
            FRAMES / 2,
            "the far cascade is one rung down, so every second frame"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **A frame that keeps a tile loads the attachment and resets the tiles it
    /// redraws itself**, and a frame that keeps none clears it — which is what
    /// shipped.
    ///
    /// The half of the cadence a redraw count cannot see: holding a tile is only
    /// worth anything if the tile survives, and the one clear this seam has
    /// covers the whole image. A frame that loaded and reset nothing would draw
    /// into tiles still holding the last frame's depths, and under reversed-Z a
    /// caster that moved away would go on occluding.
    #[test]
    fn a_frame_that_keeps_a_tile_loads_the_attachment_and_resets_the_rest() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let sun = DirectionalLight::default();

        // The first frame lays the atlas out, so it clears and redraws
        // everything whatever the console says.
        let first = frame_seen_from(device, &mut renderer, queue, &Camera::default(), &sun);
        first.release(device);
        assert_eq!(
            shadow_pass_depth_load(&recorder, 0),
            Some(LoadOp::Clear),
            "the frame that lays the atlas out has nothing to keep"
        );
        assert_eq!(
            tile_clears_in_the_shadow_pass(&recorder, 0),
            0,
            "and so resets no tile of its own"
        );

        renderer.set_shadow_cadence(Some(HOLD_TWO));
        let mut seen = recorder.commands().len();
        let mut loaded = 0;
        for step in 1..4 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a handful of frames, and the step is what is wanted"
            )]
            let camera = Camera {
                eye: Camera::default().eye + Vec3::X * 0.05 * step as f32,
                ..Camera::default()
            };
            let drawn = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
            let load = shadow_pass_depth_load(&recorder, seen).expect("a shadow pass");
            let clears = tile_clears_in_the_shadow_pass(&recorder, seen);
            let faces = renderer.shadow_faces_redrawn() as usize;
            if load == LoadOp::Load {
                loaded += 1;
                assert_eq!(
                    clears, faces,
                    "frame {step} loaded the attachment and reset {clears} of the {faces} \
                     tiles it redrew"
                );
                assert!(
                    faces < shadow::TILES,
                    "frame {step} loaded an attachment while redrawing every tile"
                );
            } else {
                assert_eq!(
                    clears, 0,
                    "frame {step} cleared the attachment and reset {clears} tiles again"
                );
            }
            seen = recorder.commands().len();
            drawn.release(device);
        }
        assert!(
            loaded > 0,
            "no frame kept a tile, so nothing here is about the load path"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// A point light costs the budget every face it draws, not one.
    ///
    /// **The half of the budget its name is about.** `shadow_faces_redrawn` is
    /// a sum over the groups a frame redrew of the maps each of them owns, and
    /// a point light owns [`shadow::POINT_FACES`] of them. Every other check
    /// here is built on the sun and a spot, which own one map each — so a face
    /// count that answered `1` for every group would satisfy all of them, and
    /// did: collapsing it passed the whole of `crcbl-render` and the mesh e2e
    /// suite. This is what binds on the difference.
    #[test]
    fn a_point_light_costs_the_budget_every_face_it_draws() {
        let _blurs = ssao_blur_switch();
        let (_recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let _cube = place_cube(&mut renderer, Mat4::IDENTITY);
        renderer.set_lights(&[shadowable_point(-1.0)]);
        let sun = DirectionalLight::default();

        // Every group is forced on the first frame — the image has held none of
        // them — so this counts what a full redraw actually costs.
        renderer.set_shadow_cadence(Some(shadow::Cadence::EVERY_FRAME));
        let first = frame_seen_from(device, &mut renderer, queue, &Camera::default(), &sun);
        first.release(device);
        let point_faces = u32::try_from(shadow::POINT_FACES).expect("a handful of faces");
        assert!(
            renderer.shadow_lights().base_of(0).is_some(),
            "the point must hold a run of tiles, or there is no multi-face group to count"
        );
        assert!(
            renderer.shadow_faces_redrawn() >= point_faces,
            "a frame that redrew a point light's whole cube reported {} faces, fewer than the \
             {point_faces} it owns — so the budget is counting groups rather than the maps they \
             draw, and a cube would slip past a budget of one",
            renderer.shadow_faces_redrawn()
        );
    }

    /// **A frame never redraws more tiles than the budget allows**, and every
    /// map still gets its turn.
    ///
    /// The starvation half is what makes this more than an inequality: a
    /// schedule that spent the budget on the same maps every frame would satisfy
    /// the bound and never draw the rest at all.
    #[test]
    fn the_face_budget_bounds_a_frame_and_starves_no_map() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let sun = DirectionalLight::default();

        let first = frame_seen_from(device, &mut renderer, queue, &Camera::default(), &sun);
        first.release(device);
        assert!(
            renderer.shadow_lights().base_of(0).is_some(),
            "the spot must hold a run, or the budget has only the cascades to bind on"
        );

        // One tile a frame: the two cascades and the spot's map cannot all fit,
        // so something is held on every frame of the run below.
        const BUDGET: u32 = 1;
        renderer.set_shadow_cadence(Some(shadow::Cadence {
            hold: 1,
            faces: BUDGET as usize,
        }));
        const FRAMES: usize = 12;
        let mut drew = [0usize; 3];
        for step in 1..=FRAMES {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a handful of frames, and the step is what is wanted"
            )]
            let camera = Camera {
                eye: Camera::default().eye + Vec3::X * 0.02 * step as f32,
                ..Camera::default()
            };
            let drawn = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
            assert!(
                renderer.shadow_faces_redrawn() <= BUDGET,
                "frame {step} redrew {} tiles against a budget of {BUDGET}",
                renderer.shadow_faces_redrawn()
            );
            drew[0] += usize::from(renderer.shadow_cascade_redrawn(0));
            drew[1] += usize::from(renderer.shadow_cascade_redrawn(1));
            drew[2] += usize::from(renderer.shadow_slot_redrawn(0));
            drawn.release(device);
        }
        for (map, count) in drew.iter().enumerate() {
            assert!(
                *count > 0,
                "map {map} was never redrawn in {FRAMES} frames under a budget of \
                 {BUDGET}: {drew:?}"
            );
        }

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **A light that jumps further than its own radius resets the cadence**,
    /// and one that drifts inside it does not.
    ///
    /// `docs/plan/45-shadows.md`'s "a moving light or camera cut resets it",
    /// made precise: a map whose centre has moved past the map's own reach is
    /// not a frame out of date, it is about somewhere else — so it is redrawn on
    /// the frame that notices rather than when its turn comes round. Both arms,
    /// because a reset that fired on every move would be the cadence switched
    /// off.
    #[test]
    fn a_light_that_jumps_past_its_own_radius_resets_the_cadence() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let sun = DirectionalLight::default();
        let camera = Camera::default();
        // The coarsest hold there is, so a redraw of the spot's map on the
        // frames below is the reset and not its turn.
        renderer.set_shadow_cadence(Some(shadow::Cadence {
            hold: 8,
            faces: shadow::TILES,
        }));

        let mut settle = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
        settle.release(device);
        for _ in 0..2 {
            settle = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
            settle.release(device);
        }
        assert!(
            !renderer.shadow_cadence_reset(),
            "the scene has settled, so nothing is about somewhere else"
        );

        // A drift well inside the light's own radius: out of date, not moved.
        renderer.set_lights(&[shadowable_spot(-1.0 + 0.5)]);
        let drifted = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
        drifted.release(device);
        assert!(
            !renderer.shadow_cadence_reset(),
            "a light that moved a sixteenth of its radius is a map a frame out of date"
        );

        // And a jump past it. `shadowable_spot`'s radius is what the reach is
        // taken from, so this is a light standing where its own map says
        // nothing.
        renderer.set_lights(&[shadowable_spot(-1.0 + 40.0)]);
        let jumped = frame_seen_from(device, &mut renderer, queue, &camera, &sun);
        jumped.release(device);
        assert!(
            renderer.shadow_cadence_reset(),
            "a light that jumped five times its own radius must not hold its map"
        );
        assert!(
            renderer.shadow_slot_redrawn(0),
            "and the reset is only a reset if the map is actually redrawn"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **A console knob about *sampling* the atlas does not redraw it.**
    ///
    /// `r_shadow_filter` and `r_shadow_split` decide how the colour pass reads
    /// a map, and not one byte of what the depth pass writes into it. Yet the
    /// shadow views' blocks are the frame's block spread, and the cache record
    /// is those blocks' bytes — so until the views zeroed the row, a person
    /// typing `r_shadow_filter box` redrew every map in the atlas for nothing,
    /// and in this crate's own test process the filter tests moving that cell
    /// redrew the atlas under `each_thing_the_atlas_is_drawn_from_redraws_it`
    /// once in every five or six workspace runs on 2026-09-04. Holding the lock
    /// here is what makes the move below the only one in flight.
    #[test]
    fn a_moved_shadow_filter_leaves_a_still_atlas_alone() {
        let switch = shadow_filter_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let camera = Camera::default();
        let sun = DirectionalLight::default();
        let mut seen = 0usize;
        assert!(
            drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "the first frame of all draws the atlas"
        );
        assert!(
            !drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "a still second frame has to hold it, or the move below proves nothing"
        );

        switch.filter(shadow::Filter::Box);
        switch.seam(0.5);
        assert!(
            !drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "`r_shadow_filter` and `r_shadow_split` moved and the atlas was drawn again: a \
             sampling knob reached the record of what the maps were drawn from"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **Every input the atlas is drawn from redraws it**, one at a time.
    ///
    /// The half that stops "cached everything, for ever" from passing every
    /// assertion above. Each arm moves exactly one thing and asserts the next
    /// frame draws again — and then holds still and asserts it stops, so an arm
    /// cannot pass by having broken the cache outright.
    ///
    /// The camera is in here because it is not obvious: a shadow map is drawn
    /// from a light, not from the eye. But a cascade is fitted to the camera's
    /// own frustum, and every shadow cull selects detail at the camera's pixels
    /// — `SHADOW_LOD_BIAS` — so a moved eye is a different atlas.
    ///
    /// `settles` is how many still frames it takes to go quiet again, and it is
    /// two for exactly one arm: `InstancePool`'s carry-forward puts a moved
    /// instance's `previous_transform` back at rest on the frame *after* the
    /// move, and that is a write, which is a change this record cannot tell
    /// from any other. See `InstancePool::revision`, which says so.
    #[test]
    fn each_thing_the_atlas_is_drawn_from_redraws_it() {
        let camera = Camera::default();
        let sun = DirectionalLight::default();
        let moved_camera = Camera {
            eye: camera.eye + Vec3::X * 4.0,
            ..camera
        };
        type Change = fn(&mut ForwardRenderer, InstanceHandle);
        let arms: [(&str, Change, &Camera, &DirectionalLight, usize); 4] = [
            (
                "a caster moved",
                |renderer, cube| {
                    renderer.set_instance(
                        cube,
                        &InstanceDesc {
                            mesh: DEMO_CUBE,
                            material: DEMO_UNTINTED,
                            transform: Mat4::from_translation(Vec3::X),
                        },
                    );
                },
                &camera,
                &sun,
                2,
            ),
            (
                "a caster removed",
                |renderer, cube| renderer.remove_instance(cube),
                &camera,
                &sun,
                1,
            ),
            (
                "the light moved",
                |renderer, _| renderer.set_lights(&[shadowable_spot(1.5)]),
                &camera,
                &sun,
                1,
            ),
            ("the camera moved", |_, _| {}, &moved_camera, &sun, 1),
        ];

        for (what, change, camera, sun, settles) in arms {
            let (recorder, device, queue) = open();
            let device = device.as_ref();
            let (mut renderer, cube) = shadow_cache_scene(device, queue);
            let mut seen = 0usize;
            // Two frames, so the atlas is drawn and then held: the arm's own
            // assertion is about a frame that would otherwise have cached.
            for round in 0..2 {
                let drew = drew_the_atlas(
                    &recorder,
                    &mut seen,
                    device,
                    &mut renderer,
                    queue,
                    &Camera::default(),
                    &DirectionalLight::default(),
                );
                assert_eq!(
                    drew,
                    round == 0,
                    "{what}: the still frames before the change did not settle"
                );
            }

            change(&mut renderer, cube);
            assert!(
                drew_the_atlas(
                    &recorder,
                    &mut seen,
                    device,
                    &mut renderer,
                    queue,
                    camera,
                    sun
                ),
                "{what} and the atlas was not drawn again"
            );
            // And it goes quiet again in exactly `settles` frames, so the arm
            // cannot pass by having broken the cache rather than by having
            // invalidated it — and a change that started redrawing for ever
            // fails here rather than going unnoticed.
            for round in 0..settles {
                assert_eq!(
                    drew_the_atlas(
                        &recorder,
                        &mut seen,
                        device,
                        &mut renderer,
                        queue,
                        camera,
                        sun
                    ),
                    round + 1 < settles,
                    "{what}: the still frames after the change did not settle in {settles}"
                );
            }

            renderer.destroy(device);
            recorder.assert_valid();
        }
    }

    /// **A frame with a skinned object in it always redraws the atlas**, and
    /// that is a limit written down rather than a gap.
    ///
    /// A skinned caster's vertices are produced by a compute dispatch into the
    /// pool, so there is nothing on the host to compare: the palette a caller
    /// hands over is not the geometry, and the geometry is never read back. The
    /// honest answer is to redraw, and the dishonest one is a map of a pose the
    /// character has left.
    #[test]
    fn a_frame_with_a_skinned_object_never_caches_the_atlas() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        fixture.renderer.set_lights(&[shadowable_spot(-1.0)]);
        let imported = swapchain_image_at(device, TEST_EXTENT);
        let mut pool = crate::TransientPool::new();

        // The same palette and the same plan every lap, so nothing a host-side
        // comparison could see has moved.
        for lap in 0..3 {
            fixture.begin(device);
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            fixture.renderer.add_skinned_passes(
                &mut graph,
                &pool,
                target,
                TEST_EXTENT,
                &fixture.skinning,
            );
            let compiled = graph.compile(&pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("skinned lap"),
                queue,
            });
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("the graph executed");
            device.destroy_command_buffer(encoder.finish().expect("recording succeeded"));
            assert!(
                !fixture.renderer.shadow_atlas_cached(),
                "lap {lap} held a shadow map of a pose no host-side comparison can vouch for"
            );
        }

        fixture.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **Switching the shadows off clears the atlas once, and then holds the
    /// clear**, which is the same cache answering a frame that draws nothing.
    ///
    /// The switched-off frame is not itself a cached frame: the pass has to run
    /// to write the reversed-Z clear that makes every comparison come back lit,
    /// and the image holds the last drawn maps until it does. What *is* cached
    /// is every frame after it, and the record for a switched-off frame is
    /// deliberately blind to the camera and the casters, because nothing about
    /// them reaches an atlas nothing draws into.
    #[test]
    fn switching_the_shadows_off_clears_the_atlas_once_and_then_holds_it() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, cube) = shadow_cache_scene(device, queue);
        let camera = Camera::default();
        let sun = DirectionalLight::default();
        let mut seen = 0usize;

        assert!(
            drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "the first frame draws the maps"
        );
        renderer.set_effect_request(EffectRequest {
            programmatic: EffectOverride::none().force(RenderEffects::SHADOWS, Some(false)),
            ..EffectRequest::default()
        });
        assert!(
            drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "the frame that switched the shadows off has to run the pass that clears the maps \
             out of the image"
        );
        assert!(
            !drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "a second switched-off frame cleared an atlas that was already clear"
        );
        // And nothing about the scene reaches a frame that draws no map: the
        // caster moves and the atlas is still the clear it was.
        renderer.set_instance(
            cube,
            &InstanceDesc {
                mesh: DEMO_CUBE,
                material: DEMO_UNTINTED,
                transform: Mat4::from_translation(Vec3::X),
            },
        );
        assert!(
            !drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "a caster moved with the shadows off, and the frame cleared the atlas over again"
        );

        renderer.set_effect_request(EffectRequest::default());
        assert!(
            drew_the_atlas(
                &recorder,
                &mut seen,
                device,
                &mut renderer,
                queue,
                &camera,
                &sun
            ),
            "switching the shadows back on has to draw the maps the clear replaced"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **A frame the graph refuses leaves the shadow atlas where it was.**
    ///
    /// [`RenderGraph::compile`] returns a [`Result`], so a frame can be
    /// described in full and then thrown away, and
    /// [`CompiledGraph::execute`](crate::CompiledGraph::execute) — the only
    /// thing that writes the pool's ledger — never runs. That is the case a
    /// renderer-held `ResourceState`, stepped forward while the passes were
    /// declared, gets wrong: it would already say [`ResourceState::ShaderRead`]
    /// for an image no barrier has touched, and [`InitialClaim::Tracked`] cannot
    /// object, because with nothing recorded there is nothing to contradict. The
    /// guard would be silent on precisely the drift it exists to find, which is
    /// why [`imported_state`] reads the ledger instead of a second copy of it.
    ///
    /// The observable is the first barrier the *next* frame records on the
    /// atlas. `Undefined -> DepthStencilWrite` is the transition an image
    /// nothing has written needs; `ShaderRead -> DepthStencilWrite` names an old
    /// layout the image was never in, which is a layout mismatch on every
    /// backend and a discarded image on some.
    #[test]
    fn a_refused_frame_leaves_the_shadow_atlas_where_it_was() {
        use crcbl_hal::null::Command;

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let atlas = renderer.shadow_atlas();
        let placeholder = renderer.shadow_placeholder;
        let mut pool = crate::TransientPool::new();
        let imported = swapchain_image(device);

        // The refused frame: every pass a real frame declares, and beside them
        // one transient nothing reads or writes. `compile` answers
        // `UnusedTransient` and the whole description — the shadow atlas's
        // import included — is dropped without a command being recorded.
        renderer
            .begin_frame(
                device,
                &Camera::default(),
                &DirectionalLight::default(),
                (64, 48),
            )
            .expect("write");
        {
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, &pool, target, (64, 48));
            graph.create_image("orphan", TransientImageDesc::scene_color((64, 48)));
            let refused = graph
                .compile(&pool)
                .expect_err("a transient no pass declares");
            assert!(
                matches!(refused, crate::GraphError::UnusedTransient { .. }),
                "the frame has to be refused for the reason this test is about — a graph that \
                 compiled would go on to execute and the ledger would be written after all: \
                 {refused:?}"
            );
        }
        assert_eq!(
            pool.imported_image_use(atlas),
            None,
            "nothing executed, so the ledger has nothing to say about the atlas — this is the \
             emptiness `InitialClaim::Tracked` cannot check against"
        );

        // And the frame after it, which does run.
        renderer
            .begin_frame(
                device,
                &Camera::default(),
                &DirectionalLight::default(),
                (64, 48),
            )
            .expect("write");
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        renderer.add_passes(&mut graph, &pool, target, (64, 48));
        let compiled = graph.compile(&pool).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("frame after a refusal"),
            queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), None)
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");

        // The first barrier each of the two images gets. Only the refused frame
        // ran before this one and it recorded nothing, so these are the frame's
        // opening transitions.
        let mut opened: Vec<(crcbl_hal::ImageHandle, ResourceState, ResourceState)> = Vec::new();
        for command in recorder.commands() {
            let Command::Barrier { images, .. } = command else {
                continue;
            };
            for barrier in images {
                if barrier.image != atlas && barrier.image != placeholder {
                    continue;
                }
                if opened.iter().any(|(image, _, _)| *image == barrier.image) {
                    continue;
                }
                opened.push((barrier.image, barrier.from, barrier.to));
            }
        }
        for (image, from, to) in &opened {
            assert_eq!(
                *from,
                ResourceState::Undefined,
                "a barrier declared {from:?} -> {to:?} on an image the refused frame never gave a \
                 layout to. The declaration is what makes the transition true, so one that has \
                 moved on without an executed frame behind it names a source scope and an old \
                 layout the image was never in — and the ledger is empty, so nothing refuses it \
                 either. Image {image:?}"
            );
        }
        // The loop above is silent on an empty list and on a short one, and a
        // wrong declaration shortens it: an image declared in the state it is
        // wanted in needs no transition, so it drops out of the stream
        // altogether rather than appearing with the wrong source.
        assert_eq!(
            opened.len(),
            2,
            "the atlas and its placeholder are both imported every frame and both are wanted in a \
             state they are not in, so both have to be barriered: {opened:?}"
        );
        assert!(
            opened
                .iter()
                .any(|(image, _, to)| *image == atlas && *to == ResourceState::DepthStencilWrite),
            "and the atlas's is the shadow pass taking it as a depth attachment: {opened:?}"
        );

        renderer.destroy(device);
        device.destroy_command_buffer(commands);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The import audit still refuses an engine site whose declaration is
    /// wrong**, now that the shadow atlas's is read out of the same ledger it is
    /// checked against.
    ///
    /// That pair is deliberately circular and this test is what keeps it from
    /// swallowing the guard whole: `shadow-atlas` and `shadow-placeholder` say
    /// what [`imported_state`] just read, so [`InitialClaim::Tracked`] can only
    /// ever find them agreeing — their safety comes from there being one answer
    /// rather than from the check. Every *other* import in the frame declares a
    /// constant, and `ssao-placeholder` is the one this drives a ledger entry
    /// away from: uploaded into [`ResourceState::ShaderRead`] at build and
    /// declared so on every frame, so a graph that leaves it somewhere else is a
    /// declaration the next `compile` has to refuse.
    #[test]
    fn the_guard_still_catches_an_engine_import_that_lies() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let mut pool = crate::TransientPool::new();
        let imported = swapchain_image(device);
        let occlusion = renderer.ambient_occlusion_placeholder.image;

        // A graph of this test's own, which moves the renderer's occlusion
        // placeholder somewhere `add_passes` does not expect. Same image handle,
        // so the entry it leaves in the ledger is the one the next `compile`
        // reads for `ssao-placeholder`.
        let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("move the placeholder"),
            queue,
        });
        {
            let mut graph = crate::RenderGraph::new(queue);
            let moved = graph.import_image(
                "moved-placeholder",
                ImportedImage {
                    image: occlusion,
                    view: renderer.ambient_occlusion_placeholder.view,
                    format: Format::Rgba8Unorm,
                    extent: (1, 1),
                    initial: ResourceState::ShaderRead,
                    claim: InitialClaim::Tracked,
                    final_state: ResourceState::TransferSrc,
                },
            );
            graph
                .add_copy_pass("read the placeholder")
                .use_image(moved, ResourceState::TransferSrc)
                .execute(|_| {});
            graph
                .compile(&pool)
                .expect("a legal frame")
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("the graph executed");
        }
        let commands = encoder.finish().expect("recording succeeded");
        assert_eq!(
            pool.imported_image_use(occlusion),
            Some(ResourceState::TransferSrc),
            "the ledger has to have moved for the frame below to have anything to disagree with"
        );

        renderer
            .begin_frame(
                device,
                &Camera::default(),
                &DirectionalLight::default(),
                (64, 48),
            )
            .expect("write");
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        renderer.add_passes(&mut graph, &pool, target, (64, 48));
        let refused = graph
            .compile(&pool)
            .expect_err("`ssao-placeholder` declares a constant the ledger no longer agrees with");
        let crate::GraphError::ImportStateMismatch {
            resource,
            declared,
            left,
        } = &refused
        else {
            panic!("the audit has to name the import it refused, not merely fail: {refused:?}");
        };
        assert_eq!(resource, "ssao-placeholder");
        assert_eq!(*declared, ResourceState::ShaderRead);
        assert_eq!(*left, ResourceState::TransferSrc);

        renderer.destroy(device);
        device.destroy_command_buffer(commands);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// Compiles one frame and names the passes it turned out to be, in order.
    ///
    /// Compiled and not executed: what a pass costs is not the question here,
    /// only which of them the frame declared. Names rather than a count since
    /// the toggles landed — a count says a frame lost two passes and a list says
    /// *which* two, which is the difference between "the switch did something"
    /// and "the switch did the thing it is for".
    fn passes_in_a_frame(
        device: &dyn Device,
        queue: QueueHandle,
        renderer: &mut ForwardRenderer,
        imported: ImportedImage,
        pool: &crate::TransientPool,
        extent: (u32, u32),
    ) -> Vec<String> {
        renderer
            .begin_frame(
                device,
                &Camera::default(),
                &DirectionalLight::default(),
                extent,
            )
            .expect("write");
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        renderer.add_passes(&mut graph, pool, target, extent);
        graph
            .compile(pool)
            .expect("a legal frame")
            .passes()
            .iter()
            .map(|pass| pass.label().to_string())
            .collect()
    }

    /// A spot at `x` that [`shadow::Selection`] will give a tile: finite, a
    /// radius, a direction and a cone well inside the widest one allowed.
    /// A shadowed point light, which is [`shadow::POINT_FACES`] maps rather
    /// than one.
    ///
    /// The spot beside it is one tile and one face, so a scene built only from
    /// spots cannot tell a budget counting **faces** from one counting groups —
    /// which is what this exists to give a check something to bind on.
    fn shadowable_point(x: f32) -> Light {
        Light::Point(crate::light::PointLight {
            position: Vec3::new(x, 2.0, 0.0),
            radius: 8.0,
            color: Vec3::ONE,
            fill: false,
        })
    }

    fn shadowable_spot(x: f32) -> Light {
        Light::Spot(crate::light::SpotLight {
            position: Vec3::new(x, 2.0, 0.0),
            radius: 8.0,
            color: Vec3::ONE,
            direction: -Vec3::Y,
            inner_angle: 0.3,
            outer_angle: 0.6,
            fill: false,
        })
    }

    /// The page's anisotropy is the plan's default clamped to the device, and
    /// one on a device that was not granted the feature — whatever its limit
    /// says.
    ///
    /// The last arm is the one worth having: `gate_limits_on_features` narrows
    /// a declined feature's limit on `crcbl-vk`, but a backend that did not
    /// would report sixteen beside an absent feature, and a clamp that read the
    /// limit alone would ask for a sampler the device refuses.
    #[test]
    fn the_anisotropy_is_the_default_clamped_to_what_the_device_granted() {
        let caps = |features: Features, max_sampler_anisotropy: f32| DeviceCaps {
            features,
            limits: crcbl_hal::Limits {
                max_sampler_anisotropy,
                ..crcbl_hal::Limits::desktop()
            },
        };
        let granted = Features::SAMPLER_ANISOTROPY;
        assert!(
            (ForwardRenderer::anisotropy_for(&caps(granted, 16.0)) - DEFAULT_ANISOTROPY).abs()
                < f32::EPSILON,
            "a device above the default gets the default"
        );
        assert!(
            (ForwardRenderer::anisotropy_for(&caps(granted, 4.0)) - 4.0).abs() < f32::EPSILON,
            "a device below the default gets its own limit"
        );
        assert!(
            (ForwardRenderer::anisotropy_for(&caps(Features::empty(), 16.0)) - 1.0).abs()
                < f32::EPSILON,
            "a device that was not granted the feature gets one, whatever its limit says"
        );
    }

    /// A new anisotropy is a new sampler, and every slot moves onto it at its
    /// own frame — the old one living until the last slot has left it.
    ///
    /// Counted on the null recorder rather than looked at: the sampler count
    /// rises by one at the call, holds through every slot's frame but the
    /// last, and falls back there, while the live group count never moves at
    /// all — every rebuilt group replaced one. A rebuild that leaked, one that
    /// destroyed a group a slot in flight still held, and one that never
    /// happened each move one of those counts; a second lap of the ring
    /// creating any group at all would be a rebuild that did not know it was
    /// done.
    #[test]
    fn a_new_anisotropy_moves_each_slot_at_its_own_frame_and_retires_the_old() {
        let (recorder, device, queue) =
            open_with(Features::GPU_DRIVEN.union(Features::SAMPLER_ANISOTROPY));
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        assert!(
            (renderer.anisotropy() - DEFAULT_ANISOTROPY).abs() < f32::EPSILON,
            "a renderer nobody has called the setter on samples at the default"
        );
        let samplers = recorder.live_objects(ObjectKind::Sampler);
        let groups = recorder.live_objects(ObjectKind::BindGroup);
        let groups_created = || {
            recorder
                .events()
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::Created {
                            kind: ObjectKind::BindGroup,
                            ..
                        }
                    )
                })
                .count()
        };

        renderer
            .set_anisotropy(device, DEFAULT_ANISOTROPY)
            .expect("the device allows it");
        assert_eq!(
            recorder.live_objects(ObjectKind::Sampler),
            samplers,
            "the value in force creates nothing"
        );

        renderer
            .set_anisotropy(device, 4.0)
            .expect("the device allows it");
        assert!((renderer.anisotropy() - 4.0).abs() < f32::EPSILON);
        assert_eq!(
            recorder.live_objects(ObjectKind::Sampler),
            samplers + 1,
            "the new sampler stands beside the one every slot still names"
        );
        assert_eq!(
            recorder.live_objects(ObjectKind::BindGroup),
            groups,
            "no group moves until its slot's frame"
        );

        for round in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 48),
                )
                .expect("write");
            let expected = if round + 1 < FRAMES_IN_FLIGHT {
                samplers + 1
            } else {
                samplers
            };
            assert_eq!(
                recorder.live_objects(ObjectKind::Sampler),
                expected,
                "round {round}: the replaced sampler lives until the last slot has left it"
            );
            assert_eq!(
                recorder.live_objects(ObjectKind::BindGroup),
                groups,
                "round {round}: every rebuilt group replaced one"
            );
        }
        let created = groups_created();
        for _ in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 48),
                )
                .expect("write");
        }
        assert_eq!(
            groups_created(),
            created,
            "a slot already on the sampler in force rebuilds nothing"
        );

        // The clamp: the device's ceiling above it, off below one, the default
        // for a value that is not a number.
        let ceiling = device.caps().limits.max_sampler_anisotropy;
        renderer
            .set_anisotropy(device, ceiling * 4.0)
            .expect("clamped to what the device allows");
        assert!((renderer.anisotropy() - ceiling).abs() < f32::EPSILON);
        renderer
            .set_anisotropy(device, 0.0)
            .expect("one is the value that turns it off");
        assert!((renderer.anisotropy() - 1.0).abs() < f32::EPSILON);
        renderer
            .set_anisotropy(device, f32::NAN)
            .expect("the default is a sampler the device allows");
        assert!((renderer.anisotropy() - DEFAULT_ANISOTROPY).abs() < f32::EPSILON);

        recorder.assert_valid();
        renderer.destroy(device);
        assert_eq!(
            recorder.live_objects(ObjectKind::Sampler),
            0,
            "the samplers no slot ever adopted went with the renderer"
        );
    }

    /// A device without the feature samples isotropically whatever is asked:
    /// the ceiling is one, so the clamp lands on the value already in force and
    /// no sampler is created that `create_sampler` would refuse.
    #[test]
    fn a_device_without_the_feature_keeps_the_page_isotropic_whatever_is_asked() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        assert!(
            !device
                .caps()
                .features
                .contains(Features::SAMPLER_ANISOTROPY),
            "the default request does not name the feature"
        );
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        assert!((renderer.anisotropy() - 1.0).abs() < f32::EPSILON);
        let samplers = recorder.live_objects(ObjectKind::Sampler);

        renderer
            .set_anisotropy(device, DEFAULT_ANISOTROPY)
            .expect("asks for nothing the device lacks");
        assert!((renderer.anisotropy() - 1.0).abs() < f32::EPSILON);
        assert_eq!(recorder.live_objects(ObjectKind::Sampler), samplers);
        renderer.destroy(device);
    }

    /// The knob clamps to the range it documents, in both directions and on a
    /// value that is neither.
    ///
    /// **The `NaN` arm is the one worth having.** A slider that divided by a
    /// zero, or a settings file that carried a nonsense string, reaches this
    /// with a value that fails every comparison it is put through — so a naive
    /// `clamp` would keep it, `internal_extent` would multiply by it, and the
    /// `as u32` below would produce a zero extent and a device error a long way
    /// from here.
    #[test]
    fn the_render_scale_is_clamped_to_the_range_it_documents() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        assert!(
            (renderer.render_scale() - 1.0).abs() < f32::EPSILON,
            "a renderer nobody has called the setter on draws at full scale"
        );

        renderer.set_render_scale(0.5);
        assert!((renderer.render_scale() - 0.5).abs() < f32::EPSILON);

        // Above one is supersampling, which this filter is the wrong one for.
        renderer.set_render_scale(2.0);
        assert!((renderer.render_scale() - 1.0).abs() < f32::EPSILON);

        renderer.set_render_scale(0.01);
        assert!((renderer.render_scale() - MIN_RENDER_SCALE).abs() < f32::EPSILON);

        renderer.set_render_scale(f32::NAN);
        assert!((renderer.render_scale() - 1.0).abs() < f32::EPSILON);
        renderer.set_render_scale(f32::INFINITY);
        assert!((renderer.render_scale() - 1.0).abs() < f32::EPSILON);

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// The internal extent is the target rounded, never zero, and is the target
    /// itself at full scale.
    ///
    /// **The floor is not decoration.** A transient image of zero extent is a
    /// device error rather than a small frame, and a 4×3 target at the minimum
    /// scale is exactly the arithmetic that would produce one.
    #[test]
    fn the_internal_extent_rounds_and_never_reaches_zero() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        assert_eq!(
            renderer.internal_extent((1920, 1080)),
            (1920, 1080),
            "full scale is the absence of the feature, not a resize by one"
        );

        renderer.set_render_scale(0.5);
        assert_eq!(renderer.internal_extent((1920, 1080)), (960, 540));

        // Rounds rather than truncating: 1920 * 0.7 is 1344, and 1080 * 0.7 is
        // 756 — both exact — so a target that does *not* divide evenly is what
        // shows the rounding. 1281 * 0.7 = 896.7.
        renderer.set_render_scale(0.7);
        assert_eq!(renderer.internal_extent((1281, 1080)), (897, 756));

        renderer.set_render_scale(MIN_RENDER_SCALE);
        assert_eq!(renderer.internal_extent((4, 3)), (1, 1));
        assert_eq!(
            renderer.internal_extent((0, 0)),
            (1, 1),
            "a minimised window is the caller's zero extent, and this may not \
             pass it through as one"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **The upscale is a pass below full scale and is not there above it**, and
    /// the frame is otherwise the same list.
    ///
    /// This is the observable the knob is: everything else about it — the
    /// clamps, the rounding — is arithmetic that could be right while the frame
    /// went on being drawn at the caller's extent. What says the feature works
    /// is that the pass appears, that it appears **last**, and that switching it
    /// off leaves the frame that was there before it existed.
    #[test]
    fn the_upscale_is_the_last_pass_and_only_below_full_scale() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image_at(device, TEST_EXTENT);
        let pool = crate::TransientPool::new();

        let full = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        assert!(
            !full.contains(&"upscale".to_string()),
            "a frame at full scale has no upscale in it: {full:?}"
        );

        renderer.set_render_scale(0.5);
        let scaled = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        assert_eq!(
            scaled.iter().filter(|label| *label == "upscale").count(),
            1,
            "a scaled frame records exactly one upscale: {scaled:?}"
        );
        // Last of the passes that draw. The statistics copy is genuinely after
        // it and says so — see the comment on `add_copy_pass`'s placement.
        let upscale = scaled
            .iter()
            .position(|label| label == "upscale")
            .expect("the assertion above");
        let tonemap = scaled
            .iter()
            .position(|label| label == "tonemap")
            .expect("every frame tonemaps");
        assert!(
            upscale > tonemap,
            "the upscale carries what the post chain ended with: {scaled:?}"
        );

        // **The pyramid gets shorter, and that is the evidence the rest of the
        // frame really is smaller.** Everything above this is about the last
        // pass; a knob that added an upscale while going on rendering at the
        // caller's extent would pass every one of those assertions and save
        // nothing. `crate::hiz`'s chain is as long as the extent allows, so the
        // level count is a number read out of the frame that can only be the
        // internal extent's.
        let levels = |passes: &[String]| {
            passes
                .iter()
                .filter(|label| label.starts_with("hiz-"))
                .count()
        };
        let internal = renderer.internal_extent(TEST_EXTENT);
        assert_eq!(levels(&full), crate::hiz::levels_for(TEST_EXTENT) as usize);
        assert_eq!(levels(&scaled), crate::hiz::levels_for(internal) as usize);
        assert!(
            levels(&scaled) < levels(&full),
            "at this extent the halved frame has to lose a level, or this \
             assertion is not looking at anything: {scaled:?}"
        );

        // And nothing else moved: an upscale is a pass added to the frame, not a
        // frame rebuilt around one. The pyramid is the one part that is a
        // function of the extent, so it is compared by the count above and
        // dropped from the list here.
        let without_hiz = |passes: &[String]| -> Vec<String> {
            passes
                .iter()
                .filter(|label| !label.starts_with("hiz-") && *label != "upscale")
                .cloned()
                .collect()
        };
        assert_eq!(
            without_hiz(&scaled),
            without_hiz(&full),
            "the scaled frame is the full-scale frame, one pass longer and one \
             pyramid shorter"
        );

        renderer.set_render_scale(1.0);
        let back = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        assert_eq!(
            back, full,
            "the knob goes back, which is what a settings slider does"
        );

        renderer.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **[`ForwardRenderer::MAX_PASSES`] bounds the frame, and lands on it.**
    ///
    /// It is what a caller sizes [`PassTimers`](crate::PassTimers) with, so both
    /// halves matter: a bound under the widest frame times part of it and drops
    /// the rest, and a bound well over it buys query sets nothing ever writes.
    /// A shadowable spot per light slot fills every one of them, which is the
    /// most culls a frame can run and so the widest frame there is — it must land
    /// exactly on the constant, and the frame with no shadowed light at all must
    /// be short of it by exactly the culls those slots would have added.
    ///
    /// The ground grid is switched on for **both** frames, because the widest
    /// frame is the one that has it: it is off by default and not a
    /// [`RenderEffects`] bit, so nothing else here would put its pass in. The
    /// debug draw layer is on for both frames for the same reason, and it needs
    /// a segment appended before each rather than one switch, because
    /// [`begin_frame`](ForwardRenderer::begin_frame) clears its buffer.
    ///
    /// **And the frames are drawn at [`WIDEST_EXTENT`] rather than at
    /// [`TEST_EXTENT`], with every effect forced on and the render scale off
    /// full.** The bloom chain's length is a function of the extent the frame is
    /// drawn at — see [`crate::bloom`] — so the widest frame this renderer
    /// records is a *large* one, and a bound checked against a 64×48 frame would
    /// sit six passes above what that frame runs and this assertion would have
    /// to be a `<=`. It is an `==` on purpose: a bound well over the widest
    /// frame buys query sets nothing writes, and only an exact comparison
    /// notices.
    ///
    /// **The render scale is what makes those two requirements compose.** The
    /// upscale is a pass only below full scale, and below full scale the chain
    /// runs at the *internal* extent — so a frame with both is one whose
    /// internal extent is the longest chain's, and whose target is larger than
    /// that. Halving a target of 2048×1024 is what arranges it, and the two
    /// assertions below check the arrangement rather than trusting it.
    ///
    /// [`WIDEST_EXTENT`]: fn@the_pass_bound_is_the_widest_frame_the_renderer_records
    #[test]
    fn the_pass_bound_is_the_widest_frame_the_renderer_records() {
        /// The extent whose bloom chain is the longest [`crate::bloom`] will
        /// build: six halvings of 512 leave sixteen by eight, which is still at
        /// or above that module's floor. The widest frame is drawn at this,
        /// which is why it is the internal one and not the target.
        const WIDEST_INTERNAL: (u32, u32) = (1024, 512);
        /// Half, so the frame records an upscale and draws at
        /// [`WIDEST_INTERNAL`].
        const WIDEST_SCALE: f32 = 0.5;
        /// The caller's extent: twice [`WIDEST_INTERNAL`] in each dimension.
        const WIDEST_EXTENT: (u32, u32) = (WIDEST_INTERNAL.0 * 2, WIDEST_INTERNAL.1 * 2);

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        // **A scene whose probes ask to be updated**, because the widest frame
        // is the one with every pass in it and
        // `docs/plan/50-irradiance-probes.md`'s two are recorded by a volume
        // rather than by an effect bit — the ground grid's argument exactly. A
        // renderer built on the demo's empty grid records neither, and the bound
        // would then be checked against a frame that could not reach it.
        let mut scene = scene::demo();
        scene.capacities.probes = 1;
        scene.probes = scene::ProbeGrid {
            volume: crcbl_shaders::probe::ProbeVolume {
                origin: [0.0; 3],
                inv_spacing: [1.0; 3],
                counts: [1, 1, 1],
                levels: 1,
            },
            probes: vec![crcbl_shaders::probe::GpuProbe::ZERO],
            update: ProbeUpdate::EveryFrame,
        };
        let mut renderer =
            ForwardRenderer::with_scene(device, queue, Format::Rgba8UnormSrgb, &scene)
                .expect("built");
        renderer
            .set_ground_grid(device, Some(GridStyle::default()))
            .expect("the null backend builds every pipeline");
        renderer.set_render_scale(WIDEST_SCALE);
        assert_eq!(
            renderer.internal_extent(WIDEST_EXTENT),
            WIDEST_INTERNAL,
            "the scale has to take the target to the extent whose chain is the \
             longest one, or the bound below is checked against a frame that \
             could not reach it"
        );
        // **Both post effects forced on.** Neither is in the default camera
        // stack — see [`RenderEffects::DEFAULT_STACK`], which leaves the lens out
        // for one reason and the resolve out for another — and the widest frame
        // is the one that has every effect in it, the ground grid's argument
        // exactly.
        renderer.set_effect_request(EffectRequest {
            programmatic: EffectOverride::none().force(OUTSIDE_THE_DEFAULT, Some(true)),
            ..EffectRequest::default()
        });
        let imported = swapchain_image_at(device, WIDEST_EXTENT);
        let mut pool = crate::TransientPool::new();

        // The debug draw layer on for both frames below, because it is a pass
        // the bound counts and no effect bit switches on — the ground grid's
        // argument again. A segment before each, since `begin_frame` clears the
        // buffer: the layer is immediate mode.
        let switch = debug_draw_switch();
        switch.set(true);
        // And the occlusion pair's second blur, for exactly that reason: it is a
        // pass [`Ssao::MAX_PASSES`] counts and no effect bit switches on, so a
        // frame at the shipping count comes one short of the bound and this test
        // would be asserting the bound against a frame that could not reach it.
        let blurs = ssao_blur_switch();
        blurs.set(i64::from(Ssao::MAX_BLUR_PASSES));
        // And the comparison seam, which is the occlusion chain's other
        // conditional pass — see `crate::ssao`'s `r_ssao_split`. Held for the
        // same reason the blur count is: the bound counts a frame that compares,
        // so a frame that does not could never reach it.
        let split = ssao_split_switch();
        split.set(0.5);
        renderer
            .debug_draw()
            .line(Vec3::ZERO, Vec3::X, [1.0, 1.0, 1.0, 1.0]);
        let bare =
            passes_in_a_frame(device, queue, &mut renderer, imported, &pool, WIDEST_EXTENT).len();
        assert_eq!(
            crate::bloom::mips_for(WIDEST_INTERNAL),
            crate::bloom::MAX_MIPS,
            "the widest frame has to be one whose chain is the longest one there \
             is, or the bound below is checked against a frame that could not \
             reach it"
        );
        assert!(
            bare < ForwardRenderer::MAX_PASSES as usize,
            "a frame with no shadowed light runs {bare} passes, which is already the \
             bound of {} — then the bound does not cover the slots this scene left free",
            ForwardRenderer::MAX_PASSES
        );

        // One spot per slot, spaced so no two of them share an influence — the
        // list has to fill every slot or the bound below is checked against a
        // frame that could not reach it.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a handful of slots, and the value is only a position"
        )]
        let spots: Vec<Light> = (0..shadow::LIGHT_SLOTS)
            .map(|slot| shadowable_spot(1.0 + slot as f32))
            .collect();
        renderer.set_lights(&spots);
        renderer
            .debug_draw()
            .line(Vec3::ZERO, Vec3::X, [1.0, 1.0, 1.0, 1.0]);
        let widest =
            passes_in_a_frame(device, queue, &mut renderer, imported, &pool, WIDEST_EXTENT).len();
        assert_eq!(
            renderer.shadow_lights.slots().iter().flatten().count(),
            shadow::LIGHT_SLOTS,
            "the spots must actually hold every slot, or this is the bare frame again \
             under another name"
        );
        assert_eq!(
            widest,
            ForwardRenderer::MAX_PASSES as usize,
            "the widest frame is what the bound is for"
        );
        assert_eq!(
            widest - bare,
            DrawGen::MAX_PASSES as usize * shadow::LIGHT_SLOTS,
            "a filled slot costs a cull's passes and nothing else"
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **A toggle reaches the recorded frame, and takes exactly its own passes
    /// with it.**
    ///
    /// The observable is the compiled pass list, because the thing that could
    /// otherwise be true is that the switch is stored and read by nothing: a
    /// renderer holding `SHADOWS` clear and recording every shadow cull anyway
    /// draws a frame that looks right, reports the right effect set, and has not
    /// switched anything off.
    ///
    /// So each arm compares the whole list rather than its length. Two passes
    /// fewer is satisfied by removing the wrong two, and it is satisfied by a
    /// frame that lost its tonemap.
    ///
    /// The shadow arm is the one that cannot be written by name — a cascade's
    /// cull passes are labelled exactly as the camera's are — so it is written as
    /// its two halves instead: the count moves by a cull's worth per cascade and
    /// per filled light slot, and everything from the `shadow` pass onwards is
    /// unchanged. The `shadow` pass itself **stays**, because it is what writes
    /// the clear that reads as "fully lit".
    #[test]
    fn each_effect_toggle_removes_exactly_the_passes_it_owns() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        // One shadowed light beside the cascades, so the shadow arm below covers
        // a light slot's cull as well as a cascade's.
        renderer.set_lights(&[shadowable_spot(-1.0)]);
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        // [`OUTSIDE_THE_DEFAULT`] forced on and then `off` forced off, on
        // [`frame_switching_off`]'s terms exactly: the default camera stack
        // leaves both of them out — see [`RenderEffects::DEFAULT_STACK`] — so a
        // control built on the default alone would have nothing for their arms
        // to remove. `force` clears the other side, so an arm whose `off` is one
        // of them still resolves to off.
        //
        // [`frame_switching_off`]: fn@frame_switching_off
        let without = |renderer: &mut ForwardRenderer, off: RenderEffects| {
            renderer.set_effect_request(EffectRequest {
                programmatic: EffectOverride::none()
                    .force(OUTSIDE_THE_DEFAULT, Some(true))
                    .force(off, Some(false)),
                ..EffectRequest::default()
            });
            passes_in_a_frame(device, queue, renderer, imported, &pool, TEST_EXTENT)
        };

        let all_on = without(&mut renderer, RenderEffects::empty());
        assert_eq!(
            renderer.effects(),
            RenderEffects::all(),
            "the control frame has to be the every-effect one"
        );
        assert_eq!(
            renderer.shadow_lights.slots().iter().flatten().count(),
            1,
            "the spot must actually hold a slot, or the shadow arm below is only about cascades"
        );

        // **None of the three leaves anything behind it**, and for the same
        // reason: nothing reads what it would have written. The occlusion pair's
        // reader binds the 1×1 white placeholder instead — `mesh.slang` clamps
        // its `Load` into it — and the reflection pair's and the chain's readers
        // both tonemap whatever image they would have read anyway.
        //
        // The chain's labels are written out rather than built from
        // `crate::bloom`'s arithmetic, because a table generated from the code
        // under test cannot fail: it is `TEST_EXTENT`'s two levels — two
        // downsamples, the one upsample between them, and the composite — and if
        // that extent moves this is the assertion that says so.
        for (off, gone) in [
            (
                RenderEffects::AMBIENT_OCCLUSION,
                ["ssao", "ssao-blur", "ssao-upsample"].as_slice(),
            ),
            (
                RenderEffects::CONTACT_SHADOWS,
                // One pass and no blur — `crate::contact_shadows`'s header says
                // why the occlusion pair's second pass has no counterpart here.
                ["contact-shadows"].as_slice(),
            ),
            (
                RenderEffects::REFLECTIONS,
                // **The pyramid goes with the march**, which is the whole of
                // what makes it an optimisation rather than a resource: it is
                // recorded on the frames that reflect and on no others. Two
                // levels at `TEST_EXTENT`, written out on the chain's terms
                // above.
                ["hiz-1", "hiz-2", "ssr", "ssr-blur"].as_slice(),
            ),
            (
                RenderEffects::BLOOM,
                [
                    "bloom-down-1",
                    "bloom-down-2",
                    "bloom-up-1",
                    "bloom-composite",
                ]
                .as_slice(),
            ),
            (
                RenderEffects::AUTO_EXPOSURE,
                // All three go together and the tonemap stays: the pass reads
                // the measurement only when the block says to, and the block
                // says so on exactly the frames these three ran on.
                ["exposure-clear", "exposure-histogram", "exposure-reduce"].as_slice(),
            ),
        ] {
            let labels = without(&mut renderer, off);
            assert_eq!(
                renderer.effects(),
                RenderEffects::all().difference(off),
                "{off:?}: the frame must have resolved to the set the request asked for"
            );
            // **The control frame has to contain what this arm removes**, and
            // that is not a restatement of the comparison below: the comparison
            // builds `expected` by filtering `gone` out of `all_on`, so an arm
            // naming a pass the control never recorded filters nothing, matches,
            // and passes whether the effect draws anything or not. Removing the
            // resolve pass entirely left this loop green until this assertion
            // existed.
            for label in gone {
                assert!(
                    all_on.iter().any(|recorded| recorded == label),
                    "{off:?}: the every-effect frame does not record `{label}`, so removing it \
                     below proves nothing: {all_on:#?}"
                );
            }
            let expected: Vec<String> = all_on
                .iter()
                .filter(|label| !gone.contains(&label.as_str()))
                .cloned()
                .collect();
            assert_eq!(
                labels, expected,
                "{off:?}: the frame must lose {gone:?}, gain nothing in their place, and keep \
                 every other pass"
            );
        }

        // **The two antialiasing tiers share one resolve slot**, so neither is
        // a plain removal and neither belongs in the loop above: switching SMAA
        // off while FXAA stays on swaps three passes for one rather than losing
        // three, and the loop's "gain nothing in their place" comparison is
        // exactly what that violates. Three frames say it instead.
        let both = all_on.clone();
        let cheap = without(&mut renderer, RenderEffects::SMAA);
        let neither = without(
            &mut renderer,
            RenderEffects::SMAA.union(RenderEffects::ANTIALIASING),
        );
        const SMAA_PASSES: [&str; 3] = ["smaa-edges", "smaa-weights", "smaa-blend"];
        let resolve_of = |labels: &[String]| -> Vec<String> {
            labels
                .iter()
                .filter(|label| *label == "fxaa" || SMAA_PASSES.contains(&label.as_str()))
                .cloned()
                .collect()
        };
        let without_resolve = |labels: &[String]| -> Vec<String> {
            labels
                .iter()
                .filter(|label| *label != "fxaa" && !SMAA_PASSES.contains(&label.as_str()))
                .cloned()
                .collect()
        };
        assert_eq!(
            resolve_of(&both),
            SMAA_PASSES.map(str::to_string).to_vec(),
            "with both bits set the higher tier takes the slot and the cheap one \
             records nothing"
        );
        assert_eq!(
            resolve_of(&cheap),
            vec!["fxaa".to_string()],
            "with only the cheap bit set the slot is FXAA's"
        );
        assert!(
            resolve_of(&neither).is_empty(),
            "with neither bit set the frame has no resolve at all: {neither:#?}"
        );
        // And nothing else moved: the tiers swap in one slot rather than
        // rebuilding the frame around themselves.
        assert_eq!(without_resolve(&both), without_resolve(&cheap));
        assert_eq!(without_resolve(&both), without_resolve(&neither));
        // The slot is where the ordering argument puts it — directly after the
        // last pass that writes the display image, which at this extent is the
        // tonemap.
        for labels in [&both, &cheap] {
            let at = labels
                .iter()
                .position(|label| label == "tonemap")
                .expect("a frame has to reach the swapchain");
            assert_eq!(
                &labels[at + 1..=at + resolve_of(labels).len()],
                resolve_of(labels).as_slice(),
                "the resolve must follow the tonemap: {labels:#?}"
            );
        }

        let no_shadows = without(&mut renderer, RenderEffects::SHADOWS);
        assert_eq!(
            renderer.effects(),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
        );
        assert_eq!(
            all_on.len() - no_shadows.len(),
            DrawGen::MAX_PASSES as usize * (shadow::CASCADES + 1),
            "shadows off must drop a cull's passes per cascade and per filled slot"
        );
        let from_the_atlas = |labels: &[String]| {
            let at = labels
                .iter()
                .position(|label| label == "shadow")
                .expect("the atlas pass is recorded whether or not anything draws into it");
            labels[at..].to_vec()
        };
        assert_eq!(
            from_the_atlas(&no_shadows),
            from_the_atlas(&all_on),
            "only the culls go: the atlas pass and everything after it are unchanged"
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The contact march is a pass a frame records only when it was asked
    /// for, and the mask it writes is what the forward pass then reads.**
    ///
    /// Two observables, because either alone is satisfied by a mistake the other
    /// one catches:
    ///
    /// * **The compiled pass list.** `contact-shadows` appears between the
    ///   prepass whose depth it marches and the forward pass that binds it, and
    ///   appears at all only on the frame that asked. The toggle test above
    ///   already says a frame loses this pass and nothing else; what is added
    ///   here is *where* it sits, which is the half a set comparison cannot see
    ///   — a march recorded after the pass that reads it compiles and draws a
    ///   mask nothing ever looks at.
    /// * **The forward pass's own barriers.** On the frame that marched, one
    ///   more image is transitioned into [`ResourceState::ShaderRead`] before
    ///   the forward pass opens than on the frame that did not — which is the
    ///   mask leaving the colour attachment it was written as. That is what says
    ///   the pass's output reaches the shading, rather than being written into a
    ///   transient and dropped while the shader goes on reading the 1×1
    ///   placeholder. The placeholder is imported in `ShaderRead` already and
    ///   needs no transition, so it contributes nothing to this count and the
    ///   difference is exactly the mask.
    ///
    /// [`RenderEffects::CONTACT_SHADOWS`] is out of the default camera stack
    /// while its re-bless is pending — see that constant — so the `on` frame has
    /// to force it and the `off` frame is what a view saying nothing draws.
    #[test]
    fn the_contact_march_is_recorded_only_when_asked_and_its_mask_reaches_the_forward_pass() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        let frame = |renderer: &mut ForwardRenderer, on: bool| -> (Vec<String>, usize) {
            renderer.set_effect_request(EffectRequest {
                programmatic: EffectOverride::none()
                    .force(RenderEffects::CONTACT_SHADOWS, Some(on)),
                ..EffectRequest::default()
            });
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    TEST_EXTENT,
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, &pool, target, TEST_EXTENT);
            let compiled = graph.compile(&pool).expect("a legal frame");
            let labels: Vec<String> = compiled
                .passes()
                .iter()
                .map(|pass| pass.label().to_string())
                .collect();
            let readable = compiled
                .passes()
                .iter()
                .find(|pass| pass.label() == "forward")
                .expect("every frame draws its geometry")
                .barriers()
                .images
                .iter()
                .filter(|barrier| barrier.to == ResourceState::ShaderRead)
                .count();
            (labels, readable)
        };

        let (marched, marched_reads) = frame(&mut renderer, true);
        assert!(
            renderer.effects().contains(RenderEffects::CONTACT_SHADOWS),
            "the forced frame has to have resolved with the bit set"
        );
        let (bare, bare_reads) = frame(&mut renderer, false);
        assert!(
            !renderer.effects().contains(RenderEffects::CONTACT_SHADOWS),
            "and the control frame without it"
        );

        assert!(
            !bare.iter().any(|label| label == "contact-shadows"),
            "a frame that did not ask must record no march at all: {bare:#?}"
        );
        let at = |labels: &[String], wanted: &str| {
            labels
                .iter()
                .position(|label| label == wanted)
                .unwrap_or_else(|| panic!("no `{wanted}` pass in {labels:#?}"))
        };
        let march = at(&marched, "contact-shadows");
        assert!(
            march > at(&marched, "depth-prepass"),
            "the march reads the prepass's depth, so it cannot precede it: {marched:#?}"
        );
        assert!(
            march < at(&marched, "forward"),
            "and the forward pass binds what it wrote, so it cannot follow it: {marched:#?}"
        );

        assert_eq!(
            marched_reads,
            bare_reads + 1,
            "the forward pass has to take one more image into `ShaderRead` on the frame that \
             marched — the mask. Equal counts mean the pass wrote a transient the shading \
             never reads."
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The ground grid is off until a caller asks, and then it is the last
    /// pass of the frame — after the tonemap.**
    ///
    /// Three claims and each is a separate way to get this wrong. Off by default
    /// is what keeps every sample and every golden image where they were. *After
    /// the tonemap* is the placement decision: added a line earlier it would
    /// draw into the HDR scene colour and be tonemapped like geometry, which
    /// compiles, draws a grid, and gives it a colour that moves with the
    /// exposure. And switching it off again has to remove the pass, not merely
    /// stop reporting it.
    ///
    /// The whole list is compared rather than its length, for the reason the
    /// effect-toggle test gives: one more pass is satisfied by gaining the wrong
    /// one.
    #[test]
    fn the_ground_grid_is_opt_in_and_lands_after_the_tonemap() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        let off = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        assert!(
            renderer.ground_grid().is_none(),
            "a renderer nobody asked has no grid"
        );
        assert!(
            !off.contains(&"grid".to_string()),
            "the grid must not be in a frame nobody asked for it in: {off:#?}"
        );

        renderer
            .set_ground_grid(device, Some(GridStyle::default()))
            .expect("the null backend builds every pipeline");
        assert_eq!(renderer.ground_grid(), Some(&GridStyle::default()));
        let on = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);

        let mut expected = off.clone();
        // The statistics copy is added after every render pass and stays last —
        // see `add_passes`. So the grid goes in front of it, which is directly
        // after the tonemap.
        let tonemap = expected
            .iter()
            .position(|label| label == "tonemap")
            .expect("a frame has to reach the swapchain");
        expected.insert(tonemap + 1, "grid".to_string());
        assert_eq!(
            on, expected,
            "the grid must be the one pass gained, and it must sit after the tonemap"
        );

        renderer
            .set_ground_grid(device, None)
            .expect("switching off builds nothing");
        assert!(renderer.ground_grid().is_none());
        assert_eq!(
            passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT),
            off,
            "switching the grid off has to remove its pass, not just stop reporting it"
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **A second occlusion blur is a pass the frame records, an image it takes
    /// out of the pool, and a triangle its counter reports** — or it is a switch
    /// that does nothing.
    ///
    /// `crcbl`'s `forward_e2e::occlusion` measures what the second blur is worth
    /// by driving the pipelines itself, so nothing there would notice that a
    /// real frame never records it. The three things below are what a frame has
    /// to do differently, and each of them fails silently on its own: a missing
    /// pass draws a slightly rougher picture, a missing image is a bind of the
    /// wrong one, and a counter short by one is a number nobody reads until it
    /// is quoted.
    ///
    /// **This test owns `r_ssao_blur_passes` for its duration**, through
    /// [`ssao_blur_switch`] and for [`debug_draw_switch`]'s reason.
    /// The id [`Ssao::add_passes`] hands back is the reconstruction's target,
    /// whichever blur ran into it.
    ///
    /// **The one thing about that chain no other check can see.** The pass list
    /// is identical either way, the graph accepts a written-and-never-read
    /// transient without complaint, and every golden is at one blur where the
    /// two blur answers coincide — so a frame bound to the wrong image draws
    /// something nobody would question, which is exactly what the function's
    /// own doc warns about. Verified 2026-08-31 by returning `blurred` from the
    /// two-blur arm: `crcbl-render`'s unit tests, the forward and mesh e2e
    /// suites and lantern's golden all stayed green.
    ///
    /// **Since the halving it is a second silent failure and a louder one.**
    /// Every working image is [`crate::ssao::half_extent`] of the frame, so a
    /// returned working id is not merely a rougher channel — it is an image a
    /// quarter of the size bound where `mesh.slang` reads at `SV_Position`,
    /// which its clamp turns into occlusion smeared over the top-left quarter of
    /// the frame rather than into an error.
    #[test]
    fn the_occlusion_id_handed_back_is_the_reconstruction_target() {
        let (_recorder, device, queue) = open();
        let device = device.as_ref();
        let mut ssao = Ssao::new(device, 1, ForwardRenderer::build_fullscreen).expect("built");
        let gathered = crate::ssao::half_extent(TEST_EXTENT);

        let one = {
            let mut graph = crate::RenderGraph::new(queue);
            let depth =
                graph.create_image("scene-depth", TransientImageDesc::scene_depth(TEST_EXTENT));
            let raw = graph.create_image("ssao", TransientImageDesc::ambient_occlusion(gathered));
            let blurred = graph.create_image(
                "ssao-blurred",
                TransientImageDesc::ambient_occlusion(gathered),
            );
            let upsampled = graph.create_image(
                "ssao-upsampled",
                TransientImageDesc::ambient_occlusion(TEST_EXTENT),
            );
            let got = ssao.add_passes(
                &mut graph,
                0,
                TEST_EXTENT,
                depth,
                crate::ssao::OcclusionImages {
                    raw,
                    blurred,
                    again: None,
                    upsampled,
                },
            );
            (got == upsampled, got == blurred || got == raw)
        };
        assert!(one.0, "one blur finishes in the reconstruction's image");
        assert!(
            !one.1,
            "and never in one of the half-extent images the chain worked in"
        );

        let two = {
            let mut graph = crate::RenderGraph::new(queue);
            let depth =
                graph.create_image("scene-depth", TransientImageDesc::scene_depth(TEST_EXTENT));
            let raw = graph.create_image("ssao", TransientImageDesc::ambient_occlusion(gathered));
            let blurred = graph.create_image(
                "ssao-blurred",
                TransientImageDesc::ambient_occlusion(gathered),
            );
            let again = graph.create_image(
                "ssao-blurred-2",
                TransientImageDesc::ambient_occlusion(gathered),
            );
            let upsampled = graph.create_image(
                "ssao-upsampled",
                TransientImageDesc::ambient_occlusion(TEST_EXTENT),
            );
            let got = ssao.add_passes(
                &mut graph,
                0,
                TEST_EXTENT,
                depth,
                crate::ssao::OcclusionImages {
                    raw,
                    blurred,
                    again: Some(again),
                    upsampled,
                },
            );
            (got == upsampled, got == again || got == blurred)
        };
        assert!(
            two.0,
            "two blurs finish in the reconstruction's image as well, which is the id the forward \
             pass binds"
        );
        assert!(
            !two.1,
            "and not in a blur's own image -- a frame bound to one of those is a frame nobody \
             would question"
        );
    }

    /// **The gather runs at half the frame and the reconstruction runs at all
    /// of it** — or the halving is a description nobody rendered into.
    ///
    /// The pass list cannot see this: three passes with the right labels in the
    /// right order is exactly what a chain that halved nothing would record. The
    /// **render area** is where the halving is observable, and the graph derives
    /// it from each pass's colour attachment — so this is the check that the
    /// transients [`ForwardRenderer::add_passes`] created are the ones
    /// [`crate::ssao::half_extent`] sized, and that the reconstruction was given
    /// the frame-sized one.
    ///
    /// Neither mistake is an error anywhere else. Working images at the frame's
    /// extent are four times the march and a reconstruction reading the corner
    /// of its input; a frame-sized image handed to the march and a half-sized
    /// one to the forward pass is occlusion smeared over a quarter of the
    /// picture by `mesh.slang`'s clamp. Both are pictures.
    ///
    /// **This test owns `r_ssao_blur_passes` for its duration**, through
    /// [`ssao_blur_switch`] and for that function's second reason: it looks the
    /// occlusion chain's passes up by name, and a second blur arriving from
    /// another thread's switch is a pass this frame did not ask for.
    #[test]
    fn the_occlusion_chain_gathers_at_half_the_frame_and_reconstructs_at_all_of_it() {
        let _switch = ssao_blur_switch();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();
        renderer
            .begin_frame(
                device,
                &Camera::default(),
                &DirectionalLight::default(),
                TEST_EXTENT,
            )
            .expect("write");
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        renderer.add_passes(&mut graph, &pool, target, TEST_EXTENT);
        let compiled = graph.compile(&pool).expect("a legal frame");
        let area_of = |label: &str| {
            let pass = compiled
                .passes()
                .iter()
                .find(|pass| pass.label() == label)
                .unwrap_or_else(|| panic!("the frame records no `{label}` pass at all"));
            let area = pass.render_area();
            (area.width, area.height)
        };

        let gathered = crate::ssao::half_extent(TEST_EXTENT);
        assert_ne!(
            gathered, TEST_EXTENT,
            "the fixture extent has to halve to something else, or every assertion below holds \
             of a chain that halved nothing"
        );
        assert_eq!(
            area_of("ssao"),
            gathered,
            "the march has to run over the gather extent"
        );
        assert_eq!(
            area_of("ssao-blur"),
            gathered,
            "and so does the blur over what the march wrote"
        );
        assert_eq!(
            area_of("ssao-upsample"),
            TEST_EXTENT,
            "the reconstruction writes the channel `mesh.slang` reads at `SV_Position`, so it \
             runs at the frame's own extent"
        );

        drop(compiled);
        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The comparison seam records a second march, scissored to the columns
    /// the first one was kept off.**
    ///
    /// `crate::split`'s geometry, read off the encoder rather than off the code
    /// that drove it: two passes, and two `SetScissor` rectangles that tile the
    /// gather extent between them.
    ///
    /// **What this cannot see is which block each march bound.** The reference
    /// backend records a bind group as a handle and a layout, not as the
    /// resources behind it, so pointing both marches at one buffer leaves this
    /// test green — measured, by doing exactly that. The group inequality below
    /// is kept because it is free and would catch the two marches sharing a
    /// cache, which is a different fault; what covers the blocks is
    /// `crate::ssao`'s `the_seams_other_side_is_what_the_chain_ships`, and what
    /// is not covered at all is the wiring between them. `docs/backlog.md`
    /// carries that gap.
    ///
    /// **The control is the same frame without a seam**, and it is what makes
    /// the assertions mean anything: a chain that scissored unconditionally
    /// would satisfy every claim below and change every golden in the workspace,
    /// so the frame nobody asked to compare has to record one march and set no
    /// rectangle at all.
    ///
    /// Holds [`ssao_split_switch`] for [`ssao_blur_switch`]'s reason: the seam
    /// is process-wide state and a frame that picked one up from another
    /// thread's test is a frame with a pass this one did not ask for.
    #[test]
    fn the_comparison_seam_marches_twice_over_columns_that_tile_the_gather() {
        use crcbl_hal::null::Command;

        let _blurs = ssao_blur_switch();
        let split = ssao_split_switch();
        // Third, after the two above: the only other test that takes this one
        // takes it alone, so this is the whole order and there is no cycle in
        // it. Moved below, because two blocks holding the same scalars are
        // equal whatever the seam did.
        let slices = ssao_slice_switch();
        // The count the seam's far side reads, and one that is not it. Read off
        // the variable rather than written down: which end of the range ships
        // has changed once already, and a test naming the wrong end would be
        // comparing the shipped configuration against itself.
        let ships = match crate::ssao::r_ssao_slices.default() {
            crcbl_console::Value::Int(count) => *count,
            other => panic!("`r_ssao_slices` defaults to {other:?}, which is not a count"),
        };
        let moved = if ships == i64::from(ssao::SLICE_COUNT_MAX) {
            i64::from(ssao::SLICE_COUNT_DEFAULT)
        } else {
            i64::from(ssao::SLICE_COUNT_MAX)
        };
        assert_ne!(
            ships, moved,
            "the two slice counts this compares are one count, so the blocks below would differ \
             by nothing whatever the seam did"
        );
        slices.set(ships);

        let gathered = crate::ssao::half_extent(TEST_EXTENT);
        // One entry per march the frame recorded: the last rectangle that pass
        // left the scissor at, and the group it bound.
        //
        // **The last and not the only one.** The graph opens every pass with a
        // scissor over the whole render area, so a march that sets none of its
        // own still records one — which is why "did it scissor at all" is not
        // the question and "what did it end up cropped to" is.
        let marches = |recorder: &Recorder, from: usize| {
            let mut inside = false;
            let mut found: Vec<(crcbl_hal::Rect2d, crcbl_hal::BindGroupHandle)> = Vec::new();
            let mut rect = None;
            let mut group = None;
            for command in &recorder.commands()[from..] {
                if let Some((_, label)) = command.opens_pass() {
                    inside = matches!(label, Some("ssao" | "ssao-shipped"));
                    (rect, group) = (None, None);
                    continue;
                }
                if matches!(command, Command::EndRenderPass | Command::EndComputePass) {
                    if let (true, Some(rect), Some(group)) = (inside, rect, group) {
                        found.push((rect, group));
                    }
                    inside = false;
                    continue;
                }
                if !inside {
                    continue;
                }
                match command {
                    Command::SetScissor(at) => rect = Some(*at),
                    Command::BindGroup { group: bound, .. } => group = Some(*bound),
                    _ => {}
                }
            }
            found
        };

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        split.set(0.0);
        let alone = frame_seen_from(
            device,
            &mut renderer,
            queue,
            &Camera::default(),
            &DirectionalLight::default(),
        );
        let plain = marches(&recorder, 0);
        let after = recorder.commands().len();
        let (console, shipped) = renderer.ssao.blocks(renderer.frame);
        let (uncompared_console, uncompared_shipped) = (
            recorder.buffer_bytes(console).expect("the console's block"),
            recorder.buffer_bytes(shipped).expect("the seam's block"),
        );
        alone.release(device);

        split.set(0.5);
        slices.set(moved);
        let frame = frame_seen_from(
            device,
            &mut renderer,
            queue,
            &Camera::default(),
            &DirectionalLight::default(),
        );
        let compared = marches(&recorder, after);
        let (console, shipped) = renderer.ssao.blocks(renderer.frame);
        let (compared_console, compared_shipped) = (
            recorder.buffer_bytes(console).expect("the console's block"),
            recorder.buffer_bytes(shipped).expect("the seam's block"),
        );
        frame.finish(device, renderer);

        assert_eq!(
            plain.len(),
            1,
            "a frame comparing nothing recorded {} march(es); the seam is off, so there is one",
            plain.len()
        );
        assert_eq!(
            (
                plain[0].0.x,
                plain[0].0.y,
                plain[0].0.width,
                plain[0].0.height
            ),
            (0, 0, gathered.0, gathered.1),
            "a frame comparing nothing left its march cropped to something other than the whole \
             gather extent {gathered:?} — so the chain crops whether or not anybody asked it to, \
             and every golden in this workspace was blessed on a frame that does not"
        );

        assert_eq!(
            compared.len(),
            2,
            "a frame comparing two configurations recorded {} march(es)",
            compared.len()
        );
        let (left, right) = (compared[0].0, compared[1].0);
        assert_eq!(
            (left.x, left.y, right.y),
            (0, 0, 0),
            "both halves start at the top and the left one at the edge"
        );
        assert_eq!(
            i64::from(right.x),
            i64::from(left.width),
            "the second march starts where the first one ended, or a column is drawn twice"
        );
        assert_eq!(
            (left.width + right.width, left.height, right.height),
            (gathered.0, gathered.1, gathered.1),
            "the two rectangles have to cover the gather extent {gathered:?} exactly"
        );
        assert!(
            left.width < gathered.0 && right.width < gathered.0,
            "one half covers the whole extent, so the other draws over it and the seam is a \
             rectangle that crops nothing"
        );
        assert_ne!(
            compared[0].1, compared[1].1,
            "the two marches bound one group, so they cannot be reading two blocks"
        );

        // How far the reference backend can follow the far side. It will say
        // what bytes a buffer holds and it will not say which buffer a group
        // names, so the two together are "there are two blocks, and they differ
        // by the knob that was moved" — not "the right march read the right
        // one". Pointing both marches at one buffer still passes; see this
        // module's entry in `docs/backlog.md`.
        assert_eq!(
            uncompared_console, uncompared_shipped,
            "the console sat at its defaults and the two blocks still differ, so the seam's far \
             side is not this frame with the shipped scalars — it is another frame"
        );
        assert_ne!(
            compared_console, compared_shipped,
            "one slice count was moved off the shipped one and both blocks still hold the same \
             bytes, so a comparison shows the same picture twice"
        );

        recorder.assert_valid();
    }

    /// Every gather pipeline the frame bound, in the order the marches ran.
    ///
    /// [`the_comparison_seam_marches_twice_over_columns_that_tile_the_gather`]'s
    /// `marches` closure asking the other question of the same stream: that one
    /// reads the scissor and the group, and this one reads the pipeline. Only
    /// the `ssao` and `ssao-shipped` passes are looked at, because every other
    /// pass in the chain binds a pipeline too and a filter over the whole stream
    /// would be counting blurs.
    ///
    /// [`the_comparison_seam_marches_twice_over_columns_that_tile_the_gather`]: fn@the_comparison_seam_marches_twice_over_columns_that_tile_the_gather
    fn gathers_bound(recorder: &Recorder, from: usize) -> Vec<crcbl_hal::GraphicsPipelineHandle> {
        use crcbl_hal::null::Command;

        let mut inside = false;
        let mut found = Vec::new();
        for command in &recorder.commands()[from..] {
            if let Some((_, label)) = command.opens_pass() {
                inside = matches!(label, Some("ssao" | "ssao-shipped"));
                continue;
            }
            if matches!(command, Command::EndRenderPass | Command::EndComputePass) {
                inside = false;
                continue;
            }
            if let (true, Command::BindGraphicsPipeline(pipeline)) = (inside, command) {
                found.push(*pipeline);
            }
        }
        found
    }

    /// **`r_ssao_technique` decides which shader the gather runs**, read off the
    /// pipeline the `ssao` pass bound.
    ///
    /// The observable `docs/backlog.md` says the uniform block cannot have. The
    /// reference backend records a bind group as a handle and a layout rather
    /// than as the resources behind it, so nothing can ask which *block* a march
    /// read — but a pipeline is recorded as itself, so which *technique* a march
    /// ran is a question this backend answers. `Ssao::gathers` is what names the
    /// two, so this is an equality against the pipeline built from each shader
    /// rather than an inequality that any two handles would satisfy.
    ///
    /// **The default arm is the load-bearing half.** A selector that read the
    /// wrong variable, or a `pipeline_for` whose arms were swapped, would still
    /// change the frame when the console moved; what would go with it is every
    /// golden in this workspace, which is blessed on the technique that ships.
    ///
    /// Holds [`ssao_blur_switch`] for its own reason and
    /// [`ssao_technique_switch`] third, after it and [`ssao_split_switch`] —
    /// see `crate::ssao::TECHNIQUE_SWITCH` on the order.
    #[test]
    fn the_occlusion_technique_picks_the_gathers_pipeline() {
        let _blurs = ssao_blur_switch();
        let _seam = ssao_split_switch();
        let technique = ssao_technique_switch();
        // The name to move to, taken out of the variable's own set — see
        // `other_technique_name`.
        let other = other_technique_name();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let (ships, cheap) = renderer.ssao.gathers();
        assert_ne!(
            ships, cheap,
            "the renderer built one gather pipeline for both techniques, so nothing below can \
             tell them apart"
        );

        let shipped = frame_seen_from(
            device,
            &mut renderer,
            queue,
            &Camera::default(),
            &DirectionalLight::default(),
        );
        let by_default = gathers_bound(&recorder, 0);
        let after = recorder.commands().len();
        shipped.release(device);

        technique.set(other);
        let moved_frame = frame_seen_from(
            device,
            &mut renderer,
            queue,
            &Camera::default(),
            &DirectionalLight::default(),
        );
        let moved = gathers_bound(&recorder, after);
        moved_frame.finish(device, renderer);

        assert_eq!(
            by_default,
            vec![ships],
            "a frame nobody has touched the console on gathered through something other than \
             `ssao.slang`'s pipeline, which is what every golden in this workspace was blessed on"
        );
        assert_eq!(
            moved,
            vec![cheap],
            "`r_ssao_technique {other}` left the frame gathering through the pipeline \
             it gathered through before, so the selector reaches nothing"
        );
        recorder.assert_valid();
    }

    /// **The comparison seam's two marches run two techniques**, which is what
    /// the seam was built for.
    ///
    /// `docs/plan/sample/19-alcove.md`'s second milestone is SSAO and GTAO in
    /// one frame, and this is that claim as a pair of pipeline handles: with the
    /// technique moved the near side runs the cheap tier and the far side the
    /// one that ships, and with it at its default both marches run the same
    /// shader and the seam separates nothing.
    ///
    /// **The control is the frame at the default**, and it is what makes the
    /// first half mean anything: a far side that always ran `ssao.slang` would
    /// satisfy the inequality below whatever `shipped_technique` did, and a
    /// chain that had simply forgotten to pass the far side a pipeline of its
    /// own would too.
    #[test]
    fn the_comparison_seam_marches_through_two_techniques() {
        let _blurs = ssao_blur_switch();
        let seam = ssao_split_switch();
        let technique = ssao_technique_switch();
        let other = other_technique_name();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let (ships, cheap) = renderer.ssao.gathers();

        seam.set(0.5);
        let compared = frame_seen_from(
            device,
            &mut renderer,
            queue,
            &Camera::default(),
            &DirectionalLight::default(),
        );
        let one_technique = gathers_bound(&recorder, 0);
        let after = recorder.commands().len();
        compared.release(device);

        technique.set(other);
        let frame = frame_seen_from(
            device,
            &mut renderer,
            queue,
            &Camera::default(),
            &DirectionalLight::default(),
        );
        let two_techniques = gathers_bound(&recorder, after);
        frame.finish(device, renderer);

        assert_eq!(
            one_technique,
            vec![ships, ships],
            "a seam with the technique at its default has to march the shipped gather twice — \
             both sides read `shipped`'s configuration, and the picture is one technique either \
             side of a line"
        );
        assert_eq!(
            two_techniques,
            vec![cheap, ships],
            "with `r_ssao_technique {other}` the near side has to run the cheap tier \
             and the far side what ships; the far side follows the variable's *default*, exactly \
             as the four scalars do"
        );
        recorder.assert_valid();
    }

    /// **The frame block carries the console's shadow filter on the near side
    /// of the seam, the shipped one on the far side, and the column
    /// [`crate::split::halves`] counted.**
    ///
    /// [`the_comparison_seam_marches_through_two_techniques`]'s claim for the
    /// other seam, and it has to be a claim about *bytes* rather than about
    /// pass handles: the occlusion chain compares two pipelines and this
    /// compares two arms of one, so there is nothing in the pass list to look
    /// at. What a person sees is decided entirely by the row this asserts.
    ///
    /// **The frame at the default is the control**, and it is what makes the
    /// rest mean anything: a block that always wrote the shipped filter into
    /// both lanes would satisfy the third case's far side whatever
    /// `shipped_filter` did, and one that copied the near lane into the far one
    /// would satisfy the first two.
    ///
    /// The column is compared against `halves`' own answer rather than against
    /// a number written here, which is the seam module's whole point: a test
    /// that multiplied the fraction by the width itself would be a second
    /// opinion about the rounding, and the floor is exactly what that module's
    /// `the_seam_takes_the_floor_of_the_column_it_asks_for` exists to pin.
    ///
    /// [`the_comparison_seam_marches_through_two_techniques`]: fn@the_comparison_seam_marches_through_two_techniques
    #[test]
    fn the_frame_block_carries_a_shadow_filter_for_each_side_of_the_seam() {
        let switch = shadow_filter_switch();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        // The seam is counted over the extent the scene is *drawn* at, which is
        // what `SV_Position.x` is denominated in — not the caller's window.
        let internal = renderer.internal_extent(TEST_EXTENT);
        let (left, _) = crate::split::halves(internal, 0.5).expect("a seam down the middle");
        assert!(
            left.width > 0,
            "a seam with no columns on one side would make every assertion below hold of a \
             frame that compares nothing"
        );

        let ships = shadow::shipped_filter();
        let other = shadow::Filter::Box;
        assert_ne!(
            other.mode(),
            ships.mode(),
            "the filter this test moves the console to is the one that ships, so the seam it \
             asks for compares one kernel against itself"
        );

        let draw = |renderer: &mut ForwardRenderer| {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    TEST_EXTENT,
                )
                .expect("write");
            let bytes = recorder
                .buffer_bytes(renderer.uniforms[renderer.frame])
                .expect("this frame's uniform block is live");
            let at = mesh::FRAME_UNIFORMS_SIZE - 16;
            core::array::from_fn(|lane| {
                u32::from_le_bytes(
                    bytes[at + lane * 4..at + lane * 4 + 4]
                        .try_into()
                        .expect("four bytes"),
                )
            })
        };

        let shipped: [u32; 4] = draw(&mut renderer);
        assert_eq!(
            shipped,
            [ships.mode(), ships.mode(), 0, 0],
            "a frame nobody has touched the console on must carry the shipped filter in both \
             lanes and no column, which is the row every golden in this workspace was blessed \
             under"
        );

        switch.filter(other);
        let unsplit: [u32; 4] = draw(&mut renderer);
        assert_eq!(
            unsplit,
            [other.mode(), other.mode(), 0, 0],
            "`r_shadow_filter {}` with no seam has to reach every pixel, so both lanes carry it",
            other.label()
        );

        switch.seam(0.5);
        let compared: [u32; 4] = draw(&mut renderer);
        assert_eq!(
            compared,
            [other.mode(), ships.mode(), left.width, 0],
            "a seam at 0.5 has to put the console's filter left of `halves`' column and the \
             shipped one right of it; the far side follows the variable's *default*, exactly \
             as the occlusion chain's does"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    #[test]
    fn a_second_occlusion_blur_is_a_pass_the_frame_records() {
        let switch = ssao_blur_switch();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device);
        let pool = crate::TransientPool::new();

        let once = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        let counted_once = renderer.counters().instances;
        assert!(
            once.contains(&"ssao-blur".to_string()),
            "the shipping frame has to blur once, or the arm below is about a frame with no \
             occlusion in it at all: {once:#?}"
        );
        assert!(
            !once.contains(&"ssao-blur-2".to_string()),
            "a frame nobody moved the switch for must record one blur: {once:#?}"
        );

        switch.set(2);
        let twice = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        let counted_twice = renderer.counters().instances;
        assert!(
            twice.contains(&"ssao-blur-2".to_string()),
            "`r_ssao_blur_passes 2` must put a second blur in the frame: {twice:#?}"
        );
        assert_eq!(
            twice.len(),
            once.len() + 1,
            "the second blur is the only pass that may join the frame: {twice:#?}"
        );
        assert_eq!(
            counted_twice,
            counted_once + FULLSCREEN_DRAWS,
            "the frame recorded a full-screen triangle its counter did not report"
        );

        // The pass has to write somewhere the forward pass then reads, and the
        // graph is what would refuse a frame that bound an image nothing wrote.
        // `passes_in_a_frame` compiles rather than executes, so this is the
        // check that the third transient was requested at all.
        assert!(
            twice.iter().any(|pass| pass == "forward"),
            "the frame still has to reach its forward pass: {twice:#?}"
        );
        recorder.assert_valid();
    }

    /// Every host-written buffer at the end of a whole ring of frames, keyed by
    /// the handle written and holding what that handle last held.
    ///
    /// A whole ring rather than one frame, because the blocks a frame writes are
    /// themselves rings — a buffer per frame in flight, [`Ssao`]'s among them —
    /// so one frame touches one slot of each and two single-frame batches would
    /// be comparing different buffers. A ring writes every slot, so two batches'
    /// keys line up and a difference between them is a difference in *bytes*.
    ///
    /// Only the events this call appends are read, so a caller can take one
    /// batch after another off the same recorder.
    fn host_buffers_over_a_ring(
        recorder: &Recorder,
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        queue: QueueHandle,
    ) -> std::collections::BTreeMap<u64, Vec<u8>> {
        let from = recorder.events().len();
        for _ in 0..FRAMES_IN_FLIGHT {
            frame(device, renderer, queue).release(device);
        }
        recorder.events()[from..]
            .iter()
            .filter_map(|event| match event {
                Event::BufferWritten { buffer, .. } => Some(*buffer),
                _ => None,
            })
            .map(|buffer| {
                let bytes = recorder
                    .buffer_bytes(buffer)
                    .expect("the stream only names buffers this recorder created");
                (buffer.to_bits(), bytes)
            })
            .collect()
    }

    /// **`r_ssao_slices` reaches the block `ssao.slang` reads, on every frame of
    /// the ring, and moves nothing else in the frame.**
    ///
    /// The device test drives the two occlusion pipelines directly and
    /// [`a_second_occlusion_blur_is_a_pass_the_frame_records`] moves the *blur*
    /// count, so until this the slice count was a variable with a `get_i64` and
    /// no evidence that a rendered frame ever carried it: `slice_count` could
    /// have returned its default unconditionally and every check in this
    /// workspace would still be green.
    ///
    /// So the observable is the bytes, taken off the null recorder's write log
    /// after a whole ring of frames at each setting and compared **by handle**.
    /// Two claims come out of that comparison and both are load-bearing:
    ///
    /// * **Some block moved** — every ring slot, in fact, which is what says the
    ///   count reaches the frame at all rather than only the slot the last frame
    ///   happened to land in.
    /// * **Every block that moved, moved only there.** A frame is a few dozen
    ///   host writes and the slice count has business with exactly one word of
    ///   one of them; a setting that perturbed a matrix, an instance or a shadow
    ///   view would be drawing a different frame for a reason nobody asked for,
    ///   and the "at least one differs" half alone would pass on it happily.
    #[test]
    fn four_slices_reach_the_frame_the_shader_reads() {
        let switch = ssao_slice_switch();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        // A ring rendered before either batch is taken, because the two below
        // are compared by handle and the renderer's opening frame writes blocks
        // no later frame writes again: it fills a cold shadow atlas, so the
        // shadow views' uniform blocks and the draw-generation blocks of the
        // cascades that were drawn are written once and never again while the
        // cache holds. Those would be in the first batch and missing from the
        // second, and a key that is only in one of them is a difference this
        // test cannot say anything about.
        for _ in 0..FRAMES_IN_FLIGHT {
            frame(device, &mut renderer, queue).release(device);
        }
        let shipping = host_buffers_over_a_ring(&recorder, device, &mut renderer, queue);
        switch.set(i64::from(ssao::SLICE_COUNT_MAX));
        let asked = host_buffers_over_a_ring(&recorder, device, &mut renderer, queue);
        assert_eq!(
            shipping.keys().collect::<Vec<_>>(),
            asked.keys().collect::<Vec<_>>(),
            "the two batches must write the same buffers, or they are not comparable",
        );

        // Which bytes moving the count alone is allowed to move, asked of
        // `SsaoParams::to_bytes` rather than written here as an offset: the
        // block's layout is `crcbl_shaders::ssao`'s, and a field inserted ahead
        // of the count would leave a number spelled here pointing at the radius.
        let block_of = |slices| {
            ssao::SsaoParams {
                inv_proj: [0.0; 16],
                proj: [0.0; 16],
                inv_view: [0.0; 16],
                radius: 0.0,
                slices,
                intensity: ssao::INTENSITY_DEFAULT,
                bent_normals: crate::ssao::bent_normals(),
            }
            .to_bytes()
        };
        let at_shipping = block_of(ssao::SLICE_COUNT_DEFAULT);
        let at_max = block_of(ssao::SLICE_COUNT_MAX);
        let count_word: Vec<usize> = (0..ssao::PARAMS_SIZE)
            .filter(|at| at_shipping[*at] != at_max[*at])
            .collect();

        let mut moved = 0;
        for (handle, before) in &shipping {
            let after = &asked[handle];
            if before == after {
                continue;
            }
            moved += 1;
            let mut only_the_count = before.clone();
            for at in &count_word {
                assert_eq!(
                    before[*at], at_shipping[*at],
                    "buffer {handle} differs between the batches but did not go in holding the \
                     shipping slice count, so this is some other write moving",
                );
                only_the_count[*at] = at_max[*at];
            }
            assert_eq!(
                after, &only_the_count,
                "buffer {handle} moved somewhere other than the slice count's word: the setting \
                 is perturbing a block it has no business in",
            );
        }
        assert_eq!(
            moved, FRAMES_IN_FLIGHT,
            "`r_ssao_slices 4` must reach every slot of the occlusion ring; a count that moved \
             none of them is a console variable no rendered frame reads",
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **The debug draw layer costs a frame nothing until something is appended
    /// *and* the console switch is on, and then it is the pass immediately
    /// before the tonemap.**
    ///
    /// Four claims, and each is a separate way to get this wrong.
    ///
    /// * **Off by default** is what keeps every golden image in the workspace
    ///   where it was blessed.
    /// * **A segment appended while the switch is off records nothing**, so a
    ///   system may append unconditionally.
    /// * **The switch on with an empty buffer records nothing either** — this is
    ///   the "costs nothing when empty" claim as a check rather than a sentence:
    ///   the compiled pass list is compared whole against the frame drawn before
    ///   the layer was switched on, so a pass that clears, blits or barriers
    ///   anything would show up here.
    /// * **Before the tonemap**, which is
    ///   `docs/plan/18-render-features.md`'s interaction rule and the opposite of
    ///   the ground grid above. Recorded a line later it would draw into the
    ///   display-space target and be free of the exposure, which compiles and
    ///   draws lines and is the wrong picture.
    ///
    /// And the fifth, which is what "immediate mode" means: the frame *after*
    /// the one that appended records nothing again, because
    /// [`ForwardRenderer::begin_frame`] cleared the buffer.
    ///
    /// The whole list is compared rather than its length, for the ground grid
    /// test's reason: one more pass is satisfied by gaining the wrong one.
    ///
    /// **This test owns `r_debug_draw` for its duration**, through
    /// [`debug_draw_switch`]: a plain `cargo test` run shares one process
    /// between tests, so the two checks here that move it have to take turns.
    /// `cargo nextest`, which CI runs, gives each a process of its own and needs
    /// none of it.
    #[test]
    fn the_debug_draw_layer_is_off_by_default_and_lands_before_the_tonemap() {
        let switch = debug_draw_switch();
        let _blurs = ssao_blur_switch();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        assert!(
            !crate::DebugDraw::enabled(),
            "the layer must be off before anything switches it on, or every claim below is \
             about a different default"
        );
        let off = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        assert!(
            !off.contains(&"debug-draw".to_string()),
            "the layer must not be in a frame nobody switched it on for: {off:#?}"
        );

        renderer
            .debug_draw()
            .line(Vec3::ZERO, Vec3::X, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(
            passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT),
            off,
            "a segment appended while the switch is off must record no pass"
        );

        switch.set(true);
        assert_eq!(
            passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT),
            off,
            "the switch on with an empty buffer must record the frame the layer's absence would"
        );

        renderer
            .debug_draw()
            .aabb(-Vec3::ONE, Vec3::ONE, [0.0, 1.0, 0.0, 1.0]);
        let on = passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        let mut expected = off.clone();
        let tonemap = expected
            .iter()
            .position(|label| label == "tonemap")
            .expect("a frame has to reach the swapchain");
        expected.insert(tonemap, "debug-draw".to_string());
        assert_eq!(
            on, expected,
            "the layer must be the one pass gained, and it must sit before the tonemap"
        );

        assert_eq!(
            passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT),
            off,
            "the buffer is immediate mode: the frame after the one that appended must record \
             nothing"
        );

        switch.set(false);
        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The debug draw layer's own call is counted in the frame's draws.**
    ///
    /// [`ForwardRenderer::counters`]' draw count is compared against the
    /// commands the null backend actually recorded, which is the same instrument
    /// `the_forward_counters_are_the_recorded_draws_and_admit_what_they_cannot_know`
    /// uses — so a layer whose line list was recorded and not counted fails here
    /// rather than quietly making every panel's draw row one short.
    ///
    /// Both directions are asserted: the frame that drew a segment records one
    /// more draw than the frame before it, *and* the counter moved by the same
    /// one. A count wired to a constant satisfies neither.
    ///
    /// It owns `r_debug_draw` for its duration, on
    /// `the_debug_draw_layer_is_off_by_default_and_lands_before_the_tonemap`'s
    /// terms.
    #[test]
    fn the_debug_draw_layers_call_is_counted_in_the_frames_draws() {
        let switch = debug_draw_switch();
        let _blurs = ssao_blur_switch();

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        // A different sun in each of the two frames, so both of them redraw the
        // shadow atlas: the claim below is that the layer's own call is the one
        // draw the second frame gained, and a second frame that cached its
        // atlas would have lost the atlas's draws at the same time. See
        // [`moving_sun`].
        let quiet = frame_moving_the_sun(device, &mut renderer, queue, 0);
        let without = renderer.counters().draws;
        assert_eq!(
            without,
            recorded_draws(&recorder) as u64,
            "the counter and the frame's recorded draws disagree before the layer draws at all"
        );
        quiet.release(device);

        switch.set(true);
        renderer
            .debug_draw()
            .line(Vec3::ZERO, Vec3::X, [1.0, 1.0, 1.0, 1.0]);
        let drawn = frame_moving_the_sun(device, &mut renderer, queue, 1);
        let with = renderer.counters().draws;
        assert_eq!(
            with,
            without + 1,
            "the layer records one draw, so the count has to move by one"
        );
        assert_eq!(
            recorded_draws(&recorder) as u64,
            without + with,
            "the two frames' recorded draws and the two counts disagree, so the layer's call is \
             counted somewhere it was not recorded or recorded somewhere it was not counted"
        );

        switch.set(false);
        drawn.finish(device, renderer);
        recorder.assert_valid();
    }

    /// **The depth-only passes run the depth-only entry point, and the colour
    /// pass does not.**
    ///
    /// `docs/plan/43-render-standards.md` §2 split the vertex pool so a pass
    /// that wants a clip position and no more could fetch stream 0 alone, and
    /// [`MeshModules::depth_pipeline`] naming `mesh.slang`'s `depthVertexMain`
    /// is the whole of how that split is spent. **Nothing else in the tree sees
    /// it.** `crcbl_shaders::mesh`'s
    /// `the_depth_entry_point_fetches_the_position_stream_alone` reads the
    /// compiled artifact and would pass with the entry point built and unused;
    /// no golden moves, because the two stages compute the same clip position on
    /// purpose; the render graph records a legal frame either way; and the
    /// measured price says nothing — pointing this pipeline back at `vertexMain`
    /// cost the depth prepass about a tenth more on lavapipe and nothing to
    /// three figures on an RX 7900 XTX, measured 2026-08-31. So the wiring is
    /// asserted here, against the reference backend, which records the entry
    /// point a pipeline names precisely so this question has an answer.
    ///
    /// **Both halves, so the check is shown to discriminate.** A lookup that
    /// answered `depthVertexMain` for every pipeline would satisfy the first
    /// assertion alone, and the colour pipeline is the one that must *not* have
    /// moved. The labels are the descriptors' own and are what the two are told
    /// apart by, so a renamed pipeline fails at the `expect` rather than
    /// silently matching nothing.
    #[test]
    fn the_depth_pipeline_names_the_depth_only_vertex_stage_and_the_colour_one_does_not() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        // A mesh stage has no vertex entry point to name, and this whole rung is
        // about the raster arm — see [`MeshModules::depth_pipeline`]. The null
        // preset reports no `Features::MESH_SHADER`, so this is what it built;
        // asserted rather than assumed, because a preset that grew the feature
        // would leave every assertion below looking for pipelines that do not
        // exist.
        assert_ne!(
            renderer.geometry_path(),
            crcbl_hal::GeometryPath::MeshShader,
            "this device built a mesh-shader path, whose geometry stage names no vertex \
             entry point at all"
        );

        let pipelines = recorder.graphics_pipeline_vertex_entries();
        let entry_of = |label: &str| -> String {
            let found: Vec<&str> = pipelines
                .iter()
                .filter(|(name, _)| name.as_deref() == Some(label))
                .map(|(_, entry)| entry.as_str())
                .collect();
            assert_eq!(
                found.len(),
                1,
                "the renderer built {} pipeline(s) labelled `{label}`, out of {pipelines:?}",
                found.len()
            );
            found[0].to_owned()
        };

        assert_eq!(
            entry_of("shadow cascade"),
            "depthVertexMain",
            "the shadow atlas and the depth prepass share this pipeline, and it is the one \
             that reads the position stream alone"
        );
        assert_eq!(
            entry_of("forward mesh"),
            "vertexMain",
            "the colour pass needs every attribute the depth pass skips, so it is the entry \
             point that reads a whole vertex"
        );

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// **The wireframe view swaps the colour pass's pipeline and nothing else.**
    ///
    /// The mechanism, at the level a null device can observe it: which pipeline
    /// handle each pass of the recorded stream binds. A field on the renderer
    /// would say only that a flag moved — this says the *frame* moved, and says
    /// which part of it did.
    ///
    /// Four claims, and each rules out a different way of passing wrongly:
    ///
    /// * the filled pipeline is bound on a frame nobody asked, and the wireframe
    ///   one does not exist;
    /// * switching on **removes** the filled pipeline from the stream — a second
    ///   pipeline that was built and never bound would still leave it there;
    /// * every other pipeline the frame binds is unchanged, so the depth prepass
    ///   and the shadow cascades are still filling triangles;
    /// * and switching off puts the stream back exactly, so the view is a toggle
    ///   rather than a one-way door.
    #[test]
    fn the_wireframe_view_swaps_the_colour_passs_pipeline_and_no_other() {
        use crcbl_hal::null::Command;

        // The pipelines a frame binds include one per occlusion blur, and the
        // blur count is a process-global console variable — so a concurrent
        // test holding `r_ssao_blur_passes` at two adds a bind to one of the
        // frames below and not to the others. It reddened this assertion on
        // 2026-08-31 under a workspace `cargo test`.
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) =
            open_with(Features::GPU_DRIVEN | Features::POLYGON_MODE_LINE);
        let device = device.as_ref();
        assert!(
            ForwardRenderer::supports_wireframe(device),
            "this test needs the line fill mode, and asked the device for it",
        );
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        place_demo(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, Mat4::IDENTITY);
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        // The pipelines one frame binds, in record order. Cumulative recorder,
        // so each call takes the tail its own frame added.
        let mut seen = 0usize;
        // A different sun each lap, so the shadow atlas is redrawn in every one
        // of them: the comparison below is "which pipelines did this frame
        // bind", and a lap that cached its atlas would bind the depth-only
        // pipeline one fewer time for a reason that has nothing to do with the
        // wireframe. See [`moving_sun`].
        let mut lap = 0u32;
        let mut pipelines_in_a_frame = |renderer: &mut ForwardRenderer, pool: &mut _| {
            lap += 1;
            renderer
                .begin_frame(device, &Camera::default(), &moving_sun(lap), (64, 48))
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, pool, target, (64, 48));
            let compiled = graph.compile(pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("wireframe lap"),
                queue,
            });
            compiled
                .execute(device, pool, encoder.as_mut(), None)
                .expect("the graph executed");
            encoder.finish().expect("recording succeeded");
            let commands = recorder.commands();
            let bound: Vec<GraphicsPipelineHandle> = commands[seen..]
                .iter()
                .filter_map(|command| match command {
                    Command::BindGraphicsPipeline(handle) => Some(*handle),
                    _ => None,
                })
                .collect();
            seen = commands.len();
            bound
        };

        let filled = renderer.mesh_pipeline;
        let off = pipelines_in_a_frame(&mut renderer, &mut pool);
        assert!(
            off.contains(&filled),
            "a frame nobody asked has to bind the filled pipeline: {off:?}",
        );
        assert!(!renderer.wireframe());
        assert!(
            renderer.wireframe_pipeline.is_none(),
            "the wireframe pipeline must not be built until it is asked for",
        );

        renderer
            .set_wireframe(device, true)
            .expect("a device with the line fill mode builds it");
        assert!(renderer.wireframe());
        let lines = renderer
            .wireframe_pipeline
            .expect("switching on is what builds it");
        let on = pipelines_in_a_frame(&mut renderer, &mut pool);
        assert!(
            !on.contains(&filled),
            "the colour pass still bound the filled pipeline: {on:?}",
        );
        assert_eq!(
            on.iter().filter(|handle| **handle == lines).count(),
            1,
            "the wireframe pipeline has to be bound exactly once — by the colour pass: {on:?}",
        );
        let substituted: Vec<GraphicsPipelineHandle> = on
            .iter()
            .map(|handle| if *handle == lines { filled } else { *handle })
            .collect();
        assert_eq!(
            substituted, off,
            "the frame gained or lost a pipeline bind beyond the colour pass's: {on:?} against \
             {off:?}",
        );

        renderer
            .set_wireframe(device, false)
            .expect("switching off builds nothing");
        assert!(!renderer.wireframe());
        assert_eq!(
            pipelines_in_a_frame(&mut renderer, &mut pool),
            off,
            "switching the view off has to put the filled pipeline back, not just stop reporting \
             it",
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The colour pass tests against the prepass's depth instead of clearing
    /// and rewriting it, so each pixel is shaded once** — and a wireframe frame
    /// does the opposite, because lines do not reproduce a filled prepass.
    ///
    /// Both halves have to hold together or the frame is wrong in one of two
    /// ways, so both are asserted here. The attachment alone read-only while the
    /// pipeline still wrote would be a pipeline writing to a read-only
    /// attachment; the pipeline alone on `GreaterOrEqual` against an attachment
    /// this pass had just *cleared* would reject every fragment and draw a black
    /// frame. Neither shows up as an error — the first is a validation message
    /// on one backend and undefined behaviour on the rest, the second is a
    /// picture — which is why the observable is read off the compiled graph and
    /// off the pipeline state rather than off a screenshot.
    #[test]
    fn the_colour_pass_shades_once_per_pixel_against_the_prepasss_depth() {
        let (recorder, device, queue) =
            open_with(Features::GPU_DRIVEN | Features::POLYGON_MODE_LINE);
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        place_demo(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, Mat4::IDENTITY);
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        // Each pass's depth attachment, off the graph's own dump: the label it
        // was declared under, and the `load`/`store`/`write` triple the backend
        // is handed. A pass with no depth attachment contributes nothing, which
        // is what makes the lookup below a search rather than an index.
        let depth_of = |renderer: &mut ForwardRenderer, pool: &mut _, label: &str| {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 48),
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, pool, target, (64, 48));
            let dump = graph.compile(pool).expect("a legal frame").dump();
            let mut pass = String::new();
            let mut found = None;
            for line in dump.lines() {
                if let Some(rest) = line.split(" pass \"").nth(1) {
                    pass = rest.trim_end_matches('"').to_string();
                    if let Some((name, _)) = pass.split_once('"') {
                        pass = name.to_string();
                    }
                }
                if pass == label
                    && let Some(at) = line.find("depth load=")
                {
                    // Without the slot note the dump appends: which physical
                    // image the transient landed on is aliasing's business, not
                    // this assertion's.
                    let role = &line[at..];
                    let role = role.split_once(" [").map_or(role, |(head, _)| head);
                    found = Some(role.trim().to_string());
                }
            }
            found.unwrap_or_else(|| panic!("no depth attachment on `{label}`:\n{dump}"))
        };

        assert_eq!(
            depth_of(&mut renderer, &mut pool, "depth-prepass"),
            "depth load=Clear store=Store write=yes",
            "the prepass is what puts the depth there, so it clears, writes and keeps it",
        );
        assert_eq!(
            depth_of(&mut renderer, &mut pool, "forward"),
            "depth load=Load store=Store write=no",
            "and the colour pass reads it: a clear here would throw away the prepass and \
             pay for the overdraw again, and a write would be a second pass writing the \
             same values",
        );
        assert_eq!(
            MeshModules::color_depth_stencil(PolygonMode::Fill),
            Some(DepthStencilState {
                format: Format::D32Float,
                depth_write: false,
                depth_compare: CompareOp::GreaterOrEqual,
                stencil: None,
                bias: DepthBias::default(),
            }),
            "and the pipeline agrees with the attachment: equality against the prepass, \
             reachable only through the `OrEqual` under reversed-Z",
        );

        renderer
            .set_wireframe(device, true)
            .expect("this device was asked for the line fill mode");
        assert_eq!(
            depth_of(&mut renderer, &mut pool, "forward"),
            "depth load=Clear store=Store write=yes",
            "a wireframe frame keeps the old shape: `PolygonMode::Line` interpolates depth \
             along the edge rather than across the face, so testing it against a filled \
             prepass rejects most of the wireframe — and *loading* that depth under the \
             writing default's `Greater` would reject all of it",
        );
        assert_eq!(
            MeshModules::color_depth_stencil(PolygonMode::Line),
            MeshModules::depth_stencil(),
            "which is the writing default, the same state the depth-only twin uses",
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **A device with no line fill mode is refused, not quietly filled.**
    ///
    /// The failure this rules out is the silent one: a caller who binds a key to
    /// this, presses it, and gets the solid frame back with nothing anywhere
    /// saying why — which on a model whose silhouette hides its interior is
    /// indistinguishable from a wireframe that worked. WebGPU is the real device
    /// this happens on; `open`'s null device stands in for it, because
    /// [`DeviceDesc::for_adapter`] asks for
    /// [`Features::GPU_DRIVEN`] and nothing else.
    #[test]
    fn a_device_without_the_line_fill_mode_refuses_the_wireframe_rather_than_filling_it() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        assert!(
            !ForwardRenderer::supports_wireframe(device),
            "this device was not asked for the line fill mode, so it must not report it",
        );
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

        let refused = renderer
            .set_wireframe(device, true)
            .expect_err("a device without the feature has to say so");
        assert!(
            matches!(
                refused,
                HalError::UnsupportedFeatures { missing } if missing == Features::POLYGON_MODE_LINE
            ),
            "the refusal has to name the feature to go and look up, and is {refused:?}",
        );
        assert!(
            !renderer.wireframe(),
            "a refused request must leave the view off rather than half on",
        );
        assert!(
            renderer.wireframe_pipeline.is_none(),
            "nothing may be left behind by a refusal",
        );
        // And switching *off* is still fine on such a device: it builds nothing.
        renderer
            .set_wireframe(device, false)
            .expect("switching off asks the device for nothing");

        renderer.destroy(device);
        recorder.assert_valid();
    }

    /// The grid's own full-screen triangle is counted as submitted, and only
    /// while it is on.
    ///
    /// [`FrameCounters::instances`] is the "submitted" half of the debug panel's
    /// row, and every full-screen pass in the frame contributes one — so a grid
    /// that drew and was not counted makes that row disagree with the frame by
    /// exactly one triangle, silently.
    #[test]
    fn the_grids_triangle_is_counted_only_while_it_is_on() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();

        passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        let off = renderer.counters();

        renderer
            .set_ground_grid(device, Some(GridStyle::default()))
            .expect("the null backend builds every pipeline");
        passes_in_a_frame(device, queue, &mut renderer, imported, &pool, TEST_EXTENT);
        let on = renderer.counters();

        assert_eq!(
            on.draws - off.draws,
            FULLSCREEN_DRAWS,
            "the grid records exactly one draw"
        );
        assert_eq!(
            on.instances - off.instances,
            FULLSCREEN_DRAWS,
            "and exactly one submitted instance"
        );

        renderer.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// One frame on a device of its own, under an override switching `off`.
    ///
    /// **A device of its own per arm**, because a [`Recorder`] accumulates for a
    /// device's whole life: asking "did the atlas pass draw" of a recorder that
    /// has seen two frames answers about both of them, and the arm that matters
    /// most here is the one whose answer must be zero.
    ///
    /// Returns what the frame reported and what its atlas pass actually drew.
    fn frame_switching_off(off: RenderEffects) -> (FrameCounters, u64, usize) {
        use crcbl_hal::null::Command;

        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let opened = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let device = opened.as_ref();
        let queue = opened.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        // A shadowed light beside the cascades, so the atlas pass has draws to
        // lose rather than being empty either way.
        renderer.set_lights(&[shadowable_spot(-1.0)]);
        // **[`OUTSIDE_THE_DEFAULT`] forced on, then `off` forced off.** The default
        // camera stack leaves both of them out — see
        // [`RenderEffects::DEFAULT_STACK`] — so a control frame built on the
        // default alone would be measuring three effects and calling it five,
        // and their arms below would be switching off something already off.
        // `force` clears the other side, so an arm whose `off` is one of them
        // still resolves to off.
        renderer.set_effect_request(EffectRequest {
            programmatic: EffectOverride::none()
                .force(OUTSIDE_THE_DEFAULT, Some(true))
                .force(off, Some(false)),
            ..EffectRequest::default()
        });

        let recorded = frame(device, &mut renderer, queue);
        assert_eq!(
            renderer.effects(),
            RenderEffects::all().difference(off),
            "the frame must have resolved to the set the request asked for"
        );
        let counters = renderer.counters();
        let instances = renderer.instances.len() as u64;
        let atlas_draws = commands_in_pass(&recorder, "shadow")
            .into_iter()
            .filter(|command| {
                matches!(
                    command,
                    Command::DrawIndexedIndirect(_)
                        | Command::DrawIndexedIndirectCount(_)
                        | Command::DrawMeshTasksIndirect(_)
                )
            })
            .count();
        assert_eq!(
            counters.draws,
            recorded_draws(&recorder) as u64,
            "the counter and the frame's own recorded stream must agree, whatever is switched off"
        );
        recorded.finish(device, renderer);
        recorder.assert_valid();
        (counters, instances, atlas_draws)
    }

    /// **A switched-off effect is work the frame did not record**, and a frame
    /// that still executes with the placeholder bound in place of what it lost.
    ///
    /// The other half of the pass-list test above, and the half a pass list
    /// cannot make: the `shadow` pass is still *there* with shadows off, so the
    /// question is whether it drew. That answer comes off the recorded command
    /// stream, which is a different instrument from `recorded_draws` — the number
    /// under test.
    ///
    /// Executed rather than only compiled, because the occlusion switch changes
    /// which image the forward pass has bound. A frame that declared a read of a
    /// transient it never created, or bound a view the graph never realised,
    /// fails here and compiles perfectly.
    #[test]
    fn a_switched_off_effect_is_work_the_frame_did_not_record() {
        let _blurs = ssao_blur_switch();
        let (control, instances, atlas_draws) = frame_switching_off(RenderEffects::empty());
        assert!(
            atlas_draws > 0,
            "with shadows on the atlas pass has to draw something, or turning it off means \
             nothing"
        );
        assert_eq!(
            control.instances,
            instances
                + fullscreen_passes(
                    RenderEffects::all(),
                    TEST_EXTENT,
                    false,
                    crate::ssao::blur_passes(),
                ) * FULLSCREEN_DRAWS,
            "the control frame submits one triangle per full-screen pass"
        );

        // Each pair is one full-screen triangle per pass, and the atlas is
        // untouched by either.
        for (off, fewer) in [
            (
                RenderEffects::AMBIENT_OCCLUSION,
                u64::from(Ssao::passes(crate::ssao::blur_passes())),
            ),
            // The contact march's one pass, and a constant rather than a
            // function of anything: it has no blur behind it and no chain.
            (
                RenderEffects::CONTACT_SHADOWS,
                u64::from(ContactShadows::PASSES),
            ),
            // The march's two passes and the reductions that feed them, which
            // is what makes this arm's number a function of the extent the way
            // the chain's below is.
            (
                RenderEffects::REFLECTIONS,
                u64::from(Ssr::PASSES) + u64::from(crate::hiz::levels_for(TEST_EXTENT)),
            ),
            // Not a constant, unlike the occlusion pair above: the chain's
            // length is a function of the extent, and this is what it is at
            // `TEST_EXTENT`.
            (
                RenderEffects::BLOOM,
                u64::from(Bloom::passes_for(TEST_EXTENT)),
            ),
        ] {
            let (counters, instances, atlas) = frame_switching_off(off);
            assert_eq!(
                counters.draws,
                control.draws - fewer * FULLSCREEN_DRAWS,
                "{off:?}: the frame must record one draw fewer per pass it did not add"
            );
            assert_eq!(
                counters.instances,
                instances
                    + fullscreen_passes(
                        RenderEffects::all().difference(off),
                        TEST_EXTENT,
                        false,
                        crate::ssao::blur_passes(),
                    ) * FULLSCREEN_DRAWS,
                "{off:?}: and must not report as submitted a triangle it never submitted"
            );
            assert_eq!(
                atlas, atlas_draws,
                "{off:?}: a screen-space effect must not touch the shadow atlas"
            );
        }

        // Shadows: the atlas pass survives and draws nothing at all, which is
        // the mechanism — a cleared reversed-Z atlas reads as fully lit.
        let (counters, _, atlas) = frame_switching_off(RenderEffects::SHADOWS);
        assert_eq!(
            atlas, 0,
            "with shadows off the atlas pass must record its clear and no draw"
        );
        assert_eq!(
            counters.draws,
            control.draws - atlas_draws as u64,
            "and the frame must record exactly the atlas pass's draws fewer"
        );
    }

    /// **Every pass the frame records gets a row in the report, except the ones
    /// that open no scope.**
    ///
    /// The observable the bound exists for: the samples' hand-picked capacity
    /// timed the first eight passes of a fourteen-pass frame and reported eight
    /// rows, which reads exactly like a frame that has eight passes. Sized from
    /// [`MAX_TIMED_PASSES`](crate::MAX_TIMED_PASSES), the report names every
    /// pass the graph compiled, in order — so a bound one short of the frame
    /// makes this list short too.
    ///
    /// A [`PassKind::Copy`](crate::graph::PassKind::Copy) is filtered out of the
    /// expectation rather than expected to be missing by accident. The seam
    /// takes a timestamp only where a pass opens and closes and a copy opens
    /// nothing, so `PassTimers` gives it no query pair and no row — a row
    /// reading 0.000 ms would be a measurement nobody made.
    #[test]
    fn every_pass_of_the_frame_gets_a_timing_row() {
        let _blurs = ssao_blur_switch();
        // And the comparison seam, so the frame this checks carries the
        // occlusion chain's second march: `docs/plan/ROADMAP.md`'s S4D row asks
        // for a *per-technique* cost, and what makes that true is this test's
        // own claim — every pass that opens a scope gets a row — applied to a
        // frame that compares. See `crate::ssao`'s `r_ssao_split`.
        let split = ssao_split_switch();
        split.set(0.5);
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        // `TIMESTAMP_QUERY` is not part of `GPU_DRIVEN` — topic 10's browsers
        // may lack it — so a device that wants timers has to ask for it, and
        // `open` above, which uses `DeviceDesc::for_adapter`, does not.
        let opened = instance
            .create_device(&DeviceDesc {
                label: Some("timed frame"),
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::TIMESTAMP_QUERY,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = opened.queue(QueueKind::Graphics).expect("always present");
        let device = opened.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        renderer.set_lights(&[shadowable_spot(-1.0), shadowable_spot(1.0)]);
        let mut timers = crate::PassTimers::new(device, FRAMES_IN_FLIGHT, crate::MAX_TIMED_PASSES)
            .expect("the tier A null adapter has timestamp queries");
        let imported = swapchain_image(device);
        let mut pool = crate::TransientPool::new();
        let mut recorded = Vec::new();
        let mut compiled_labels = Vec::new();

        // The ring is one slot longer than the frames in flight and resolves a
        // slot only when it comes round again, so the first frame's report is
        // that many frames behind it — see the [`timing`](crate::timing) docs.
        for _ in 0..FRAMES_IN_FLIGHT + 2 {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 48),
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, &pool, target, (64, 48));
            let compiled = graph.compile(&pool).expect("a legal frame");
            if compiled_labels.is_empty() {
                compiled_labels = compiled
                    .passes()
                    .iter()
                    .filter(|pass| pass.kind() != crate::graph::PassKind::Copy)
                    .map(|pass| pass.label().to_string())
                    .collect();
            }
            let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("timed frame"),
                queue,
            });
            compiled
                .execute(device, &mut pool, encoder.as_mut(), Some(&mut timers))
                .expect("the graph executed");
            recorded.push(encoder.finish().expect("recording succeeded"));
        }

        let reported: Vec<&str> = timers
            .latest()
            .passes
            .iter()
            .map(|timing| timing.label.as_str())
            .collect();
        // Anti-vacuity for the seam held above: a frame that did not compare
        // would satisfy the equality below and say nothing about a
        // per-technique cost, because there would be one technique in it.
        for label in ["ssao", "ssao-shipped"] {
            assert!(
                compiled_labels.iter().any(|pass| pass == label),
                "the frame compiled no `{label}` pass, so the seam did not reach it and this \
                 test says nothing about a comparison having a cost per side. Its passes: \
                 {compiled_labels:?}"
            );
        }
        assert_eq!(
            reported,
            compiled_labels,
            "the report must name every pass the frame compiled that opens a scope, in order \
             — {} of {} timed with a capacity of {}",
            reported.len(),
            compiled_labels.len(),
            timers.capacity()
        );

        timers.destroy(device);
        renderer.destroy(device);
        for commands in recorded {
            device.destroy_command_buffer(commands);
        }
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// One mesh-table entry, decoded from the bytes that reached the device.
    fn mesh_entry(
        recorder: &Recorder,
        renderer: &ForwardRenderer,
        id: u32,
    ) -> crcbl_shaders::mesh::GpuMesh {
        let table = recorder
            .buffer_bytes(renderer.pool.table_buffer())
            .expect("the table is live");
        let at = id as usize * crcbl_shaders::mesh::MESH_ENTRY_STRIDE;
        crcbl_shaders::mesh::GpuMesh::from_bytes(
            table[at..at + crcbl_shaders::mesh::MESH_ENTRY_STRIDE]
                .try_into()
                .expect("one whole entry"),
        )
    }

    // --- skinning: the seam a caller draws a skinned mesh through -----------

    /// A renderer with [`scene::demo`]'s cube reserved for skinning, and a
    /// [`Skinning`] built against that renderer's own vertex pool.
    struct SkinnedFixture {
        renderer: ForwardRenderer,
        skinning: Skinning,
        skinned: SkinnedMesh,
        palette: Vec<Mat4>,
        bindings: Vec<crcbl_shaders::skinning::SkinBinding>,
    }

    impl SkinnedFixture {
        fn build(device: &dyn Device, queue: QueueHandle) -> Self {
            let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
                .expect("the null backend accepts every descriptor");
            let skinned = renderer
                .reserve_skinned(DEMO_CUBE)
                .expect("a default pool has room for two halves of a cube");
            let skinning = Skinning::new(
                device,
                &crate::skinning::SkinningDesc {
                    label: Some("skinned fixture"),
                    frames: FRAMES_IN_FLIGHT,
                    ranges: 1,
                    joints: 1,
                    bindings: skinned.vertex_count(),
                    // The renderer's own pool, which is the whole of what makes
                    // the dispatch's output reachable by its draws.
                    vertices: renderer.vertex_buffer(),
                    attribute_base: renderer.attribute_base(),
                },
            )
            .expect("the null backend accepts every descriptor");
            let bindings = vec![
                crcbl_shaders::skinning::SkinBinding {
                    joints: [0; crcbl_shaders::skinning::JOINTS_PER_VERTEX],
                    weights: [1.0, 0.0, 0.0, 0.0],
                };
                skinned.vertex_count() as usize
            ];
            Self {
                renderer,
                skinning,
                skinned,
                palette: vec![Mat4::IDENTITY],
                bindings,
            }
        }

        /// One skinned frame's `begin_skinned_frame`, with this fixture's single
        /// range.
        fn begin(&mut self, device: &dyn Device) {
            let range = self.skinned.skin_range(&self.palette, &self.bindings);
            self.renderer
                .begin_skinned_frame(
                    device,
                    &mut self.skinning,
                    &[range],
                    &Camera::default(),
                    &DirectionalLight::default(),
                    TEST_EXTENT,
                )
                .expect("a legal skinning plan");
        }

        fn destroy(mut self, device: &dyn Device) {
            self.renderer.release_skinned(self.skinned);
            self.skinning.destroy(device);
            self.renderer.destroy(device);
        }
    }

    /// How many times the host wrote the mesh-table entry `id` names.
    fn table_writes(recorder: &Recorder, renderer: &ForwardRenderer, id: u32) -> usize {
        let table = renderer.pool.table_buffer();
        let at = u64::from(id) * crcbl_shaders::mesh::MESH_ENTRY_STRIDE as u64;
        recorder
            .events()
            .into_iter()
            .filter(|event| {
                matches!(
                    event,
                    crcbl_hal::null::Event::BufferWritten { buffer, offset, .. }
                        if *buffer == table && *offset == at
                )
            })
            .count()
    }

    /// The record `handle` names, out of the ring buffer **this frame draws
    /// from** rather than out of the pool's host mirror.
    ///
    /// A mirror and a buffer that disagree is exactly the defect a re-pointing
    /// design can have, so every claim about a skinned instance's bases is read
    /// off the bytes a shader would.
    fn instance_record(
        recorder: &Recorder,
        renderer: &ForwardRenderer,
        handle: InstanceHandle,
    ) -> mesh::GpuInstance {
        let buffer = renderer.instances.buffers()[renderer.frame()];
        let bytes = recorder.buffer_bytes(buffer).expect("the ring is live");
        let index = renderer
            .instances
            .index(handle)
            .expect("the object is live") as usize;
        let at = index * mesh::INSTANCE_STRIDE;
        mesh::GpuInstance::from_bytes(
            bytes[at..at + mesh::INSTANCE_STRIDE]
                .try_into()
                .expect("one whole instance"),
        )
    }

    /// **A skinned primitive takes no mesh-table entry of its own, and nothing
    /// rewrites the one it draws through.**
    ///
    /// [`crate::mesh_pool`]'s table is one host-visible buffer rather than a ring
    /// precisely because nothing rewrites an entry between frames, so a design
    /// that re-pointed an entry at the half a frame writes would be wrong only
    /// on the frames that overlap — which is to say invisibly. What alternates
    /// instead is a field of the *instance*, and instances go through a ring.
    ///
    /// So a skinned object draws through the source mesh's own entry: the same
    /// id [`add_instance`](ForwardRenderer::add_instance) would give it, still
    /// carrying the bind pose's base vertex, still written exactly once after
    /// four skinned frames. That shared entry is also what lets one mesh be
    /// drawn skinned and unskinned in the same frame.
    #[test]
    fn a_skinned_mesh_takes_no_table_entry_and_rewrites_none() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);

        let bind_pose_id = fixture.renderer.mesh_ids[DEMO_CUBE];
        assert_eq!(
            fixture.skinned.mesh_id(),
            bind_pose_id,
            "a skinned instance names the mesh it was deformed from, so the bucket, the level \
             tables and the bounding box all resolve as the bind pose's"
        );

        let bind_pose = mesh_entry(&recorder, &fixture.renderer, bind_pose_id);
        assert_eq!(
            bind_pose.base_vertex,
            fixture.skinned.input_base(),
            "the entry goes on naming the bind pose the dispatch reads, not either half it \
             writes"
        );
        for parity in 0..2 {
            assert_ne!(
                fixture.skinned.region().base(parity),
                bind_pose.base_vertex,
                "half {parity} must not be the bind pose, which the kernel reads while it \
                 writes them"
            );
        }
        assert_ne!(
            fixture.skinned.region().base(0),
            fixture.skinned.region().base(1),
            "the two halves are separate reservations, or the ping-pong is one buffer"
        );

        // Counted before the frames rather than compared against a literal:
        // `MeshPool::new` clears the whole table with one write at offset zero,
        // which lands on this entry too, so the number here is the pool's
        // start-up and not this test's business. What the loop below has to
        // leave alone is whatever it is.
        let writes_before = table_writes(&recorder, &fixture.renderer, bind_pose_id);
        assert!(
            writes_before > 0,
            "the entry was written at least once before any frame ran, or the counter below \
             is counting nothing and the comparison after the frames is vacuous"
        );

        // Four frames, which is two of each parity: enough that a design
        // rewriting an entry per frame would have written four more times.
        let imported = swapchain_image_at(device, TEST_EXTENT);
        let mut pool = crate::TransientPool::new();
        for _ in 0..4 {
            fixture.begin(device);
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            fixture.renderer.add_skinned_passes(
                &mut graph,
                &pool,
                target,
                TEST_EXTENT,
                &fixture.skinning,
            );
            graph.compile(&pool).expect("a legal frame");
        }

        assert_eq!(
            table_writes(&recorder, &fixture.renderer, bind_pose_id),
            writes_before,
            "entry {bind_pose_id} was written again by a skinned frame; the whole reason this \
             table is one buffer rather than a ring is that nothing rewrites an entry between \
             frames"
        );

        fixture.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **A skinned object's recorded base vertex follows the parity of the frame
    /// it is drawn in**, and it carries the bit that makes a shader read it.
    ///
    /// The other half of the design: the table entry never moves, so what has to
    /// alternate is a field of the *instance* — and it has to alternate in the
    /// buffer the frame actually draws from, which is why this reads the
    /// instance ring's bytes rather than the host's mirror.
    ///
    /// Three claims, and the last two are what stop the first being vacuous. The
    /// base must be the one this frame's parity names; the record must carry
    /// [`GpuInstance::BASE_VERTEX_OVERRIDE`](mesh::GpuInstance::BASE_VERTEX_OVERRIDE),
    /// without which the raster stages read the mesh entry and the field is
    /// decoration; and two consecutive frames must have named **different**
    /// halves, without which a renderer ignoring the parity entirely would pass.
    #[test]
    fn a_skinned_draw_carries_the_base_the_frames_dispatch_filled() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        let handle = fixture
            .renderer
            .add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &fixture.skinned,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            })
            .expect("a pool of thousands has room for one object");

        let mut carried = Vec::new();
        for _ in 0..2 {
            fixture.begin(device);
            let parity = fixture.skinning.parity();
            let instance = instance_record(&recorder, &fixture.renderer, handle);
            assert_eq!(
                instance.base_vertex,
                fixture.skinned.region().base(parity),
                "the buffer this frame draws from must carry the base parity {parity} writes"
            );
            assert_eq!(
                instance.flags & mesh::GpuInstance::BASE_VERTEX_OVERRIDE,
                mesh::GpuInstance::BASE_VERTEX_OVERRIDE,
                "without the bit the raster stages resolve the mesh entry's base and this \
                 field is never read"
            );
            assert_eq!(
                instance.mesh, fixture.renderer.mesh_ids[DEMO_CUBE],
                "and the record still names the source mesh, which is what its bucket, its \
                 levels and its bounding box resolve through"
            );
            carried.push(instance.base_vertex);
        }
        assert_ne!(
            carried[0], carried[1],
            "two consecutive frames must have drawn different halves, or a renderer that \
             ignored the parity entirely would pass this"
        );

        fixture.destroy(device);
        recorder.assert_valid();
    }

    /// **A skinned object's record names the half the frame before it filled**,
    /// and it starts at rest.
    ///
    /// The other half of a skinned motion vector. The base above is where a
    /// fragment *is*; this is where it *was*, and without it the geometry stages
    /// put the previous transform through this frame's deformed vertex — so a
    /// limb that swung reports the motion of the body it hangs off. See
    /// [`GpuInstance::previous_base_vertex`](mesh::GpuInstance::previous_base_vertex).
    ///
    /// Four claims, and the first and last are what stop the middle two being a
    /// restatement of the parity:
    ///
    /// * the **first** frame of a fresh object writes the base it draws out of,
    ///   because the other half holds whatever its reservation came with and a
    ///   motion vector against undefined vertices is worse than no motion;
    /// * the frames after it write
    ///   [`SkinnedRegion::previous_base`](crate::skinning::SkinnedRegion::previous_base),
    ///   which must also **differ** from the base they draw out of — a renderer
    ///   that wrote the current base into both lanes would satisfy every "it is
    ///   a legal base" reading of this and leave every skinned instance at rest;
    /// * and moving the object onto a **different region** puts it back at rest
    ///   for one frame, because the halves it now names are two runs it has
    ///   never been drawn out of.
    #[test]
    fn a_skinned_draw_carries_the_previous_half_and_starts_at_rest() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        let handle = fixture
            .renderer
            .add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &fixture.skinned,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            })
            .expect("a pool of thousands has room for one object");

        for frame in 1..=3u32 {
            fixture.begin(device);
            let parity = fixture.skinning.parity();
            let instance = instance_record(&recorder, &fixture.renderer, handle);
            assert_eq!(
                instance.base_vertex,
                fixture.skinned.region().base(parity),
                "frame {frame} must draw out of the half parity {parity} fills"
            );
            if frame == 1 {
                assert_eq!(
                    instance.previous_base_vertex, instance.base_vertex,
                    "a fresh object has no previous deformation, so its first frame owes rest \
                     rather than a half no dispatch has written"
                );
            } else {
                assert_eq!(
                    instance.previous_base_vertex,
                    fixture.skinned.region().previous_base(parity),
                    "frame {frame} must subtract against the half the frame before it filled"
                );
                assert_ne!(
                    instance.previous_base_vertex, instance.base_vertex,
                    "frame {frame} named one half twice, which is every skinned fragment at \
                     rest however far its surface moved"
                );
            }
        }

        // The same object, moved onto a region it has never been drawn out of:
        // both halves are runs no dispatch of this object's has filled, so the
        // rule that put the first frame at rest applies again.
        let second = fixture
            .renderer
            .reserve_skinned(DEMO_CUBE)
            .expect("a default pool has room for two more halves of a cube");
        assert_ne!(
            second.region().base(0),
            fixture.skinned.region().base(0),
            "the second reservation is the same two runs as the first, so nothing below is \
             about a change of region at all"
        );
        fixture.renderer.set_skinned_instance(
            handle,
            &SkinnedInstanceDesc {
                mesh: &second,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            },
        );

        for frame in 1..=2u32 {
            fixture.begin(device);
            let parity = fixture.skinning.parity();
            let instance = instance_record(&recorder, &fixture.renderer, handle);
            assert_eq!(
                instance.base_vertex,
                second.region().base(parity),
                "frame {frame} after the move must draw out of the new region"
            );
            if frame == 1 {
                assert_eq!(
                    instance.previous_base_vertex, instance.base_vertex,
                    "the first frame on a new region owes rest, exactly as a fresh object does"
                );
            } else {
                assert_eq!(
                    instance.previous_base_vertex,
                    second.region().previous_base(parity),
                    "and the frame after it is subtracting against the new region's other half"
                );
            }
        }

        fixture.renderer.release_skinned(second);
        fixture.destroy(device);
        recorder.assert_valid();
    }

    /// **A skinned object that moves carries the transform it moved from**, and
    /// the frame after it is back at rest.
    ///
    /// The transform half of a skinned motion vector, and the half the
    /// per-frame re-point used to erase: it rewrote the record through
    /// [`InstancePool::set`], which fills the previous transform out of the
    /// record it overwrites — and that record already held this frame's
    /// transform, because
    /// [`set_skinned_instance`](ForwardRenderer::set_skinned_instance) had put
    /// it there. Every skinned instance then read `previous_transform ==
    /// transform` on every frame, so a character that walked contributed
    /// nothing to the motion target from the walk, only from its deformation.
    /// [`InstancePool::set_bases`] is what the re-point goes through instead.
    ///
    /// Both frames are the claim. Without the second, a re-point that wrote
    /// some *other* stale transform into the field would pass; and the second
    /// is the pool's own rest rule reaching a skinned object at all, which a
    /// re-point that kept enrolling the element every frame would suppress
    /// for ever.
    ///
    /// Read out of the ring buffer the frame draws from, on
    /// [`instance_record`]'s terms.
    #[test]
    fn a_skinned_instance_that_moves_carries_the_transform_it_moved_from() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        let handle = fixture
            .renderer
            .add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &fixture.skinned,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            })
            .expect("a pool of thousands has room for one object");

        // Two frames standing still, so the history the move is measured
        // against is a settled record rather than a fresh one.
        fixture.begin(device);
        fixture.begin(device);

        let moved = Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0));
        fixture.renderer.set_skinned_instance(
            handle,
            &SkinnedInstanceDesc {
                mesh: &fixture.skinned,
                material: DEMO_UNTINTED,
                transform: moved,
            },
        );
        fixture.begin(device);
        let instance = instance_record(&recorder, &fixture.renderer, handle);
        assert_eq!(
            instance.transform,
            moved.to_cols_array(),
            "the frame that moved the object must draw it where it arrived"
        );
        assert_eq!(
            instance.previous_transform,
            Mat4::IDENTITY.to_cols_array(),
            "and it must say where from — a previous transform equal to the current one is \
             the re-point having overwritten the move"
        );

        fixture.begin(device);
        let settled = instance_record(&recorder, &fixture.renderer, handle);
        assert_eq!(
            settled.transform,
            moved.to_cols_array(),
            "nothing moved it again"
        );
        assert_eq!(
            settled.previous_transform, settled.transform,
            "so the frame after the move owes rest; a re-point that enrolled the element \
             every frame would keep it reporting the move for ever"
        );

        fixture.destroy(device);
        recorder.assert_valid();
    }

    /// **The graph barriers the skinning dispatch's write before every pass that
    /// pulls vertices**, and it does so because `add_skinned_passes` added the
    /// dispatch itself rather than trusting a caller to.
    ///
    /// Pass order alone would not settle it — a dispatch that ran first and was
    /// never barriered is the exact defect, and it produces a correct-looking
    /// frame on a desktop driver. So the transition out of
    /// [`ResourceState::ShaderReadWrite`] is named: there must be exactly one,
    /// it must sit after the dispatch, and it must sit at or before the first of
    /// the three passes that draw.
    #[test]
    fn the_skinning_dispatch_is_barriered_before_every_pass_that_draws() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        place_cube(&mut fixture.renderer, Mat4::IDENTITY);
        fixture
            .renderer
            .add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &fixture.skinned,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            })
            .expect("room for one object");
        fixture.begin(device);

        let imported = swapchain_image_at(device, TEST_EXTENT);
        let mut pool = crate::TransientPool::new();
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        // The same node `Skinning::add_pass` imports — one handle is one node —
        // so the barriers below can be named rather than guessed at.
        let vertices = graph.import_buffer(
            "vertex-pool",
            ImportedBuffer {
                buffer: fixture.renderer.vertex_buffer(),
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        fixture.renderer.add_skinned_passes(
            &mut graph,
            &pool,
            target,
            TEST_EXTENT,
            &fixture.skinning,
        );
        let compiled = graph.compile(&pool).expect("a legal frame");
        let labels: Vec<&str> = compiled
            .passes()
            .iter()
            .map(crate::graph::CompiledPass::label)
            .collect();

        let dispatch = labels
            .iter()
            .position(|label| *label == "skinning")
            .expect("add_skinned_passes adds the dispatch itself");
        let drawing: Vec<usize> = ["shadow", "depth-prepass", "forward"]
            .into_iter()
            .map(|wanted| {
                labels
                    .iter()
                    .position(|label| *label == wanted)
                    .unwrap_or_else(|| panic!("the frame draws through {wanted}: {labels:?}"))
            })
            .collect();
        for (label, at) in ["shadow", "depth-prepass", "forward"]
            .into_iter()
            .zip(&drawing)
        {
            assert!(
                dispatch < *at,
                "the dispatch is at {dispatch} and {label} at {at}: {labels:?}"
            );
        }

        // Each drawing pass declares the read for itself, and this is asserted
        // per pass rather than left to the single barrier below. The graph
        // transitions the region once, before the first reader, and a later
        // pass that never declared the read finds it already in the state it
        // wanted — so dropping the declaration from `forward` alone changes no
        // barrier and moves no label, and every assertion in this test still
        // passes. What it does drop is that pass's *dependency* on the
        // dispatch, which is the only thing keeping the two in this order.
        for (label, at) in ["shadow", "depth-prepass", "forward"]
            .into_iter()
            .zip(&drawing)
        {
            assert!(
                compiled.passes()[*at].reads_buffer(vertices),
                "{label} draws skinned vertices and must declare that it reads them, or \
                 nothing but declaration order keeps it after the dispatch"
            );
        }

        let released: Vec<usize> = compiled
            .passes()
            .iter()
            .enumerate()
            .filter(|(_, pass)| {
                pass.barriers().buffers.iter().any(|barrier| {
                    barrier.buffer == vertices
                        && barrier.from == ResourceState::ShaderReadWrite
                        && barrier.to == ResourceState::ShaderRead
                })
            })
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            released.len(),
            1,
            "exactly one transition out of the dispatch's write: {released:?} in {labels:?}"
        );
        let barrier = released[0];
        let first_draw = *drawing.iter().min().expect("three passes");
        assert!(
            dispatch < barrier && barrier <= first_draw,
            "the barrier is at {barrier}, the dispatch at {dispatch} and the first pass that \
             draws at {first_draw} — a draw that reads the region before the write is made \
             visible is the whole defect this seam exists to prevent"
        );

        drop(compiled);
        fixture.destroy(device);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// A frame with nothing to skin adds no dispatch and declares no read, and
    /// is the frame [`ForwardRenderer::add_passes`] would have built.
    #[test]
    fn a_frame_with_no_skinned_range_is_the_frame_add_passes_builds() {
        let _blurs = ssao_blur_switch();
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        place_cube(&mut fixture.renderer, Mat4::IDENTITY);

        let imported = swapchain_image_at(device, TEST_EXTENT);
        let pool = crate::TransientPool::new();
        let mut skinned_labels = Vec::new();
        for _ in 0..1 {
            fixture
                .renderer
                .begin_skinned_frame(
                    device,
                    &mut fixture.skinning,
                    &[],
                    &Camera::default(),
                    &DirectionalLight::default(),
                    TEST_EXTENT,
                )
                .expect("an empty plan is a legal one");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            fixture.renderer.add_skinned_passes(
                &mut graph,
                &pool,
                target,
                TEST_EXTENT,
                &fixture.skinning,
            );
            skinned_labels = graph
                .compile(&pool)
                .expect("a legal frame")
                .passes()
                .iter()
                .map(|pass| pass.label().to_string())
                .collect();
        }
        let plain = passes_in_a_frame(
            device,
            queue,
            &mut fixture.renderer,
            imported,
            &pool,
            TEST_EXTENT,
        );
        assert_eq!(
            skinned_labels, plain,
            "a frame with nothing to skin must cost the vertex pool no barrier at all"
        );

        fixture.destroy(device);
        let mut pool = pool;
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// One frame recorded end to end: `begin_frame`, the graph, and an encoder
    /// the commands land in.
    ///
    /// Returned rather than cleaned up here because the caller reads the
    /// recorder afterwards, and everything below must outlive that.
    struct Frame {
        imported: ImportedImage,
        pool: crate::TransientPool,
        commands: crcbl_hal::CommandBufferHandle,
    }

    impl Frame {
        /// Releases the frame and the image it drew into, leaving the renderer
        /// alive for another one.
        fn release(self, device: &dyn Device) {
            device.destroy_command_buffer(self.commands);
            let mut pool = self.pool;
            pool.destroy(device);
            device.destroy_image_view(self.imported.view);
            device.destroy_image(self.imported.image);
        }

        /// Releases the frame, the renderer and the image it drew into.
        fn finish(self, device: &dyn Device, renderer: ForwardRenderer) {
            self.release(device);
            renderer.destroy(device);
        }
    }

    /// Everything one labelled pass recorded, and nothing any other pass did.
    ///
    /// **Every draw assertion below is scoped through this**, and the reason is
    /// topic 18: the frame now records the same indirect call the colour pass
    /// records once per shadow cascade as well, out of that cascade's own
    /// arguments. A test filtering the whole stream for `DrawIndexedIndirect`
    /// would see one per cascade as well as the colour pass's, and could not
    /// say which pass any of them belonged to — so "the forward pass records one
    /// call per bucket" would become a claim about a total, which a shadow pass
    /// recording the wrong thing could satisfy.
    fn commands_in_pass(
        recorder: &crcbl_hal::null::Recorder,
        label: &str,
    ) -> Vec<crcbl_hal::null::Command> {
        use crcbl_hal::null::Command;

        let mut inside = false;
        let mut out = Vec::new();
        for command in recorder.commands() {
            if let Some((_, recorded)) = command.opens_pass() {
                inside = recorded == Some(label);
                continue;
            }
            if matches!(command, Command::EndRenderPass | Command::EndComputePass) {
                inside = false;
                continue;
            }
            if inside {
                out.push(command);
            }
        }
        assert!(!out.is_empty(), "no pass labelled `{label}` recorded work");
        out
    }

    /// The extent every helper frame in this module is drawn at.
    ///
    /// Named since the bloom chain, whose length is a function of it — see
    /// [`crate::bloom`] — so a test comparing a pass count against
    /// [`fullscreen_passes`] has to ask about the same size the frame was drawn
    /// at. Small on purpose: two chain levels is enough for every arm here, and
    /// [`the_pass_bound_is_the_widest_frame_the_renderer_records`] is the one
    /// test that needs a frame large enough for six.
    ///
    /// [`the_pass_bound_is_the_widest_frame_the_renderer_records`]: fn@the_pass_bound_is_the_widest_frame_the_renderer_records
    const TEST_EXTENT: (u32, u32) = (64, 48);

    /// The effects a caller has to ask for, because the default stack leaves
    /// them out — see [`RenderEffects::DEFAULT_STACK`].
    ///
    /// Named rather than written out at each of the three sites that force them
    /// on, because the point of those sites is "every effect there is" and the
    /// bug they exist to catch is a new effect joining the set and one site
    /// still measuring the old one while calling it the whole stack. They are
    /// held out of the default for different reasons — [`crate::effects`] gives
    /// each, and [`RenderEffects::CONTACT_SHADOWS`]'s is the only one that is a
    /// pending re-bless rather than a decision — and share only that a test
    /// wanting the widest frame must ask.
    ///
    /// **Named for what it is rather than for what most of it is**: the set held
    /// four post passes and a resolve when it was `POST_EFFECTS`, and the contact
    /// march is neither.
    const OUTSIDE_THE_DEFAULT: RenderEffects = RenderEffects::BLOOM
        .union(RenderEffects::ANTIALIASING)
        .union(RenderEffects::VOLUMETRIC_FOG)
        .union(RenderEffects::AUTO_EXPOSURE)
        .union(RenderEffects::SMAA)
        .union(RenderEffects::CONTACT_SHADOWS);

    /// Every effect this renderer draws is either in the default stack or in
    /// [`OUTSIDE_THE_DEFAULT`].
    ///
    /// The guard on the three sites above: they force [`OUTSIDE_THE_DEFAULT`] on and
    /// call the result "every effect", which is only true while nothing else is
    /// held out. An effect added outside the default stack and not added
    /// here makes this red before it makes a pass-list assertion red — which is
    /// the useful order, since a pass list going red says a label moved and this
    /// says what actually happened.
    #[test]
    fn forcing_the_post_effects_on_reaches_every_effect_there_is() {
        assert_eq!(
            RenderEffects::DEFAULT_STACK.union(OUTSIDE_THE_DEFAULT),
            RenderEffects::all(),
            "an effect outside the default stack that no test forces on is an effect \
             the every-effect frames below have never drawn"
        );
    }

    /// The default sun, turned a little further round for each `lap`.
    ///
    /// `docs/plan/45-shadows.md`'s static-caching rung means a second frame over
    /// a scene nothing moved in records **no shadow pass at all** — see
    /// [`ForwardRenderer::shadow_atlas_cached`]. That is the feature, and it is
    /// a difference that every A/B drawing two frames from one renderer would
    /// otherwise be measuring on top of the one it is about. Turning the sun is
    /// the smallest input the atlas depends on: the cascades are refitted, so
    /// the atlas is redrawn, and no pass, pipeline or draw of the frame moves
    /// with it.
    ///
    /// Lap zero is [`DirectionalLight::default`] exactly, so a test whose first
    /// frame is the ordinary one keeps the frame every other test draws.
    fn moving_sun(lap: u32) -> DirectionalLight {
        let sun = DirectionalLight::default();
        // A degree a lap, about the axis the default direction is not parallel
        // to. Small enough that every cascade still sees the scene, and large
        // enough that the snapped cascade origin actually moves.
        let turn = Mat4::from_rotation_y(
            f32::from(u16::try_from(lap).expect("a handful of laps")) * std::f32::consts::PI
                / 180.0,
        );
        DirectionalLight {
            direction: turn.transform_vector3(sun.direction).normalize(),
            ..sun
        }
    }

    /// [`frame`] drawn under [`moving_sun`], so this frame's shadow atlas is
    /// not the one the lap before it left in the image.
    fn frame_moving_the_sun(
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        queue: QueueHandle,
        lap: u32,
    ) -> Frame {
        frame_lit_by(device, renderer, queue, &moving_sun(lap))
    }

    fn frame(device: &dyn Device, renderer: &mut ForwardRenderer, queue: QueueHandle) -> Frame {
        frame_lit_by(device, renderer, queue, &DirectionalLight::default())
    }

    fn frame_lit_by(
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        queue: QueueHandle,
        sun: &DirectionalLight,
    ) -> Frame {
        frame_seen_from(device, renderer, queue, &Camera::default(), sun)
    }

    fn frame_seen_from(
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        queue: QueueHandle,
        camera: &Camera,
        sun: &DirectionalLight,
    ) -> Frame {
        renderer
            .begin_frame(device, camera, sun, TEST_EXTENT)
            .expect("write");
        let imported = swapchain_image(device);
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        let mut pool = crate::TransientPool::new();
        renderer.add_passes(&mut graph, &pool, target, TEST_EXTENT);
        let compiled = graph.compile(&pool).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("forward frame"),
            queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), None)
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");
        Frame {
            imported,
            pool,
            commands,
        }
    }

    /// A stand-in for the acquired swapchain image the frame normally ends in,
    /// at [`TEST_EXTENT`].
    fn swapchain_image(device: &dyn Device) -> ImportedImage {
        swapchain_image_at(device, TEST_EXTENT)
    }

    /// The same, at an extent of the caller's choosing.
    ///
    /// A frame's attachments must all be one size — the graph refuses a pass
    /// whose colour target and depth disagree — so a test drawing at anything
    /// but [`TEST_EXTENT`] has to import a target of that size too.
    fn swapchain_image_at(device: &dyn Device, extent: (u32, u32)) -> ImportedImage {
        let format = Format::Rgba8UnormSrgb;
        let image = device
            .create_image(&crcbl_hal::ImageDesc {
                label: Some("fake swapchain image"),
                image_type: crcbl_hal::ImageType::D2,
                extent: crcbl_hal::Extent3d::d2(extent.0, extent.1),
                format,
                mip_levels: 1,
                samples: 1,
                usage: crcbl_hal::ImageUsage::COLOR_ATTACHMENT | crcbl_hal::ImageUsage::PRESENT,
            })
            .expect("an image");
        let view = device
            .create_image_view(&crcbl_hal::ImageViewDesc {
                label: Some("fake swapchain view"),
                image,
                view_type: crcbl_hal::ImageViewType::D2,
                format,
                range: crcbl_hal::ImageSubresourceRange::all(format),
            })
            .expect("a view");
        ForwardRenderer::present_target(image, view, format, extent)
    }

    /// The spin is a rotation: no scale, no shear, so the cube neither grows
    /// nor mirrors as it turns, and every mesh golden blessed at a `spin` is a
    /// picture of one cube.
    ///
    /// That is also what keeps those goldens insensitive to `mesh.slang`'s
    /// `normal_basis`. A matrix with orthonormal rows and a positive
    /// determinant is its own cofactor matrix, to within a rounding step, so
    /// the vertex stage writes the normals it wrote when it multiplied by the
    /// bare 3×3 — see [`crcbl_shaders::mesh::GpuInstance::transform`], which no
    /// longer requires a caller to be rigid at all.
    #[test]
    fn the_spin_matrix_stays_rigid() {
        for seconds in [0.0f32, 0.5, 3.25, 100.0] {
            let model = ForwardRenderer::spin(seconds);
            let basis = glam::Mat3::from_mat4(model);
            let product = basis * basis.transpose();
            for (row, expected) in [
                (product.x_axis, Vec3::X),
                (product.y_axis, Vec3::Y),
                (product.z_axis, Vec3::Z),
            ] {
                assert!(
                    (row - expected).length() < 1e-5,
                    "the spin at {seconds}s is not orthonormal: {product:?}"
                );
            }
            assert!((model.determinant() - 1.0).abs() < 1e-5, "at {seconds}s");
        }
        // And it actually moves.
        assert_ne!(ForwardRenderer::spin(0.0), ForwardRenderer::spin(1.0));
    }

    /// The swapchain import says what a swapchain image actually needs, which is
    /// the knowledge the hand-written barriers used to carry.
    #[test]
    fn the_present_target_starts_undefined_and_ends_presentable() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let image = pool.insert(0).cast();
        let view = pool.insert(0).cast();
        let imported =
            ForwardRenderer::present_target(image, view, Format::Bgra8UnormSrgb, (1280, 720));
        assert_eq!(imported.initial, ResourceState::Undefined);
        assert_eq!(imported.final_state, ResourceState::Present);
        assert_eq!(imported.extent, (1280, 720));
    }

    /// The scene target is `Rgba16Float`, and the depth target is the
    /// reversed-Z format. Both are locked decisions and both are cheap to pin.
    #[test]
    fn the_scene_targets_are_hdr_and_reversed_z() {
        let color = TransientImageDesc::scene_color((64, 48));
        assert_eq!(color.format, Format::Rgba16Float);
        assert!(color.format.is_hdr_capable());
        assert!(color.usage.contains(crcbl_hal::ImageUsage::SAMPLED));

        let depth = TransientImageDesc::scene_depth((64, 48));
        assert_eq!(depth.format, Format::D32Float);
        assert_eq!(
            DepthStencilState::default().depth_compare,
            crcbl_hal::CompareOp::Greater
        );
    }
}
