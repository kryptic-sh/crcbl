//! One camera's share of the forward frame.
//!
//! [`ForwardRenderer`] owns two kinds of state, and this module is the line
//! between them. The **scene** is what every camera draws: the geometry and
//! instance pools, the materials and their pages, the probes, the shadow atlas
//! and everything that decides what it holds. A **view** is what one camera
//! needs to turn that scene into a picture: its own cull and the survivor lists
//! it writes, its frame block, its light clustering, its bind groups and the
//! ring of per-frame blocks every screen-space pass keeps.
//!
//! # Why the line is here
//!
//! Everything on this side is either sized by what one camera sees or carries
//! one camera's history from a frame to the next. Topic 25's
//! hysteresis lives in [`DrawGen::group_state`], so two cameras sharing one
//! generator would each undo the other's cut. Auto-exposure reads the previous
//! slot's measurement, and [`View::previous_view_projection`] is where the
//! motion vectors come from. Every one of those is wrong the moment a second
//! camera writes it.
//!
//! Nothing on the other side is. An instance is where it is whoever looks at
//! it, and the shadow atlas is drawn once for the frame.

use super::*;
use crate::draw_gen::OcclusionFrame;

/// How many views one [`ForwardRenderer`] draws at most, the camera
/// [`ForwardRenderer::begin_frame`] opens included.
///
/// The width of
/// [`GpuInstance::HIDDEN_VIEWS_MASK`](crcbl_shaders::mesh::GpuInstance::HIDDEN_VIEWS_MASK),
/// which is where a view's bit lives: an instance record has one bit per view
/// to say it is hidden there, and a ninth view would have no bit to cull on.
pub const MAX_VIEWS: usize = 8;

/// Names one camera a [`ForwardRenderer`] draws.
///
/// [`ViewId::PRIMARY`] is the camera [`ForwardRenderer::begin_frame`] opens and
/// always exists. Every other id comes from [`ForwardRenderer::create_view`]
/// and stays valid until [`ForwardRenderer::destroy_view`] hands it back, after
/// which the renderer may give the same id to the next view it creates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewId(u8);

impl ViewId {
    /// The camera [`ForwardRenderer::begin_frame`] opens, and the one
    /// [`ForwardRenderer::add_passes`] draws into the caller's target.
    pub const PRIMARY: Self = Self(0);

    /// This view's index: `0` for [`ViewId::PRIMARY`], and below [`MAX_VIEWS`]
    /// for every other.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The bit of [`GpuInstance::flags`](crcbl_shaders::mesh::GpuInstance::flags)
    /// that hides an instance from this view.
    pub(super) const fn hidden_bit(self) -> u32 {
        1 << (mesh::GpuInstance::HIDDEN_VIEWS_SHIFT + self.0 as u32)
    }
}

/// The views an instance is drawn in — see
/// [`ForwardRenderer::set_instance_views`].
///
/// A set of [`ViewId`]s, one bit each. [`ViewMask::ALL`] is what every instance
/// starts with, so a caller that never names a view draws everything in every
/// view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewMask(u8);

impl ViewMask {
    /// Every view, including the ones not created yet.
    pub const ALL: Self = Self(u8::MAX);
    /// No view at all: the instance stays resident, and every camera's cull
    /// rejects it before it tests a bound.
    pub const NONE: Self = Self(0);

    /// `view` alone.
    #[must_use]
    pub const fn only(view: ViewId) -> Self {
        Self(1 << view.0)
    }

    /// This set with `view` added.
    #[must_use]
    pub const fn with(self, view: ViewId) -> Self {
        Self(self.0 | (1 << view.0))
    }

    /// This set with `view` taken out.
    #[must_use]
    pub const fn without(self, view: ViewId) -> Self {
        Self(self.0 & !(1 << view.0))
    }

    /// Whether `view` draws an instance carrying this set.
    #[must_use]
    pub const fn contains(self, view: ViewId) -> bool {
        self.0 & (1 << view.0) != 0
    }

    /// The hidden-views bits an instance record carries for this set — its
    /// complement, so [`ViewMask::ALL`] is the all-zero field every record
    /// starts with.
    pub(super) const fn hidden_bits(self) -> u32 {
        ((!self.0) as u32) << mesh::GpuInstance::HIDDEN_VIEWS_SHIFT
    }

    /// The set an instance record's hidden-views bits describe.
    pub(super) const fn from_flags(flags: u32) -> Self {
        let hidden =
            (flags & mesh::GpuInstance::HIDDEN_VIEWS_MASK) >> mesh::GpuInstance::HIDDEN_VIEWS_SHIFT;
        Self(!(hidden as u8))
    }
}

/// What a [`ViewBackground::Transparent`] view's scene colour is cleared to.
const TRANSPARENT_CLEAR: [f32; 4] = [0.0; 4];

/// What [`ForwardRenderer::create_view`] builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewDesc {
    /// The effects this view may draw — intersected, every frame, with what the
    /// frame resolved (see [`ForwardRenderer::resolved_effects`]), so a view
    /// can ask for less than the frame draws and never for more.
    ///
    /// [`RenderEffects::SHADOWS`] is not a view's to take away and is ignored
    /// here: the atlas is drawn once for the frame, and every view samples it
    /// whenever the frame drew it. A [`ViewLighting::Fixed`] view also never
    /// draws [`ViewLighting::SCENE_EFFECTS`], whatever this asks for.
    pub effects: RenderEffects,
    /// What the view's picture holds where no geometry is — see
    /// [`ViewBackground`].
    pub background: ViewBackground,
    /// What lights the view's surfaces — see [`ViewLighting`].
    pub lighting: ViewLighting,
}

impl Default for ViewDesc {
    /// Every effect the frame draws, over the frame's own background, lit by
    /// the frame's own light.
    fn default() -> Self {
        Self {
            effects: RenderEffects::all(),
            background: ViewBackground::Scene,
            lighting: ViewLighting::Scene,
        }
    }
}

impl ViewDesc {
    /// A [`ViewBackground::Transparent`] view with every effect the frame draws
    /// except the ones such a view cannot take, and auto-exposure.
    ///
    /// The two antialiasing tiers are what
    /// [`ForwardRenderer::create_view`] refuses on a transparent view.
    /// [`RenderEffects::AUTO_EXPOSURE`] it would accept, and it is left out
    /// because the meter reads the whole target: the transparent background is
    /// metered as black, so a model filling a quarter of the picture would be
    /// exposed differently from the same model filling half of it. The view
    /// takes the caller's exposure instead — see
    /// [`ForwardRenderer::set_exposure`].
    #[must_use]
    pub const fn transparent() -> Self {
        Self {
            effects: RenderEffects::all().difference(
                ViewBackground::REFUSED_ON_TRANSPARENT.union(RenderEffects::AUTO_EXPOSURE),
            ),
            background: ViewBackground::Transparent,
            lighting: ViewLighting::Scene,
        }
    }
}

/// What lights a view's surfaces — see [`ViewDesc::lighting`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ViewLighting {
    /// The frame's own light: the sun and every light row
    /// [`ForwardRenderer::set_lights`] handed over, the sky's irradiance, the
    /// irradiance probes, the reflections and the fog. What the primary camera
    /// is lit by, and what every view was lit by before this choice existed.
    #[default]
    Scene,
    /// One constant light, whatever the frame is lit by: the key's direction
    /// and colour as the only direct light, its
    /// [`ambient`](DirectionalLight::ambient) as the only diffuse indirect one,
    /// and `environment` as the only thing a reflection sees.
    ///
    /// For a picture that must not change with the world around it, such as a
    /// model rendered into an inventory icon: the same model under the same
    /// fixed light draws the same pixels in a sunlit level and a dark one.
    ///
    /// # What it ignores
    ///
    /// * **The frame's sun and every light row** — a point, spot or rectangle
    ///   light near the view's camera included. The key light is the view's
    ///   one row, which is a choice rather than a limit: an icon's model is
    ///   placed away from the world, so a local light that did reach it would
    ///   be lighting it by accident.
    /// * **Every shadow.** The key light is not occluded — not by the frame's
    ///   cascades, which were fitted to the primary camera and cover whatever
    ///   happens to be there, and not by contact shadows. The view's frame
    ///   block names no shadow map: every atlas rectangle in it is empty, which
    ///   the shaders read as a map never rendered and answer lit. The frame's
    ///   atlas is still drawn and the view's instances may still cast into it;
    ///   see [`ForwardRenderer::set_instance_casts_shadow`].
    /// * **The sky and the irradiance probes**: the view's frame block carries
    ///   no sky rows and an empty probe header, and the view binds a probe
    ///   table of one zeroed row in place of the scene's — to the forward pass,
    ///   the reflection march and the water surface alike — so the diffuse
    ///   ambient is the fill alone and a reflection that finds no geometry
    ///   sees `environment` alone.
    /// * **The air**: [`Self::SCENE_EFFECTS`] are dropped from the view's
    ///   effects whatever [`ViewDesc::effects`] asked for, and the height fog
    ///   is [`Fog::NONE`].
    ///
    /// What it keeps is everything the view's own geometry decides: its
    /// materials and emission, its ambient occlusion, its reflections of
    /// itself, and every effect after the forward pass. The background is
    /// [`ViewDesc::background`]'s, so a view asking for
    /// [`ViewBackground::Scene`] still draws the frame's sky behind a model it
    /// does not light.
    Fixed {
        /// The one direct light, and in its
        /// [`ambient`](DirectionalLight::ambient) the diffuse fill.
        key: DirectionalLight,
        /// The radiance a reflection sees in every direction — a uniform
        /// environment, in linear RGB, that **may exceed 1.0** like the key's
        /// colour. It is what gives a dark glossy surface its sheen: a
        /// near-black dielectric scatters almost none of the key and the fill,
        /// and still reflects this at its `F0`, rising towards grazing angles
        /// by the split-sum Fresnel term — so what the camera sees edge-on
        /// reads brighter than what faces it. A metal reflects it tinted by
        /// its base colour.
        ///
        /// **Specular only.** A uniform environment of radiance `L` would also
        /// reach a diffuse surface, as `π·L` — what [`Self::Scene`] adds for
        /// a uniform [`Sky`](crate::Sky) of that radiance — but here that half
        /// is the key's `ambient` and nothing else, so the two are never
        /// summed and the fill means what it meant before this field existed.
        /// A caller who wants the pair to describe one physical environment
        /// sets `ambient` to `π` times this.
        ///
        /// It reaches a surface through the reflection pass, so it needs
        /// [`RenderEffects::REFLECTIONS`] in both the frame's effects and
        /// [`ViewDesc::effects`]; a view drawn without it reflects nothing.
        /// Zero is a view whose reflections find nothing but its own geometry.
        environment: Vec3,
    },
}

impl ViewLighting {
    /// The effects a [`ViewLighting::Fixed`] view never draws, because each
    /// brings the frame's light into it: the froxel volume is lit by the sun
    /// and the scene's lights, and the contact march shadows along the frame's
    /// sun.
    ///
    /// Reflections are not among them. The march's fallback is the probe table
    /// and the sky rows, and a fixed view hands it its own — the zeroed row and
    /// a sky of `environment` in every direction — so what it reflects is the
    /// view's own geometry and the fixed environment.
    pub const SCENE_EFFECTS: RenderEffects =
        RenderEffects::VOLUMETRIC_FOG.union(RenderEffects::CONTACT_SHADOWS);

    /// The effects this lighting takes out of a view's frame.
    pub(super) const fn dropped_effects(self) -> RenderEffects {
        match self {
            Self::Scene => RenderEffects::empty(),
            Self::Fixed { .. } => Self::SCENE_EFFECTS,
        }
    }
}

/// What a view's picture holds where no geometry covers a pixel — see
/// [`ViewDesc::background`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewBackground {
    /// The frame's own background: the sky where one is set, [`SCENE_CLEAR`]
    /// where none is, and an alpha of one at every pixel. What the primary
    /// camera draws, and what every view drew before this choice existed.
    #[default]
    Scene,
    /// Nothing: the view clears to transparent black, draws no sky, and its
    /// target's alpha is the frame's **coverage** — one where geometry was
    /// drawn and zero where it was not.
    ///
    /// For a picture composited over something else, such as a model rendered
    /// into an icon. The colour is **straight**, not premultiplied, which is
    /// what [`SpriteRenderer`](crate::SpriteRenderer) blends: a covered pixel
    /// holds the colour an opaque view would have drawn there, and a
    /// transparent one holds whatever light the air in front of the far plane
    /// scattered — zero on a frame without volumetric fog — which a
    /// straight-alpha blend weighs by zero.
    ///
    /// # What keeps the alpha, and what cannot
    ///
    /// The forward pass, grass and water write coverage; the volumetric
    /// composite, the reflection composite and the bloom composite carry the
    /// alpha they read; the tonemap writes it into the target. **The two
    /// antialiasing tiers cannot**: each blends a pixel's colour with its
    /// neighbours' across exactly the edges the coverage has, so an edge
    /// pixel would come out mixed with the transparent black beside it — a
    /// premultiplied colour under a straight-alpha consumer, which is a dark
    /// fringe. [`ForwardRenderer::create_view`] refuses a transparent view
    /// whose [`ViewDesc::effects`] name either of them, and
    /// [`ViewDesc::transparent`] is a description it accepts.
    ///
    /// [`render_scale`](ForwardRenderer::render_scale)'s upscale filters on
    /// the same terms and so **does not apply**: a transparent view is drawn at
    /// its target's own extent whatever the scale is.
    ///
    /// Bloom and auto-exposure keep the alpha and change the colour. Bloom's
    /// glow past a silhouette lands on pixels the coverage says are empty and
    /// is dropped with them; auto-exposure meters the background as black —
    /// see [`ViewDesc::transparent`].
    Transparent,
}

