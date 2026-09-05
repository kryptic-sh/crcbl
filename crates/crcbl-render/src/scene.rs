//! What is resident in a frame, as host-side data an application writes and
//! [`ForwardRenderer::with_scene`] consumes.
//!
//! ```text
//! SceneDesc ──▶ ForwardRenderer::with_scene ──▶ pools, table, page, buckets
//!  meshes                                        (fixed for the renderer's life)
//!  materials
//!  page
//!  capacities
//! ```
//!
//! # Why a description rather than calls
//!
//! The resident set is fixed at build and the *instances* of it are not, and
//! that split is where the seam already was: [`MeshPool`](crate::mesh_pool),
//! [`ClusterPool`](crate::cluster_pool), the bucket table and the page are
//! created once and never grow, while
//! [`InstancePool::insert`](crate::instance_pool::InstancePool::insert) is a
//! per-frame path. A runtime `add_mesh` would mean recreating the camera's
//! [`DrawGen`](crate::draw_gen::DrawGen) and every shadow one, plus every bind
//! group naming their buffers, part way through a renderer's life — which is
//! the streaming path `crate::mesh_pool`'s own docs assign to P9.
//!
//! So everything here is data, and none of it names a device: a description can
//! be built, compared and unit-tested with no GPU in the room, and the renderer
//! is what turns it into memory.
//!
//! # Order is load-bearing, in four places
//!
//! Nothing below is a list whose order is cosmetic, and three of the four fail
//! silently — a plausible frame with the wrong pixels in it, which no assertion
//! on the CPU can see.
//!
//! * **Material row 0 is what [`mesh::GpuInstance::default`] names**, so an
//!   instance written without a material id shades through
//!   [`SceneDesc::materials`]'s first entry. Put a tint there and every object
//!   nobody assigned a material to is tinted.
//! * **Mesh table ids come from upload order**, and `cull.slang` reads a
//!   bounding box out of the entry the *instance* names — level 0's, for a DAG.
//!   The renderer uploads [`SceneDesc::meshes`] in order, so a description's
//!   own order is what the ids are.
//! * **A page layer's index is its position in [`PageDesc`]**, counted per
//!   [`PageKind`], and a material row's texture column is that number. A
//!   producer that pushes its layers in one order and names them in another
//!   shades every surface with somebody else's texture — a plausible frame,
//!   built from the right images.
//! * **One bucket per description mesh, in order**, because `draw_gen.slang`'s
//!   scatter takes the first bucket whose mesh id matches. Two buckets naming
//!   one mesh would leave the second drawing nothing forever, which the bucket
//!   table being derived from this list is what prevents.
//!
//! # Instances are the other half, and they are calls rather than data
//!
//! What is resident is fixed; *where the objects are* is not. So an
//! [`InstanceDesc`] is handed to [`ForwardRenderer::add_instance`] at any point
//! in a renderer's life, and it names its mesh and its material by **index into
//! the description above** rather than by a table id only the renderer knows.
//!
//! # The engine's own scene is a caller of this
//!
//! [`demo`] is the cube, the pyramid, the open box and the dunes DAG — what the
//! renderer used to upload to itself — with the three material rows and the two
//! page layers that make §3.2's columns observable.
//! [`ForwardRenderer::new`] is `with_scene(&demo())` and nothing else, so the
//! samples and the golden suite exercise the same path an application takes
//! rather than a special case beside it.
//!
//! [`ForwardRenderer::add_instance`]: crate::forward::ForwardRenderer::add_instance
//! [`ForwardRenderer::new`]: crate::forward::ForwardRenderer::new
//! [`ForwardRenderer::with_scene`]: crate::forward::ForwardRenderer::with_scene

use std::borrow::Cow;

use crcbl_hal::HalError;
use crcbl_shaders::cluster_dag::ClusterDag;
use crcbl_shaders::mesh;
use crcbl_shaders::meshlet::MeshClusters;
use crcbl_shaders::probe;
use crcbl_shaders::vertex::UvRange;
use glam::Mat4;

/// The second material's base colour, and the whole of what makes it visible.
///
/// A factor per channel with no two alike, so multiplying by it moves every
/// colour it touches: a tint that left a channel at `1.0` would leave the
/// pyramid's white base looking like a lighting difference, and one that left
/// them equal would be a brightness change a shading bug could also produce.
/// Carried by [`DEMO_TINTED`], which is the row a second instance of the pyramid
/// mesh is placed through — the pair this exists for.
pub const PYRAMID_TINT: [f32; 4] = [0.15, 0.45, 1.0, 1.0];

/// The second material's roughness, and the whole of what makes the **shading**
/// half of the row visible.
///
/// Well under [`mesh::GpuMaterial::UNTINTED`]'s, so the same lobe under the same
/// sun draws a tight bright highlight here where the neutral row draws a broad
/// faint one. Not lower still: a roughness near zero puts the whole lobe inside
/// a handful of pixels, and a highlight a rasteriser can miss is not something
/// a frame comparison can measure.
///
/// **This is the only place in the engine where two materials differ in a
/// shading factor**, which is what makes
/// `crcbl`'s `the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one`
/// possible at all — see [`demo`] for why the shading factor rides on that row
/// rather than on a fourth.
pub const PYRAMID_ROUGHNESS: f32 = 0.25;

/// The base-colour page's extent, in texels — square, and **two**.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's
/// [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) page is one image with a
/// layer per material texture, and two texels a side is the smallest extent in
/// which a layer can be something other than a flat colour. Small on purpose,
/// and not only because this is demo content:
///
/// * A flat layer would make the golden suite pass with **no UV at all**. Four
///   texels is what makes the texture coordinate observable, because a mesh
///   whose UVs never varied would shade each face in one texel's colour.
/// * Every texel boundary is a ramp the sampler blends across — it magnifies
///   bilinear — and a ramp is where two rasterisers' UV interpolation shows up
///   as a value difference. Four texels put **one** boundary across a face in
///   each axis, at `0.5`, which is as far from a vertex as an edge can be. A
///   denser checker would put a row of disagreeable pixels through every face
///   for no more evidence.
pub const PAGE_EXTENT: u32 = 2;

/// Bytes in one layer of [`demo`]'s page: [`PAGE_EXTENT`]² RGBA texels.
const PAGE_LAYER_BYTES: usize = (PAGE_EXTENT * PAGE_EXTENT) as usize * 4;

/// The layer [`DEMO_TEXTURED`] shades with — the only one [`demo`]'s
/// base-colour page carries.
pub const CHECKER_LAYER: u32 = 0;

/// [`CHECKER_LAYER`]: four **distinct** greys, one per texel.
///
/// Distinct rather than a two-value checker, for the reason `crcbl-vk`'s sprite
/// suite records about its sheets: a two-value checker is symmetric under both a
/// flipped U and a flipped V, so either mistake would produce the same picture.
/// No two of these are equal, so any flip is a different frame.
///
/// Grey rather than coloured because the colour axis is already spoken for:
/// [`PYRAMID_TINT`] is what proves the *factor* column, and a texture that also
/// changed hue would make the two columns' evidence look alike.
pub const CHECKER_TEXELS: [u8; PAGE_LAYER_BYTES] = [
    0xFF, 0xFF, 0xFF, 0xFF, // (0, 0)
    0xB0, 0xB0, 0xB0, 0xFF, // (1, 0)
    0x70, 0x70, 0x70, 0xFF, // (0, 1)
    0x30, 0x30, 0x30, 0xFF, // (1, 1)
];

