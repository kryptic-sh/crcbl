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
//! * **Page layer 0 must be opaque white**, because a material naming no
//!   texture samples it and every untextured surface is multiplied by whatever
//!   it holds. [`PageDesc`] owns that layer for exactly this reason — see its
//!   docs.
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
use glam::Mat4;

/// The second material's base colour, and the whole of what makes it visible.
///
/// A factor per channel with no two alike, so multiplying by it moves every
/// colour it touches: a tint that left a channel at `1.0` would leave the
/// pyramid's white base looking like a lighting difference, and one that left
/// them equal would be a brightness change a shading bug could also produce.
/// See [`set_tinted_pyramid`](crate::forward::ForwardRenderer::set_tinted_pyramid),
/// which is the pair this exists for.
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
/// * Every texel boundary is a hard edge — the sampler has no mips and filters
///   nearest — and a hard edge is where two rasterisers can land on opposite
///   sides of an interpolated UV. Four texels put **one** boundary across a face
///   in each axis, at `0.5`, which is as far from a vertex as an edge can be. A
///   denser checker would put a row of disagreeable pixels through every face
///   for no more evidence.
pub const PAGE_EXTENT: u32 = 2;

/// Bytes in one layer of [`demo`]'s page: [`PAGE_EXTENT`]² RGBA texels.
const PAGE_LAYER_BYTES: usize = (PAGE_EXTENT * PAGE_EXTENT) as usize * 4;

/// The layer [`set_textured_pyramid`](crate::forward::ForwardRenderer::set_textured_pyramid)
/// shades with — the one [`demo`] appends past
/// [`PageDesc::UNTEXTURED_LAYER`].
pub const CHECKER_LAYER: u32 = 1;

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
        }
    }
}

/// §3.2's texture side: one `D2Array` image whose layers a material row selects
/// with [`GpuMaterial::base_color_texture`](mesh::GpuMaterial::base_color_texture).
///
/// One image with a layer per texture rather than an array of descriptors —
/// [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) rather than
/// [`Bindless`](crcbl_hal::BindingModel::Bindless) — because a layer index
/// needs nothing of a device where a descriptor array needs
/// [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING), which
/// `crcbl-mtl` withdraws.
///
/// # Layer 0 belongs to this type, and that is the whole point of the type
///
/// A material that names no texture samples layer 0, so **layer 0 has to decode
/// to exactly `1.0` in every channel** or every untextured surface in the frame
/// is scaled by a texel that is not one. That reads as a global albedo change,
/// which is indistinguishable from a lighting bug and visible in no assertion
/// the CPU can make.
///
/// So there is no constructor that takes layer 0: [`opaque_white`](Self::opaque_white)
/// writes it, [`push_layer`](Self::push_layer) can only append past it, and a
/// caller has no way to spell the mistake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageDesc<'a> {
    extent: u32,
    layers: Vec<Cow<'a, [u8]>>,
}

impl<'a> PageDesc<'a> {
    /// The layer a material naming no texture samples, which
    /// [`mesh::GpuMaterial::UNTINTED`] is written against.
    pub const UNTEXTURED_LAYER: u32 = 0;

    /// The texel [`UNTEXTURED_LAYER`](Self::UNTEXTURED_LAYER) is filled with.
    ///
    /// The page is `Rgba8UnormSrgb` and `0xFF` is the one value that encoding
    /// leaves alone, so the sampler returns exactly `1.0` — the same product a
    /// material was shaded by before there was a page at all.
    pub const WHITE: u8 = 0xFF;

    /// A square page of `extent` texels a side, holding
    /// [`UNTEXTURED_LAYER`](Self::UNTEXTURED_LAYER) and nothing else.
    ///
    /// # Panics
    ///
    /// If `extent` is zero, which is an image no device will create.
    #[must_use]
    pub fn opaque_white(extent: u32) -> Self {
        assert!(extent > 0, "a page must have at least one texel a side");
        let bytes = extent as usize * extent as usize * 4;
        Self {
            extent,
            layers: vec![Cow::Owned(vec![Self::WHITE; bytes])],
        }
    }