impl ViewBackground {
    /// The effects a [`ViewBackground::Transparent`] view is refused with —
    /// see that variant.
    pub const REFUSED_ON_TRANSPARENT: RenderEffects =
        RenderEffects::ANTIALIASING.union(RenderEffects::CMAA2);

    /// Whether this background leaves the target's alpha as coverage.
    pub(super) const fn is_transparent(self) -> bool {
        matches!(self, Self::Transparent)
    }
}

/// What one call to [`ForwardRenderer::add_passes_with_views`] draws into.
#[derive(Clone, Copy, Debug)]
pub struct FrameTargets<'v> {
    /// The primary camera's target, imported into the graph the call records
    /// into.
    pub target: ImageId,
    /// That target's extent.
    pub extent: (u32, u32),
    /// The skinning plan whose dispatch the frame records, or `None` for a
    /// frame that skins nothing — see
    /// [`ForwardRenderer::add_skinned_passes`].
    pub skinning: Option<&'v Skinning>,
    /// Every secondary view the frame draws, each into its own target.
    pub views: &'v [ViewTarget],
}

impl<'v> FrameTargets<'v> {
    /// The primary camera alone, into `target`.
    #[must_use]
    pub const fn primary(
        target: ImageId,
        extent: (u32, u32),
        skinning: Option<&'v Skinning>,
    ) -> Self {
        Self {
            target,
            extent,
            skinning,
            views: &[],
        }
    }
}

/// Where one secondary view's frame goes — see
/// [`ForwardRenderer::add_passes_with_views`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewTarget {
    /// The view to draw. Never [`ViewId::PRIMARY`], whose target is the one the
    /// call itself takes.
    pub view: ViewId,
    /// The image the view's frame ends in, imported into the same graph.
    pub target: ImageId,
    /// That image's extent, which the view's internal frame is sized from
    /// exactly as the primary's is sized from its own.
    pub extent: (u32, u32),
}

/// Everything [`View::build`] reads out of the scene it draws.
///
/// Handles and borrowed tables, and nothing a view owns: each is created and
/// destroyed by [`ForwardRenderer`], which outlives every view built against
/// it.
pub(super) struct ViewInputs<'a> {
    /// Which view this is, whose bit its cull rejects instances on.
    pub(super) id: ViewId,
    /// The effects it may draw — see [`ViewDesc::effects`].
    pub(super) effects: RenderEffects,
    /// What it draws where no geometry is — see [`ViewDesc::background`].
    pub(super) background: ViewBackground,
    /// What lights it — see [`ViewDesc::lighting`].
    pub(super) lighting: ViewLighting,
    /// The format the caller's target has, which the resolves and the upscale
    /// write.
    pub(super) target_format: Format,
    /// Which call the forward pass records.
    pub(super) emit: EmitTail,
    /// The instance ring, one buffer per frame in flight.
    pub(super) instances: &'a [BufferHandle],
    /// The mesh table's buffer.
    pub(super) mesh_table: BufferHandle,
    /// The bucket and level tables every generator of the scene is built from
    /// — see [`DrawGenDesc`].
    pub(super) bucket_meshes: &'a [u32],
    pub(super) bucket_modes: &'a [u32],
    pub(super) bucket_clusters: &'a [u32],
    pub(super) mesh_levels: &'a [level_select::MeshLevels],
    pub(super) level_groups: &'a [level_select::LevelGroup],
    pub(super) level_meshes: &'a [u32],
    /// [`SceneDesc::capacities`]' instance and light counts.
    pub(super) instance_capacity: u32,
    pub(super) light_capacity: u32,
    /// §3.5's clusters, on the mesh path only.
    pub(super) clusters: Option<&'a ClusterPool>,
    /// The mesh layout every group below is built against.
    pub(super) mesh_layout: BindGroupLayoutHandle,
    /// The scene's half of every group — see [`SharedBindings`]. The page
    /// sampler is per slot, because [`ForwardRenderer::adopt_page_sampler`]
    /// moves each slot to a new one on that slot's own frame.
    pub(super) vertices: BufferHandle,
    pub(super) draw_constants: BufferHandle,
    pub(super) materials: BufferHandle,
    pub(super) page: ImageViewHandle,
    pub(super) normal_page: ImageViewHandle,
    pub(super) mro_page: ImageViewHandle,
    pub(super) emissive_page: ImageViewHandle,
    pub(super) page_samplers: &'a [SamplerHandle],
    pub(super) probes: &'a [BufferHandle],
    pub(super) specular_dfg: ImageViewHandle,
    pub(super) ltc_table: ImageViewHandle,
    /// The shadow atlas and the sampler that compares against it.
    pub(super) shadow_map: ImageViewHandle,
    pub(super) shadow_sampler: SamplerHandle,
    /// The stand-ins a group names until the forward pass rebuilds it against
    /// the frame's own images.
    pub(super) ambient_occlusion: ImageViewHandle,
    pub(super) contact_shadow: ImageViewHandle,
    pub(super) probe_visibility: ImageViewHandle,
}

/// What the frame [`View::begin_frame`] opens already knows, handed to every
/// view it begins.
///
/// Copies and borrows of the renderer's own fields, assembled once per view by
/// [`ForwardRenderer`] so a view's blocks are written from exactly the numbers
/// the primary camera's were.
pub(super) struct ViewFrame<'a> {
    /// The frame-in-flight slot every block is written into.
    pub(super) slot: usize,
    /// [`ForwardRenderer::frame_serial`] for the frame being opened.
    pub(super) serial: u64,
    /// The target's aspect, and the internal extent the frame is drawn at.
    pub(super) aspect: f32,
    pub(super) extent: (u32, u32),
    /// The frame's resolved effects, before this view's own mask.
    pub(super) effects: RenderEffects,
    /// Every element the instance pool has handed out — see
    /// [`InstancePool::slot_count`].
    pub(super) instance_count: u32,
    /// The eye detail is selected from.
    pub(super) selection_eye: Vec3,
    pub(super) lod_error_budget: f32,
    pub(super) lod_hold_ratio: f32,
    /// The sun, the light rows and the shadow matrices the frame fitted.
    pub(super) scene: &'a FrameScene,
    /// What the frame's grass field holds, for the generation block this view
    /// writes — see [`crate::grass`].
    pub(super) grass: Option<crate::grass::GrassFrame>,
    /// The wind's block, as the renderer was last handed it.
    pub(super) wind: crcbl_shaders::wind::WindParams,
    pub(super) fog: Fog,
    pub(super) probe_volume: crcbl_shaders::probe::ProbeVolume,
    pub(super) attribute_base: u32,
    /// The frame's sky, resolved once — see `ForwardRenderer::refresh_sky_view`.
    pub(super) gradient: crcbl_shaders::sky::SkyGradient,
    pub(super) sky_irradiance: crcbl_shaders::probe::GpuProbe,
    pub(super) debug_view_lane: f32,
    pub(super) exposure: f32,
    pub(super) exposure_adaptation: Option<ExposureAdaptation>,
    pub(super) tonemap_curve: tonemap::TonemapCurve,
    /// The occlusion and small-feature culls this frame asks for — see
    /// [`ForwardRenderer::occlusion_culling`], resolved against the frame.
    pub(super) occlusion: OcclusionCulling,
}

/// Where [`View::add_passes`] ends a view's frame, and the two extents it is
/// drawn between.
#[derive(Clone, Copy, Debug)]
pub(super) struct ViewOutput {
    /// The image the frame ends in.
    pub(super) target: ImageId,
    /// That image's extent.
    pub(super) target_extent: (u32, u32),
    /// The internal extent the frame is drawn at — see
    /// [`ForwardRenderer::frame_extents`].
    pub(super) extent: (u32, u32),
}

/// What [`View::add_cull`] recorded, for [`View::add_passes`] to draw from.
#[derive(Clone, Debug)]
pub(super) struct ViewCull {
    /// The cull and draw-argument pair's outputs.
    pub(super) generated: GeneratedDraws,
    /// The view's record of the cut, on the mesh path.
    pub(super) selection: Option<BufferId>,
    /// The froxel grid the clustering pass filled.
    pub(super) light_grid: BufferId,
    /// The farthest-depth pyramid a two-phase occlusion frame reads and
    /// rewrites, or `None` for a frame that culls in one phase.
    pub(super) pyramid: Option<ViewPyramid>,
    /// Instances the cull tested, which the second phase tests again.
    pub(super) instance_count: u32,
}

/// [`ViewCull::pyramid`]: the pyramid's levels as this frame's graph knows them,
/// and the cull's group naming them.
#[derive(Clone, Debug)]
pub(super) struct ViewPyramid {
    pub(super) levels: Vec<ImageId>,
    pub(super) group: BindGroupHandle,
}

/// The tonemap's pipeline and what its group names besides the frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct TonemapPipeline {
    pub(super) sampler: SamplerHandle,
    pub(super) layout: BindGroupLayoutHandle,
    pub(super) pipeline_layout: PipelineLayoutHandle,
    pub(super) pipeline: GraphicsPipelineHandle,
}

/// What the frame's scene passes left in the graph, and the scene's own
/// handles, for every view's [`View::add_passes`] to read.
pub(super) struct FramePasses {
    /// The frame-in-flight slot.
    pub(super) slot: usize,
    pub(super) emit: EmitTail,
    pub(super) target_format: Format,
    /// How many occlusion blurs this frame runs — see
    /// `ForwardRenderer::frame_ssao_blurs`.
    pub(super) ssao_blurs: u32,
    /// Whether the background pass draws.
    pub(super) draws_sky: bool,
    /// What the frame's water draws, or `None` for a frame with none — see
    /// [`crate::water`].
    pub(super) water: Option<WaterFrame>,
    /// What the frame's grass field holds, or `None` for a frame with none —
    /// see [`crate::grass`].
    pub(super) grass: Option<crate::grass::GrassFrame>,
    /// The skinning dispatch's vertex pool, when the frame skins.
    pub(super) skinned: Option<BufferId>,
    /// This frame's slot of the probe ring, as a handle and as the graph's id.
    pub(super) probe_buffer: BufferHandle,
    pub(super) probe_table: BufferId,
    /// The two 1×1 stand-ins a group names when an effect is off.
    pub(super) occlusion_placeholder: ImageId,
    pub(super) contact_placeholder: ImageId,
    /// The material pages, imported once for the frame.
    pub(super) pages: MaterialPages,
    /// The atlas the shadow pass drew, or its import where it was cached.
    pub(super) shadow_atlas: ImageId,
    /// The probe-visibility maps, or their placeholder.
    pub(super) probe_visibility: ImageViewHandle,
    pub(super) mesh_layout: BindGroupLayoutHandle,
    /// The wireframe twin pair, where a caller switched the view on.
    pub(super) wireframe: Option<SidedPipelines>,
    /// The two passes' call lists, split by pipeline — see
    /// `ForwardRenderer::depth_partitions` and `ForwardRenderer::sided_partitions`.
    /// The same for every view: a bucket's offsets are a function of the bucket
    /// alone.
    pub(super) prepass_partitions: Vec<BucketDraws>,
    pub(super) color_partitions: Vec<BucketDraws>,
    pub(super) tonemap: TonemapPipeline,
}

/// What both depth prepasses declare and do not write, as the graph knows it —
/// see [`declare_prepass_reads`].
#[derive(Clone, Copy, Debug)]
struct PrepassReads {
    shadow_atlas: ImageId,
    occlusion_placeholder: ImageId,
    pages: MaterialPages,
    probe_table: BufferId,
    skinned: Option<BufferId>,
    selection: Option<BufferId>,
    prepass_stats: BufferId,
}