/// How much of each pool a scene reserves, in objects.
///
/// Every one of these is device-local memory taken at start-up and **never
/// grown**, which is the decision `crate::mesh_pool` argues in full: every bind
/// group in the renderer names those buffers, so growing one would mean
/// rewriting every descriptor mid-life. A caller sizes them for the scene it
/// means to build; a scene that outgrows one is refused rather than resized.
///
/// [`Default`] is what the engine's own [`demo`] scene has always used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacities {
    /// Vertices the geometry pool holds, across every resident mesh.
    pub vertices: u32,
    /// Indices the geometry pool holds. Four per vertex is the usual ratio for
    /// indexed triangle soup, rounded up.
    pub indices: u32,
    /// Entries the mesh table holds.
    ///
    /// Distinct meshes, not instances of them: an object count sizes
    /// [`instances`](Self::instances), and a mesh id names one entry here
    /// however many instances carry it. **A DAG occupies one entry per level**,
    /// because each level is a vertex range of its own.
    pub meshes: u32,
    /// Instances the instance pool holds, per frame in flight.
    ///
    /// Sized against topic 03's exit criterion — "sandbox scene: 10k+ instanced
    /// meshes" — rather than against what is resident today.
    ///
    /// **Raising this is not linear in memory.** `docs/plan/25-lod.md`'s
    /// hysteresis state is one word per instance per resident group per
    /// [`DrawGen`](crate::draw_gen::DrawGen), and there is a generator for the
    /// camera, one per shadow cascade and one per shadow light slot.
    ///
    /// The one capacity here that a description cannot be measured against,
    /// because objects are placed while the renderer runs rather than at build:
    /// filling it is
    /// [`InstancePoolError::PoolFull`](crate::instance_pool::InstancePoolError::PoolFull)
    /// from [`ForwardRenderer::add_instance`], which is where a caller finds out
    /// it is this number that wants raising.
    ///
    /// [`ForwardRenderer::add_instance`]: crate::forward::ForwardRenderer::add_instance
    pub instances: u32,
    /// Rows the material table holds.
    ///
    /// Distinct materials, not instances of them — a material id names one row
    /// however many instances carry it, which is the property that makes the
    /// table worth having.
    pub materials: u32,
    /// Rows the light list holds — the sun and every
    /// [`Light`](crate::light::Light) a caller sets.
    ///
    /// Overflowing this is refused rather than counted, because a light missing
    /// from the list is missing from every froxel and no counter in the frame
    /// would say so.
    pub lights: u32,
    /// Rows the irradiance probe table holds.
    ///
    /// One per probe of [`SceneDesc::probes`], which is a
    /// description-measurable number like [`materials`](Self::materials) and is
    /// refused at build rather than at runtime.
    ///
    /// **Zero is a scene with no probes**, which is what the engine's own
    /// [`demo`] has: the table still holds one cleared row, because a buffer of
    /// no bytes is not a buffer, and reading that row adds exactly nothing. The
    /// default is deliberately not larger — a probe grid is authored, and
    /// reserving device memory for one nobody wrote is memory taken from every
    /// existing caller for a feature they have not used.
    pub probes: u32,
}

impl Default for Capacities {
    fn default() -> Self {
        Self {
            vertices: 64 * 1024,
            indices: 256 * 1024,
            meshes: 1024,
            instances: 16 * 1024,
            materials: 1024,
            lights: 1024,
            probes: 0,
        }
    }
}

/// Which of the renderer's material pages a layer belongs to.
///
/// The renderer creates one `D2Array` image per kind, and a material row indexes
/// them separately: [`base_color_texture`] names a layer of
/// [`BaseColor`](Self::BaseColor)'s image and [`normal_texture`] names one of
/// [`Normal`](Self::Normal)'s. They are different formats, filtered by different
/// filters, and sized by their own extents — so a layer number means nothing
/// without the kind beside it, and this enum is what makes a caller write the
/// kind down.
///
/// `docs/plan/43-render-standards.md` §2's rung 3 appends the
/// metallic-roughness and emissive pages here; every arm below is what it has to
/// fill in.
///
/// [`base_color_texture`]: mesh::GpuMaterial::base_color_texture
/// [`normal_texture`]: mesh::GpuMaterial::normal_texture
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageKind {
    /// The base-colour page: sRGB-encoded colour, which is what glTF defines a
    /// `baseColorTexture` to be, created `Rgba8UnormSrgb` so the sampler decodes
    /// it and mipped through the alpha-weighted filter that averages in linear
    /// light.
    BaseColor,
    /// The normal page: tangent-space normals stored **linear**, as
    /// `n * 0.5 + 0.5` per channel, created `Rgba8Unorm` so nothing puts them
    /// through a transfer curve and mipped through the filter that averages the
    /// decoded vectors and renormalises.
    Normal,
}

impl PageKind {
    /// Every kind, in the order a [`PageDesc`] stores them.
    pub const ALL: [Self; 2] = [Self::BaseColor, Self::Normal];

    /// This kind's slot in [`PageDesc`]'s table.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::BaseColor => 0,
            Self::Normal => 1,
        }
    }

    /// How a refusal names this page — an adjective, read before the word
    /// "page", so `check_scene`'s sentence reads the same for every kind.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BaseColor => "base-colour",
            Self::Normal => "normal",
        }
    }
}

/// One page of [`PageDesc`]'s table: a square extent and the layers written at
/// it.
///
/// Both zero is "no scene here names this page", which is what
/// [`PageDesc::empty`] leaves every kind at and what makes the renderer create a
/// placeholder texel rather than an image. `layers` is never non-empty at an
/// extent of zero — [`PageDesc::push_layer`] refuses that — and the reverse,
/// an extent set and nothing pushed, is a page nothing samples and is treated as
/// empty at upload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Page<'a> {
    extent: u32,
    layers: Vec<Cow<'a, [u8]>>,
}

/// §3.2's texture side: one `D2Array` image per [`PageKind`], whose layers a
/// material row selects with that kind's own texture column.
///
/// One image with a layer per texture rather than an array of descriptors —
/// [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) rather than
/// [`Bindless`](crcbl_hal::BindingModel::Bindless) — because a layer index
/// needs nothing of a device where a descriptor array needs
/// [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING), which
/// `crcbl-mtl` withdraws.
///
/// # A table over the kinds, each with an extent of its own
///
/// [`set_extent`](Self::set_extent) sizes one kind and
/// [`push_layer`](Self::push_layer) appends into it, so a 512² normal page
/// beside a 2048² base-colour page is two calls rather than a resample of one
/// onto the other's size. A kind nothing names is left alone: it holds no
/// layers, [`extent`](Self::extent) reports zero for it, and the renderer
/// creates a single placeholder texel in its place rather than an `extent²`
/// image nobody samples.
///
/// # Layer 0 is an ordinary layer, and where its old invariant went
///
/// Until `docs/plan/43-render-standards.md` §2's row (d) this type **burned**
/// layer 0 on both pages — an all-white texel on the base-colour page, the
/// neutral normal on the other — because
/// [`NO_PAGE`](mesh::GpuMaterial::NO_PAGE) was zero and a material naming no
/// texture sampled it. Two arms of `check` existed to hold that:
/// layer 0 is white, layer 0 is neutral.
///
/// `NO_PAGE` is `0xFFFF` now and `mesh.slang` tests it, so the neutral value is
/// a literal in the fragment stage rather than a texel in an image, and both
/// arms are gone with the layers they guarded. **That is a trade, and it is
/// worth writing down which way it runs.** What it bought: a page nothing names
/// costs one texel rather than `extent² × 4` bytes and a third again for its
/// chain, on every scene in the tree; a producer numbers its layers from zero
/// like any other array; and the two pages stopped having to be one size. What
/// it sold: the invariant is no longer checkable on the host at all. A fragment
/// that read a page for a row carrying `NO_PAGE` used to find white and draw the
/// right frame anyway, and now it finds whatever the placeholder holds — which
/// nothing but a *picture* can report. So `crate::forward`'s
/// `PAGE_PLACEHOLDER_TEXEL` is magenta rather than white or black: the mistake
/// this type used to absorb silently is one a golden now shows loudly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageDesc<'a> {
    pages: [Page<'a>; PageKind::ALL.len()],
}