    /// Appends a layer and returns its index — the number a material row's
    /// [`base_color_texture`](mesh::GpuMaterial::base_color_texture) carries.
    ///
    /// `texels` is RGBA8, row-major, sRGB-encoded: that is what glTF defines a
    /// base-colour texture to be, and the renderer creates the image as
    /// `Rgba8UnormSrgb` so the sampler decodes it. Its length must be
    /// `extent² × 4`, which [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// checks before it uploads anything.
    pub fn push_layer(&mut self, texels: impl Into<Cow<'a, [u8]>>) -> u32 {
        let layer = u32::try_from(self.layers.len())
            .unwrap_or_else(|_| unreachable!("a page of more layers than a u32 can name"));
        self.layers.push(texels.into());
        layer
    }

    /// Texels a side.
    #[must_use]
    pub const fn extent(&self) -> u32 {
        self.extent
    }

    /// Every layer, in order: element `n` is layer `n`.
    #[must_use]
    pub fn layers(&self) -> &[Cow<'a, [u8]>] {
        &self.layers
    }

    /// Whether this page can be uploaded as written, checked by
    /// [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// before it creates anything.
    ///
    /// Two things, and they fail for different reasons.
    /// [`push_layer`](Self::push_layer) takes bytes it cannot measure against an
    /// extent it does not see, so a layer of the wrong length is an ordinary
    /// caller mistake. Layer 0's whiteness, on the other hand, is
    /// [`opaque_white`](Self::opaque_white)'s to guarantee and no caller can
    /// spell it wrong today — this is what would catch a *second* constructor
    /// that let one through, which is why it is checked here rather than
    /// asserted at construction.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] naming the layer and what is wrong with
    /// it.
    pub(crate) fn check(&self) -> Result<(), HalError> {
        let texels = self.extent as usize * self.extent as usize * 4;
        for (layer, bytes) in self.layers.iter().enumerate() {
            if bytes.len() != texels {
                return Err(HalError::InvalidDescriptor(format!(
                    "page layer {layer} carries {} bytes, and a {}×{} RGBA8 layer is {texels}",
                    bytes.len(),
                    self.extent,
                    self.extent
                )));
            }
        }
        match self.layers.first() {
            Some(white) if white.iter().all(|&texel| texel == Self::WHITE) => Ok(()),
            Some(_) => Err(HalError::InvalidDescriptor(
                "page layer 0 is not opaque white, so every material naming no texture \
                 would be scaled by a texel that is not 1.0"
                    .to_string(),
            )),
            None => Err(HalError::InvalidDescriptor(
                "a page with no layers has nothing for a material naming no texture to \
                 sample"
                    .to_string(),
            )),
        }
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
        vertices: Cow<'a, [u8]>,
        /// Triangle indices into `vertices`, **mesh-relative**: the pool's base
        /// vertex is added by the shader through the mesh table.
        indices: Cow<'a, [u32]>,
        /// The same triangles partitioned for a mesh stage. Read on
        /// [`GeometryPath::MeshShader`](crcbl_hal::GeometryPath::MeshShader) and
        /// on no other path.
        clusters: MeshClusters,
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
        /// Vertex bytes per level, finest first and **parallel to
        /// [`dag.levels`](ClusterDag::levels)** — same length, and one vertex
        /// per position of the level beside it.
        levels: Vec<Cow<'a, [u8]>>,
        /// The levels, their clusters, and the groups that relate them.
        dag: ClusterDag,
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
    /// Where it is: a **rigid** model matrix, on
    /// [`mesh::GpuInstance::transform`]'s terms.
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
    let mut page = PageDesc::opaque_white(PAGE_EXTENT);
    let checker = page.push_layer(&CHECKER_TEXELS[..]);
    debug_assert_eq!(
        checker, CHECKER_LAYER,
        "the checker is the layer past the white one"
    );

    SceneDesc {
        meshes: vec![
            MeshDesc {
                label: Cow::Borrowed("cube"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::cube_vertex_bytes()),
                    indices: Cow::Owned(mesh::cube_indices()),
                    clusters: crcbl_shaders::meshlet::cube_clusters(),
                },
            },
            MeshDesc {
                label: Cow::Borrowed("pyramid"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::pyramid_vertex_bytes()),
                    indices: Cow::Owned(mesh::pyramid_indices()),
                    clusters: crcbl_shaders::meshlet::pyramid_clusters(),
                },
            },
            MeshDesc {
                label: Cow::Borrowed("open box"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::open_box_vertex_bytes()),
                    indices: Cow::Owned(mesh::open_box_indices()),
                    clusters: crcbl_shaders::meshlet::open_box_clusters(),
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
            // instance written without a material id shades with. The layer is
            // named rather than left to `UNTINTED`'s own zero, so the two
            // agreeing is a fact visible at the call site.
            mesh::GpuMaterial {
                base_color_texture: PageDesc::UNTEXTURED_LAYER,
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
            let mut bytes = Vec::with_capacity(level.positions.len() * mesh::VERTEX_STRIDE);
            for &position in &level.positions {
                let vertex = crcbl_shaders::dunes::vertex_at(position);
                for value in vertex
                    .position
                    .iter()
                    .chain(&vertex.normal)
                    .chain(&vertex.color)
                    .chain(&vertex.uv)
                {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            Cow::Owned(bytes)
        })
        .collect();
    Geometry::Dag { levels, dag }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Row 0 is [`mesh::GpuMaterial::UNTINTED`] with the untextured layer**,
    /// and nothing else may take that place.
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
                base_color_texture: PageDesc::UNTEXTURED_LAYER,
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

    /// **Page layer 0 is opaque white**, in every texel and at whatever extent.
    ///
    /// A material naming no texture samples it, so any other value is a global
    /// albedo scale on every untextured surface — which reads as a lighting
    /// difference and is visible in no CPU assertion.
    #[test]
    fn page_layer_zero_is_white_whatever_the_caller_appends() {
        let mut page = PageDesc::opaque_white(4);
        let appended = page.push_layer(vec![0x00; 4 * 4 * 4]);
        assert_ne!(
            appended,
            PageDesc::UNTEXTURED_LAYER,
            "a caller can only append past the white layer"
        );
        assert!(
            page.layers()[PageDesc::UNTEXTURED_LAYER as usize]
                .iter()
                .all(|&texel| texel == PageDesc::WHITE),
            "layer 0 must decode to 1.0 in every channel"
        );
        assert_eq!(page.layers()[0].len(), 4 * 4 * 4);

        let demo_page = demo().page;
        assert_eq!(demo_page.extent(), PAGE_EXTENT);
        assert!(
            demo_page.layers()[PageDesc::UNTEXTURED_LAYER as usize]
                .iter()
                .all(|&texel| texel == PageDesc::WHITE)
        );
        assert_eq!(
            demo_page.layers()[CHECKER_LAYER as usize].as_ref(),
            &CHECKER_TEXELS[..]
        );
        demo_page.check().expect("the demo page uploads as written");
    }

    /// **[`PageDesc::check`] refuses each of the three pages that would draw a
    /// wrong frame**, built here through the private fields no caller has.
    ///
    /// Without this the whiteness check is a check that cannot fail:
    /// [`PageDesc::opaque_white`] is the only way in from outside the crate and
    /// it writes layer 0 itself, so nothing a caller can spell would ever reach
    /// that arm. What it is there for is a *second* constructor — slice 4's
    /// app-supplied page — and this is what says the arm still bites when one
    /// arrives.
    #[test]
    fn a_page_that_would_rescale_every_untextured_surface_is_refused() {
        let grey = PageDesc {
            extent: 2,
            layers: vec![Cow::Owned(vec![0x80; 2 * 2 * 4])],
        };
        grey.check()
            .expect_err("a grey layer 0 scales every untextured material");

        let short = PageDesc {
            extent: 2,
            layers: vec![
                Cow::Owned(vec![PageDesc::WHITE; 2 * 2 * 4]),
                Cow::Owned(vec![0x00; 4]),
            ],
        };
        short
            .check()
            .expect_err("a layer of the wrong length would upload another layer's texels");

        let empty = PageDesc {
            extent: 2,
            layers: Vec::new(),
        };
        empty
            .check()
            .expect_err("a material naming no texture has nothing to sample");

        // And the shape that is right is accepted, or the three refusals above
        // would pass on a check that refused everything.
        PageDesc {
            extent: 2,
            layers: vec![
                Cow::Owned(vec![PageDesc::WHITE; 2 * 2 * 4]),
                Cow::Owned(vec![0x00; 2 * 2 * 4]),
            ],
        }
        .check()
        .expect("a white layer 0 and a full-length layer 1");
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

        let Geometry::Dag { levels, dag } = &scene.meshes[3].geometry else {
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
            }
        );
        assert_eq!(demo().capacities, Capacities::default());
    }
}