/// Declares everything a depth prepass binds — the early one and, with the
/// occlusion cull on, the late one, which bind the same group and draw from the
/// same generator.
fn declare_prepass_reads<'g, 'a>(
    pass: PassBuilder<'g, 'a>,
    reads: &PrepassReads,
    generated: &GeneratedDraws,
    emit: EmitTail,
) -> PassBuilder<'g, 'a> {
    let pass = pass
        // Both are in this pass's bind group and neither is sampled by
        // either depth-only pipeline — but a bound descriptor whose image is
        // in the wrong layout is what
        // `VUID-vkCmdDrawIndexedIndirectCount-imageLayout-00344` names, and
        // the other backends read whatever the last writer left behind.
        .read_image(reads.shadow_atlas)
        .read_image(reads.occlusion_placeholder)
        // And the page, which the **cutout** pipeline does sample: the alpha
        // it cuts against is a texel of it. Declared on every frame rather
        // than on the masked ones, because a declaration is also what lets
        // the graph order a copy into a page layer against this pass — see
        // `base_color_page_import`.
        .read_image(reads.pages.base_color)
        // And §2's other three pages, which are in the same groups for the
        // same reason and are sampled here just as little.
        .read_image(reads.pages.normal)
        .read_image(reads.pages.mro)
        .read_image(reads.pages.emissive)
        // And the probe rows, which `mesh_layout` names in this group too
        // and which the depth-only pipeline reads not at all — declared for
        // the colour pass's reason exactly: they are device-local and
        // writable-by-copy so a gather can fill them, and the graph orders a
        // write against a pass only if that pass declared the read.
        .read_buffer(reads.probe_table);
    // `read_draw_sources` declares the *camera's* statistics buffer, because
    // that is the one the arguments came out of; the prepass writes its own
    // instead, so both are declared and the graph barriers both.
    let pass = read_draw_sources(pass, generated, emit)
        .use_buffer(reads.prepass_stats, ResourceState::ShaderReadWrite);
    // The skinned vertices, on the shadow pass's terms. This pass writes the
    // depth the occlusion pair samples and the forward pass tests against,
    // so a prepass reading the region before the dispatch is visible lays
    // down the previous pose's silhouette and the frame is rejected against
    // it.
    let pass = match reads.skinned {
        Some(vertices) => pass.read_buffer(vertices),
        None => pass,
    };
    // The camera's own cut, written here and again by the forward pass with
    // the same camera and the same budget. Shared rather than a buffer of its
    // own — unlike a cascade's, which a *later* pass would overwrite before
    // anything could read it — because the second write is the one that stands
    // and it writes the same words.
    match reads.selection {
        Some(selection) if emit.is_mesh() => {
            pass.use_buffer(selection, ResourceState::ShaderReadWrite)
        }
        _ => pass,
    }
}

/// The primary camera's overlays — nothing for a secondary view.
#[derive(Default)]
pub(super) struct Overlays<'a> {
    /// The debug draw layer, drawn into the HDR frame before the tonemap.
    pub(super) debug_draw: Option<&'a DebugDraw>,
    /// The ground grid, where a caller switched it on.
    pub(super) ground_grid: Option<&'a GroundGrid>,
    /// The shadow atlas viewer, on the frame the debug view shows it.
    pub(super) atlas_viewer: Option<&'a mut AtlasView>,
}

/// One camera's resources and history — see the [module docs](self).
#[derive(Debug)]
pub(super) struct View {
    /// The effects this view may draw, as [`ViewDesc::effects`] asked for them.
    pub(super) effects: RenderEffects,
    /// What this view draws where no geometry is, as
    /// [`ViewDesc::background`] asked for it.
    pub(super) background: ViewBackground,
    /// What lights this view, as [`ViewDesc::lighting`] asked for it.
    pub(super) lighting: ViewLighting,
    /// What this frame draws in this view: the frame's resolved effects under
    /// [`View::effects`], frozen by [`View::begin_frame`] so that the frame's
    /// two halves agree on it.
    pub(super) frame_effects: RenderEffects,
    /// The frame serial [`View::begin_frame`] last ran for — see
    /// [`ForwardRenderer::frame_serial`]. A secondary view records passes only
    /// in the frame it was begun for.
    pub(super) begun: u64,
    /// The cull and draw-argument passes, and the indirect arguments they
    /// produce.
    pub(super) draws: DrawGen,
    /// One buffer per frame in flight holding the cut the descent chose, or
    /// empty where there is no amplification stage to choose one. See
    /// [`ForwardRenderer::cluster_selection`].
    pub(super) cluster_selection: Vec<BufferHandle>,
    /// The one zeroed probe row a [`ViewLighting::Fixed`] view's groups, its
    /// reflection march and its water surface bind in place of the scene's
    /// table, or `None` for a view lit by the scene — see [`View::build`].
    pub(super) fixed_probes: Option<BufferHandle>,
    /// What [`begin_frame`](ForwardRenderer::begin_frame) last handed
    /// [`DrawGen::begin_frame`], kept so a reader can compute the same cut
    /// host-side without re-deriving it from the camera.
    ///
    /// Pixels per unit, the budget a group starts expanding over, and the budget
    /// it is held down to — topic 25's hysteresis, and
    /// [`LOD_HOLD_RATIO`] is what puts the third below the second.
    pub(super) lod_params: [f32; 3],
    /// Topic 18's light list and froxel grid, and the compute pass between them.
    pub(super) lights: LightGrid,
    /// This frame's froxel grid, as [`begin_frame`](ForwardRenderer::begin_frame)
    /// decided it from the viewport and the camera.
    ///
    /// Held rather than recomputed at [`ForwardRenderer::add_passes`], because
    /// the number of froxels the dispatch covers and the number the frame block
    /// tells the fragment stage about have to be the same one.
    pub(super) grid: Grid,
    /// `[frame]`: the frame block, one per frame in flight — see the
    /// [forward module docs](super) on why it is a ring.
    pub(super) uniforms: Vec<BufferHandle>,
    /// `[frame]`: the camera's group of the mesh layout, naming this view's
    /// frame block and survivors.
    pub(super) mesh_groups: Vec<BindGroupHandle>,
    /// `[frame]`: `tonemap.slang`'s exposure block, written by
    /// [`begin_frame`](ForwardRenderer::begin_frame).
    ///
    /// One per frame in flight for the frame uniforms' reason exactly — the
    /// previous frame may still be reading last frame's while this one is
    /// written.
    pub(super) tonemap_uniforms: Vec<BufferHandle>,
    /// `[frame]`: the tonemap group, cached against the scene target's view.
    ///
    /// Rebuilt only when that view changes, which is only on a resize. The graph
    /// hands the view to the pass body; caching against it is what keeps a
    /// steady-state frame free of descriptor writes.
    ///
    /// **One per frame in flight**, for [`crate::ssao`]'s reason: this group
    /// names [`View::tonemap_uniforms`] as well as the scene transient, and that
    /// is a ring — a single cache keyed on the view alone would hand the even
    /// frames' block to the odd frames.
    pub(super) tonemap_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `[frame]`: the entries [`View::mesh_groups`] was built from.
    ///
    /// Kept because the occlusion image is a graph transient: its view is known
    /// only at execute time, so the camera's group has to be rebuilt inside the
    /// forward pass, and re-deriving twenty bindings there would mean carrying
    /// half of `build`'s locals into the frame. Exactly two entries —
    /// [`AMBIENT_OCCLUSION_BINDING`]'s and [`CONTACT_SHADOW_BINDING`]'s — differ
    /// between the stored list and what the rebuild writes.
    pub(super) mesh_group_entries: Vec<Vec<BindGroupEntry>>,
    /// `[frame]`: the entries [`View::prepass_groups`] was built from.
    ///
    /// Kept for one rebuild only: the page sampler's, which
    /// [`ForwardRenderer::adopt_page_sampler`] performs on every group of the
    /// mesh layout a slot holds. Handles, so the cost is a few words a group.
    pub(super) prepass_group_entries: Vec<Vec<BindGroupEntry>>,
    /// `[frame]`: the camera's group rebuilt against the two screen-space
    /// channels the forward pass reads — the blurred occlusion and the contact
    /// shadow — cached against both views together.
    ///
    /// [`View::tonemap_groups`]' shape, one per frame in flight because the
    /// group it replaces is per frame in flight. Rebuilt only when a view
    /// changes, which is only on a resize or a toggle.
    ///
    /// **One cache for both channels rather than one each**, because they are
    /// two bindings of *one* group: rebuilding it twice a frame would be a
    /// descriptor write per pass per frame, which is what
    /// [`crate::ssao::cached_group`] exists to avoid. Its key is every view, so
    /// either one moving is a miss.
    ///
    /// [`View::mesh_groups`] is the fallback and is *not* dead weight: it is
    /// what the depth prepass binds, because that pass runs before there is any
    /// occlusion to name.
    pub(super) screen_channel_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `[frame]`: the depth prepass's group — the camera's, with the occlusion
    /// placeholder and **a culling-statistics buffer of its own**.
    ///
    /// The second half is the whole reason this is a group rather than
    /// [`View::mesh_groups`] reused. On the mesh-shader path the prepass runs
    /// the same amplification stage the forward pass does, and that stage counts
    /// every surviving cluster into the buffer bound at CLUSTER_CULL_STATS_BINDING — so sharing
    /// the camera's would make
    /// [`CullStats::clusters`](crate::cull_stats::CullStats::clusters) report
    /// every cluster of the frame twice, which is a plausible number and a wrong
    /// one.
    ///
    /// **Nothing reads what this counts and nothing clears it.** It is a sink: a
    /// wrapping `u32` whose value is never looked at, which is the honest price
    /// of a prepass that shares a pipeline with the pass it precedes.
    pub(super) prepass_groups: Vec<BindGroupHandle>,
    /// `[frame]`: the sink [`View::prepass_groups`] counts into.
    ///
    /// Held so the prepass can declare it and the graph can barrier it. A ring
    /// rather than one buffer for every other per-frame resource's reason: the
    /// previous frame's submission may still be writing last frame's.
    pub(super) prepass_stats: Vec<BufferHandle>,
    /// This frame's camera view-projection, as
    /// [`begin_frame`](ForwardRenderer::begin_frame) computed it.
    ///
    /// Kept because the ground grid's pass needs it and `add_passes` has no
    /// camera: recomputing it there would be a second `aspect` to get wrong, and
    /// a grid drawn through a camera the frame is not drawn with lands on the
    /// wrong pixels while still looking like a grid.
    pub(super) camera_view_proj: Mat4,
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
    pub(super) previous_view_projection: Option<Mat4>,
    /// `docs/plan/18-render-features.md`'s occlusion pair — see [`crate::ssao`].
    pub(super) ssao: Ssao,
    /// Topic 45's contact-shadow march — see
    /// [`crate::contact_shadows`].
    pub(super) contact_shadows: ContactShadows,
    /// `docs/plan/18-render-features.md`'s depth pyramid, which the reflection
    /// march climbs — see [`crate::hiz`].
    pub(super) hiz: Hiz,
    /// Topic 03 §3.3's farthest-depth pyramid,
    /// which the occlusion cull reads — see [`crate::occlusion_cull`].
    pub(super) occlusion_pyramid: OcclusionPyramid,
    /// `docs/plan/18-render-features.md`'s reflection march — see
    /// [`crate::ssr`].
    pub(super) ssr: Ssr,
    /// The froxel volume and its composite — see
    /// [`crate::volumetric`].
    pub(super) volumetric: Volumetric,
    /// Topic 43 §6's auto-exposure — see
    /// [`crate::exposure`]. Named for what it owns rather than for the value:
    /// [`ForwardRenderer::exposure`] is the number a caller set.
    pub(super) auto_exposure: Exposure,
    /// `docs/plan/18-render-features.md`'s bloom chain — see [`crate::bloom`].
    pub(super) bloom: Bloom,
    /// The cheap antialiasing tier — see
    /// [`crate::fxaa`].
    pub(super) fxaa: Fxaa,
    /// The higher antialiasing tier — see
    /// [`crate::cmaa2`]. It takes the resolve slot from [`View::fxaa`] on the
    /// frames [`RenderEffects::CMAA2`] is set for, and neither is built per
    /// frame: both exist, and at most one records.
    pub(super) cmaa2: Cmaa2,
    /// [`crate::upscale`], and it draws nothing at a
    /// [`render_scale`](ForwardRenderer::render_scale) of `1.0`.
    pub(super) upscale: Upscale,
    /// Topic 43 §8's background pass — see
    /// [`crate::sky_pass`]. It draws on no frame whose sky is [`Sky::NONE`],
    /// which is every frame until a caller calls
    /// [`set_sky`](ForwardRenderer::set_sky).
    pub(super) sky_pass: SkyPass,
    /// `docs/plan/55-water.md`'s surface passes — see [`crate::water`]. They
    /// draw on no frame with no bodies, which is every frame until a caller
    /// calls [`set_water`](ForwardRenderer::set_water).
    pub(super) water: Water,
    /// `docs/plan/57-grass.md`'s three passes — see [`crate::grass`]. They run
    /// on no frame with no field, which is every frame until a caller calls
    /// [`set_grass`](ForwardRenderer::set_grass).
    pub(super) grass: crate::grass::Grass,
}

impl View {
    /// Creates one camera's resources against the scene `inputs` describes.
    ///
    /// **All or nothing**: a failure part-way releases what this call had
    /// created, so the caller has nothing of the view to clean up, and a success
    /// hands every handle to the returned [`View`] — none of them is left in a
    /// rollback the caller might run later.
    ///
    /// `service` is called after each subsystem is built, and between the
    /// grass pass's pipelines — see
    /// [`ForwardRenderer::with_scene_serviced`], which is where the reason and
    /// the measurement are.
    ///
    /// # Errors
    ///
    /// [`HalError`] if any buffer, group or pipeline could not be created.
    pub(super) fn build(
        device: &dyn Device,
        queue: QueueHandle,
        inputs: &ViewInputs<'_>,
        service: &mut dyn FnMut(),
    ) -> Result<Self, HalError> {
        let mut rollback = Rollback::default();
        let built = Self::build_into(device, queue, inputs, &mut rollback, service);
        if built.is_err() {
            rollback.run(device);
        }
        built
    }