impl<'a> PageDesc<'a> {
    /// The texel an all-white base-colour layer is filled with.
    ///
    /// The page is `Rgba8UnormSrgb` and `0xFF` is the one value that encoding
    /// leaves alone, so the sampler returns exactly `1.0` — the same product a
    /// material was shaded by before there was a page at all.
    ///
    /// **Nothing needs such a layer any more.** A material naming no texture
    /// carries [`mesh::GpuMaterial::NO_PAGE`], which is out of band, and the
    /// fragment stage multiplies by the literal `1.0` instead of by this. The
    /// constant stays for the callers that want a white layer *deliberately* —
    /// `crcbl/tests/mesh_e2e/base_color_page.rs` is the one that measured, on a
    /// device, that the two routes shade a surface to the same bits, and
    /// `crcbl/src/screenshot.rs`'s alpha mask keeps this RGB under a cut alpha.
    pub const WHITE: u8 = 0xFF;

    /// The neutral tangent-space normal texel: `(0.5, 0.5, 1.0)` in RGBA8,
    /// which is `docs/plan/43-render-standards.md` §2's neutral normal.
    ///
    /// # It is not exactly flat, and the shader is what makes "no map" exact
    ///
    /// An eight-bit unorm channel has no `0.5`: `0x80` decodes to `128 / 255`,
    /// and `t * 2 - 1` turns that into `1 / 255` rather than zero — a tangent
    /// space normal about a fifth of a degree off vertical. That is small and it
    /// is not nothing, and this engine compares its goldens across four
    /// backends with no tolerance, so a material sampling this texel and
    /// perturbing by it would move every frame in the tree that draws a lit
    /// surface. `mesh.slang`'s `shading_normal_of` therefore tests the layer
    /// index against [`mesh::GpuMaterial::NO_PAGE`] and returns the surface
    /// normal untouched;
    /// `crcbl_shaders::mesh::a_neutral_normal_texel_is_not_exactly_flat`
    /// measures the error this constant would otherwise introduce.
    ///
    /// So what a layer of this is *for* is the material that names it
    /// explicitly and the read that runs off the end of an authored layer's
    /// edge — a neutral texel is what those must find, rather than the
    /// `(0, 0, 0)` an unwritten image holds, which decodes to a normal pointing
    /// straight backwards.
    pub const NEUTRAL_NORMAL: [u8; 4] = [0x80, 0x80, 0xFF, 0xFF];

    /// A description that names no page at all: every kind empty, and nothing
    /// but one placeholder texel apiece allocated on the device.
    ///
    /// A caller adds a page by sizing it with
    /// [`set_extent`](Self::set_extent) and pushing layers into it with
    /// [`push_layer`](Self::push_layer). A scene whose every material row
    /// carries [`NO_PAGE`](mesh::GpuMaterial::NO_PAGE) — `apps/alcove` and
    /// `apps/sundial` are both such scenes — stops here.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Sizes `kind`'s page: texels a side, square.
    ///
    /// Every layer of that kind is then exactly `extent² × 4` bytes, which
    /// `check` is what enforces. The kinds are separate device
    /// images and each carries its own number, so sizing one says nothing about
    /// the other.
    ///
    /// # Panics
    ///
    /// If `extent` is zero, which is an image no device will create, or if
    /// `kind` already holds a layer — those layers were authored against the
    /// extent that was in force when they were pushed, and moving it under them
    /// would refuse every one of them at `check` with a byte
    /// count nobody wrote.
    pub fn set_extent(&mut self, kind: PageKind, extent: u32) {
        assert!(extent > 0, "a page must have at least one texel a side");
        let page = &mut self.pages[kind.index()];
        assert!(
            page.layers.is_empty(),
            "the {} page already holds {} layer(s) authored at {} texels a side",
            kind.label(),
            page.layers.len(),
            page.extent
        );
        page.extent = extent;
    }

    /// Appends a layer to `kind` and returns its index — the number the
    /// material row's own texture column carries.
    ///
    /// `texels` is RGBA8, row-major, and exactly `extent² × 4` bytes, which
    /// [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// checks before it uploads anything. What the bytes *mean* is the kind's,
    /// and the two differ: a [`BaseColor`](PageKind::BaseColor) layer is
    /// sRGB-encoded, because that is what glTF defines a base-colour texture to
    /// be and the renderer creates that image as `Rgba8UnormSrgb` so the sampler
    /// decodes it; a [`Normal`](PageKind::Normal) layer is **linear**, a
    /// tangent-space normal encoded as `n * 0.5 + 0.5` per channel, and its
    /// image is `Rgba8Unorm` so nothing decodes it at all — see
    /// `crcbl_render::forward::NORMAL_PAGE_FORMAT` and
    /// `docs/plan/44-lighting.md`'s rung 2.
    ///
    /// # Panics
    ///
    /// If `kind` has no extent yet: a layer's length is only meaningful against
    /// one, so [`set_extent`](Self::set_extent) comes first.
    pub fn push_layer(&mut self, kind: PageKind, texels: impl Into<Cow<'a, [u8]>>) -> u32 {
        let page = &mut self.pages[kind.index()];
        assert!(
            page.extent > 0,
            "the {} page has no extent, so this layer's length would be measured against \
             nothing: set_extent comes first",
            kind.label()
        );
        let layer = u32::try_from(page.layers.len())
            .unwrap_or_else(|_| unreachable!("a page of more layers than a u32 can name"));
        page.layers.push(texels.into());
        layer
    }

    /// `kind`'s texels a side, or **zero** when nothing has named that page.
    #[must_use]
    pub const fn extent(&self, kind: PageKind) -> u32 {
        self.pages[kind.index()].extent
    }

    /// Every layer of `kind`, in order: element `n` is layer `n`.
    #[must_use]
    pub fn layers(&self, kind: PageKind) -> &[Cow<'a, [u8]>] {
        &self.pages[kind.index()].layers
    }