    /// [`View::build`]'s body, with every handle it creates placed in
    /// `rollback` until the view is whole.
    fn build_into(
        device: &dyn Device,
        queue: QueueHandle,
        inputs: &ViewInputs<'_>,
        rollback: &mut Rollback,
        service: &mut dyn FnMut(),
    ) -> Result<Self, HalError> {
        let frames = inputs.instances.len();
        let draws = DrawGen::new(
            device,
            queue,
            &DrawGenDesc {
                label: Some("forward"),
                instances: inputs.instances,
                mesh_table: inputs.mesh_table,
                bucket_meshes: inputs.bucket_meshes,
                bucket_modes: inputs.bucket_modes,
                bucket_clusters: inputs.bucket_clusters,
                mesh_levels: inputs.mesh_levels,
                level_groups: inputs.level_groups,
                level_meshes: inputs.level_meshes,
                instance_capacity: inputs.instance_capacity,
                hidden_view: inputs.id.hidden_bit(),
                // Every camera can cull by occlusion: the regions it adds are a
                // few words a bucket, and the switch is per frame.
                mode: crcbl_shaders::draw_gen::DrawMode::Occlusion,
            },
        )?;
        let runs: Vec<BufferHandle> = (0..frames).map(|frame| draws.runs(frame)).collect();
        let args: Vec<BufferHandle> = (0..frames).map(|frame| draws.args(frame)).collect();
        // What the amplification stage reads and writes, and nothing else does:
        // this frame's frustum, and the culling statistics its surviving
        // clusters are counted into.
        let cull_params: Vec<BufferHandle> =
            (0..frames).map(|frame| draws.cull_params(frame)).collect();
        let cull_stats: Vec<BufferHandle> = (0..frames)
            .map(|frame| draws.visible_count(frame))
            .collect();
        rollback.draws = Some(draws);
        service();

        // Topic 25's observable: one word per resident cluster,
        // holding the cut the descent chose. Empty where there is no
        // amplification stage, which is the same condition CLUSTER_SELECTION_BINDING exists
        // under — and the two cannot disagree, because this vector is what
        // decides whether the entry is written.
        //
        // **One buffer per frame in flight**, on `cull_stats`' terms exactly: a
        // frame still in flight is a frame still writing, and one buffer shared
        // across the ring would have the next frame's dispatch overwriting what
        // this one recorded. `TRANSFER_SRC` because reading it is the point.
        //
        // Allocated on the whole mesh path rather than only where there is an
        // amplification stage to write them, because the layout declares
        // CLUSTER_SELECTION_BINDING there — see the layout, which is where that is argued. On
        // a device with no task stage nothing writes them and
        // `ForwardRenderer::cluster_selection` still answers `None`, so the
        // cost is the allocation and nothing else.
        let mut cluster_selection: Vec<BufferHandle> = Vec::new();
        if inputs.emit.is_mesh() {
            let count = inputs
                .clusters
                .unwrap_or_else(|| unreachable!("the mesh path implies a cluster pool"))
                .count();
            for frame in 0..frames {
                let buffer = device.create_buffer(&BufferDesc {
                    label: Some(&format!("cluster selection {frame}")),
                    size: u64::from(count) * 4,
                    usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_SRC),
                    memory: MemoryLocation::DeviceLocal,
                })?;
                rollback.buffers.push(buffer);
                cluster_selection.push(buffer);
            }
        }

        // Topic 18's light list and the froxel grid its compute pass fills.
        //
        // Built after the cull because it needs the culling-statistics ring:
        // its overflow counter is a word of that buffer, which is what keeps
        // topic 03 §3.6's readback at one.
        rollback.lights = Some(LightGrid::new(
            device,
            &LightGridDesc {
                label: Some("lights"),
                frames,
                lights: inputs.light_capacity,
                froxels: FROXEL_CAPACITY,
                stats: &cull_stats,
            },
        )?);
        service();
        let lights = rollback.lights.as_ref().expect("just stored");
        let draws = rollback.draws.as_ref().expect("stored above");

        // **A fixed view's probe table is one zeroed row**, bound in place of
        // the scene's. Its frame block carries the empty probe header, and
        // `mesh.slang` evaluates that header as row 0 of whatever table is
        // bound — which is zero for a scene with no probes only because that
        // scene's table is. Host-uploaded and read-only, on the light list's
        // terms, and written once: nothing ever changes a zero.
        //
        // **Zero even when the view has an environment**, which reaches the
        // reflection march as its sky rows instead. A probe row is weighed by
        // the frame's per-probe visibility map before it is divided back out,
        // and `(a * w) / w` is not always `a` in floating point — so a lit row
        // here could come back rounded differently from one level to the next.
        // A zero comes back zero whatever it is weighed by.
        let fixed_probes = match inputs.lighting {
            ViewLighting::Scene => None,
            ViewLighting::Fixed { .. } => {
                let buffer = device.create_buffer(&BufferDesc {
                    label: Some("fixed view probes"),
                    size: crcbl_shaders::probe::PROBE_STRIDE as u64,
                    usage: BufferUsage::STORAGE,
                    memory: MemoryLocation::HostUpload,
                })?;
                rollback.buffers.push(buffer);
                device.write_buffer(buffer, 0, &crcbl_shaders::probe::GpuProbe::ZERO.to_bytes())?;
                Some(buffer)
            }
        };

        let mut uniforms = Vec::with_capacity(frames);
        let mut mesh_groups = Vec::with_capacity(frames);
        let mut mesh_group_entries = Vec::with_capacity(frames);
        let mut prepass_groups = Vec::with_capacity(frames);
        let mut prepass_stats = Vec::with_capacity(frames);
        let mut prepass_group_entries = Vec::with_capacity(frames);
        for (frame, &slot_instances) in inputs.instances.iter().enumerate() {
            // Everything a group of this layout names that is the same in all of
            // this frame's. The per-group half is what `MeshGroup` below varies,
            // and the two exist so the colour pass's group and the shadow pass's
            // are one description rather than two that agree today.
            let shared = SharedBindings {
                vertices: inputs.vertices,
                draw_constants: inputs.draw_constants,
                mesh_table: inputs.mesh_table,
                materials: inputs.materials,
                page: inputs.page,
                normal_page: inputs.normal_page,
                mro_page: inputs.mro_page,
                emissive_page: inputs.emissive_page,
                page_sampler: inputs.page_samplers[frame],
                clusters: inputs.clusters,
                shadow_sampler: inputs.shadow_sampler,
                lights: lights.lights(frame),
                light_grid: lights.grid(frame),
                probes: fixed_probes.unwrap_or(inputs.probes[frame]),
                tables: draws.tables(),
                specular_dfg: inputs.specular_dfg,
                ltc_table: inputs.ltc_table,
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
                group_state: inputs.emit.is_mesh().then(|| draws.group_state()),
                // The colour pass reads the finished atlas. Its own pass writes
                // nothing to it, so there is no conflict to avoid here.
                shadow_map: inputs.shadow_map,
                // The placeholder even for the camera's group: the occlusion
                // image is a graph transient and its view does not exist until
                // execute time. `add_passes` rebuilds this group against the real
                // one and caches it, and *this* group is what the depth prepass
                // binds — which runs before there is any occlusion to name.
                ambient_occlusion: inputs.ambient_occlusion,
                // The same, one binding along, and for the same reason.
                contact_shadow: inputs.contact_shadow,
                probe_visibility: inputs.probe_visibility,
            }
            .entries(&shared);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("mesh frame"),
                layout: inputs.mesh_layout,
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
            // [`View::screen_channel_groups`].
            mesh_group_entries.push(entries);

            // The depth prepass's group: this one again, counting its clusters
            // somewhere the camera's counter cannot see. See
            // [`View::prepass_groups`] for why that matters, and note that only
            // the amplification stage writes this binding, although both mesh
            // layouts retain it to preserve native argument indices.
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
                group_state: inputs.emit.is_mesh().then(|| draws.group_state()),
                shadow_map: inputs.shadow_map,
                ambient_occlusion: inputs.ambient_occlusion,
                contact_shadow: inputs.contact_shadow,
                probe_visibility: inputs.probe_visibility,
            }
            .entries(&shared);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("depth prepass"),
                layout: inputs.mesh_layout,
                entries: &entries,
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            prepass_groups.push(group);
            prepass_stats.push(stats);
            // For the page sampler's rebuild — see [`View::prepass_group_entries`].
            prepass_group_entries.push(entries);
        }

        // The exposure block, one per frame in flight for the frame uniforms'
        // reason exactly — the previous frame may still be reading last frame's
        // while this one is written. See the forward module docs on the ring.
        let mut tonemap_uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
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
            frames,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- the screen-space contact-shadow march ---
        //
        // Stored whole for the pair above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction.
        rollback.contact_shadows = Some(ContactShadows::new(
            device,
            frames,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- the screen-space reflection march ---
        //
        // Stored whole for the pair above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction.
        rollback.ssr = Some(Ssr::new(
            device,
            queue,
            frames,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- the Hi-Z pyramid the march climbs ---
        //
        // After the march it serves and before the chain below, on their reason:
        // `Rollback::run` releases in the reverse order of construction. Its
        // pipeline is the only one in this file with a depth attachment and no
        // colour one — see [`ForwardRenderer::build_depth_fullscreen`].
        rollback.hiz = Some(Hiz::new(
            device,
            frames,
            ForwardRenderer::build_depth_fullscreen,
        )?);
        service();
        // The occlusion cull's farthest pyramid beside it, built from the same
        // reduction. Its images wait for a frame's extent.
        rollback.occlusion_pyramid = Some(OcclusionPyramid::new(
            device,
            frames,
            ForwardRenderer::build_depth_fullscreen,
        )?);
        service();

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
            frames,
            FROXEL_CAPACITY,
            inputs.shadow_map,
            inputs.shadow_sampler,
            lights,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- auto-exposure ---
        //
        // Stored whole for the volume's reason, and after it because
        // `Rollback::run` releases in the reverse order of construction. It runs
        // between the chain and the tonemap in the frame as well: it bins the
        // picture the tonemap is about to read, which is the one with the lens
        // already on it — see [`crate::exposure`].
        rollback.exposure = Some(Exposure::new(device, queue, frames)?);
        service();

        // --- the bloom chain ---
        //
        // Stored whole for the pair above's reason, and after them because
        // `Rollback::run` releases in the reverse order of construction. It owns
        // a **linear** sampler of its own; the tonemap's sampler is `Nearest` on
        // purpose and `crate::bloom` says why the chain cannot share it.
        rollback.bloom = Some(Bloom::new(
            device,
            frames,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

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
            frames,
            inputs.target_format,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- the higher antialiasing tier ---
        //
        // The same slot, filled by CMAA2's two dispatches and one draw
        // instead of FXAA's one draw — see [`crate::cmaa2`], which says why the
        // two are built together and at most one recorded. It carries no lookup
        // table, so unlike the tier it replaced it takes no `queue`: there is
        // nothing to upload.
        rollback.cmaa2 = Some(Cmaa2::new(
            device,
            frames,
            inputs.target_format,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- the render-scale upscale ---
        //
        // The second pass that writes the caller's target rather than a
        // transient of its own. It draws on no frame at a render scale of
        // `1.0`, which is every frame until a caller moves it — see
        // [`crate::upscale`].
        rollback.upscale = Some(Upscale::new(
            device,
            frames,
            inputs.target_format,
            ForwardRenderer::build_fullscreen,
        )?);
        service();

        // --- the background ---
        //
        // Built last, so `Rollback::run` releases it first. It writes the scene
        // target rather than the caller's — it is scene content, drawn before
        // the operator, unlike the ground grid — and takes the depth format as
        // well, because it is the one full-screen pass in this frame that tests
        // against an attachment. See [`crate::sky_pass`].
        rollback.sky_pass = Some(SkyPass::new(
            device,
            frames,
            Format::Rgba16Float,
            Format::D32Float,
            ForwardRenderer::build_tested_fullscreen,
        )?);
        service();

        // --- the water surface ---
        //
        // After the background, so `Rollback::run` releases it before that
        // pass. It takes the atlas's sampler, which the renderer owns for its
        // life, and the full-screen builder with a depth state, because its copy
        // pass writes a colour target and a depth one at once. See
        // [`crate::water`].
        rollback.water = Some(Water::new(
            device,
            frames,
            inputs.shadow_sampler,
            ForwardRenderer::build_fullscreen_with,
        )?);
        service();

        // --- the grass field ---
        //
        // After the water surface, so `Rollback::run` releases it first. It
        // takes the atlas's sampler for the same reason that one does: the
        // cards are lit through a copy of the forward pass's cascade walk, and
        // the walk reads the atlas through a comparison sampler the renderer
        // owns for its life. See [`crate::grass`].
        rollback.grass = Some(crate::grass::Grass::new(
            device,
            frames,
            inputs.shadow_sampler,
            service,
        )?);
        service();

        // Whole, so the rollback lets go of every handle: the view owns them
        // from here, and a rollback still naming one would release it twice.
        let view =
            Self {
                effects: inputs.effects,
                background: inputs.background,
                lighting: inputs.lighting,
                // Replaced by every `begin_frame`, on `lod_params`' terms.
                frame_effects: inputs.effects,
                // No frame has begun it.
                begun: 0,
                draws: rollback.draws.take().unwrap_or_else(|| {
                    unreachable!("draw generation was placed in the rollback above")
                }),
                cluster_selection,
                fixed_probes,
                // Overwritten by the first `begin_frame`, which is the only thing
                // that can know the viewport. A zero scale with a budget of zero
                // selects nothing at all, and there is no frame yet to select for.
                lod_params: [0.0, 0.0, 0.0],
                lights: rollback.lights.take().unwrap_or_else(|| {
                    unreachable!("the light grid was placed in the rollback above")
                }),
                // Overwritten by the first `begin_frame` on `lod_params`' terms: a
                // one-froxel grid is the smallest legal one, and there is no
                // viewport yet to size a real one against.
                grid: Grid {
                    x: 1,
                    y: 1,
                    slices: 1,
                    tile_pixels: 1,
                },
                uniforms,
                mesh_groups,
                tonemap_uniforms,
                tonemap_groups: vec![None; frames],
                mesh_group_entries,
                prepass_group_entries,
                screen_channel_groups: vec![None; frames],
                prepass_groups,
                prepass_stats,
                // Replaced by every `begin_frame`, which `add_passes` documents as
                // having to run first.
                camera_view_proj: Mat4::IDENTITY,
                // No frame has been drawn, so there is no previous camera — see the
                // field, which says why that is not the identity.
                previous_view_projection: None,
                ssao: rollback.ssao.take().unwrap_or_else(|| {
                    unreachable!("the occlusion pair was placed in the rollback above")
                }),
                contact_shadows: rollback.contact_shadows.take().unwrap_or_else(|| {
                    unreachable!("the contact march was placed in the rollback above")
                }),
                hiz: rollback.hiz.take().unwrap_or_else(|| {
                    unreachable!("the pyramid was placed in the rollback above")
                }),
                occlusion_pyramid: rollback.occlusion_pyramid.take().unwrap_or_else(|| {
                    unreachable!("the occlusion pyramid was placed in the rollback above")
                }),
                ssr: rollback.ssr.take().unwrap_or_else(|| {
                    unreachable!("the reflection march was placed in the rollback above")
                }),
                volumetric: rollback.volumetric.take().unwrap_or_else(|| {
                    unreachable!("the froxel volume was placed in the rollback above")
                }),
                auto_exposure: rollback.exposure.take().unwrap_or_else(|| {
                    unreachable!("the histogram was placed in the rollback above")
                }),
                bloom: rollback.bloom.take().unwrap_or_else(|| {
                    unreachable!("the bloom chain was placed in the rollback above")
                }),
                fxaa: rollback.fxaa.take().unwrap_or_else(|| {
                    unreachable!("the resolve was placed in the rollback above")
                }),
                cmaa2: rollback.cmaa2.take().unwrap_or_else(|| {
                    unreachable!("the higher tier was placed in the rollback above")
                }),
                upscale: rollback.upscale.take().unwrap_or_else(|| {
                    unreachable!("the upscale was placed in the rollback above")
                }),
                sky_pass: rollback
                    .sky_pass
                    .take()
                    .unwrap_or_else(|| unreachable!("the sky was placed in the rollback above")),
                water: rollback
                    .water
                    .take()
                    .unwrap_or_else(|| unreachable!("the water was placed in the rollback above")),
                grass: rollback
                    .grass
                    .take()
                    .unwrap_or_else(|| unreachable!("the grass was placed in the rollback above")),
            };
        rollback.buffers.clear();
        rollback.bind_groups.clear();
        Ok(view)
    }

    /// Writes every per-frame block this view reads, for `camera`, into the
    /// slot `frame` names — the camera's half of
    /// [`ForwardRenderer::begin_frame`].
    ///
    /// Returns the frame block it wrote, which the shadow views of the primary
    /// camera are spread from.
    ///
    /// # Errors
    ///
    /// [`HalError`] if any of the view's blocks could not be written.
    pub(super) fn begin_frame(
        &mut self,
        device: &dyn Device,
        camera: &Camera,
        frame: &ViewFrame<'_>,
        sky_view: Option<&PresentedSky>,
    ) -> Result<mesh::FrameUniforms, HalError> {
        let scene = frame.scene;
        let slot = frame.slot;
        let extent = frame.extent;
        // Read before this call advances it below: the first occlusion phase
        // reprojects through the matrix the pyramid it reads was drawn with.
        let previous_view_projection = self.previous_view_projection;
        // One matrix, used twice. Recomputing it for the frustum below would be
        // two chances to pass a different aspect ratio, and the failure that
        // produces — geometry culled against a camera the frame does not draw
        // with — is invisible until something at the edge of the screen
        // disappears.
        let view_projection = camera.view_projection(frame.aspect);
        // What this frame draws here, frozen for the reason the frame's own
        // effects are: this call and the passes have to agree. The shadow bit is
        // the frame's whatever this view asked for — see [`ViewDesc::effects`] —
        // and the effects that bring the frame's light in are this view's
        // lighting's to take away — see [`ViewLighting::SCENE_EFFECTS`].
        self.frame_effects = frame
            .effects
            .intersection(self.effects.union(RenderEffects::SHADOWS))
            .difference(self.lighting.dropped_effects());
        // What lights this view: the frame's sun, rows, probes, sky, fog and
        // shadow maps, or the one constant light its description fixed and
        // nothing of the frame's. Resolved once here so every block below reads
        // the same answer — a sun taken from one and a fill from the other
        // would be a view lit by half of each.
        //
        // **A fixed view's atlas rectangles are all empty**, which is how its
        // key light goes unshadowed with no shader of its own: every sampler of
        // the atlas answers lit for an empty rectangle before it reads a texel
        // (`mesh.slang`'s `atlas_rect_is_empty`), and the key is the view's only
        // row, so nothing else could name a tile.
        //
        // **A fixed view's environment is the sky its reflections see**, and
        // only its reflections: the gradient the march falls back to, with all
        // three bands the one radiance and no atmosphere, while the frame
        // block's L1 sky — the diffuse half — stays zero, so the fill is the
        // whole diffuse ambient. A uniform gradient is that radiance along
        // every direction and, to rounding, under every lobe the march
        // prefilters. The background pass keeps the frame's gradient: the
        // view's background is [`ViewDesc::background`]'s, not its lighting's.
        let key_row;
        let (light, rows, probe_volume, sky, fog, atlas_rects, reflected, reflected_air) =
            match self.lighting {
                ViewLighting::Scene => (
                    scene.light,
                    &scene.rows[..],
                    frame.probe_volume,
                    frame.sky_irradiance,
                    frame.fog,
                    scene.atlas_rects,
                    frame.gradient,
                    sky_view,
                ),
                ViewLighting::Fixed { key, environment } => {
                    key_row = [sun_row(&key)];
                    let band = environment.to_array();
                    (
                        key,
                        &key_row[..],
                        crcbl_shaders::probe::ProbeVolume::default(),
                        crcbl_shaders::probe::GpuProbe::ZERO,
                        Fog::NONE,
                        [[0.0; 4]; shadow::TILES],
                        crcbl_shaders::sky::SkyGradient {
                            zenith: band,
                            horizon: band,
                            ground: band,
                        },
                        None,
                    )
                }
            };
        self.begun = frame.serial;
        // The same matrix again for the ground grid, whose pass `add_passes`
        // records and which has no camera to ask.
        self.camera_view_proj = view_projection;
        // Topic 25's two selection numbers, from this frame's
        // viewport and this frame's camera. An orthographic projection has no
        // distance falloff for the metric to divide by, so it selects under a
        // budget nothing satisfies and draws the base level whole — see
        // [`LOD_BUDGET_NONE`].
        let lod_scale = camera.projection.pixels_per_unit(extent.1 as f32);
        let lod_budget = if camera.projection.is_orthographic() {
            LOD_BUDGET_NONE
        } else {
            frame.lod_error_budget
        };
        self.lod_params = [lod_scale, lod_budget, lod_budget * frame.lod_hold_ratio];

        // The grid this frame's viewport and camera get. An orthographic camera
        // has no view depth to slice by — its `clip.w` is 1 everywhere — so it
        // runs with one slice, which `light_cluster.slang` builds a different
        // way rather than pretending it is a perspective frustum.
        let perspective = !camera.projection.is_orthographic();
        self.grid = Grid::for_frame(extent, perspective, self.lights.froxel_capacity());
        self.lights.begin_frame(
            device,
            slot,
            rows,
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
            slot,
            self.grid,
            FrameView {
                extent,
                view_projection,
                eye: camera.eye,
                perspective,
            },
            Medium {
                fog: frame.fog,
                sun: &scene.light,
                cascades: &scene.cascades,
                light_view_proj: &scene.light_view_proj,
                atlas_rects: &scene.atlas_rects,
            },
        )?;

        let gradient = frame.gradient;
        let uniforms = mesh::FrameUniforms {
            view_proj: view_projection.to_cols_array(),
            camera_position: camera.eye.extend(1.0).to_array(),
            // The ambient's `w` is the normals view's switch — see
            // `set_normals_view` and the constants it names. A renderer nobody
            // has called that on writes the `0.0` this line has always written,
            // which is what makes every golden image untouched by the feature.
            ambient: light.ambient.extend(frame.debug_view_lane).to_array(),
            // **The frame's cascades, whichever camera this is.** They were
            // fitted to the primary camera and drawn once; a secondary view
            // samples the same maps through the same matrices, which is
            // `docs/plan/29-fp-rendering.md`'s "shadow maps reused".
            shadow_view_proj: scene.shadow_view_proj,
            cascade_far: scene.cascades.far,
            shadow_params: Cascades::params(),
            cluster_grid: self.grid.to_frame_block(),
            light_view_proj: scene.light_view_proj,
            // The scene's grid, unchanged since `with_scene` read it: the
            // probes are static and nothing here varies them per frame. A
            // description with no probes leaves the default, which evaluates to
            // exactly zero in the shader.
            probes: probe_volume,
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
                    fog.density
                },
                fog.falloff,
                fog.reference_height,
                0.0,
            ],
            fog_color: fog.color.extend(0.0).to_array(),
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
            // about topic 43 §2's two streams that a
            // shader cannot derive: it is the pool's *capacity*, and a shader
            // has never been told that. Fixed for the life of the pool and
            // written every frame anyway, because this is the block that
            // carries it and there is no cheaper place. `yzw` are padding.
            vertex_pool: [frame.attribute_base, 0, 0, 0],
            // Where each atlas slot's map went, out of the allocator
            // `shadow::Selection::update` just spent — the *whole* of what the
            // sampling side knows about the atlas's shape, so a map is read
            // from the rectangle it was rendered into whatever size that was.
            // None at all on a fixed view — see `atlas_rects` above.
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
        // else that could look at it: this runs exactly once per
        // `InstancePool::rotate` for a view that is drawn every frame, so the
        // camera's history moves on the same frame boundary the instances' does.
        // See the field.
        self.previous_view_projection = Some(view_projection);
        device.write_buffer(self.uniforms[slot], 0, &uniforms.to_bytes())?;

        // The extent auto-exposure bins, which is the internal one — the
        // histogram reads the scene target, not the caller's window.
        //
        // Written whether or not this frame adds the passes, on every other
        // block's terms: one written only on the frames that measure is stale on
        // the frame a caller first switches the effect on.
        self.auto_exposure
            .begin_frame(device, slot, extent, frame.exposure_adaptation)?;

        // The tonemap's one number, written here rather than in `add_passes` for
        // every other block's reason: a pass body runs at execute time, and the
        // buffer it reads has to have been written before the frame was
        // submitted.
        device.write_buffer(
            self.tonemap_uniforms[slot],
            0,
            &tonemap::TonemapParams {
                exposure: frame.exposure,
                // **Resolved rather than requested**, on `resolved_effects`'
                // terms: a readout view runs the clamp whatever the caller set,
                // because its pixels are data — see `resolved_tonemap_curve`.
                curve: frame.tonemap_curve,
                // **The switch, and it is this view's effects rather than the
                // caller's request**: a device that refused the effect draws the
                // frame with the number a caller set, and reading a buffer no
                // pass wrote would be reading whatever the last frame in this
                // slot left there.
                auto_exposure: self.frame_effects.contains(RenderEffects::AUTO_EXPOSURE),
                // The frame's coverage into the target's alpha on a transparent
                // view, and the opaque one every other view writes — see
                // [`ViewBackground::Transparent`].
                coverage_alpha: self.background.is_transparent(),
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
        let projection = camera.projection.matrix(frame.aspect);
        let inv_projection = projection.inverse();
        self.ssao.begin_frame(
            device,
            slot,
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
            slot,
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
            slot,
            ssr::SsrParams {
                inv_proj: inv_projection.to_cols_array(),
                proj: projection.to_cols_array(),
                inv_view: camera.view().inverse().to_cols_array(),
                probe_volume,
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
                // nothing. A fixed view's is its environment — see `reflected`
                // above.
                sky: reflected.rows(),
                // And the arm, on the background pass's terms below: an
                // atmosphere frame hands the march the sun its LUT was built
                // around, and every other frame — a fixed view's included —
                // the exactly-zero `w` that leaves `ssr.slang` evaluating the
                // gradient it always did.
                atmosphere: match reflected_air {
                    Some(presented) => {
                        let sun = presented.view.sun_direction();
                        [sun[0], sun[1], sun[2], crcbl_shaders::sky::ATMOSPHERE_ON]
                    }
                    None => [0.0, 0.0, 0.0, crcbl_shaders::sky::ATMOSPHERE_OFF],
                },
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
            slot,
            inv_projection.to_cols_array(),
            camera.view().inverse().to_cols_array(),
            &gradient,
            sky_view.map(|presented| &presented.view),
        )?;
        // The water surface's own block: the sun its scattering is lit by and
        // whether this frame's air is the froxel column's. Written whether or
        // not the frame has water, on the blocks above's terms. The sun is
        // normalised on `sun_row`'s terms — a caller may hand the renderer any
        // vector, and the shader reads this as a unit direction.
        self.water.begin_frame(
            device,
            slot,
            crcbl_shaders::water::WaterParams {
                sun_direction: light.direction.normalize_or_zero().extend(0.0).to_array(),
                sun_color: light.color.extend(0.0).to_array(),
                froxels: self.frame_effects.contains(RenderEffects::VOLUMETRIC_FOG),
            },
        )?;
        // The grass field's two blocks: the generation pass's, whose camera is
        // this view's, and the wind's, which the renderer holds for the whole
        // frame. Written whether or not the frame has a field, on the block
        // above's terms.
        self.grass.begin_frame(
            device,
            slot,
            frame
                .grass
                .as_ref()
                .map_or_else(Default::default, |grass| grass.gen_params(camera.eye)),
            frame.wind,
            crcbl_shaders::grass::Params {
                limits: [
                    crate::grass::SLOT_CAPACITY,
                    frame
                        .grass
                        .as_ref()
                        .map_or(1, crate::grass::GrassFrame::rows),
                    0,
                    0,
                ],
                // **How many pixels a metre spans at one unit of view depth.**
                // The projection's own `x` scale is `1 / (aspect · tan(fov/2))`
                // for a perspective camera and `1 / half-width` for an
                // orthographic one, and half the viewport's width takes a
                // normalised-device length to pixels — so a card of `w` metres
                // at a view depth of `d` is `w · this / d` pixels across, which
                // is exactly what `grassCardLevel` reads. Taken off the very
                // matrix this frame draws with rather than from the camera's
                // own fields, so a projection this view resolved differently
                // cannot disagree with it.
                screen: [
                    view_projection.x_axis.x * 0.5 * extent.0 as f32,
                    0.0,
                    0.0,
                    0.0,
                ],
            },
        )?;
        // The bloom chain's blocks, one row per step: each step needs the texel
        // size of the image it reads, and the chain's shape is a function of the
        // extent alone. Written whether or not the chain is in this frame, on
        // the two blocks above's terms — a row nobody reads costs sixteen bytes.
        self.bloom.begin_frame(device, slot, extent)?;
        // The resolve's block: the reciprocal of the extent and three constants.
        // Written on the chain's terms above — a frame that adds no resolve pays
        // for twenty bytes nobody reads, and a block written only on the frames
        // that use it is a block that is stale on the frame a caller switches
        // the effect on.
        self.fxaa.begin_frame(device, slot, extent)?;
        // The higher tier's block: the extent and the two list capacities it
        // implies, written on the resolve's terms above. Sixteen bytes, and a
        // frame that resolves through the other tier pays for them and reads
        // none.
        self.cmaa2.begin_frame(device, slot, extent)?;
        // The upscale's block: the internal extent and its reciprocal. Written
        // on the two above's terms — a frame at full scale adds no upscale pass
        // and pays for sixteen bytes nobody reads, and a block written only on
        // the frames that use it is stale on the frame a caller moves the knob.
        self.upscale.begin_frame(device, slot, extent)?;

        // The camera and the two selection numbers go to the cull/draw-argument
        // pair as well as into the block above, and they are handed over rather
        // than re-derived: topic 25's uniform cut runs there, the
        // mesh path's per-cluster descent runs off the block, and a frame that
        // selected detail against one camera while drawing with another is a
        // difference nothing in the frame can see.
        //
        // **The eye is the selection's**, which is the camera's unless a caller
        // pinned the primary camera's — `set_frozen_selection_eye`, and the
        // whole of what that feature is. It reaches this parameter block and
        // nothing else: the frustum handed over with it is extracted from this
        // frame's own view-projection, and the frame block written above carries
        // `camera.eye`, so a pinned selection changes which cut is chosen and
        // nothing about what is culled, faced or drawn.
        //
        // **And what the cull does beyond the frustum.** The pyramid is made to
        // match this frame's extent first — a resize replaces it and forgets
        // its history — and a frame with no pyramid to read culls by the
        // frustum alone, which is what a one-texel target gets.
        let history = self.occlusion_pyramid.take_history(extent);
        let cull = match self.draws.occlusion_layout() {
            Some(layout) if frame.occlusion.any() => {
                self.occlusion_pyramid.prepare(device, extent, layout)?;
                if self.occlusion_pyramid.cull_group().is_some() {
                    FrameCull::Occlusion(OcclusionFrame {
                        view_projection,
                        previous_view_projection: previous_view_projection
                            .unwrap_or(view_projection),
                        target: extent,
                        pyramid_levels: self.occlusion_pyramid.levels(),
                        occlusion: frame.occlusion.occlusion,
                        // A first frame has no previous camera, and a chain
                        // just rebuilt holds nothing.
                        history: history && previous_view_projection.is_some(),
                        small_feature_pixels: frame.occlusion.small_feature_pixels,
                    })
                } else {
                    FrameCull::Plain
                }
            }
            _ => FrameCull::Plain,
        };
        self.draws.begin_frame_with(
            device,
            slot,
            &Frustum::from_view_projection(view_projection),
            frame.instance_count,
            crate::draw_gen::Selection {
                camera_position: [
                    frame.selection_eye.x,
                    frame.selection_eye.y,
                    frame.selection_eye.z,
                ],
                lod_params: self.lod_params,
            },
            &cull,
        )?;
        Ok(uniforms)
    }

    /// Records this view's cull and draw-argument dispatches and the light
    /// clustering after them — the passes every other pass of the view reads.
    ///
    /// Separate from [`View::add_passes`] because the primary camera records
    /// these before the shadow atlas, which is where they always ran, and a
    /// secondary view records them straight before the rest of its frame.
    pub(super) fn add_cull(
        &self,
        graph: &mut RenderGraph<'_>,
        pool: &TransientPool,
        slot: usize,
        instance_count: u32,
    ) -> ViewCull {
        // The cull dispatch and the draw-argument dispatch, before anything
        // draws. Every barrier between them and the passes that draw — including
        // the one into `IndirectArgument` — is the graph's, computed from what
        // each pass declares.
        //
        // **Through the occlusion entry point** on a frame `begin_frame` began
        // with occlusion or small-feature culling, reading the pyramid the last
        // frame left: `DrawGen::begin_frame_with` recorded which.
        let occlusion_group = self
            .occlusion_pyramid
            .cull_group()
            .filter(|_| self.draws.uses_occlusion_entry(slot));
        let (generated, pyramid) = match occlusion_group {
            Some(group) => {
                let levels = self.occlusion_pyramid.import(graph, pool);
                let generated = self.draws.add_occlusion_passes(
                    graph,
                    slot,
                    instance_count,
                    &crate::draw_gen::PyramidInputs {
                        levels: &levels,
                        group,
                    },
                );
                let two_phase =
                    self.draws.frame_mode(slot) == crcbl_shaders::draw_gen::DrawMode::Occlusion;
                (
                    generated,
                    two_phase.then_some(ViewPyramid { levels, group }),
                )
            }
            None => (self.draws.add_passes(graph, slot, instance_count), None),
        };
        // Topic 25's record of the view's cut. Written by exactly
        // one mesh pass of this view, so what the graph orders here is this
        // frame's write against the next frame's use of the same slot.
        let selection = self
            .cluster_selection
            .get(slot)
            .map(|&buffer| import_read_write(graph, "cluster-selection", buffer));
        // Topic 18's clustering, **after the pair above and before anything
        // draws**. After, because the clearing dispatch is what zeroes the
        // overflow counter this pass adds to, and the two are ordered by the
        // graph out of the one id they both declare — which is why `stats` is
        // the id the draw generator handed back rather than a second import of
        // the same buffer. It has no other dependency on the cull: lights are
        // assigned to froxels, and a froxel is a property of the camera.
        let light_grid = self
            .lights
            .add_pass(graph, slot, generated.visible_count_id, self.grid);
        ViewCull {
            generated,
            selection,
            light_grid,
            pyramid,
            instance_count,
        }
    }

    /// Records this view's frame: its prepass, the screen-space passes, the
    /// forward pass and every pass after it, ending in `target`.
    ///
    /// `output` is where the frame ends and how large it is drawn; `cull` is
    /// what [`View::add_cull`] recorded for this frame. The scene's passes — the
    /// skinning dispatch, the shadow atlas and the probe gather — are already
    /// in `graph`, and `passes` names what they left behind.
    ///
    /// Returns the image the tonemap read, which is the HDR frame before the
    /// operator.
    pub(super) fn add_passes<'a, F>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        passes: &FramePasses,
        cull: ViewCull,
        output: ViewOutput,
        overlays: Overlays<'a>,
        overlay: F,
    ) -> ImageId
    where
        F: FnOnce(&mut RenderGraph<'a>, ForwardOverlayTargets),
    {
        let ViewOutput {
            target,
            target_extent,
            extent,
        } = output;
        let frame = passes.slot;
        // What this frame draws in this view, frozen by `begin_frame` — see
        // [`View::frame_effects`]. Read once here so every conditional below is
        // about one value rather than about a read of a field each.
        let effects = self.frame_effects;
        let upscaling = extent != target_extent;
        let ViewCull {
            generated,
            selection,
            light_grid,
            pyramid: culled_pyramid,
            instance_count,
        } = cull;
        let emit = passes.emit;
        let skinned = passes.skinned;
        // The frame's sky, on every view but a transparent one — whose
        // background is the empty one it clears to. See
        // [`ViewBackground::Transparent`].
        let transparent = self.background.is_transparent();
        let draws_sky = passes.draws_sky && !transparent;
        // The table the reflection march and the water surface fall back to:
        // the scene's, or a fixed view's zeroed row — the one its forward
        // groups were built naming, see [`View::fixed_probes`]. The graph read
        // stays the scene's slot, which the frame wrote either way.
        let probe_buffer = self.fixed_probes.unwrap_or(passes.probe_buffer);
        let probe_table = passes.probe_table;
        let occlusion_placeholder = passes.occlusion_placeholder;
        let contact_placeholder = passes.contact_placeholder;
        let shadow_atlas = passes.shadow_atlas;
        let MaterialPages {
            base_color: base_color_page,
            normal: normal_page,
            mro: mro_page,
            emissive: emissive_page,
        } = passes.pages;
        let wireframe = passes.wireframe;
        let prepass_partitions = passes.prepass_partitions.clone();
        let color_partitions = passes.color_partitions.clone();
        let group = self.mesh_groups[frame];

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
            let again = (passes.ssao_blurs > 1).then(|| {
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
                TransientImageDesc::contact_shadows(extent),
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
        // what it wrote. Nothing does yet — TAA (`docs/backlog.md`) is the
        // first pass that will, and until then `DebugView::Motion`
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
                TransientImageDesc::display_color(extent, passes.target_format),
            )
        } else {
            target
        };
        //
        // **Two tiers share that slot and at most one fills it** — see
        // [`RenderEffects::CMAA2`], which is where "never both" is written down.
        // The shape above is the same either way; what differs is what the
        // resolve reads besides the tonemap's image. CMAA2's two working
        // buffers are declared inside its own pass group rather than here,
        // where the tier it replaced declared two images: they are buffers,
        // and [`crate::cmaa2`]'s header says why the graph owns them all the
        // same.
        let resolving = effects.intersects(RenderEffects::ANTIALIASING.union(RenderEffects::CMAA2));
        let display = if resolving {
            graph.create_image(
                "display-color",
                TransientImageDesc::display_color(extent, passes.target_format),
            )
        } else {
            present
        };
        // --- the depth prepass ---
        //
        // `docs/plan/18-render-features.md`'s prepass, and it is unusually cheap:
        // the depth-only pipeline is already the shadow cascades' own, built from
        // the same modules and the same layout as the colour pipeline, so driven
        // with the camera's draws and a copy of the camera's bind group it *is* a
        // scene depth prepass — no new pipeline and no new shader. On a frame
        // whose material table masks nothing it is `depthVertexMain` and no
        // fragment stage, which is why the split-stream vertex pool pays for this
        // pass as well as for the atlas; a scene that masks something records its
        // masked buckets under the cutout twin and its opaque ones under this
        // one, and `depth_partitions` is where that split and its price are
        // argued.
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
        // module compiled in one invocation — and the cutout pipeline runs
        // `vertexMain` itself, so a masked frame's prepass is not merely the same
        // arithmetic as the colour pass's but the same code. The observable is a screenshot:
        // holes are not subtle, and the render-e2e goldens are what would catch
        // them on a rasteriser this machine does not have.
        let depth_group = self.prepass_groups[frame];
        // The prepass's own cluster counter — see
        // [`View::prepass_stats`]. Imported in the state the last frame
        // on this slot left it in, on `cluster-selection`'s terms exactly: a
        // barrier naming `Undefined` as its source carries no source scope, so it
        // would order this frame's write against nothing.
        let prepass_stats = import_read_write(graph, "prepass-stats", self.prepass_stats[frame]);
        let prepass_reads = PrepassReads {
            shadow_atlas,
            occlusion_placeholder,
            pages: passes.pages,
            probe_table,
            skinned,
            selection,
            prepass_stats,
        };
        // **Which of the generator's draw regions the prepass draws.** With the
        // occlusion cull on it is the survivors the first phase passed — the
        // late prepass below draws the ones the second phase rescued, and the
        // forward pass region 0, which is both. Off, it is region 0, which is
        // every survivor, exactly as before the cull existed.
        let two_phase = culled_pyramid.is_some();
        let early_region = if two_phase {
            crcbl_shaders::draw_gen::EARLY_REGION
        } else {
            0
        };
        let prepass = graph.add_render_pass("depth-prepass").depth(
            scene_depth,
            LoadOp::Clear,
            StoreOp::Store,
            crcbl_hal::ClearValue {
                depth: crcbl_hal::depth::CLEAR,
                ..crcbl_hal::ClearValue::default()
            },
        );
        let prepass = declare_prepass_reads(prepass, &prepass_reads, &generated, emit);
        let early_partitions = if two_phase {
            prepass_partitions.clone()
        } else {
            Vec::new()
        };
        prepass.execute(move |ctx| {
            let encoder = ctx.encoder();
            // One `open` per partition and its own buckets under it: the whole
            // pass is still one indirect call per bucket, drawn under the
            // pipeline that bucket's mode asked for.
            for partition in &prepass_partitions {
                partition.open(encoder);
                partition.record_region(encoder, depth_group, &generated, early_region);
            }
        });

        // --- the occlusion cull's second phase ---
        //
        // Topic 03 §3.3: this frame's farthest
        // pyramid out of the early depth, every survivor the first phase marked
        // tested against it, and the ones it rescues drawn into the same depth
        // before anything reads it. See [`crate::occlusion_cull`].
        if let Some(culled) = &culled_pyramid {
            let levels = culled.levels.clone();
            let group = culled.group;
            self.occlusion_pyramid
                .add_passes(graph, frame, scene_depth, &levels);
            self.draws.add_late_passes(
                graph,
                frame,
                instance_count,
                &generated,
                &crate::draw_gen::PyramidInputs {
                    levels: &levels,
                    group,
                },
            );
            // **Loaded, not cleared**: the early prepass's depth is what the
            // rescued survivors are drawn into, and what everything after reads
            // is the two together.
            let late = graph.add_render_pass("depth-prepass-late").depth(
                scene_depth,
                LoadOp::Load,
                StoreOp::Store,
                crcbl_hal::ClearValue::default(),
            );
            let late = declare_prepass_reads(late, &prepass_reads, &generated, emit);
            late.execute(move |ctx| {
                let encoder = ctx.encoder();
                for partition in &early_partitions {
                    partition.open(encoder);
                    partition.record_region(
                        encoder,
                        depth_group,
                        &generated,
                        crcbl_shaders::draw_gen::LATE_REGION,
                    );
                }
            });
        }

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
        // Topic 45's contact march, or the one texel that
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
            // Transparent black on a transparent view: zero coverage where no
            // geometry lands, and no colour for the air in front of the far
            // plane to add to but its own.
            .clear_color(
                scene_color,
                if transparent {
                    TRANSPARENT_CLEAR
                } else {
                    SCENE_CLEAR
                },
            )
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
            // **And §2's other three pages, every one of which this pass's
            // fragment stage literally samples** — the normal map it perturbs
            // through, the packed roughness and metalness its lobe takes, and
            // the emissive radiance it adds. See `normal_page_import`.
            .read_image(normal_page)
            .read_image(mro_page)
            .read_image(emissive_page)
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
            // the irradiance probes' gather can write them — and
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
        // `View::mesh_group_entries`.
        //
        //
        // The rebuild is unconditional, so there is one shape of forward pass
        // rather than two: the group is cached against the views it was built
        // from, so an AO-off frame naming the placeholder's view and an AO-on
        // frame naming the blur's target are the same code and one cache miss
        // apiece when a toggle moves. **The key is both views**, because a group
        // naming two transients is stale as soon as either one moves.
        let entries = self.mesh_group_entries[frame].as_slice();
        let mesh_layout = passes.mesh_layout;
        // The captured probe-visibility maps, or the one-texel placeholder when
        // nothing has been captured or the console switch is off. It rides
        // through the same cache as the two channels beside it — it is not a
        // graph transient, but it does change between frames when the switch
        // moves, and a group cached against a view it no longer names is the
        // failure that cache exists to stop.
        let probe_visibility_view = passes.probe_visibility;
        let cached_mesh = &mut self.screen_channel_groups[frame];
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
            // One `open` per partition and its own buckets under it, on the
            // depth prepass's terms: the pass is still one indirect call per
            // bucket, drawn under the pipeline that bucket's side asked for.
            for partition in &color_partitions {
                partition.open(encoder);
                partition.record(encoder, group, &generated);
            }
        });

        // `docs/plan/57-grass.md`'s field, **after the forward pass and before
        // the background**. Grass is opaque scene content with a cutout: the
        // depth its cards write has to be in the buffer before the sky decides
        // which pixels it covers, before the Hi-Z pyramid is built and before
        // the reflection march reads the frame. A frame with no field adds
        // nothing here and takes no transient — see [`crate::grass`].
        if let Some(field) = passes.grass {
            let frame_block = self.uniforms[frame];
            self.grass.add_passes(
                graph,
                frame,
                crate::grass::GrassImages {
                    color: scene_color,
                    depth: scene_depth,
                    shadow_atlas,
                },
                crate::grass::GrassInputs {
                    frame_block,
                    lights: self.lights.lights(frame),
                    cluster_lights: self.lights.grid(frame),
                    light_grid,
                    field,
                },
            );
        }

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

        // What the water surface reads out of three other passes' state, read
        // before those passes take their mutable borrows below — see
        // [`crate::water`] for why it binds their handles rather than copies.
        let water_reads = (
            self.uniforms[frame],
            self.ssr.uniforms(frame),
            self.ssr.sky_prefilter_view(),
            self.volumetric.buffers(frame),
            self.sky_pass.lut(frame),
        );

        // The froxel volume, and it composites over
        // the sky as well as over the geometry — a pixel at the far plane is a
        // whole column of air, which is exactly what makes a distant horizon
        // read as distant.
        //
        // **Before the reflection march**, where `mesh.slang`'s closed form also
        // ran: the march reads the scene colour as reflected radiance, and fog
        // it can see is fog the surface it bounced off could see. The reflection
        // the blur adds afterwards is still unfogged, which is the same gap the
        // analytic path has and `docs/backlog.md` carries.
        let (scene_color, froxel_ids) = match fogged {
            Some(composited) => {
                let ids = self.volumetric.add_passes(
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
                (composited, Some(ids))
            }
            None => (scene_color, None),
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
                // Read before the split borrow below: `self.ssr` is taken
                // mutably by `add_passes` and this names a different field.
                let sky_view_lut = self.sky_pass.lut(frame);
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
                    SsrEnvironment {
                        probes: probe_buffer,
                        probe_id: probe_table,
                        // The same view the forward pass's own group named, so
                        // the reflection's probe fallback is weighed by the
                        // maps the diffuse gather was weighed by — or by the
                        // placeholder, when there are none and every probe
                        // keeps all of its weight.
                        probe_visibility: probe_visibility_view,
                        // The background pass's own LUT slot, not a copy of it
                        // — see [`crate::sky_pass::SkyPass::lut`]. A frame with
                        // no atmosphere binds the zeroes that buffer was
                        // created holding, and `ssr.slang` returns before
                        // reading it.
                        sky_view: sky_view_lut,
                    },
                );
                composited
            }
            None => scene_color,
        };

        // `docs/plan/55-water.md`'s surface, **after the reflection composite
        // and before the bloom chain**. After, because the surface computes its
        // own reflection and is not a receiver of the shared march, which has
        // already run; before, because water is scene content and the chain is
        // a lens. It draws into the image the chain would have read and into the
        // scene depth, in place, so every pass below reads the same two images
        // it always did. A frame with no bodies adds nothing here and takes no
        // transient — see [`crate::water`] — and requests its two after every
        // other image this view asks for, so the pool hands each of those the
        // physical image it had before water existed.
        if let Some(mesh) = passes.water.clone() {
            let color_copy =
                graph.create_image("water-color", TransientImageDesc::scene_color(extent));
            let depth_copy =
                graph.create_image("water-depth", TransientImageDesc::hiz_level(extent));
            let (frame_block, reflection_block, sky_prefilter, froxels, sky_view) = water_reads;
            let inputs = WaterInputs {
                frame_block,
                reflection_block,
                froxels,
                froxel_ids,
                probes: probe_buffer,
                probe_id: probe_table,
                probe_visibility: passes.probe_visibility,
                sky_prefilter,
                sky_view,
                mesh,
            };
            self.water.add_passes(
                graph,
                frame,
                WaterImages {
                    color: tonemapped,
                    depth: scene_depth,
                    color_copy,
                    depth_copy,
                    shadow_atlas,
                },
                inputs,
            );
        }

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

        // Topic 43 §6's auto-exposure, and it reads
        // the image the tonemap is about to read: the frame with the medium, the
        // reflection and the chain already in it, which is the picture a viewer
        // sees and therefore the one to expose for. Binning `scene_color`
        // instead would expose the frame for a picture nobody looks at.
        //
        // Read first because the passes borrow the ring for the rest of this
        // function, and the tonemap below needs the handle out of it.
        let measured = self.auto_exposure.measured(frame);
        if effects.contains(RenderEffects::AUTO_EXPOSURE) {
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
        if let Some(debug_draw) = overlays.debug_draw {
            debug_draw.add_pass(graph, frame, tonemapped, scene_depth);
        }

        // **None of the display work under the shadow atlas viewer.** It fills
        // every display pixel with a `DontCare` load, so a tonemapped frame or a
        // grid recorded first would be stored and immediately discarded. The HDR
        // scene passes above stay: callers still read their output.
        if overlays.atlas_viewer.is_none() {
            // The tonemap group names a *graph-owned* view, so it can only be built
            // once the graph has realised one. It is cached against the view handle
            // and therefore rebuilt only on a resize.
            let TonemapPipeline {
                sampler,
                layout,
                pipeline_layout,
                pipeline: tonemap_pipeline,
            } = passes.tonemap;
            let exposure_block = self.tonemap_uniforms[frame];
            let cached = &mut self.tonemap_groups[frame];

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
                    let entries = [
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
                        &entries,
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
            if let Some(grid) = overlays.ground_grid {
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
        }

        // --- the shadow atlas viewer ---
        //
        // **After the grid and before the resolve, and it replaces what both of
        // them were about.** Sundial's atlas viewer:
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
        if let Some(atlas_viewer) = overlays.atlas_viewer {
            atlas_viewer.add_pass(graph, frame, shadow_atlas, display);
        }

        overlay(
            graph,
            ForwardOverlayTargets {
                display,
                depth: scene_depth,
                internal_extent: extent,
            },
        );

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
        // **Which tier fills the slot is decided here, once.** CMAA2 is the
        // higher one and takes it whenever its bit is on, whatever the FXAA bit
        // says — the two are never both recorded, which is what
        // [`RenderEffects::CMAA2`] means by a tier that is off being a frame with
        // fewer passes rather than a shader branch. Everything the paragraph
        // above says about the grid, the UI and the ordering holds for either.
        if display != present {
            if effects.contains(RenderEffects::CMAA2) {
                self.cmaa2.add_passes(graph, frame, display, present);
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

        tonemapped
    }

    /// Releases every resource this view owns.
    ///
    /// The caller's to call once no frame in flight still names them — the
    /// same contract [`ForwardRenderer::destroy`] has, because these are the
    /// handles that frame's passes and groups bind.
    pub(super) fn destroy(self, device: &dyn Device) {
        for (_, group) in self.tonemap_groups.into_iter().flatten() {
            device.destroy_bind_group(group);
        }
        for buffer in self.tonemap_uniforms {
            device.destroy_buffer(buffer);
        }
        self.grass.destroy(device);
        self.water.destroy(device);
        self.sky_pass.destroy(device);
        self.upscale.destroy(device);
        self.cmaa2.destroy(device);
        self.fxaa.destroy(device);
        self.bloom.destroy(device);
        self.auto_exposure.destroy(device);
        self.volumetric.destroy(device);
        self.ssr.destroy(device);
        self.hiz.destroy(device);
        self.occlusion_pyramid.destroy(device);
        self.contact_shadows.destroy(device);
        self.ssao.destroy(device);
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
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        for buffer in self.cluster_selection {
            device.destroy_buffer(buffer);
        }
        if let Some(buffer) = self.fixed_probes {
            device.destroy_buffer(buffer);
        }
        self.lights.destroy(device);
        self.draws.destroy(device);
    }
}

/// # Views
///
/// `docs/plan/29-fp-rendering.md`'s second camera: a view draws the scene this
/// renderer already holds, through a camera of its own, into a target of its
/// own — and shares every pool, page, probe and shadow map with the primary
/// camera rather than building them again.
impl ForwardRenderer {
    /// Builds a view of this renderer's scene and hands back its id.
    ///
    /// **What a view costs** is one camera's share of the frame and nothing
    /// else: its own cull and draw-argument buffers (sized by the instance
    /// capacity, like the primary camera's), its frame blocks, its light
    /// clustering and the per-frame blocks of every screen-space pass. Geometry,
    /// materials, pages, probes and the shadow atlas are the scene's, and a
    /// view reads them where they already are.
    ///
    /// A view draws nothing until a frame begins it — see
    /// [`begin_view`](Self::begin_view) and
    /// [`add_passes_with_views`](Self::add_passes_with_views).
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] when [`MAX_VIEWS`] views already exist,
    /// or for a [`ViewBackground::Transparent`] view whose effects name one of
    /// [`ViewBackground::REFUSED_ON_TRANSPARENT`] — a filter that would blend
    /// its coverage edges with the empty background, see that variant. And
    /// whatever the device refuses while building one — in which case nothing
    /// of the view is left behind.
    pub fn create_view(
        &mut self,
        device: &dyn Device,
        queue: QueueHandle,
        desc: &ViewDesc,
    ) -> Result<ViewId, HalError> {
        let refused = desc
            .effects
            .intersection(ViewBackground::REFUSED_ON_TRANSPARENT);
        if desc.background.is_transparent() && !refused.is_empty() {
            return Err(HalError::InvalidDescriptor(format!(
                "a transparent view cannot run {refused:?}: an antialiasing filter blends each \
                 edge pixel with the transparent black beside it, which a straight-alpha consumer \
                 draws as a dark fringe. Take them out of `ViewDesc::effects`, as \
                 `ViewDesc::transparent()` does"
            )));
        }
        let free = (0..MAX_VIEWS - 1)
            .find(|index| self.views.get(*index).is_none_or(Option::is_none))
            .ok_or_else(|| {
                HalError::InvalidDescriptor(format!(
                    "a forward renderer draws at most {MAX_VIEWS} views, the primary camera \
                     included, and every one is in use"
                ))
            })?;
        let id = ViewId(u8::try_from(free + 1).unwrap_or_else(|_| unreachable!("below MAX_VIEWS")));
        let probes: Vec<BufferHandle> = (0..FRAMES_IN_FLIGHT)
            .map(|frame| self.probes.buffer(frame))
            .collect();
        let probe_visibility = self.probe_visibility_placeholder.view;
        let view = View::build(
            device,
            queue,
            &ViewInputs {
                id,
                effects: desc.effects,
                background: desc.background,
                lighting: desc.lighting,
                target_format: self.target_format,
                emit: self.emit,
                instances: self.instances.buffers(),
                mesh_table: self.pool.table_buffer(),
                bucket_meshes: &self.draw_tables.bucket_meshes,
                bucket_modes: &self.bucket_modes,
                bucket_clusters: &self.draw_tables.bucket_clusters,
                mesh_levels: &self.draw_tables.mesh_levels,
                level_groups: &self.draw_tables.level_groups,
                level_meshes: &self.draw_tables.level_meshes,
                instance_capacity: self.draw_tables.instance_capacity,
                light_capacity: self.draw_tables.light_capacity,
                clusters: self.clusters.as_ref(),
                mesh_layout: self.mesh_layout,
                vertices: self.pool.vertex_buffer(),
                draw_constants: self.draw_constants,
                materials: self.materials.buffer(),
                page: self.base_color_page.view,
                normal_page: self.normal_page.view,
                mro_page: self.mro_page.view,
                emissive_page: self.emissive_page.view,
                // The sampler each slot's groups name today, so a view built
                // while a new anisotropy is still moving through the ring is
                // adopted on the same frames the primary camera's groups are.
                page_samplers: &self.slot_page_samplers,
                probes: &probes,
                specular_dfg: self.specular_dfg.view,
                ltc_table: self.ltc_table.view,
                shadow_map: self.shadow_atlas_view,
                shadow_sampler: self.shadow_sampler,
                ambient_occlusion: self.ambient_occlusion_placeholder.view,
                contact_shadow: self.contact_shadow_placeholder.view,
                probe_visibility,
            },
            &mut || {},
        )?;
        if self.views.len() <= free {
            self.views.resize_with(free + 1, || None);
        }
        self.views[free] = Some(view);
        Ok(id)
    }

    /// Releases `view` and hands its id back.
    ///
    /// **Every instance is drawn in the view the id names next**: the bit this
    /// view hid instances with is cleared from every record, so a view created
    /// later under the same id starts from [`ViewMask::ALL`] rather than from
    /// what a view it never was had hidden.
    ///
    /// Like [`destroy`](Self::destroy), the caller's to call once no frame in
    /// flight still names the view's buffers and groups.
    ///
    /// # Panics
    ///
    /// If `view` is [`ViewId::PRIMARY`], which lives as long as the renderer,
    /// or names no view this renderer built.
    pub fn destroy_view(&mut self, device: &dyn Device, view: ViewId) {
        assert!(
            view != ViewId::PRIMARY,
            "the primary camera is released with the renderer, not on its own"
        );
        let built = self
            .views
            .get_mut(view.index() - 1)
            .and_then(Option::take)
            .unwrap_or_else(|| panic!("{view:?} is not a view this renderer built"));
        built.destroy(device);
        self.instances.clear_flags(view.hidden_bit());
    }

    /// Writes `view`'s blocks for this frame, drawn through `camera` into a
    /// target of `extent`.
    ///
    /// **After [`begin_frame`](Self::begin_frame), in the same frame**, which is
    /// what rotated the instance ring, skinned what moves and fitted the shadow
    /// maps: a view writes into the slot that call chose, lit by the sun it was
    /// handed and shadowed through the maps the primary camera's frame draws —
    /// unless its [`ViewDesc::lighting`] fixes a light of its own. A view begun and then not named in
    /// [`add_passes_with_views`](Self::add_passes_with_views) records nothing.
    ///
    /// # Errors
    ///
    /// [`HalError`] if one of the view's blocks could not be written.
    ///
    /// # Panics
    ///
    /// If `view` is [`ViewId::PRIMARY`] — its blocks are `begin_frame`'s — or
    /// names no view this renderer built, or if no frame has begun.
    pub fn begin_view(
        &mut self,
        device: &dyn Device,
        view: ViewId,
        camera: &Camera,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        assert!(
            view != ViewId::PRIMARY,
            "the primary camera's blocks are written by `begin_frame`"
        );
        let background = self
            .views
            .get(view.index() - 1)
            .and_then(Option::as_ref)
            .unwrap_or_else(|| panic!("{view:?} is not a view this renderer built"))
            .background;
        let scene = self
            .frame_scene
            .take()
            .expect("`begin_view` follows the `begin_frame` that opened the frame");
        let mut frame = self.view_frame(&scene, extent, self.instances.slot_count(), camera.eye);
        frame.extent = self.view_extent(background, extent);
        let written = self
            .views
            .get_mut(view.index() - 1)
            .and_then(Option::as_mut)
            .unwrap_or_else(|| panic!("{view:?} is not a view this renderer built"))
            .begin_frame(device, camera, &frame, self.sky_view.as_ref());
        self.frame_scene = Some(scene);
        written.map(|_| ())
    }

    /// Sets the views `handle` is drawn in. Every instance starts in
    /// [`ViewMask::ALL`].
    ///
    /// **A camera's cull and nothing else**: a view the instance is hidden from
    /// rejects it before testing a bound, and every shadow map still draws it —
    /// so a weapon hidden from a scope's view goes on casting the shadow the
    /// primary camera sees.
    ///
    /// Not a move: the record's transform history is left as it is, so a
    /// visibility change does not report motion. A stale handle is ignored, on
    /// [`set_instance`](Self::set_instance)'s terms.
    pub fn set_instance_views(&mut self, handle: InstanceHandle, views: ViewMask) {
        if let Some(record) = self.instances.get(handle) {
            let flags =
                (record.flags & !mesh::GpuInstance::HIDDEN_VIEWS_MASK) | views.hidden_bits();
            self.instances.set_flags(handle, flags);
        }
    }

    /// The views `handle` is drawn in, or `None` for a stale handle.
    #[must_use]
    pub fn instance_views(&self, handle: InstanceHandle) -> Option<ViewMask> {
        self.instances
            .get(handle)
            .map(|record| ViewMask::from_flags(record.flags))
    }

    /// `view`'s draw generator — [`draws`](Self::draws) for a view other than
    /// the primary camera.
    ///
    /// # Panics
    ///
    /// If `view` names no view this renderer built.
    #[must_use]
    pub fn view_draws(&self, view: ViewId) -> &DrawGen {
        if view == ViewId::PRIMARY {
            return &self.primary.draws;
        }
        &self
            .views
            .get(view.index() - 1)
            .and_then(Option::as_ref)
            .unwrap_or_else(|| panic!("{view:?} is not a view this renderer built"))
            .draws
    }

    /// The target's aspect and the internal extent a frame into `target` is
    /// drawn at.
    ///
    /// **The aspect is the target's and the extent is the internal render
    /// extent's.** What a viewer sees is the target: the upscale maps the whole
    /// internal image onto the whole of it, so a frame composed for the internal
    /// extent's own aspect would be composed for a rectangle nobody looks at.
    /// What the rounding of the two extents leaves is a sub-pixel
    /// non-squareness in the internal target, which the upscale undoes on the
    /// way out.
    ///
    /// A minimised window reports a zero extent in *either* dimension, and
    /// `Projection::matrix` asserts a finite positive aspect. Guarding only the
    /// height left `extent.0 == 0` producing `0.0`, which trips that assert and
    /// takes the frame loop down with it.
    ///
    /// The extent is the one a frame is *drawn* at, which is the caller's at a
    /// render scale of `1.0` and smaller below it. Every use of it sizes
    /// something — the cluster grid, the LOD metric's pixel budget, the Hi-Z
    /// pyramid's height, the bloom chain, the resolve's texel size — and every
    /// one of those wants the extent the pixels are actually at. See
    /// [`Self::set_render_scale`].
    pub(super) fn frame_extents(&self, target: (u32, u32)) -> (f32, (u32, u32)) {
        let aspect = if target.0 == 0 || target.1 == 0 {
            1.0
        } else {
            target.0 as f32 / target.1 as f32
        };
        (aspect, self.internal_extent(target))
    }

    /// The internal extent a secondary view with `background` is drawn at, for
    /// a target of `target`: [`frame_extents`](Self::frame_extents)' answer,
    /// except that a [`ViewBackground::Transparent`] view is always drawn at
    /// its target's own extent — the upscale would filter its coverage edges,
    /// see that variant.
    pub(super) fn view_extent(&self, background: ViewBackground, target: (u32, u32)) -> (u32, u32) {
        if background.is_transparent() {
            target
        } else {
            self.frame_extents(target).1
        }
    }

    /// What every view's [`View::begin_frame`] reads of this frame, for a view
    /// drawn into a target of `target_extent`.
    pub(super) fn view_frame<'s>(
        &self,
        scene: &'s FrameScene,
        target_extent: (u32, u32),
        instance_count: u32,
        selection_eye: Vec3,
    ) -> ViewFrame<'s> {
        let (aspect, extent) = self.frame_extents(target_extent);
        // The frame's gradient, resolved once and handed to all three of the
        // things that want a sky: the L1 projection below for the ambient term,
        // the march's block for a reflection that hit nothing, and the pass that
        // draws the background. One `SkyGradient` rather than three readings of
        // the field, so the sky a surface is lit by cannot differ from the one
        // behind it.
        //
        // An atmosphere replaces the gradient outright — see `set_atmosphere`.
        // `gradient_fit` is what the reflection pass and the background's own
        // gradient rows take, `irradiance` is the ambient term, and the LUT
        // itself is what the background samples. The first two are taken when
        // a march finishes rather than here — see `PresentedSky` — so a frame
        // whose sun has not moved reads them.
        let sky_view = self.sky_view.as_ref();
        let gradient = match sky_view {
            Some(presented) => presented.gradient,
            None => self.sky.gradient(),
        };
        // Projected on the host — once per gradient, and for an atmosphere once
        // per completed march rather than once per frame — which is also why
        // the shading rule that governs `mesh.slang` has nothing to say about
        // it: these coefficients reach every backend as uploaded numbers.
        let sky_irradiance = match sky_view {
            Some(presented) => presented.irradiance,
            None => gradient.irradiance(),
        };
        ViewFrame {
            slot: self.frame,
            serial: self.frame_serial,
            aspect,
            extent,
            effects: self.frame_effects,
            instance_count,
            selection_eye,
            lod_error_budget: self.lod_error_budget,
            lod_hold_ratio: self.lod_hold_ratio,
            scene,
            // The field as it stands, and the weather over it. Read here rather
            // than in `View::begin_frame` because that method takes `&mut self`
            // on the view and the field is the *renderer's*.
            grass: self.grass.frame(),
            wind: self.wind,
            fog: self.fog,
            probe_volume: self.probe_volume,
            attribute_base: self.pool.attribute_base(),
            gradient,
            sky_irradiance,
            debug_view_lane: self.debug_view_lane(),
            exposure: self.exposure,
            exposure_adaptation: self.exposure_adaptation,
            tonemap_curve: self.resolved_tonemap_curve(),
            occlusion: self.resolved_occlusion_culling(),
        }
    }
}

// Uses `forward::tests`' fixtures, which are native-only with them.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