    /// Whether this description can be uploaded as written, checked by
    /// [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// before it creates anything.
    ///
    /// Two things, and they fail for different reasons.
    /// [`push_layer`](Self::push_layer) takes bytes it cannot measure against an
    /// extent it does not see, so a layer of the wrong length is an ordinary
    /// caller mistake. Layers at *no* extent, on the other hand, are unspellable
    /// through this type's own methods — `set_extent` refuses zero and
    /// `push_layer` refuses a page without one — so that arm is what would catch
    /// a *second* constructor that let one through, which is why it is checked
    /// here rather than asserted at construction.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] naming the kind, the layer and what is
    /// wrong with it.
    pub(crate) fn check(&self) -> Result<(), HalError> {
        for kind in PageKind::ALL {
            let page = &self.pages[kind.index()];
            if page.layers.is_empty() {
                continue;
            }
            if page.extent == 0 {
                return Err(HalError::InvalidDescriptor(format!(
                    "the {} page carries {} layer(s) at no extent, so there is no image for \
                     them to go into",
                    kind.label(),
                    page.layers.len()
                )));
            }
            let texels = page.extent as usize * page.extent as usize * 4;
            for (layer, bytes) in page.layers.iter().enumerate() {
                if bytes.len() != texels {
                    return Err(HalError::InvalidDescriptor(format!(
                        "{} page layer {layer} carries {} bytes, and a {}×{} RGBA8 layer is \
                         {texels}",
                        kind.label(),
                        bytes.len(),
                        page.extent,
                        page.extent
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Who writes an irradiance volume's rows: the description that authored them,
/// or `docs/plan/50-irradiance-probes.md`'s every-frame updater.
///
/// # Why this is a field of the volume and not an effect bit or a console
/// variable
///
/// **Not a [`RenderEffects`](crate::effects::RenderEffects) bit**: those are a
/// frame-wide toggle with a settings key each — `every_effect_has_a_key_and_no_two_share_one`
/// is what holds that pairing — and a new key shifts the options demo's fader
/// indices in `web/tools/browser-e2e.mjs`. What decides whether a *volume* is
/// updated is a property of that volume, not of the frame.
///
/// **Not a console variable**: a convar is process-global, and the pass-list
/// tests run every scene of a fixture in one process. A scene's answer would
/// then depend on what ran before it.
///
/// So it is a field, and [`Authored`](Self::Authored) is [`Default`]: a scene
/// that says nothing records neither of the updater's two passes and pays
/// nothing for them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProbeUpdate {
    /// The rows are the ones [`ProbeGrid::probes`] carries, uploaded once and
    /// never written again.
    ///
    /// Every scene that existed before the updater, and every scene whose
    /// lighting a golden pins: `crates/crcbl/tests/golden/probes.png` is the
    /// anti-vacuity floor for the whole probe read path and stands on rows a
    /// Rust mirror can predict.
    #[default]
    Authored,
    /// The rows are overwritten every frame by the reflective-shadow-map
    /// gather — `crcbl_render::rsm` draws the map and
    /// `crcbl_render::probe_gather` reads it into the table.
    ///
    /// [`ProbeGrid::probes`] is then the volume's *size* rather than its
    /// contents: the rows still have to be there, because the table is sized
    /// from them and `check` holds the two to each other, but what a frame reads
    /// is what the gather wrote.
    ///
    /// **It needs the sun's near cascade.** The map is the cascade's own draws
    /// through the cascade's own matrix, so a frame drawing without
    /// [`RenderEffects::SHADOWS`](crate::effects::RenderEffects::SHADOWS) — or
    /// one whose shadow cadence held cascade 0 — records neither pass and leaves
    /// the rows as the last gather wrote them. The renderer keeps cascade 0 off
    /// the static-cache hold for exactly this reason, so the ordinary frame does
    /// run it; a caller that has raised `r_shadow_cadence` past one has asked
    /// for the cascade to be redrawn less often and gets a bounce that follows
    /// it.
    ///
    /// **Probes outside the near cascade gather nothing.** The map covers
    /// cascade 0's camera-following sphere, so a volume larger than that sphere
    /// has rows the updater never reaches.
    /// [`ForwardRenderer::follow_probe_volume`](crate::forward::ForwardRenderer::follow_probe_volume)
    /// is what closes that for a volume small enough to follow the camera: it
    /// re-centres each level on a tracked point by whole probe steps and moves
    /// the position table the gather reads with it. A volume authored to cover
    /// its whole scene has nowhere to step to and keeps the rows the cascade
    /// cannot reach.
    EveryFrame,
}

/// `docs/plan/18-render-features.md`'s irradiance volume: where the probes are,
/// and what each of them holds.
///
/// The diffuse half of that topic's global-illumination row. `mesh.slang`
/// interpolates the grid trilinearly and **adds** the result to the flat ambient
/// term, so a description with no probes draws the frame it always did — see
/// [`crcbl_shaders::probe`], which is where the spherical harmonics live and
/// where they are checked against the literature.
///
/// # The irradiance is authored, not baked
///
/// [`GpuProbe::accumulate`](probe::GpuProbe::accumulate) is how an application
/// turns an environment into a row, and it is the only correct way to fill one.
/// There is no bake tool, on a hard prerequisite rather than on taste: a gather
/// bake casts rays at scene triangles, and this engine has no ray-triangle
/// intersector and no BVH — `crcbl_phys`'s `query` module has ray-vs-sphere,
/// ray-vs-AABB and ray-vs-capsule and nothing else.
///
/// # [`Default`] is the volume that changes no pixel
///
/// No probes and a grid of no extent, which is what every existing caller gets:
/// the shader's fetch clamps onto the table's cleared first row and adds
/// exactly zero. There is no branch anywhere selecting it, and no
/// [`RenderEffects`](crate::effects::RenderEffects) bit — the off-switch is the
/// scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeGrid {
    /// Where the grid is, how far apart its probes are, and how many there are
    /// on each axis.
    pub volume: probe::ProbeVolume,
    /// What fills [`probes`](Self::probes) once the scene is running.
    ///
    /// [`ProbeUpdate::Authored`] by default, which is the frame every scene drew
    /// before the updater landed.
    pub update: ProbeUpdate,
    /// One row per probe, the clipmap's levels one after another finest first
    /// and **`x`-fastest** within each, each level's cells wrapped by its own
    /// scroll offset:
    /// [`ProbeVolume::row`](probe::ProbeVolume::row) is the index, and it is
    /// `level_row(level) + (z · counts.y + y) · counts.x + x` for the volume as
    /// authored, before anything has scrolled.
    ///
    /// Its length must be [`volume`](Self::volume)'s
    /// [`total`](probe::ProbeVolume::total) — one level's worth *per level* —
    /// which
    /// [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// checks before it creates anything, through this type's own `check`.
    pub probes: Vec<probe::GpuProbe>,
}

impl ProbeGrid {
    /// Whether this grid can be uploaded as written, checked by
    /// [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// before it creates anything.
    ///
    /// The one thing that has to hold: the volume's counts, multiplied out and
    /// taken once per level, come to exactly as many rows as there are probes.
    /// **That is what bounds the shader's fetch**, and it is not a tidiness —
    /// `mesh.slang` addresses a cell through the counts and a level through the
    /// rows one level holds, so a grid claiming more probes than it carries
    /// would read rows the description never wrote, and one claiming fewer would
    /// leave part of the volume unlit while the table held the light for it.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] naming both numbers.
    pub(crate) fn check(&self) -> Result<(), HalError> {
        let total = self.volume.total() as usize;
        if total == self.probes.len() {
            return Ok(());
        }
        Err(HalError::InvalidDescriptor(format!(
            "the probe grid is {}×{}×{} over {} level(s), which is {total} \
             probe(s), and the description carries {}",
            self.volume.counts[0],
            self.volume.counts[1],
            self.volume.counts[2],
            self.volume.level_count(),
            self.probes.len()
        )))
    }
}

/// One resident mesh's geometry, as the renderer needs it: triangles for a
/// vertex stage, clusters for a mesh stage, and nothing else.
///
/// The clusters are supplied rather than built, because building them is
/// `crcbl-scene`'s `build_meshlets` and the renderer must not depend on that
/// crate — §3.5 makes the meshlet build a **bake step** for exactly that reason.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry<'a> {
    /// A mesh with no level hierarchy: one vertex range, one index range, one
    /// run of clusters, drawn from every camera.
    Flat {
        /// Vertices as [`mesh::MeshVertex`] bytes — [`mesh::VERTEX_STRIDE`] per
        /// vertex, little-endian, exactly what
        /// [`cube_vertex_bytes`](mesh::cube_vertex_bytes) produces.
        ///
        /// **Interleaved, which is not how the pool stores them.** A record is
        /// a position and then its attributes; [`crate::MeshPool::upload`]
        /// separates the two streams, so a caller describes geometry once and
        /// the position region reaches the device contiguous.
        vertices: Cow<'a, [u8]>,
        /// The scale and offset every UV lane in `vertices` was quantised
        /// against — [`UvRange::from_uvs`] over the mesh's float coordinates,
        /// before any vertex was built.
        ///
        /// It has to travel with the bytes: a `unorm16` lane means nothing
        /// without it, and the pool cannot recover it from lanes that are
        /// already quantised. It reaches the shaders through
        /// [`mesh::GpuMesh::uv_range`].
        uv_range: UvRange,
        /// Triangle indices into `vertices`, **mesh-relative**: the pool's base
        /// vertex is added by the shader through the mesh table.
        indices: Cow<'a, [u32]>,
        /// The same triangles partitioned for a mesh stage. Read on
        /// [`GeometryPath::MeshShader`](crcbl_hal::GeometryPath::MeshShader) and
        /// on no other path.
        clusters: MeshClusters,
        /// Per-mesh bits for [`mesh::GpuMesh::flags`] — today only
        /// [`MESH_AUTHORED_TANGENTS`](mesh::GpuMesh::MESH_AUTHORED_TANGENTS).
        ///
        /// **Zero is the honest value for a caller that builds its vertices
        /// with [`mesh::MeshVertex::from_normal`]**, which is every mesh this
        /// engine authors for itself: that constructor fills the frame with
        /// [`orthonormal_basis`](crcbl_shaders::vertex::orthonormal_basis)'
        /// stand-in, which is orthonormal and agrees with no UV
        /// parameterisation, so a normal map read through it would be rotated by
        /// an angle nobody chose. Setting the bit is a claim that these
        /// vertices came from a real `TANGENT`, and the fragment stage takes the
        /// caller at its word — the screen-space derivative frame is what it
        /// falls back to otherwise.
        flags: u32,
    },
    /// `docs/plan/25-lod.md`'s cluster DAG: several levels of one surface, each
    /// its own vertex range, with the grouping that relates them.
    ///
    /// # A coarse level has no attributes, and the caller has to supply them
    ///
    /// `crcbl_scene::simplify` is **position-only** and says so in its own
    /// module docs: a coarser level's vertices are wherever the collapses put
    /// them and belong to no vertex of the level below, so the decimator carries
    /// no normals and no UVs. [`ClusterDag`] therefore holds positions, and
    /// this variant's `levels` is where the attributes come back — one
    /// vertex-byte array per level, which the caller produces however it can.
    ///
    /// [`demo`]'s dunes patch can do it because the surface is analytic:
    /// `crcbl_shaders::dunes::vertex_at` evaluates the height field at a
    /// position rather than interpolating an attribute nothing recorded. An
    /// application-supplied DAG needs attribute-aware simplification or
    /// nearest-source attribute transfer, which is unbuilt topic 25 work — so
    /// this variant is usable today only by a caller that can answer that
    /// question for its own surface.
    Dag {
        /// The range every level's UV lanes were quantised against — **one for
        /// all of them**.
        ///
        /// The mesh path resolves a row through the instance's mesh, which for
        /// a DAG is level 0, so a coarser level's vertices are decoded through
        /// level 0's range. A caller that quantised each level against its own
        /// bounds would see the coarse levels' texture slide.
        uv_range: UvRange,
        /// Vertex bytes per level, finest first and **parallel to
        /// [`dag.levels`](ClusterDag::levels)** — same length, and one vertex
        /// per position of the level beside it.
        levels: Vec<Cow<'a, [u8]>>,
        /// The levels, their clusters, and the groups that relate them.
        dag: ClusterDag,
        /// Per-mesh bits for [`mesh::GpuMesh::flags`] — today only
        /// [`MESH_AUTHORED_TANGENTS`](mesh::GpuMesh::MESH_AUTHORED_TANGENTS).
        ///
        /// **Zero is the honest value for a caller that builds its vertices
        /// with [`mesh::MeshVertex::from_normal`]**, which is every mesh this
        /// engine authors for itself: that constructor fills the frame with
        /// [`orthonormal_basis`](crcbl_shaders::vertex::orthonormal_basis)'
        /// stand-in, which is orthonormal and agrees with no UV
        /// parameterisation, so a normal map read through it would be rotated by
        /// an angle nobody chose. Setting the bit is a claim that these
        /// vertices came from a real `TANGENT`, and the fragment stage takes the
        /// caller at its word — the screen-space derivative frame is what it
        /// falls back to otherwise.
        flags: u32,
    },
}

impl Geometry<'_> {
    /// How many mesh table entries this geometry occupies: one for a flat mesh,
    /// one per level for a DAG.
    ///
    /// Never zero, and it is what a description's mesh ids are spaced by.
    #[must_use]
    pub fn levels(&self) -> usize {
        match self {
            Self::Flat { .. } => 1,
            Self::Dag { dag, .. } => dag.levels.len(),
        }
    }
}

/// One mesh a scene makes resident.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshDesc<'a> {
    /// Debug name. A DAG's levels are named `"{label} level {n}"`, so a capture
    /// says which level a buffer belongs to.
    pub label: Cow<'a, str>,
    /// The triangles and clusters themselves.
    pub geometry: Geometry<'a>,
}

/// Everything a [`ForwardRenderer`](crate::forward::ForwardRenderer) makes
/// resident, and how much room it reserves for it.
///
/// Read once, at [`with_scene`](crate::forward::ForwardRenderer::with_scene),
/// and never again: the pools it sizes are created there and none of them grows.
/// The order of [`meshes`](Self::meshes) and [`materials`](Self::materials) is
/// load-bearing — see the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDesc<'a> {
    /// The resident meshes, in the order their table ids are handed out.
    pub meshes: Vec<MeshDesc<'a>>,
    /// §3.2's material table, row by row. **Row 0 is what an instance written
    /// without a material id names**, so it is the row every unassigned object
    /// shades through.
    pub materials: Vec<mesh::GpuMaterial>,
    /// The base-colour page the rows above index.
    pub page: PageDesc<'a>,
    /// `docs/plan/18-render-features.md`'s irradiance volume, **added** to the
    /// flat ambient term wherever it covers.
    ///
    /// [`ProbeGrid::default`] is a grid of nothing and changes no pixel, which
    /// is what a description that does not mention it gets.
    pub probes: ProbeGrid,
    /// How much room each pool reserves.
    pub capacities: Capacities,
}

/// One object in the scene: which resident mesh it draws, which row it shades
/// through, and where it is.
///
/// Handed to [`ForwardRenderer::add_instance`] and
/// [`ForwardRenderer::set_instance`], at any point in a renderer's life — this
/// is the half of the scene that is calls rather than data, for the reason the
/// [module docs](self) open with.
///
/// # Both fields are description indices, not table ids
///
/// [`mesh`](Self::mesh) is an index into [`SceneDesc::meshes`] and
/// [`material`](Self::material) one into [`SceneDesc::materials`] — the
/// positions a caller wrote its own description in, which is the only numbering
/// it has. Neither is the id the GPU reads: a DAG occupies one mesh table entry
/// **per level**, so a description's fourth mesh is very often not table entry
/// four, and an instance always names level 0's entry whatever level it ends up
/// drawn at. The renderer holds that mapping and is what resolves it.
///
/// [`ForwardRenderer::add_instance`]: crate::forward::ForwardRenderer::add_instance
/// [`ForwardRenderer::set_instance`]: crate::forward::ForwardRenderer::set_instance
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstanceDesc {
    /// Which [`SceneDesc::meshes`] entry this object draws.
    pub mesh: usize,
    /// Which [`SceneDesc::materials`] row it shades through.
    ///
    /// Row 0 is the one [`mesh::GpuInstance::default`] names, so it is what an
    /// object shades through by omission — see [`SceneDesc::materials`].
    pub material: usize,
    /// Where it is: a model matrix, on [`mesh::GpuInstance::transform`]'s
    /// terms — **any affine one**, non-uniform scale included, because the mesh
    /// shaders build the normal transform out of it rather than assuming its
    /// 3×3 is orthonormal.
    pub transform: Mat4,
}

/// Where [`demo`]'s cube is in [`SceneDesc::meshes`], and therefore what an
/// [`InstanceDesc::mesh`] naming it carries.
///
/// **Public because placing an object is the caller's job.** [`demo`] says what
/// is resident and [`ForwardRenderer::add_instance`] is what puts one of those
/// meshes in the frame, so a caller of the demo scene needs a name for the entry
/// it is placing. A literal `0` would mean whatever the first entry happens to
/// be, and that list's order is load-bearing in the four ways the
/// [module docs](self) open with.
///
/// [`ForwardRenderer::add_instance`]: crate::forward::ForwardRenderer::add_instance
pub const DEMO_CUBE: usize = 0;
/// Where [`demo`]'s pyramid is, on [`DEMO_CUBE`]'s terms.
pub const DEMO_PYRAMID: usize = 1;
/// Where [`demo`]'s open box is, on [`DEMO_CUBE`]'s terms.
pub const DEMO_OPEN_BOX: usize = 2;
/// Where [`demo`]'s dunes patch is, on [`DEMO_CUBE`]'s terms.
///
/// The description's only [`Geometry::Dag`], so a caller placing it asks
/// [`ForwardRenderer::culls_clusters`] first — see
/// [`ForwardRenderer::add_instance`], which is where that condition is written
/// down.
///
/// [`ForwardRenderer::add_instance`]: crate::forward::ForwardRenderer::add_instance
/// [`ForwardRenderer::culls_clusters`]: crate::forward::ForwardRenderer::culls_clusters
pub const DEMO_DUNES: usize = 3;

/// [`demo`]'s neutral material row, and what an [`InstanceDesc::material`]
/// naming it carries.
///
/// **Row 0 is not a convention this module may pick**: it is the row
/// [`mesh::GpuInstance::default`] names, so it is what an instance written by
/// omission shades through. See [`SceneDesc::materials`].
pub const DEMO_UNTINTED: usize = 0;
/// [`demo`]'s tinted row — [`PYRAMID_TINT`] and [`PYRAMID_ROUGHNESS`], on
/// [`DEMO_UNTINTED`]'s terms.
pub const DEMO_TINTED: usize = 1;
/// [`demo`]'s textured row — [`CHECKER_LAYER`], on [`DEMO_UNTINTED`]'s terms.
pub const DEMO_TEXTURED: usize = 2;

/// The engine's own scene: the cube, the pyramid, the open box and the dunes
/// DAG, with the three material rows and the two page layers the golden suite
/// reads.
///
/// **This is what the renderer used to upload to itself**, moved out unchanged —
/// [`ForwardRenderer::new`](crate::forward::ForwardRenderer::new) is
/// `with_scene(&demo())`, so every existing caller draws the frame it always
/// did.
///
/// # Why each mesh is here
///
/// * The **cube** is `docs/plan/02-vulkan-backend.md`'s rung 3, and the first
///   resident of the pools.
/// * The **pyramid** is second, so the pool's second resident is at a non-zero
///   base vertex — the one thing that can tell a working base vertex from one
///   silently cancelled out.
/// * The **open box** is five clusters, one flat face each, four of them at a
///   non-zero [`Meshlet::vertex_offset`](crcbl_shaders::meshlet::Meshlet::vertex_offset):
///   the same argument one layer down, and the only resident whose cluster count
///   is not one.
/// * The **dunes patch** is the only one with a DAG, and it is analytic, which
///   is what lets [`Geometry::Dag`] be filled in at all — see that variant.
///
/// # Why three material rows and not four
///
/// The three are one row and two edits of it, and no edit can be mistaken for
/// another — which is what makes each of §3.2's columns its own evidence. The
/// textured row differs from the untinted one in its page layer and nothing
/// else. The tinted row differs in its base-colour factor and in
/// [`PYRAMID_ROUGHNESS`], which is two columns rather than one, and the reason
/// that is still separable is that the two do disjoint things to a frame: a
/// factor cannot narrow a highlight and a roughness cannot tint a surface, so
/// the colour assertions read one and the highlight assertion reads the other.
///
/// The roughness had to go on *that* row rather than on either of the others,
/// and it is the geometry that decides it. `Scene::Cube` puts the tinted pyramid
/// at `+X` and the default sun comes from `+X`, so its front face is the one
/// place in that frame where a surface sits at the mirror direction — and a
/// specular lobe's width is only legible where its peak actually lands. On the
/// two left-hand pyramids the same pair of roughnesses draws very nearly the
/// same face, which is exactly why the frame comparison uses them as its
/// control.
#[must_use]
pub fn demo() -> SceneDesc<'static> {
    let mut page = PageDesc::empty();
    page.set_extent(PageKind::BaseColor, PAGE_EXTENT);
    let checker = page.push_layer(PageKind::BaseColor, &CHECKER_TEXELS[..]);
    debug_assert_eq!(
        checker, CHECKER_LAYER,
        "the checker is this page's first and only layer"
    );

    SceneDesc {
        meshes: vec![
            MeshDesc {
                label: Cow::Borrowed("cube"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::cube_vertex_bytes()),
                    uv_range: mesh::demo_uv_range(),
                    indices: Cow::Owned(mesh::cube_indices()),
                    clusters: crcbl_shaders::meshlet::cube_clusters(),
                    // **Unmarked, and every mesh below it too.**
                    // `crcbl_shaders::mesh` builds these three out of positions,
                    // normals and UVs — `MeshVertex::from_normal`, whose frame
                    // is a stand-in — so none of them has a tangent that agrees
                    // with its own texture coordinates. Claiming otherwise would
                    // point the fragment stage at eight bytes that mean nothing
                    // for sampling.
                    flags: 0,
                },
            },
            MeshDesc {
                label: Cow::Borrowed("pyramid"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::pyramid_vertex_bytes()),
                    uv_range: mesh::demo_uv_range(),
                    indices: Cow::Owned(mesh::pyramid_indices()),
                    clusters: crcbl_shaders::meshlet::pyramid_clusters(),
                    flags: 0,
                },
            },
            MeshDesc {
                label: Cow::Borrowed("open box"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::open_box_vertex_bytes()),
                    uv_range: mesh::demo_uv_range(),
                    indices: Cow::Owned(mesh::open_box_indices()),
                    clusters: crcbl_shaders::meshlet::open_box_clusters(),
                    flags: 0,
                },
            },
            MeshDesc {
                label: Cow::Borrowed("dunes"),
                geometry: dunes_geometry(),
            },
        ],
        materials: vec![
            // **First, so it is row 0** — which is what
            // `mesh::GpuInstance::default` names, and therefore what an
            // instance written without a material id shades with. The page
            // column is named rather than left to `UNTINTED`'s own, so the two
            // agreeing is a fact visible at the call site.
            mesh::GpuMaterial {
                base_color_texture: mesh::GpuMaterial::NO_PAGE,
                ..mesh::GpuMaterial::UNTINTED
            },
            mesh::GpuMaterial {
                base_color: PYRAMID_TINT,
                roughness: PYRAMID_ROUGHNESS,
                ..mesh::GpuMaterial::UNTINTED
            },
            mesh::GpuMaterial {
                base_color_texture: CHECKER_LAYER,
                ..mesh::GpuMaterial::UNTINTED
            },
        ],
        page,
        // **No probes**, and that is what keeps every golden in the tree
        // byte-identical: the grid is authored per scene, this description
        // authors none, and a grid of nothing adds exactly zero. The sample
        // that has one is `apps/lantern`'s room.
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}

/// The dunes patch, level by level.
///
/// The geometry of a coarser level is positions and nothing else — the
/// decimator carries no attributes — so `dunes::vertex_at` is what turns each
/// into a vertex, by evaluating the surface rather than by interpolating an
/// attribute nothing recorded. See [`Geometry::Dag`], which is where that
/// constraint is stated for every caller.
fn dunes_geometry() -> Geometry<'static> {
    let dag = crcbl_shaders::cluster_dag::dunes_dag();
    let levels = dag
        .levels
        .iter()
        .map(|level| {
            let vertices: Vec<mesh::MeshVertex> = level
                .positions
                .iter()
                .map(|&position| crcbl_shaders::dunes::vertex_at(position))
                .collect();
            Cow::Owned(mesh::vertex_bytes(&vertices))
        })
        .collect();
    Geometry::Dag {
        // Every level's, because `dunes::vertex_at` maps the patch's extent
        // onto the unit square whatever the decimator did to a position — see
        // `crcbl_shaders::dunes::uv_range`.
        uv_range: crcbl_shaders::dunes::uv_range(),
        levels,
        dag,
        // Unmarked, for the three meshes above's reason: `dunes::vertex_at`
        // evaluates a height field and hands `MeshVertex::from_normal` a normal,
        // so the frame it encodes is the arbitrary stand-in.
        flags: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Row 0 is [`mesh::GpuMaterial::UNTINTED`] naming no page**, and nothing
    /// else may take that place.
    ///
    /// It is the row [`mesh::GpuInstance::default`] names, so every instance
    /// written by omission shades through it. A description that inserted the
    /// tint first would put a blue factor and a tighter lobe on every object
    /// nobody assigned a material to — the two pyramids would swap materials,
    /// and the frame would still look like a frame.
    #[test]
    fn the_demo_scene_shades_by_omission_through_an_untinted_row() {
        let scene = demo();
        assert_eq!(
            usize::try_from(mesh::GpuInstance::default().material).expect("a table of three rows"),
            DEMO_UNTINTED,
            "the row a caller names by omission and the one it names by index are the same row"
        );
        assert_eq!(
            scene.materials[mesh::GpuInstance::default().material as usize],
            mesh::GpuMaterial {
                base_color_texture: mesh::GpuMaterial::NO_PAGE,
                ..mesh::GpuMaterial::UNTINTED
            },
            "row 0 is what an instance with no material id names"
        );
        // And the other two are edits of it that a frame can tell apart — a
        // second row equal to the first would make every pair the golden suite
        // compares prove nothing.
        assert_ne!(scene.materials[0], scene.materials[1]);
        assert_ne!(scene.materials[0], scene.materials[2]);
        assert_eq!(
            scene.materials[0].base_color, scene.materials[2].base_color,
            "the texture pair must share a factor, or it is evidence about two columns"
        );
    }

    /// **[`PageDesc::empty`] allocates nothing, and the two kinds are sized and
    /// numbered apart.**
    ///
    /// The whole of what row (d) changed, in one place: a description starts
    /// with no page at all, a kind nothing names keeps an extent of zero, and
    /// each kind's layers count from zero on their own — so a row pointed at a
    /// base-colour layer number is not pointed at a normal layer that happens to
    /// exist.
    #[test]
    fn a_page_names_each_kind_apart_and_starts_naming_none() {
        let page = PageDesc::empty();
        for kind in PageKind::ALL {
            assert_eq!(page.extent(kind), 0, "{} starts unsized", kind.label());
            assert!(
                page.layers(kind).is_empty(),
                "{} starts empty",
                kind.label()
            );
        }
        page.check().expect("a description naming no page is legal");

        // Two extents, and the smaller one is the normal page — the shape the
        // one shared extent could not describe at all.
        let mut page = PageDesc::empty();
        page.set_extent(PageKind::BaseColor, 4);
        page.set_extent(PageKind::Normal, 2);
        let colour = page.push_layer(PageKind::BaseColor, vec![0x11; 4 * 4 * 4]);
        let normal = page.push_layer(PageKind::Normal, vec![0x22; 2 * 2 * 4]);
        assert_eq!(
            (colour, normal),
            (0, 0),
            "each kind numbers its own layers, and both start at zero"
        );
        assert_eq!(
            (
                page.extent(PageKind::BaseColor),
                page.extent(PageKind::Normal)
            ),
            (4, 2),
            "one kind's extent says nothing about the other's"
        );
        assert_eq!(page.layers(PageKind::BaseColor)[0][0], 0x11);
        assert_eq!(page.layers(PageKind::Normal)[0][0], 0x22);
        page.check()
            .expect("two kinds at two extents, each layer its own extent's worth");
    }

    /// **[`PageDesc::check`] refuses every page the renderer could not upload**,
    /// including the one shape no caller can spell, built here through the
    /// private fields no caller has.
    ///
    /// The length arm is an ordinary caller mistake: `push_layer` takes bytes it
    /// cannot measure. The no-extent arm is not spellable through the type's own
    /// methods at all — which is exactly why it is checked rather than asserted,
    /// and why it is built here from the fields.
    #[test]
    fn a_page_the_device_could_not_upload_is_refused() {
        // The shape that is right, which every wrong one below is one field
        // away from — and which is checked last, so a `check` that refused
        // everything could not pass this test.
        let good = || {
            let mut page = PageDesc::empty();
            page.set_extent(PageKind::BaseColor, 2);
            page.set_extent(PageKind::Normal, 2);
            page
        };

        for kind in PageKind::ALL {
            let mut short = good();
            short.push_layer(kind, vec![0x00; 4]);
            let refusal = short
                .check()
                .expect_err("a layer of the wrong length would upload another layer's texels");
            let HalError::InvalidDescriptor(said) = refusal else {
                panic!("a description is refused as a description");
            };
            assert!(
                said.contains(&format!("{} page layer 0 carries 4 bytes", kind.label())),
                "the refusal must name the kind and the layer, and it said {said:?}"
            );

            // Layers at no extent: the pair `set_extent` and `push_layer`
            // between them make unreachable, and the arm that would catch a
            // second constructor letting one through.
            let mut unsized_page = PageDesc::empty();
            unsized_page.pages[kind.index()]
                .layers
                .push(Cow::Owned(vec![0x00; 2 * 2 * 4]));
            let refusal = unsized_page
                .check()
                .expect_err("layers at no extent name an image that was never sized");
            let HalError::InvalidDescriptor(said) = refusal else {
                panic!("a description is refused as a description");
            };
            assert!(
                said.contains(&format!(
                    "the {} page carries 1 layer(s) at no extent",
                    kind.label()
                )),
                "the refusal must name the kind, and it said {said:?}"
            );
        }

        // Every layer is unconstrained but for its length, on both kinds, so the
        // accepted shape carries two of each and nothing white or neutral about
        // any of them.
        let mut whole = good();
        for kind in PageKind::ALL {
            whole.push_layer(kind, vec![0x00; 2 * 2 * 4]);
            whole.push_layer(kind, vec![0x7B; 2 * 2 * 4]);
        }
        whole
            .check()
            .expect("two full-length layers on each kind is a page the renderer uploads");
    }

    /// **[`demo`]'s page is its checker and nothing else**, at
    /// [`CHECKER_LAYER`], and it names no normal page at all.
    ///
    /// The layer number is the one [`DEMO_TEXTURED`] carries, so a page that
    /// pushed its checker anywhere else would shade that row through a layer the
    /// description does not have — refused at `with_scene` — or, worse, through
    /// one it does.
    #[test]
    fn the_demo_page_is_one_checker_layer_and_no_normal_page() {
        let page = demo().page;
        assert_eq!(page.extent(PageKind::BaseColor), PAGE_EXTENT);
        assert_eq!(
            page.layers(PageKind::BaseColor).len(),
            1,
            "the demo page is the checker alone: nothing burns a layer any more"
        );
        assert_eq!(
            page.layers(PageKind::BaseColor)[CHECKER_LAYER as usize].as_ref(),
            &CHECKER_TEXELS[..]
        );
        assert_eq!(
            demo().materials[DEMO_TEXTURED].base_color_texture,
            CHECKER_LAYER,
            "the textured row names the layer the page actually pushed"
        );
        assert_eq!(
            page.extent(PageKind::Normal),
            0,
            "nothing in the demo scene names a normal map, so that page is one \
             placeholder texel on the device"
        );
        page.check().expect("the demo page uploads as written");
    }

    /// The description the renderer consumes really is the resident set it used
    /// to hold: four meshes in the order the mesh ids are handed out, with the
    /// DAG last and carrying a vertex array per level.
    #[test]
    fn the_demo_scene_describes_the_residents_in_upload_order() {
        let scene = demo();
        let labels: Vec<&str> = scene
            .meshes
            .iter()
            .map(|mesh| mesh.label.as_ref())
            .collect();
        assert_eq!(labels, ["cube", "pyramid", "open box", "dunes"]);
        // And each index a caller places one of them by names that entry. A
        // constant off by one puts an application's cube wherever the pyramid
        // is — a frame that draws, of the wrong mesh.
        assert_eq!(
            [DEMO_CUBE, DEMO_PYRAMID, DEMO_OPEN_BOX, DEMO_DUNES].map(|mesh| labels[mesh]),
            ["cube", "pyramid", "open box", "dunes"]
        );
        assert_eq!(
            scene.meshes[..3]
                .iter()
                .map(|mesh| mesh.geometry.levels())
                .collect::<Vec<_>>(),
            [1, 1, 1],
            "the flat residents are one mesh table entry each"
        );

        let Geometry::Dag { levels, dag, .. } = &scene.meshes[3].geometry else {
            panic!("the dunes patch is the description's only DAG");
        };
        assert_eq!(
            levels.len(),
            dag.levels.len(),
            "a level with no vertices would draw its clusters out of the level below"
        );
        for (depth, (bytes, level)) in levels.iter().zip(&dag.levels).enumerate() {
            assert_eq!(
                bytes.len(),
                level.positions.len() * mesh::VERTEX_STRIDE,
                "level {depth} carries one vertex per position"
            );
        }
        assert!(
            dag.levels.len() > 1,
            "a one-level DAG is a flat mesh with extra steps, and would select nothing"
        );
    }

    /// The capacities a description defaults to are the ones the renderer has
    /// always reserved. A default that moved would resize five device-local
    /// pools for every existing caller at once.
    #[test]
    fn the_default_capacities_are_the_ones_the_engine_shipped() {
        assert_eq!(
            Capacities::default(),
            Capacities {
                vertices: 64 * 1024,
                indices: 256 * 1024,
                meshes: 1024,
                instances: 16 * 1024,
                materials: 1024,
                lights: 1024,
                probes: 0,
            }
        );
        assert_eq!(demo().capacities, Capacities::default());
    }

    /// **The engine's own scene has no probes**, which is the whole of why
    /// adding the grid moved no golden: an empty volume evaluates to exactly
    /// zero and the shader adds it to the ambient term.
    #[test]
    fn the_demo_scene_authors_no_probes() {
        let scene = demo();
        assert_eq!(scene.probes, ProbeGrid::default());
        assert!(scene.probes.probes.is_empty());
        assert_eq!(scene.probes.volume.total(), 0);
        assert_eq!(scene.capacities.probes, 0);
        scene
            .probes
            .check()
            .expect("an empty grid uploads as written");
    }

    /// **A grid whose counts disagree with its rows is refused**, in both
    /// directions — the check that bounds the shader's fetch.
    #[test]
    fn a_grid_that_would_read_rows_it_does_not_carry_is_refused() {
        let two = vec![probe::GpuProbe::ZERO; 2];
        let claims_more = ProbeGrid {
            update: ProbeUpdate::Authored,
            volume: probe::ProbeVolume {
                counts: [2, 2, 1],
                ..probe::ProbeVolume::default()
            },
            probes: two.clone(),
        };
        claims_more
            .check()
            .expect_err("a 2×2×1 grid carrying two probes would read rows it never wrote");

        let claims_fewer = ProbeGrid {
            update: ProbeUpdate::Authored,
            volume: probe::ProbeVolume {
                counts: [1, 1, 1],
                ..probe::ProbeVolume::default()
            },
            probes: two.clone(),
        };
        claims_fewer
            .check()
            .expect_err("a one-probe grid carrying two leaves one unreachable");

        // And the shape that agrees is accepted, or the two refusals above
        // would pass on a check that refused everything.
        ProbeGrid {
            update: ProbeUpdate::Authored,
            volume: probe::ProbeVolume {
                counts: [2, 1, 1],
                inv_spacing: [1.0; 3],
                origin: [0.0; 3],
                levels: 1,
                steps: probe::ProbeSteps::default(),
            },
            probes: two.clone(),
        }
        .check()
        .expect("two probes on a 2×1×1 grid");

        // **A level is a whole copy of the grid**, so a two-level clipmap of
        // the same counts needs twice the rows — a check that only multiplied
        // the counts would accept this pair the wrong way round.
        let clipmap = probe::ProbeVolume {
            counts: [2, 1, 1],
            inv_spacing: [1.0; 3],
            origin: [0.0; 3],
            levels: 2,
            steps: probe::ProbeSteps::default(),
        };
        ProbeGrid {
            update: ProbeUpdate::Authored,
            volume: clipmap,
            probes: two.clone(),
        }
        .check()
        .expect_err("a two-level clipmap of two probes a level needs four rows");
        ProbeGrid {
            update: ProbeUpdate::Authored,
            volume: clipmap,
            probes: vec![probe::GpuProbe::ZERO; 4],
        }
        .check()
        .expect("four rows fill a two-level clipmap of two probes a level");
    }
}
