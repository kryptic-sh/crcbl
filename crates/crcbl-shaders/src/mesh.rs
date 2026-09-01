//! The geometry and the uniform block `mesh.slang` reads, in the byte layouts
//! that shader declares.
//!
//! Same argument as [`triangle`](crate::triangle): `struct MeshVertex`,
//! `struct FrameUniforms` and `struct GpuInstance` in `shaders/mesh.slang` fix
//! a memory layout, and any producer of those bytes has to agree with it
//! exactly. Keeping them in the
//! crate that owns the source means there is one place to change rather than one
//! per consumer — and there are already three consumers (the sandbox,
//! `crcbl-render`'s forward pass, and `crcbl-vk`'s golden-image suite), which is
//! the number at which a duplicated layout silently stops matching.
//!
//! # This is demo geometry, and the plan expects some
//!
//! `docs/plan/02-vulkan-backend.md`'s ladder puts a "hardcoded cube/sphere" at
//! rung 3, and a cube is the honest choice: six flat faces with distinct normals
//! make a directional light *visible* in a way a sphere's smooth shading does
//! not, and six distinct face colours make an orientation mistake a different
//! picture rather than a plausible one. Real meshes arrive from assets at P9 and
//! are ranges in a global pool (`docs/plan/03-gpu-driven-rendering.md` §3.1).
//!
//! # Indexed, unlike the triangle
//!
//! The cube is 24 vertices and 36 indices rather than 36 loose vertices. Vertex
//! pulling and the index buffer are orthogonal — the shader still indexes a
//! storage buffer with `SV_VertexID`, which for an indexed draw is the value
//! read out of the index buffer — and this is the first thing in the engine to
//! exercise [`bind_index_buffer`] and `draw_indexed` at all.
//!
//! [`bind_index_buffer`]: https://docs.rs/crcbl-hal

use crate::probe::{PROBE_VOLUME_SIZE, ProbeVolume};
use crate::vertex::{QTangent, UvRange};

/// Bytes one vertex spends in the **position** stream: a `float3`, no padding.
///
/// `docs/plan/43-render-standards.md` §2's 2026-08-30 layout, stream 0. This is
/// the whole of what a depth prepass or a shadow cascade has to fetch, and it
/// is why the pool keeps the two streams in regions of their own rather than
/// interleaved — see [`MeshVertex`], whose docs carry the arithmetic.
///
/// A struct of three scalar `float`s rather than a `float3`: `std430` gives a
/// `float3` an alignment of sixteen and would round the record up to it, where
/// three scalars pack to twelve on all four targets. [`MESH_ENTRY_STRIDE`] is
/// the same trick and `the_mesh_entry_layout_matches_the_offsets_slangc_emits`
/// is what says the targets really do lay it out that way.
pub const POSITION_STRIDE: usize = 12;

/// Bytes one vertex spends in the **attribute** stream: five `uint`s, no
/// padding.
///
/// §2's stream 1, in the order [`MeshVertex::attribute_bytes`] writes them: two
/// words of `snorm16x4` tangent frame, one of `unorm16x2` for each UV set, and
/// one of `rgba8`.
///
/// **Twenty, where the plan's table says thirty-two.** The table's own rows add
/// to twenty — 8 + 4 + 4 + 4 — and the prose beside it carried the wider figure
/// from an earlier draft. The arithmetic is what this constant follows; padding
/// up to a round number would be four bytes a vertex that nothing reads.
pub const ATTRIBUTE_STRIDE: usize = 20;

/// Bytes one vertex costs a caller's description, both streams together.
///
/// A description hands the renderer one array of [`MeshVertex`] records — see
/// [`MeshVertex::to_bytes`] — and [`MeshPool`](https://docs.rs/crcbl-render) is
/// what splits them into the two regions the shaders read. So this is the
/// stride of the *description*, and [`POSITION_STRIDE`] and
/// [`ATTRIBUTE_STRIDE`] are the strides of the two pool regions.
pub const VERTEX_STRIDE: usize = POSITION_STRIDE + ATTRIBUTE_STRIDE;

/// How many cascades the sun's shadow map is split into.
///
/// `docs/plan/18-render-features.md`'s shadow section asks for 2–3 cascades.
/// The same number is `static const uint SHADOW_CASCADES` in
/// `shaders/mesh.slang` and `shaders/mesh_cluster.slang`, and this module's
/// `the_cascade_count_matches_the_one_the_shaders_declare` is what keeps the
/// three from drifting — a block whose array is shorter on one side than the
/// other puts every member after it at the wrong offset, which no compiler
/// anywhere sees.
///
/// **Four is the ceiling**, set by [`FrameUniforms::cascade_far`] being one
/// `float4`.
pub const SHADOW_CASCADES: usize = 2;

/// How many **tiles** the atlas has for shadowed lights, beside the cascades.
///
/// `docs/plan/18-render-features.md`'s 2026-08-13 decision: the atlas is a fixed
/// tile grid, the sun's cascades take the first tiles, and the rest are handed
/// out one per shadowed spot and six per shadowed point. A light that gets no
/// tiles still lights and simply does not occlude.
///
/// **A tile, not a light**: a point light owns [`SHADOW_POINT_FACES`] of these
/// and a spot owns one, so this is not how many lights a frame can shadow —
/// `crcbl_render::shadow::LIGHT_SLOTS` is that number, and it is a host-side
/// budget because nothing in a shader counts lights.
///
/// Here rather than in `crcbl_render::shadow` for [`SHADOW_CASCADES`]'s reason
/// exactly: [`FrameUniforms::light_view_proj`] is an array of this length, so a
/// block sized differently on the two sides puts every member after it at the
/// wrong offset. The same drift test covers it.
///
/// **Two point cubes and two spots**, which is what the 2026-08-26 re-tiling of
/// the atlas bought: the grid gained a column and a row while [`SHADOW_TILE`]
/// shrank to keep their product where it was, so the atlas image did not change
/// size at all. What a scene gets for it is a *second* point light that
/// occludes — one cube is [`SHADOW_POINT_FACES`] tiles, and until then this
/// region was one tile past exactly one of them — paid for in shadow-map
/// resolution rather than in memory.
pub const SHADOW_LIGHT_TILES: usize = 14;

/// Tiles one point light's shadow map is: the six faces of a cube.
///
/// `docs/plan/18-render-features.md`: six atlas tiles rather than a cube map, so
/// one image, one sampler, one barrier story and one allocator serve the sun,
/// the spots and the points alike. The face order is `+X, -X, +Y, -Y, +Z, -Z`
/// — the cube-map convention — and `crcbl_render::shadow::face_axis` is the one
/// place it is written down on the host, `point_face` in `shaders/mesh.slang`
/// the one place on the device.
pub const SHADOW_POINT_FACES: usize = 6;

/// The side of one **root cell** of the shadow atlas, in texels.
///
/// The largest map `crcbl_render::shadow::AtlasAllocator` can hand out, and one
/// cell of the grid its quadtrees are rooted on — not the side of every map,
/// which since `docs/plan/45-shadows.md`'s priority rung is whatever level a
/// light's coverage earned it.
///
/// **A host number, and no longer the shader's.** Every shadow bias
/// `mesh.slang` applies — a cascade's as much as a cone's — is denominated in
/// the texels of the map being sampled, and that file reads each map's own side
/// out of its `shadow_atlas_rect` through `tile_texels`. See
/// `PUNCTUAL_DEPTH_BIAS_TEXELS` there, and `crcbl_render::shadow`'s
/// `DEPTH_BIAS_TEXELS` for the sun's.
///
/// Chosen with [`SHADOW_ATLAS_COLUMNS`] rather than on its own: the two multiply
/// to the atlas's extent, and the 2026-08-26 re-tiling picked the pair that
/// leaves that product where it was — so the widened grid costs no texels, no
/// `D32Float` memory and no allocation change, only per-tile resolution.
pub const SHADOW_TILE: u32 = 768;

/// Root cells across the shadow atlas.
///
/// At least [`SHADOW_CASCADES`], which is what keeps the cascades in the *top
/// row* — see `crcbl_render::shadow::tile_origin`, and the cascade goldens,
/// which are what say the arrangement survived a change to the grid's shape.
///
/// Their origins move whenever this or [`SHADOW_TILE`] does, so a change to
/// either is a change every golden of a shadowed scene has to be re-blessed
/// through; what the bound protects is the *arrangement*, not the texels.
///
/// **No shader reads it.** It sizes the image and it is the root order
/// `crcbl_render::shadow::AtlasAllocator` subdivides, both of which are host
/// facts; the sampling side reads a rectangle out of
/// [`FrameUniforms::shadow_atlas_rect`] and knows nothing about a grid. It lives
/// here beside [`SHADOW_TILE`], which the shader does read, because the two
/// multiply to the extent and picking one without the other is how the atlas
/// changes size by accident.
pub const SHADOW_ATLAS_COLUMNS: u32 = 4;

/// Root cells down it.
///
/// A grid rather than one row: a point light is [`SHADOW_POINT_FACES`] tiles of
/// exactly this kind, and a row long enough to hold them beside the cascades
/// would be an image wider than some devices' limit. Read by no shader, on
/// [`SHADOW_ATLAS_COLUMNS`]' terms.
pub const SHADOW_ATLAS_ROWS: u32 = 4;

/// Slots the shadow atlas has: [`SHADOW_CASCADES`] for the sun and
/// [`SHADOW_LIGHT_TILES`] for the lights that fit.
///
/// A **slot** is one map — one matrix of [`FrameUniforms::light_view_proj`] or
/// [`FrameUniforms::shadow_view_proj`], and one row of
/// [`FrameUniforms::shadow_atlas_rect`], which is what this sizes. Where that
/// map lives in the image is the rectangle rather than the index, which is what
/// lets one map be a different size from the next;
/// `crcbl_render::shadow::AtlasAllocator` is what decides it.
pub const SHADOW_ATLAS_TILES: usize = SHADOW_CASCADES + SHADOW_LIGHT_TILES;

/// How far in front of a cascade's near plane still writes depth, in world
/// units.
///
/// A caster standing between the sun and a cascade's sphere is outside the
/// sphere and must still darken what is inside it, so
/// `crcbl_render::shadow::cascade_matrix` pulls the light's box back this far —
/// which is where the number is spent, and that module's `CASTER_REACH` takes
/// it from here rather than declaring a second copy.
///
/// **The sampling side reads it too**, which is why it lives here rather than
/// there: a cascade of radius `r` is an orthographic box `2 r + this` deep, and
/// `mesh.slang`'s `sun_penumbra_texels` needs exactly that factor to turn a
/// difference of two shadow-clip depths back into the metres a penumbra is
/// measured in. Two copies of it would put the blocker search and the matrix it
/// is inverting on different numbers, and the picture that produces is a
/// penumbra scaled wrongly rather than anything that looks like a bug.
/// The depth one tile of the shadow atlas is reset to before it is redrawn.
///
/// The reversed-Z far plane — `crcbl_hal::depth::CLEAR` — and this crate
/// declares it because `mesh.slang`'s `depthClearVertexMain` writes it: a pass
/// that keeps some tiles and redraws others cannot clear one tile through the
/// attachment's load operation, so the tiles it does redraw are reset by a
/// primitive covering each of them. A clear quad and a load-operation clear that
/// disagreed would leave a held tile and a redrawn one answering differently
/// where nothing was drawn, which is a shadow that appears and disappears with
/// the cadence.
pub const SHADOW_ATLAS_CLEAR_DEPTH: f32 = 0.0;

pub const SHADOW_CASTER_REACH: f32 = 40.0;

/// Every descriptor binding `mesh.slang` declares in set 0, ascending.
///
/// **A list rather than a count, because the numbers are not contiguous** — the
/// shader leaves gaps where bindings were retired, and a consumer that assumed
/// `0..N` would cover descriptors the module does not declare and miss the ones
/// it does.
///
/// This exists because a *hand-written* copy of this table is a real thing in
/// the tree: `crcbl`'s `forward_e2e::depth_probe` drives the module directly
/// rather than through `ForwardRenderer`, so it builds the whole bind group
/// layout itself. Nothing held the two together, and on 2026-09-01 the
/// contact-shadow channel arrived at binding 28, the probe's table did not gain
/// it, and three of its tests **segfaulted** on lavapipe and WARP — the layout
/// left a declared descriptor uncovered, which is supposed to be refused at
/// pipeline creation and is not on a software adapter. Nothing named the
/// binding; nothing named anything.
///
/// The test below holds this constant to the shader, and the probe holds its
/// own table to this constant. So a binding added to `mesh.slang` and nowhere
/// else fails here first, under a plain `cargo test`, on a machine with no GPU.
pub const DECLARED_BINDINGS: [u32; 19] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 15, 16, 20, 21, 22, 23, 25, 26, 27, 28,
];

const _: () = assert!(
    (SHADOW_ATLAS_COLUMNS * SHADOW_ATLAS_ROWS) as usize == SHADOW_ATLAS_TILES,
    "every root cell of the atlas is one slot's, and every slot has a cell to \
     take: an atlas short of a cell is a map the allocator cannot place, and one \
     over is texels nothing can ever write"
);

const _: () = assert!(
    SHADOW_ATLAS_COLUMNS as usize >= SHADOW_CASCADES,
    "the cascades are the first tiles and are in the top row; a grid narrower \
     than the cascade count wraps one of them onto the next row and moves the \
     texels every cascade golden was blessed from"
);

const _: () = assert!(
    SHADOW_LIGHT_TILES >= SHADOW_POINT_FACES,
    "a point light's six faces have to fit in the light region at all, or the \
     budget refuses every point light there can ever be"
);

/// Bytes in the frame uniform block.
///
/// One `float4x4` (64), two `float4` (16 each), [`SHADOW_CASCADES`] more
/// `float4x4`, two closing `float4`, a `uint4`, [`SHADOW_LIGHT_TILES`] more
/// `float4x4`, the irradiance grid's [`PROBE_VOLUME_SIZE`] header, the LOD and
/// fog rows, the sky's three spherical-harmonic rows, the previous frame's
/// `float4x4`, the vertex pool's `uint4` and [`SHADOW_ATLAS_TILES`] `float4` of
/// atlas rectangles. Checked against the `Offset` decorations `slangc` emits by
/// this module's `the_uniform_block_matches_the_offsets_slangc_emits`.
pub const FRAME_UNIFORMS_SIZE: usize = 96
    + 64 * SHADOW_CASCADES
    + 48
    + 64 * SHADOW_LIGHT_TILES
    + PROBE_VOLUME_SIZE
    + 16
    + 32
    + 48
    + 64
    + 16
    + 16 * SHADOW_ATLAS_TILES;

/// Bytes per [`GpuInstance`], and the stride of the instance storage buffer.
///
/// Two `float4x4` (64 each) then six `uint` (4 each) and two `uint` of
/// explicit padding, which is what makes the record the same size on all four
/// targets:
/// `std430`, WGSL and MSL round a struct up to its alignment and DXIL's
/// structured-buffer stride does not, so the tail is declared in the shader
/// rather than left to each target's rule. Checked against the `ArrayStride 160`
/// and the `Offset` decorations `slangc` emits by this module's
/// `the_instance_layout_matches_the_offsets_slangc_emits`.
pub const INSTANCE_STRIDE: usize = 160;

/// `uint` lanes of declared padding at the end of a [`GpuInstance`], past the
/// last field a shader reads.
///
/// Named because [`GpuInstance::to_bytes`] has to leave exactly this many
/// untouched and `the_instance_layout_matches_the_offsets_slangc_emits` has to
/// find exactly this many zeroed — see [`INSTANCE_STRIDE`], which is where the
/// four targets' disagreement about an implicit tail is written out.
pub const INSTANCE_PAD_WORDS: usize = 2;

/// Bytes per [`GpuMesh`], and the stride of the mesh-table storage buffer.
///
/// Three `uint`, then ten `float` — the box's two corners and then
/// [`GpuMesh::uv_range`]'s scale and offset — and then one closing `uint`,
/// [`GpuMesh::flags`], with no padding anywhere. Checked against the
/// `ArrayStride` and the `Offset` decorations `slangc` emits by this module's
/// `the_mesh_entry_layout_matches_the_offsets_slangc_emits` — which is the test
/// that says all three targets really did lay it out this way, since a `std430`
/// struct of scalars is one of the few whose stride an implementation could
/// round up without anything else noticing.
///
/// **Fifty-six, and the four bytes are [`GpuMesh::flags`]' — appended, never
/// inserted.** Every offset below it is where it was before the flags existed,
/// which is what let the normal-map slice widen this row without moving a
/// single field the cull and draw-argument passes read.
pub const MESH_ENTRY_STRIDE: usize = 56;

/// Bytes per [`GpuMaterial`], and the stride of the material-table storage
/// buffer.
///
/// One `float4` and then twelve scalar words, no padding at all: sixty-four is
/// already a multiple of the `float4`'s sixteen. Checked against the
/// `ArrayStride` and the `Offset` decorations `slangc` emits by this module's
/// `the_material_layout_matches_the_offsets_slangc_emits`.
///
/// # Sixty-four, which is `docs/plan/43-render-standards.md` §2's own number
///
/// That section's table sizes the row at 64 bytes for "four page rows … plus
/// the alpha cutoff and flags", and the four page rows are what make the
/// arithmetic tight. Written as four separate `uint`s the row wants eighteen
/// words — 72, which `std430` rounds to 80 — so the four layer indices ride
/// **two per word**, sixteen bits each — `color_normal_pages` and
/// `mro_emissive_pages` as `shaders/mesh.slang` names them, four plain fields on
/// this side. A page layer index is bounded by the device's maximum array
/// layers, which no target this engine runs on reports above a few thousand, so
/// sixteen bits is not a limit anybody reaches — and it is the same trick the
/// vertex stream is built out of, with the same kind of unpack beside it in the
/// shader.
pub const MATERIAL_STRIDE: usize = 64;

/// The largest page layer index [`GpuMaterial`] can carry, since the four
/// indices ride sixteen bits each — see [`MATERIAL_STRIDE`].
///
/// [`GpuMaterial::to_bytes`] saturates at this rather than truncating: a
/// truncated index names some *other* layer and shades a surface with a texture
/// nobody chose, where a saturated one names a layer the page almost certainly
/// does not have and is refused by
/// [`ForwardRenderer::with_scene`](https://docs.rs/crcbl-render), which checks
/// every row against the page's length.
pub const MAX_PAGE_LAYER: u32 = u16::MAX as u32;

/// Bytes in one draw's constant block.
///
/// One `uint` and three more of padding. `std140` requires a uniform block's
/// size to be a multiple of 16, and the padding is in the shader struct rather
/// than implied so that both sides write the same number — see
/// `DrawConstants` in `shaders/mesh.slang`.
pub const DRAW_CONSTANTS_SIZE: usize = 16;

/// One vertex in the two-stream layout `docs/plan/43-render-standards.md` §2
/// decided on 2026-08-30.
///
/// # Two streams, one record
///
/// A caller describes geometry as an array of these, [`VERTEX_STRIDE`] bytes
/// each — [`to_bytes`](Self::to_bytes), which is the position first and the
/// attributes after it. What reaches the device is *not* that array:
/// [`MeshPool`](https://docs.rs/crcbl-render) splits it, so the pool's buffer
/// is [`POSITION_STRIDE`] bytes per vertex of positions followed by
/// [`ATTRIBUTE_STRIDE`] bytes per vertex of attributes, and a pass that wants
/// geometry alone reads a contiguous run of [`POSITION_STRIDE`] instead of
/// striding over [`VERTEX_STRIDE`] and discarding most of every fetch. That is
/// the whole point of the split; the depth-only entry point that spends it is a
/// later rung.
///
/// **One buffer, two regions, and that is forced.** The obvious spelling is two
/// storage buffers, one bound per stream. The raster path's bind group layout
/// already binds [`PORTABLE_STORAGE_BUFFERS_PER_STAGE`] storage buffers in its
/// vertex stage — every one a WebGPU device guarantees — so a ninth is a
/// renderer that cannot be built in a browser. Two regions of one binding costs
/// nothing a separate buffer would have saved: the memory system sees the same
/// two contiguous runs either way.
///
/// [`PORTABLE_STORAGE_BUFFERS_PER_STAGE`]: https://docs.rs/crcbl-hal
///
/// # Every attribute is quantised, and [`crate::vertex`] is the arithmetic
///
/// The normal and the tangent are one [`QTangent`], each UV pair is `unorm16x2`
/// over the mesh's own [`UvRange`], and the colour is `rgba8`. Nothing here
/// re-derives any of that: [`from_normal`](Self::from_normal) is what every
/// constructor in the tree calls, and it calls that module.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    /// Object-space position — the whole of stream 0.
    pub position: [f32; 3],
    /// The tangent frame as a `snorm16x4` quaternion, handedness in the sign of
    /// `w`. Replaces the normal *and* the tangent.
    pub qtangent: QTangent,
    /// Base-colour texture coordinates, quantised onto the mesh's
    /// [`GpuMesh::uv_range`].
    pub uv0: [u16; 2],
    /// The second UV set's lanes, on the same range.
    ///
    /// **Nothing fills them and nothing reads them yet.** `crcbl_scene`'s
    /// importer reads `TEXCOORD_0` alone and reports a material sampling
    /// `TEXCOORD_1` as skipped, so every constructor in this workspace writes
    /// zero here and no shader declares a second coordinate. The lanes are in
    /// the layout because §2 decided the layout once and every golden in the
    /// tree re-blesses when it changes; they are not a working second UV set,
    /// and the importer that fills them is `docs/backlog.md`'s.
    pub uv1: [u16; 2],
    /// Linear RGBA albedo as `rgba8`.
    pub color: [u8; 4],
}

impl MeshVertex {
    /// A vertex whose frame is [`orthonormal_basis`]' stand-in for `normal` —
    /// what a mesh that ships no tangent gets.
    ///
    /// The engine's own meshes take this arm — greybox faces, the demo scenes,
    /// every procedural quad — because they are authored as positions, normals
    /// and UVs and have no tangent to carry. A primitive whose glTF accessor
    /// list holds `TANGENT` goes through [`from_frame`](Self::from_frame)
    /// instead, with that accessor's `w` deciding the handedness; the importer
    /// marks the difference on the mesh with
    /// [`GpuMesh::MESH_AUTHORED_TANGENTS`], and a mesh without it takes the
    /// fragment stage's screen-space frame rather than the stand-in encoded
    /// here.
    ///
    /// `range` is the mesh's, not this vertex's: a UV lane means nothing
    /// without the scale and offset it was quantised against, and that pair
    /// rides in [`GpuMesh::uv_range`] for the whole mesh. Compute it with
    /// [`UvRange::from_uvs`] over every coordinate the mesh carries, before
    /// building any vertex.
    ///
    /// [`orthonormal_basis`]: crate::vertex::orthonormal_basis
    #[must_use]
    pub fn from_normal(
        position: [f32; 3],
        normal: [f32; 3],
        color: [f32; 4],
        uv: [f32; 2],
        range: &UvRange,
    ) -> Self {
        let (tangent, bitangent) = crate::vertex::orthonormal_basis(normal);
        Self::from_frame(
            position,
            crate::vertex::TangentFrame {
                tangent,
                bitangent,
                normal,
            },
            color,
            uv,
            range,
        )
    }

    /// The same vertex from a frame the caller already has.
    #[must_use]
    pub fn from_frame(
        position: [f32; 3],
        frame: crate::vertex::TangentFrame,
        color: [f32; 4],
        uv: [f32; 2],
        range: &UvRange,
    ) -> Self {
        Self {
            position,
            qtangent: QTangent::encode(frame),
            uv0: range.encode(uv),
            uv1: [0; 2],
            color: crate::vertex::encode_rgba8(color),
        }
    }

    /// The record a description holds: [`position_bytes`](Self::position_bytes)
    /// then [`attribute_bytes`](Self::attribute_bytes).
    ///
    /// Little-endian throughout, which is what `std430` means on every target
    /// this engine has.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; VERTEX_STRIDE] {
        let mut bytes = [0u8; VERTEX_STRIDE];
        bytes[..POSITION_STRIDE].copy_from_slice(&self.position_bytes());
        bytes[POSITION_STRIDE..].copy_from_slice(&self.attribute_bytes());
        bytes
    }

    /// The inverse of [`to_bytes`](Self::to_bytes).
    #[must_use]
    pub fn from_bytes(bytes: &[u8; VERTEX_STRIDE]) -> Self {
        let word = |at: usize| {
            u32::from_le_bytes(
                bytes[at..at + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let halves = |at: usize| [word(at) as u16, (word(at) >> 16) as u16];
        let qtangent = [halves(POSITION_STRIDE), halves(POSITION_STRIDE + 4)];
        Self {
            position: [
                f32::from_bits(word(0)),
                f32::from_bits(word(4)),
                f32::from_bits(word(8)),
            ],
            qtangent: QTangent([
                qtangent[0][0] as i16,
                qtangent[0][1] as i16,
                qtangent[1][0] as i16,
                qtangent[1][1] as i16,
            ]),
            uv0: halves(POSITION_STRIDE + 8),
            uv1: halves(POSITION_STRIDE + 12),
            color: word(POSITION_STRIDE + 16).to_le_bytes(),
        }
    }

    /// This vertex's [`POSITION_STRIDE`] bytes of stream 0.
    #[must_use]
    pub fn position_bytes(&self) -> [u8; POSITION_STRIDE] {
        let mut bytes = [0u8; POSITION_STRIDE];
        for (lane, value) in self.position.iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// This vertex's [`ATTRIBUTE_STRIDE`] bytes of stream 1, as the `uint`s the
    /// shaders unpack.
    ///
    /// Each pair of sixteen-bit lanes rides in one word, low lane first — which
    /// is what a little-endian `uint` load and a `snorm16x2`/`unorm16x2` vertex
    /// fetch both mean by "first".
    #[must_use]
    pub fn attribute_bytes(&self) -> [u8; ATTRIBUTE_STRIDE] {
        let pair = |low: u16, high: u16| u32::from(low) | (u32::from(high) << 16);
        let lanes = self.qtangent.0;
        let words = [
            pair(lanes[0] as u16, lanes[1] as u16),
            pair(lanes[2] as u16, lanes[3] as u16),
            pair(self.uv0[0], self.uv0[1]),
            pair(self.uv1[0], self.uv1[1]),
            u32::from_le_bytes(self.color),
        ];
        let mut bytes = [0u8; ATTRIBUTE_STRIDE];
        for (index, word) in words.iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

/// A run of vertices as the bytes a description carries — [`VERTEX_STRIDE`]
/// each, in order.
#[must_use]
pub fn vertex_bytes(vertices: &[MeshVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_STRIDE);
    for vertex in vertices {
        bytes.extend_from_slice(&vertex.to_bytes());
    }
    bytes
}

/// The frame's non-geometry inputs, matching `struct FrameUniforms` in
/// `shaders/mesh.slang`.
///
/// Every matrix is stored the way [`glam::Mat4::to_cols_array`] produces it;
/// see that shader's header for why no transpose is needed on the way in.
///
/// [`glam::Mat4::to_cols_array`]: https://docs.rs/glam
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameUniforms {
    /// World → clip, column-major. Reversed-Z: `1.0` at the near plane, `0.0`
    /// at infinity.
    pub view_proj: [f32; 16],
    /// World-space eye position in `xyz`.
    pub camera_position: [f32; 4],
    /// Flat ambient term in `rgb`, and the normals view's switch in `w`.
    ///
    /// **Not a light and not a row.** It stands in for the bounces the direct
    /// terms do not carry, so it has no position, no froxel and no shadow. The
    /// sun sat beside it here as a direction and a colour until
    /// `docs/plan/18-render-features.md`'s light list existed; it is
    /// [`GpuLight`](crate::light::GpuLight) row now, and the shader has no
    /// special case for it.
    ///
    /// `w` is [`NORMALS_VIEW_OFF`](Self::NORMALS_VIEW_OFF) or
    /// [`NORMALS_VIEW_ON`](Self::NORMALS_VIEW_ON), and those two constants carry
    /// the account of why the switch rides in this lane rather than in one of its
    /// own.
    pub ambient: [f32; 4],
    /// World → cascade `i`'s shadow clip, column-major, one per cascade.
    ///
    /// Read by the **fragment** stage, to put a shaded point back into the
    /// shadow map's space. The shadow pass itself does not read it: it binds a
    /// second copy of this whole block whose [`view_proj`](Self::view_proj) *is*
    /// the cascade matrix, so the depth-only pipeline runs the same vertex and
    /// mesh stages as the colour pass rather than a second transform path that
    /// could disagree with it.
    pub shadow_view_proj: [[f32; 16]; SHADOW_CASCADES],
    /// Component `i` is how far from the eye cascade `i` reaches, in world
    /// units. Components past [`SHADOW_CASCADES`] are unread.
    pub cascade_far: [f32; 4],
    /// `x`, `y`: one shadow-atlas texel in `u` and in `v`. `z`: the constant
    /// depth bias the **cascades** compare with. `w`: the normal-offset
    /// coefficient beside it — how far along its own geometric normal a
    /// receiver is moved before the lookup, per unit of `sin(acos(Ng·L))`.
    ///
    /// Both texel sizes are carried even where the grid is square and they are
    /// equal: the shader's kernel steps in tile space and scales back by
    /// [`SHADOW_ATLAS_COLUMNS`] and [`SHADOW_ATLAS_ROWS`], and the grid stops
    /// being square the moment a point light's six tiles arrive.
    ///
    /// The two are the sun's alone. A spot's map is a perspective projection
    /// whose depth precision is distributed nothing like a cascade's, so it
    /// carries its own pair as shader constants — see `punctual_visibility` in
    /// `shaders/mesh.slang`, which is where that is argued.
    pub shadow_params: [f32; 4],
    /// The froxel grid's extent: `x` froxels across the screen, `y` down it, `z`
    /// depth slices, `w` unread padding `std140` aligns a vector to sixteen
    /// bytes with.
    ///
    /// Everything else about the split — the tile size, the slice count for a
    /// perspective camera, the depth range and the exponential distribution over
    /// it — is a constant [`crate::light`] and the two shaders each declare, so
    /// there is nothing here for them to disagree about. `z` is
    /// [`CLUSTER_DEPTH_SLICES`](crate::light::CLUSTER_DEPTH_SLICES) for a
    /// perspective camera and `1` for an orthographic one, which has no view
    /// depth to slice by.
    ///
    /// This slot held `docs/plan/25-lod.md`'s selection numbers until the light
    /// list arrived, and **no shader had read them since hysteresis landed**:
    /// the projection moved into `draw_gen.slang`, which owns the state it
    /// needs, and `mesh_cluster.slang`'s amplification stage reads that pass's
    /// answer rather than the numbers. A dead vector in a block every pipeline
    /// binds is not somewhere to leave a value nothing reads.
    pub cluster_grid: [u32; 4],
    /// World → **light tile** `i`'s shadow clip, column-major, one per tile of
    /// the atlas's light region.
    ///
    /// Perspective and reversed-Z, unlike [`shadow_view_proj`](Self::shadow_view_proj)
    /// above: a spot is a cone and a cone is a frustum, and a point light's face
    /// is a 90° one.
    /// `crcbl_render::shadow::spot_matrix` and `point_matrix` build them and
    /// `crcbl_render::shadow::Selection` decides whose they are; a tile no light
    /// holds carries whatever was last written there and is read by nothing,
    /// because the rows that would name it carry
    /// [`NO_SHADOW_TILE`](crate::light::NO_SHADOW_TILE).
    ///
    /// **Indexed by tile rather than by light**, which is what lets one light own
    /// six of them: [`GpuLight::shadow_tile`](crate::light::GpuLight::shadow_tile)
    /// carries the *first* of a light's tiles and a point light's faces are the
    /// [`SHADOW_POINT_FACES`] entries from there.
    ///
    /// **Last in the block rather than beside the cascades**, so adding it moved
    /// no existing member's offset — which is what let the cascade goldens stay
    /// byte-identical across the change that introduced it.
    pub light_view_proj: [[f32; 16]; SHADOW_LIGHT_TILES],
    /// `docs/plan/18-render-features.md`'s irradiance grid: where the probes
    /// are, how far apart, and how many.
    ///
    /// The rows themselves are a storage buffer of
    /// [`GpuProbe`](crate::probe::GpuProbe) — only the header rides here, on
    /// [`cluster_grid`](Self::cluster_grid)'s terms, because a fragment needs it
    /// before it knows which row to fetch.
    ///
    /// **[`ProbeVolume::default`] is a grid of nothing**, and the whole feature
    /// is additive: a scene with no probes evaluates to exactly zero and the
    /// shader has no branch for it. See [`crate::probe`].
    ///
    /// **Last in the block**, for [`light_view_proj`](Self::light_view_proj)'s
    /// reason exactly: no existing member's offset moves, so every golden
    /// blessed before this member existed still matches.
    ///
    /// [`ProbeVolume::default`]: crate::probe::ProbeVolume::default
    pub probes: ProbeVolume,
    /// `docs/plan/25-lod.md`'s selection numbers, the same four lanes
    /// [`crate::draw_gen::Params::lod_params`] carries: `x` is how many pixels
    /// one unit of length subtends one unit from the eye, `y` is the budget an
    /// unexpanded group's projected error must exceed to expand, `z` is the
    /// budget an expanded one is held down to before it collapses again, and `w`
    /// is unread padding.
    ///
    /// **Written for the screen-error heatmap and read by that alone.**
    /// `mesh_cluster.slang`'s mesh stage projects a cluster's producing group's
    /// error itself under [`HEATMAP_VIEW_ON`](Self::HEATMAP_VIEW_ON) and anchors
    /// the ramp on the two budgets; `mesh.slang` declares the row and never
    /// reads it, because the two files are one buffer.
    ///
    /// This is the *same* pair `draw_gen.slang` judged the cut against, not a
    /// second derivation: `crcbl_render::ForwardRenderer` computes them once a
    /// frame and writes them into both blocks. An overlay drawn against numbers
    /// the selection did not use is one nobody can hold against the picture.
    ///
    /// **Last in the block**, for [`probes`](Self::probes)' reason exactly: no
    /// existing member's offset moves, so every golden blessed before this
    /// member existed still matches.
    pub lod_params: [f32; 4],
    /// Exponential height fog: `x` the density at the reference height, per
    /// world unit of travel; `y` the height over which that density falls by a
    /// factor of `e`; `z` the reference height itself, in world units; `w`
    /// unread padding.
    ///
    /// **Zero density is exactly no fog**, not nearly none. The optical depth
    /// is then exactly zero, [`crate::fog::transmittance`] of zero is exactly
    /// one, and `mesh.slang` composites as `lit * t + fog * (1 - t)` rather
    /// than as a `lerp` — which at `t = 1` is `lit + (fog - lit)` and is *not*
    /// `lit` for an HDR value far from the fog colour. So the feature is data
    /// with no branch selecting it, on
    /// [`probes`](Self::probes)' terms, and every golden blessed before fog
    /// existed still matches.
    ///
    /// `crcbl_render::ForwardRenderer::set_fog` is what writes it, and
    /// [`crate::fog`] is the arithmetic both sides run.
    ///
    /// **Last in the block**, on [`lod_params`](Self::lod_params)' terms.
    pub fog_params: [f32; 4],
    /// The radiance fog scatters towards the eye, in `rgb`; `w` unread padding.
    ///
    /// Pre-tonemap and unclamped, like everything else this pass produces: it
    /// is a radiance and not a display colour, so a bright sky's fog is allowed
    /// to be above one and the bloom chain is allowed to see it.
    ///
    /// Read only through [`fog_params`](Self::fog_params)'s density, so its
    /// value is unobservable while that density is zero.
    pub fog_color: [f32; 4],
    /// The red channel of the sky's L1 irradiance, as
    /// [`GpuProbe::sh_r`](crate::probe::GpuProbe::sh_r) packs one: the linear
    /// band in `xyz` and the constant band in `w`, so the shader evaluates it
    /// as `dot(row, float4(N, 1))` with no shuffle.
    ///
    /// A distant environment reaches a diffuse surface in exactly this shape,
    /// which is why the sky and the irradiance grid share it — `mesh.slang`
    /// adds the two, rather than choosing between them.
    /// [`crate::sky::SkyGradient::irradiance`] is what produces the three rows
    /// and `crcbl_render::ForwardRenderer::set_sky` is what writes them.
    ///
    /// **Zero is exactly off**, on [`fog_params`](Self::fog_params)' terms: the
    /// three dot products are zero, so the sum the ambient term starts from is
    /// the one it was before a sky existed, bit for bit.
    ///
    /// **Last in the block**, on [`lod_params`](Self::lod_params)' terms.
    pub sky_sh_r: [f32; 4],
    /// The green channel of the same, on [`sky_sh_r`](Self::sky_sh_r)'s terms.
    pub sky_sh_g: [f32; 4],
    /// The blue channel of the same, on [`sky_sh_r`](Self::sky_sh_r)'s terms.
    pub sky_sh_b: [f32; 4],
    /// World → clip **as the previous frame's camera saw it**, column-major:
    /// the camera-side twin of
    /// [`GpuInstance::previous_transform`](GpuInstance::previous_transform).
    ///
    /// A rigid body's motion vector is made of exactly two things — where the
    /// object went and where the viewer went — and this is the second.
    /// `mesh.slang`'s `motion_vector` is what subtracts them, and its docs carry
    /// the convention the result is in.
    ///
    /// **`crcbl_render::ForwardRenderer` owns the advance**, once per frame at
    /// the boundary `crcbl_render::InstancePool::rotate` settles a transform's
    /// previous value on. The first frame carries its own
    /// [`view_proj`](Self::view_proj): a camera that has not moved yet has not
    /// moved, and a zero matrix here would put every pixel of the first frame in
    /// motion.
    ///
    /// **Last in the block**, on [`lod_params`](Self::lod_params)' terms.
    pub previous_view_proj: [f32; 16],
    /// Where the vertex pool's **attribute** region begins, as a `uint` index
    /// into the pool read as words; `yzw` unread padding `std140` aligns a
    /// vector to sixteen bytes with.
    ///
    /// `docs/plan/43-render-standards.md` §2's two streams live in one buffer:
    /// the first [`POSITION_STRIDE`] bytes per vertex of positions, then
    /// [`ATTRIBUTE_STRIDE`] bytes per vertex of attributes. A vertex `v`'s
    /// position is at word `3 v` and its attributes at word `x + 5 v`, which
    /// is the only thing a shader cannot derive for itself — the boundary is
    /// the pool's capacity, and a shader has never been told that.
    ///
    /// **One number for the whole pool, not one per mesh**, and that is what
    /// makes it right for a skinned instance too: `GpuInstance::base_vertex`
    /// overrides which vertices a draw reads, and both regions are addressed
    /// off the same override with no second field to keep in step.
    ///
    /// [`MeshPool::attribute_base`](https://docs.rs/crcbl-render) is what
    /// computes it, and the skinning dispatch is handed the same number through
    /// [`skinning::Params::attribute_base`](crate::skinning::Params::attribute_base)
    /// because that pass binds no frame block.
    ///
    /// **Last in the block**, on [`lod_params`](Self::lod_params)' terms: no
    /// existing member's offset moves.
    pub vertex_pool: [u32; 4],
    /// Where atlas slot `i`'s map is in the shadow atlas: a scale into the
    /// image in `xy` and an offset in `zw`, so a point `t` of that map's own
    /// `0..1` space is at `zw + t * xy`.
    ///
    /// **This is what replaced a tile index into a fixed grid**, which is
    /// `docs/plan/45-shadows.md`'s atlas rung: a slot's map used to be a cell of
    /// an [`SHADOW_ATLAS_COLUMNS`] by [`SHADOW_ATLAS_ROWS`] grid the shader
    /// derived from the index, so every map was one size and could only be that
    /// size. Reading the rectangle instead lets a far or small light take a
    /// halving of a cell with no second sampling path anywhere —
    /// `mesh.slang`'s `atlas_uv` and `atlas_step` are the whole of the reading
    /// side.
    ///
    /// Indexed by **slot** rather than by light, on
    /// [`light_view_proj`](Self::light_view_proj)'s terms exactly: the cascades
    /// are the first [`SHADOW_CASCADES`] rows and a light's tile index plus
    /// `SHADOW_CASCADES` is its own. `crcbl_render::shadow::Selection` fills
    /// them out of `crcbl_render::shadow::AtlasAllocator`.
    ///
    /// A slot no map was rendered into carries a **zero** rectangle, which
    /// nothing reads for `light_view_proj`'s reason — the rows that could name
    /// one carry [`NO_SHADOW_TILE`](crate::light::NO_SHADOW_TILE) — and which
    /// says "empty" plainly in a block dumped for debugging.
    ///
    /// **Last in the block**, on [`lod_params`](Self::lod_params)' terms.
    pub shadow_atlas_rect: [[f32; 4]; SHADOW_ATLAS_TILES],
}

impl FrameUniforms {
    /// [`ambient`](Self::ambient)`.w` for the shaded picture — the ordinary
    /// frame, and what every writer of this block carried before the normals
    /// view existed.
    ///
    /// **Zero is why the switch is in this lane.** `mesh.slang`'s fragment stage
    /// needed one spare scalar in a block that is already laid out, and the
    /// alternatives each cost something this does not:
    ///
    /// * [`camera_position`](Self::camera_position)`.w` is the other lane the
    ///   shader documents as unused, and every writer sets it to `1.0` — so a
    ///   switch there would have turned the debug view on for every frame the
    ///   engine has ever drawn.
    /// * A `float4` of its own at the end of the block moves no existing
    ///   member's offset, which is the precedent
    ///   [`light_view_proj`](Self::light_view_proj) and [`probes`](Self::probes)
    ///   set — but this struct is also built field-by-field, with no `..default`
    ///   spread, by `crates/crcbl/tests/forward_e2e/depth_probe.rs`, and a new
    ///   member stops that compiling.
    ///
    /// A lane every writer already leaves at exactly zero is the one change that
    /// costs nothing at either end.
    pub const NORMALS_VIEW_OFF: f32 = 0.0;

    /// [`ambient`](Self::ambient)`.w` for the normals view: `mesh.slang`'s
    /// fragment stage writes the **world-space** surface normal as
    /// `n * 0.5 + 0.5` instead of shading, so +X reads red, +Y green and +Z blue
    /// and an inverted face reads the complement of the colour it should have.
    ///
    /// The shader compares against half way between this and
    /// [`NORMALS_VIEW_OFF`](Self::NORMALS_VIEW_OFF) rather than for equality —
    /// see its `NORMALS_VIEW` — because a float lane is no place for an
    /// `==`. `crcbl_render::ForwardRenderer::set_normals_view` is what writes it.
    pub const NORMALS_VIEW_ON: f32 = 1.0;

    /// [`ambient`](Self::ambient)`.w` for the LOD view: `mesh_cluster.slang`'s
    /// mesh stage replaces each cluster's vertex colour with a hue chosen by the
    /// DAG level that cluster was decimated to, and `mesh.slang`'s fragment
    /// stage passes it through unshaded.
    ///
    /// **Two sentinels in one lane**, so the shader tests this threshold before
    /// the normals one — a `2.0` clears both. The two indirect paths have no
    /// per-cluster level at all and draw one flat grey, which is the difference
    /// this view exists to show: cluster LOD selects across a single mesh and
    /// per-instance LOD cannot.
    ///
    /// `crcbl_render::ForwardRenderer::set_lod_view` is what writes it.
    pub const LOD_VIEW_ON: f32 = 2.0;

    /// [`ambient`](Self::ambient)`.w` for the **screen-error heatmap**, the LOD
    /// tint's sibling: `mesh_cluster.slang`'s mesh stage replaces each cluster's
    /// vertex colour with a ramp position taken from the projected error the LOD
    /// selection judged that cluster's producing group on, and `mesh.slang`'s
    /// fragment stage passes it through unshaded exactly as it does the tint.
    ///
    /// **Three sentinels in one lane now**, and the shader tests them outermost
    /// first — a `3.0` clears all three thresholds, so the heatmap's has to win.
    /// `the_heatmap_view_threshold_lies_above_the_lod_view` is what holds the
    /// interleaving.
    ///
    /// The tint says which level a patch came from; this says how close that
    /// patch is to the budget that would change it. Both are mesh-path only, for
    /// the same reason: the two indirect paths select one level per *instance*
    /// and have no per-cluster number of either kind — `mesh.slang`'s vertex
    /// stage writes one flat grey and the difference is the comparison.
    ///
    /// `crcbl_render::ForwardRenderer::set_heatmap` is what writes it.
    pub const HEATMAP_VIEW_ON: f32 = 3.0;

    /// [`ambient`](Self::ambient)`.w` for the **occlusion view**: `mesh.slang`'s
    /// fragment stage draws the ambient-occlusion channel alone as grey instead
    /// of shading, one white and fully occluded black.
    ///
    /// **Four sentinels in one lane now**, and this is the outermost — a `4.0`
    /// clears every threshold below it, so the shader has to test this one
    /// first. `the_occlusion_view_threshold_lies_above_the_heatmap` holds the
    /// interleaving.
    ///
    /// Unlike the tint and the heatmap this is a **fragment**-stage view and
    /// costs the geometry stages nothing to produce, so it works on every
    /// `GeometryPath` rather than on the mesh-shader one alone. What it draws on
    /// a frame without `RenderEffects::AMBIENT_OCCLUSION` is white, because that
    /// is the 1×1 image the renderer binds in place of a computed channel — the
    /// shader's own branch says why that is the honest answer rather than a gap.
    ///
    /// `crcbl_render::ForwardRenderer::set_occlusion_view` is what writes it.
    pub const OCCLUSION_VIEW_ON: f32 = 4.0;

    /// [`ambient`](Self::ambient)`.w` for the **motion view**: `mesh.slang`'s
    /// fragment stage draws the motion vector it is about to write into its
    /// third target, stretched by [`MOTION_VIEW_SCALE`] and centred on grey,
    /// instead of shading.
    ///
    /// **The outermost sentinel now**, and this is the one the shader has to
    /// test first — it clears every threshold below it.
    /// `the_motion_view_threshold_lies_above_the_occlusion_view` holds the
    /// interleaving.
    ///
    /// A **fragment**-stage view like the occlusion one, so it draws on every
    /// `GeometryPath` rather than on the mesh-shader path alone: both geometry
    /// stages emit the two clip positions the subtraction needs.
    ///
    /// **It is the motion-vector target's only observer today.** Nothing reads
    /// the attachment yet — `docs/plan/49-antialiasing.md`'s TAA is the first
    /// pass that will — so this view is what says the subtraction is the right
    /// one, and `crates/crcbl/tests/mesh_e2e/motion.rs` is what reads it.
    ///
    /// `crcbl_render::ForwardRenderer::set_motion_view` is what writes it.
    pub const MOTION_VIEW_ON: f32 = 5.0;

    /// The bytes a uniform buffer holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; FRAME_UNIFORMS_SIZE] {
        // A function rather than a closure capturing the cursor, because the
        // integer member below is written through the *same* cursor and a
        // closure holding it would not let anything else touch it. One writer,
        // one order, and a member that moved shows up as a `debug_assert` rather
        // than as a silently shifted offset.
        fn put(bytes: &mut [u8], at: &mut usize, values: &[f32]) {
            for value in values {
                bytes[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
                *at += 4;
            }
        }

        let mut bytes = [0u8; FRAME_UNIFORMS_SIZE];
        let mut at = 0usize;
        put(&mut bytes, &mut at, &self.view_proj);
        put(&mut bytes, &mut at, &self.camera_position);
        put(&mut bytes, &mut at, &self.ambient);
        for matrix in &self.shadow_view_proj {
            put(&mut bytes, &mut at, matrix);
        }
        put(&mut bytes, &mut at, &self.cascade_far);
        put(&mut bytes, &mut at, &self.shadow_params);
        // The one integer member, and it is written through the same running
        // cursor rather than at a computed offset: `std140` puts a `uint4`
        // exactly where a `float4` would go, and a second writer here is a
        // second place for the layout to drift.
        for value in self.cluster_grid {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for matrix in &self.light_view_proj {
            put(&mut bytes, &mut at, matrix);
        }
        // The grid header, written by the type that owns its layout rather than
        // unpacked here: it is three `std140` rows with two padding lanes in
        // them, and a second spelling of that is a second place for it to drift.
        bytes[at..at + PROBE_VOLUME_SIZE].copy_from_slice(&self.probes.to_bytes());
        at += PROBE_VOLUME_SIZE;
        put(&mut bytes, &mut at, &self.lod_params);
        put(&mut bytes, &mut at, &self.fog_params);
        put(&mut bytes, &mut at, &self.fog_color);
        put(&mut bytes, &mut at, &self.sky_sh_r);
        put(&mut bytes, &mut at, &self.sky_sh_g);
        put(&mut bytes, &mut at, &self.sky_sh_b);
        put(&mut bytes, &mut at, &self.previous_view_proj);
        for value in self.vertex_pool {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for rect in &self.shadow_atlas_rect {
            put(&mut bytes, &mut at, rect);
        }
        debug_assert_eq!(at, FRAME_UNIFORMS_SIZE);
        bytes
    }
}

/// How far the motion view stretches a motion vector before centring it on
/// grey, and `static const float MOTION_VIEW_SCALE` in `mesh.slang`.
///
/// Declared on both sides because a test reading the encoded picture has to
/// undo the encoding, and the shader is where it is applied. A motion of
/// `0.5 / MOTION_VIEW_SCALE` of the frame in one frame lands on the end of the
/// ramp; the `Rgba16Float` scene target carries anything past that unclamped.
/// `the_motion_view_scale_is_the_one_the_host_declares` holds the two equal.
///
/// **A pixel at rest encodes to one half in both channels, give or take one
/// half-float step**: the subtraction behind it is two multiplication chains
/// over equal inputs, not one, so it lands a last bit either side of zero and
/// the colour export rounds that into the step below one half. The shader's
/// copy carries the account, and `crates/crcbl/tests/mesh_e2e/motion.rs`'s
/// `REST_TOLERANCE` is that step measured.
pub const MOTION_VIEW_SCALE: f32 = 8.0;

/// One drawable object, matching `struct GpuInstance` in `shaders/mesh.slang`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's instance record: "transform,
/// mesh id, material id, flags", plus the sector id its 2026-07-27 correction
/// adds. [`crcbl_render::InstancePool`] is what writes these, one storage buffer
/// element per instance, by delta upload.
///
/// # One of the five fields is reserved, and that is deliberate
///
/// [`GpuInstance::transform`] and [`GpuInstance::mesh`] are read by the vertex
/// stage, [`GpuInstance::material`] by the fragment stage the vertex stage
/// hands it to, and [`GpuInstance::flags`] by the cull pass. [`GpuInstance::sector`] is here
/// because **changing this layout after a shader, a cull pass and a draw
/// generator all index it is the expensive path**, and adding a field is the
/// cheap one now. Its own docs say which slice consumes it; it is not working
/// camera-relative rendering and should not be read as evidence that one
/// exists.
///
/// The material id was the other one until 2026-08, and what moved it was the
/// table it names: see [`GpuMaterial`], which is §3.2's factors *and* the
/// base-colour texture layer they multiply.
///
/// [`crcbl_render::InstancePool`]: https://docs.rs/crcbl-render
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuInstance {
    /// Model → **sector**, column-major, the way [`glam::Mat4::to_cols_array`]
    /// produces it.
    ///
    /// **Any affine transform**: rotation, translation and scale, non-uniform
    /// scale included. There is no rigidity obligation on a caller, because the
    /// mesh shaders take a normal through this matrix's *cofactor* matrix
    /// rather than through its 3×3 part — `normal_basis`, declared in both
    /// `mesh.slang` and `mesh_cluster.slang` and held to one spelling by
    /// `every_shader_that_transforms_a_normal_builds_it_with_one_function` below.
    /// So a scaled instance shades with a normal that is still perpendicular to
    /// its surface.
    ///
    /// This field used to *require* rigidity, on the grounds that the shader
    /// transformed normals with the bare 3×3. Nothing enforced it and the
    /// engine's own scenes broke it — `crcbl::screenshot`'s `Scene::Ao` trough
    /// and `Scene::Probes` room are each an open box under a non-uniform scale
    /// — so the shader learned the transform a normal actually needs instead.
    ///
    /// Sector-local and `f32` rather than world-space and `f64` because that is
    /// what makes delta upload survive camera motion: an object that does not
    /// move has a transform that does not change, whatever the camera does.
    /// See [`GpuInstance::sector`] for the half of that which is not built.
    ///
    /// [`glam::Mat4::to_cols_array`]: https://docs.rs/glam
    pub transform: [f32; 16],
    /// Where this instance was **last frame**: [`transform`] one frame behind,
    /// so a fragment can say where it came from.
    ///
    /// [`InstancePool`](https://docs.rs/crcbl-render) owns this the way it owns
    /// [`GpuInstance::LIVE`] — it writes the value whatever a caller passed,
    /// because the pool is what already holds the record's previous contents
    /// and a caller keeping its own copy is a copy to drift. On the frame an
    /// instance moves it holds the transform the instance moved *from*; on
    /// every other frame it equals [`transform`], which is zero motion, and
    /// that includes the frame the instance is created because a spawn did not
    /// travel from anywhere.
    ///
    /// **Nothing reads it yet, and that is the point of it being here.**
    /// `docs/plan/43-render-standards.md` §9: temporal antialiasing, temporal
    /// reflections, temporal upscaling, per-object motion blur and screen-space
    /// global illumination all want a motion vector, and a motion vector wants
    /// this and a target to write itself into that no pass here has. Reserving
    /// the slot is smaller than any one of those and unblocks all of them,
    /// where widening the record later moves every offset below it in four
    /// shader copies and re-blesses every golden.
    ///
    /// [`transform`]: GpuInstance::transform
    pub previous_transform: [f32; 16],
    /// Which mesh to draw: an index into the mesh table, whose entries are
    /// [`GpuMesh`].
    ///
    /// The vertex stage resolves this instance's base vertex through it, so an
    /// instance's geometry is decided by data the GPU reads rather than by the
    /// draw call — which is what lets one draw cover instances of different
    /// meshes, and what §3.3's cull pass needs in order to emit draws at all.
    ///
    /// [`MeshPool`](https://docs.rs/crcbl-render) is what hands these out and
    /// what keeps the table in step with them; an id whose mesh has been freed
    /// resolves to an entry that is all zeroes, which is the empty range rather
    /// than another mesh's.
    pub mesh: u32,
    /// Which material to shade with: an index into the material table, whose
    /// rows are [`GpuMaterial`].
    ///
    /// The vertex stage passes it to the fragment stage as a flat varying, and
    /// the fragment stage multiplies that row's [`GpuMaterial::base_color`]
    /// into the interpolated albedo, so two instances of the same mesh
    /// differing only here are two colours in one draw — which is the whole of
    /// what an id that indexes a table buys, and what nothing could observe
    /// while this was reserved.
    ///
    /// [`MaterialTable`](https://docs.rs/crcbl-render) is what hands these out.
    /// A row nothing has written is all zeroes, which is a **black** material
    /// rather than a harmless one: unlike a mesh id, whose zero entry is the
    /// empty range and draws nothing, there is no material value that means
    /// "no material", so every instance names one it was given.
    pub material: u32,
    /// Which sector [`GpuInstance::transform`] is relative to.
    ///
    /// **Reserved and unconsumed — this is not camera-relative rendering.** The
    /// 2026-07-27 correction's other half is a per-frame f64 sector→camera
    /// offset table that the vertex and cull shaders add, and that table does
    /// not exist. Until it does, every instance is in sector 0 and `transform`
    /// is a plain model → world matrix. The field is here now because the
    /// format is cheap to extend today and expensive to extend once §3.3's
    /// shaders index it.
    pub sector: u32,
    /// Per-instance bits. [`GpuInstance::LIVE`] is the only one defined; the
    /// rest are reserved.
    ///
    /// A `u32` rather than a `bitflags` type, which is what
    /// [`crcbl_hal::Features`] would be the pattern to follow: this crate has
    /// **no dependencies at all**, deliberately — see its `Cargo.toml` — and
    /// `bitflags` would be the first. The field is the byte layout a shader
    /// reads either way, so the choice is about what the Rust side spells, and
    /// nothing here is willing to spend the crate's defining property on it.
    ///
    /// [`crcbl_hal::Features`]: https://docs.rs/crcbl-hal
    pub flags: u32,
    /// The pool vertex this instance's indices are relative to, read **instead
    /// of** the mesh table's when [`GpuInstance::BASE_VERTEX_OVERRIDE`] is set
    /// in [`GpuInstance::flags`] and ignored when it is not.
    ///
    /// What makes a deformed instance drawable: [`crate::skinning`]'s dispatch
    /// writes a skinned copy of a mesh's vertices into a run of the same pool,
    /// and this is how the instance drawn out of that run says where it is. The
    /// instance goes on naming its **source** mesh in [`GpuInstance::mesh`], so
    /// the bucket it is scattered into, the level tables it resolves through and
    /// the bounding box it is culled against are the source mesh's and need no
    /// entry of their own.
    ///
    /// **The bucket stays authoritative without the bit**, which is what a
    /// `Geometry::Dag` needs: its level is chosen per instance on the GPU and
    /// the base belongs to the *selected* level, not to the entry the instance
    /// names.
    pub base_vertex: u32,
    /// The pool vertex the **previous frame's** deformation of this instance
    /// was written into, read only when [`GpuInstance::BASE_VERTEX_OVERRIDE`]
    /// is set in [`GpuInstance::flags`] and ignored when it is not.
    ///
    /// What makes a skinned instance's motion vector its own. The geometry
    /// stages take a fragment's previous clip position from
    /// [`previous_transform`](GpuInstance::previous_transform) applied to the
    /// vertex at this base; without it they apply that matrix to the vertex at
    /// [`base_vertex`](GpuInstance::base_vertex), which is the pose *this*
    /// frame's dispatch wrote — so a limb that swung reports the motion of the
    /// body it hangs off and nothing else.
    ///
    /// **It is a pool vertex and not a region half**: [`crate::skinning`]'s
    /// output region is two runs a frame alternates between, and this is
    /// whichever of them the frame before this one filled.
    /// `crcbl_render::ForwardRenderer::point_skinned_instances` is what writes
    /// it, beside the current base, and a rigid instance leaves both at zero
    /// along with the bit.
    pub previous_base_vertex: u32,
}

impl GpuInstance {
    /// [`GpuInstance::flags`] bit 0: this element is a **live instance**.
    ///
    /// Clear means the slot holds a removed instance's leftovers, and
    /// `cull.slang` rejects it before it reads anything else in the record —
    /// which is what makes an array walked from element zero safe to walk.
    /// [`InstancePool`](https://docs.rs/crcbl-render) owns the bit: it sets it
    /// on every write and clears it on removal, so a caller neither has to
    /// remember to set it nor can accidentally clear it.
    ///
    /// **A zeroed record is therefore dead**, which is the direction that fails
    /// safely: a slot nothing has written is a slot nothing draws.
    /// [`GpuInstance::default`] has no bit set for that reason.
    pub const LIVE: u32 = 1 << 0;

    /// [`GpuInstance::flags`] bit 1: read this record's
    /// [`base_vertex`](GpuInstance::base_vertex) instead of the base vertex of
    /// the mesh entry the draw resolved.
    ///
    /// **A bit rather than a sentinel in the field**, because zero is a legal
    /// base vertex — it is the pool's first run — so no in-range value is free
    /// to mean "no override". A sentinel would have to be [`u32::MAX`], which is
    /// not what a zeroed element holds, and a zeroed element reading as "draw
    /// out of vertex 0" is the direction that fails towards drawing something.
    /// The bit keeps [`GpuInstance::default`] all zeroes, which is the property
    /// [`GpuInstance::LIVE`] rests on.
    ///
    /// **And a bit rather than spare bits of an existing field.** Packing the
    /// base into what is left of `flags` or `sector` would cap the vertex pool
    /// at whatever those bits address, silently, at a size nothing in the code
    /// names. The record is one `uint` wider instead, which costs a whole
    /// 16-byte lane — see [`INSTANCE_STRIDE`].
    pub const BASE_VERTEX_OVERRIDE: u32 = 1 << 1;

    /// The bytes one storage-buffer element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; INSTANCE_STRIDE] {
        let mut bytes = [0u8; INSTANCE_STRIDE];
        let mut at = 0usize;
        for value in self.transform.iter().chain(&self.previous_transform) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in [
            self.mesh,
            self.material,
            self.sector,
            self.flags,
            self.base_vertex,
            self.previous_base_vertex,
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        // Short of the stride, and deliberately: the lanes past the last field
        // are the padding `shaders/mesh.slang` declares, and they stay as this
        // buffer was initialised.
        debug_assert_eq!(INSTANCE_STRIDE - at, INSTANCE_PAD_WORDS * size_of::<u32>());
        bytes
    }

    /// The inverse of [`GpuInstance::to_bytes`].
    ///
    /// So a producer that keeps its instances packed the way the buffer holds
    /// them — which is what makes a dirty range one contiguous upload — can
    /// still read one back without keeping a second, drifting copy.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; INSTANCE_STRIDE]) -> Self {
        let float_at = |offset: usize| {
            f32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let uint_at = |offset: usize| {
            u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let mut transform = [0.0f32; 16];
        for (index, value) in transform.iter_mut().enumerate() {
            *value = float_at(index * 4);
        }
        let mut previous_transform = [0.0f32; 16];
        for (index, value) in previous_transform.iter_mut().enumerate() {
            *value = float_at(64 + index * 4);
        }
        Self {
            transform,
            previous_transform,
            mesh: uint_at(128),
            material: uint_at(132),
            sector: uint_at(136),
            flags: uint_at(140),
            base_vertex: uint_at(144),
            previous_base_vertex: uint_at(148),
        }
    }
}

/// One resident mesh's range in the geometry pools and its local-space bounds,
/// matching `struct GpuMesh` in `shaders/mesh.slang`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.1's three integers, in the buffer
/// the *GPU* resolves them out of: [`GpuInstance::mesh`] indexes an array of
/// these, and the vertex stage adds [`GpuMesh::base_vertex`] to every index it
/// reads. [`MeshPool`](https://docs.rs/crcbl-render) is what writes them.
///
/// [`GpuMesh::base_index`] and [`GpuMesh::index_count`] are read by nothing
/// today — the CPU still records the draws, and `draw_indexed` takes those two
/// numbers directly. They are in the record because §3.3's cull pass builds its
/// indirect draws out of exactly this range, and a table carrying only what the
/// vertex stage reads would have to change layout the day it does.
///
/// The bounds are the same §3.3's, and they live here rather than in a table of
/// their own because they share the range's lifetime exactly: written when a
/// mesh is suballocated, cleared when it is freed, in the same call. `cull.slang`
/// reads them — see [`crate::cull`] — and `mesh.slang` does not.
///
/// An entry naming no mesh is all zeroes, and [`GpuMesh::index_count`] is the
/// field that says so: a zero index count is a range with nothing to draw,
/// where a zero base vertex is an ordinary value the pool's first mesh has.
/// The all-zero bounds are a degenerate point box at the origin, which is
/// exactly why the cull pass rejects on the index count rather than on them.
///
/// `PartialEq` but **not** `Eq`, unlike the ids-only record it used to be: the
/// bounds are floats, and a type that claims total equality over an `f32` is
/// claiming something `NaN` makes untrue.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuMesh {
    /// The mesh's first vertex in the vertex pool. Added to every index a draw
    /// of it reads, which is what lets the pool store indices mesh-relative.
    pub base_vertex: u32,
    /// The mesh's first index in the index pool.
    pub base_index: u32,
    /// How many indices the mesh has; zero for an entry naming no mesh.
    pub index_count: u32,
    /// Lowest corner of the mesh's local-space axis-aligned bounding box.
    ///
    /// **Local space**: the box bounds the vertices as they sit in the pool,
    /// before any instance transform. One mesh drawn by a thousand instances
    /// has one of these, which is the whole reason it is a mesh-table field
    /// rather than an instance one.
    pub bounds_min: [f32; 3],
    /// Highest corner of the same box.
    pub bounds_max: [f32; 3],
    /// The scale and offset every [`MeshVertex::uv0`] lane of this mesh was
    /// quantised against, and the pair the shaders read a coordinate back
    /// through.
    ///
    /// **Per mesh, which is what buys the sixteen bits.** A mesh that tiles its
    /// texture forty times and one that does not would otherwise share a range
    /// forty times wider than either needs — see [`UvRange`], where that is
    /// argued in full.
    ///
    /// **A table field rather than a draw constant**, though
    /// `docs/plan/43-render-standards.md` §2 says "the draw constants": the row
    /// is what both geometry paths already fetch to resolve a mesh's vertices,
    /// and the mesh path fetches it through `instance.mesh` where the raster
    /// path fetches it through the bucket's — so a pair carried in either
    /// pass's own constant block would be a second thing to keep in step for
    /// no gain. The row *is* this mesh's constants.
    ///
    /// **Every level of a DAG carries the same range**, because the mesh path
    /// reads level 0's row while drawing a coarser level's vertices. The caller
    /// that supplies a DAG's levels supplies one range for all of them.
    pub uv_range: UvRange,
    /// Per-mesh bits the raster stages read. Today there is one:
    /// [`MESH_AUTHORED_TANGENTS`](Self::MESH_AUTHORED_TANGENTS).
    ///
    /// **Zero is the honest default**, which is what makes a widened row safe:
    /// a mesh nobody marked has no authored tangent frame, and the shading takes
    /// the screen-space derivative frame for it rather than trusting eight bytes
    /// that agree with no UV parameterisation. Every one of the engine's own
    /// meshes is in that position — see
    /// [`MeshVertex::from_normal`]'s stand-in — so an
    /// unwritten flags word describes them correctly.
    pub flags: u32,
}

impl GpuMesh {
    /// [`flags`](Self::flags) bit 0: this mesh's vertices carry a **real**
    /// tangent frame, one that agrees with its UV parameterisation.
    ///
    /// Set by whoever built the vertices out of a `TANGENT` accessor —
    /// `crcbl_scene::gltf_render` is the only such producer today — and clear
    /// for every mesh whose frame is
    /// [`orthonormal_basis`](crate::vertex::orthonormal_basis)' stand-in, which
    /// is arbitrary about the normal and therefore samples a normal map along
    /// an axis the author never chose.
    ///
    /// **What the bit selects is which frame the fragment stage perturbs in**:
    /// the interpolated vertex frame when it is set, and Schüler's cotangent
    /// frame from `ddx`/`ddy` of world position and UV when it is not. Both are
    /// in `shaders/mesh.slang`, and the selection is a branch on this bit —
    /// which is uniform across a primitive, so it costs nothing a per-pixel
    /// select would have saved.
    ///
    /// `docs/plan/43-render-standards.md` §2's rung 1: the vertex route is the
    /// one to take because only a stored tangent's `w` recovers the handedness
    /// a mirrored UV shell needs, and the derivative frame is what a mesh with
    /// no tangent gets until MikkTSpace fills one.
    pub const MESH_AUTHORED_TANGENTS: u32 = 1;

    /// The bytes one mesh-table element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; MESH_ENTRY_STRIDE] {
        let mut bytes = [0u8; MESH_ENTRY_STRIDE];
        let mut at = 0usize;
        for value in [self.base_vertex, self.base_index, self.index_count] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in self
            .bounds_min
            .iter()
            .chain(&self.bounds_max)
            .chain(&self.uv_range.scale)
            .chain(&self.uv_range.offset)
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&self.flags.to_le_bytes());
        at += 4;
        debug_assert_eq!(at, MESH_ENTRY_STRIDE);
        bytes
    }

    /// The inverse of [`GpuMesh::to_bytes`].
    ///
    /// So a test — or §3.6's debug readback — can decode what the table
    /// actually holds rather than trusting a host-side copy of it.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; MESH_ENTRY_STRIDE]) -> Self {
        let uint_at = |offset: usize| {
            u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let float_at = |offset: usize| {
            f32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        Self {
            base_vertex: uint_at(0),
            base_index: uint_at(4),
            index_count: uint_at(8),
            bounds_min: [float_at(12), float_at(16), float_at(20)],
            bounds_max: [float_at(24), float_at(28), float_at(32)],
            uv_range: UvRange {
                scale: [float_at(36), float_at(40)],
                offset: [float_at(44), float_at(48)],
            },
            flags: uint_at(52),
        }
    }
}

/// One material's shading factors and its base-colour texture, matching
/// `struct GpuMaterial` in `shaders/mesh.slang`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's material table:
/// [`GpuInstance::material`] indexes an array of these and the fragment stage
/// multiplies [`GpuMaterial::base_color`] and the texel
/// [`GpuMaterial::base_color_texture`] selects into the vertex albedo, then
/// shades it with the one GGX lobe [`GpuMaterial::metallic`] and
/// [`GpuMaterial::roughness`] parameterise.
/// [`MaterialTable`](https://docs.rs/crcbl-render) is what writes them.
///
/// # The texture index is an `ArrayPages` layer, not a `Bindless` slot
///
/// §3.2 pairs the table with "a bindless texture array
/// ([`BindingModel::Bindless`]) or texture array pages ([`ArrayPages`])", and
/// says the table "holds texture indices + factors". The index here is the
/// second of those: `mesh.slang` binds **one** `Texture2DArray` and the number
/// selects a layer of it. A `Bindless` slot would be an index into a runtime
/// sized array *of descriptors*, which needs
/// `Features::DESCRIPTOR_INDEXING` — a feature `crcbl-mtl` withdraws — where a
/// layer index needs nothing at all. So one column serves every device, and
/// what a bindless device gains later is capacity, not a different field.
///
/// # A zeroed row is black, and there is no empty value
///
/// [`GpuMesh`] has one — `index_count == 0` is an entry naming no mesh — and
/// this record deliberately has nothing equivalent. Every RGBA factor is a
/// material somebody could want, including all zeroes, so "unwritten" and
/// "black" are the same bytes and nothing can tell them apart. The consequence
/// is a contract rather than a defect: an instance names a material it was
/// given, and a row nobody wrote shades black, which is visible immediately
/// rather than plausible. **The texture column does not change that**: a zeroed
/// row names layer 0, and zero times any texel is still black.
///
/// The two shading factors do not change it either, and one of them is worth
/// being precise about: a zeroed row is `metallic 0.0`, so its diffuse albedo
/// is zero and its `F0` is the dielectric `0.04` — the row is black apart from
/// a mirror-sharp four-per-cent highlight where a light happens to reflect off
/// it. That is a smaller signal than the flat black the row had before this
/// column existed, and it is still nothing anyone would mistake for a material
/// they authored.
///
/// `PartialEq` but not `Eq`, for [`GpuMesh`]'s reason: several fields are floats.
///
/// [`BindingModel::Bindless`]: https://docs.rs/crcbl-hal
/// [`ArrayPages`]: https://docs.rs/crcbl-hal
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuMaterial {
    /// Linear RGBA factor multiplied into the vertex albedo and the texel.
    ///
    /// Linear, like every other colour that reaches the scene target: the
    /// tonemap pass and the swapchain's sRGB encode are what turn this into
    /// pixels, so `[1.0; 4]` is the material that changes nothing.
    pub base_color: [f32; 4],
    /// Which layer of the pass's base-colour page this material samples.
    ///
    /// **Layer 0 is the untextured value by convention**, and the convention is
    /// the page owner's to keep: whoever fills the page writes a white texel
    /// there, so a material that names no texture multiplies by `1.0`.
    /// `crcbl_render::scene`'s
    /// [`PageDesc`](https://docs.rs/crcbl-render/latest/crcbl_render/scene/struct.PageDesc.html)
    /// is the one that does — it owns layer 0 and gives a caller no way to
    /// spell it any other way — and [`GpuMaterial::UNTINTED`] is the row that
    /// relies on it.
    pub base_color_texture: u32,
    /// How metallic the surface is: `0.0` a dielectric, `1.0` a conductor.
    ///
    /// glTF's `pbrMetallicRoughness.metallicFactor` exactly, which is what lets
    /// [`crcbl_scene::gltf_import`](https://docs.rs/crcbl-scene) assign it with
    /// no conversion. `mesh.slang` reads it twice: it is what the specular
    /// lobe's `F0` interpolates between `0.04` and the base colour on, and it is
    /// what scales the diffuse albedo *down* — a conductor has no diffuse
    /// lobe at all.
    ///
    /// **A metal has no ambient term either**, because ambient scales that same
    /// diffuse albedo. What it gets instead is a reflection: `ssr.slang` marches
    /// for one, and a march that finds nothing returns the irradiance probes as
    /// its environment, so a fully metallic surface out of every light's reach
    /// is the environment rather than black. It is black only under a zeroed
    /// probe volume with nothing on screen to reflect. That is the model being
    /// right rather than the shader being wrong — see
    /// `docs/plan/18-render-features.md`.
    pub metallic: f32,
    /// How rough the surface is: `0.0` a mirror, `1.0` fully diffuse.
    ///
    /// glTF's `pbrMetallicRoughness.roughnessFactor`, on
    /// [`metallic`](Self::metallic)'s terms. `mesh.slang` squares it into the
    /// GGX `alpha` — the usual perceptual parameterisation, so equal steps here
    /// are roughly equal steps in the look — and clamps it away from zero,
    /// which a normal distribution divides by.
    pub roughness: f32,
    /// How the base-colour texture's sampling UV is derived: which of
    /// [`GpuMaterial::TILING_AUTHORED`] or [`GpuMaterial::TILING_PHYSICAL`] the
    /// fragment stage takes.
    ///
    /// [`TILING_AUTHORED`](Self::TILING_AUTHORED) is `0`, so a row nobody wrote
    /// — and every material authored before this field existed, which spreads
    /// [`UNTINTED`](Self::UNTINTED) and leaves this at its `0` — samples the
    /// vertex UV exactly as it always did. [`TILING_PHYSICAL`](Self::TILING_PHYSICAL)
    /// instead derives the UV from the surface's world-space extent divided by
    /// [`tile_metres`](Self::tile_metres), so a texture cell measures one
    /// [`tile_metres`](Self::tile_metres) of surface however large the face is —
    /// which is what makes a greybox grid read as a metric ruler. See
    /// `physical_tile_uv` in `shaders/mesh.slang`.
    pub tiling: u32,
    /// How many world-space metres one repeat of the texture spans under
    /// [`TILING_PHYSICAL`](Self::TILING_PHYSICAL); unread under
    /// [`TILING_AUTHORED`](Self::TILING_AUTHORED).
    ///
    /// `1.0` is one texture cell per metre, so a 2 m face shows a 2×2 grid of
    /// the tile and a 1 m face shows one. [`UNTINTED`](Self::UNTINTED) carries
    /// `1.0` so a row spread from it and switched to physical tiling gets the
    /// one-metre default without naming it. The shader clamps it off zero before
    /// dividing, so a physical row that left it at [`default`](Self::default)'s
    /// `0.0` collapses the whole surface onto one texel rather than dividing by
    /// zero.
    pub tile_metres: f32,
    /// Linear radiance this surface emits, added to the shaded colour and
    /// scaled by nothing.
    ///
    /// **`[0.0; 3]` is a surface that emits nothing**, and that is what makes
    /// this additive in the strict sense: it landed in the three words the row
    /// already padded with, so no earlier member's offset moved, and zero added
    /// to a colour is that colour exactly on every target. Not one golden in the
    /// tree moved when it arrived.
    ///
    /// **A radiance, not a factor and a strength.** glTF splits it — an
    /// `emissiveFactor` in `0..=1` and `KHR_materials_emissive_strength`'s
    /// multiplier over it — and their product is what a shader wants. The scene
    /// target is `Rgba16Float`, so a value above one is representable and is
    /// what the bloom chain reads as a glow.
    ///
    /// Three scalars on the shader side rather than a `float3`, for the reason
    /// `shaders/mesh.slang` gives on `GpuMaterial::emissive_r`: `std430` aligns
    /// a `float3` to sixteen and would have taken the row to 64 bytes.
    pub emissive: [f32; 3],
    /// Which layer of the pass's **normal** page this material samples, on
    /// [`base_color_texture`](Self::base_color_texture)'s terms exactly.
    ///
    /// **Zero means "no normal map"**, and it means it twice over. Layer 0 of
    /// the normal page is the neutral texel — see
    /// [`PageDesc::NEUTRAL_NORMAL`](https://docs.rs/crcbl-render), which owns it
    /// the way it owns the white one — *and* the fragment stage selects the
    /// interpolated surface normal outright rather than the perturbed one when
    /// this is zero. Both are needed: an 8-bit unorm cannot encode `0.5`
    /// exactly, so the neutral texel decodes to a tangent-space normal about
    /// `0.22°` off `(0, 0, 1)` and a page fetch alone would move every golden in
    /// the tree by a last bit. `a_neutral_normal_texel_is_not_exactly_flat` is
    /// that error measured.
    ///
    /// glTF's `normalTexture.index` reaches it through
    /// `crcbl_scene::gltf_render`, which is what turns a document's image into a
    /// page layer.
    pub normal_texture: u32,
    /// glTF's `normalTexture.scale`: how far the sampled normal is tilted off
    /// the surface.
    ///
    /// The specification defines it as scaling the **`x` and `y`** of the
    /// decoded tangent-space normal and leaving `z` alone —
    /// `normalize((<sampled> * 2 - 1) * vec3(scale, scale, 1))`, glTF 2.0
    /// §3.9.3 — so `1.0` is the texture as authored, `0.0` is a flat surface and
    /// a value above one exaggerates. [`UNTINTED`](Self::UNTINTED) carries
    /// `1.0`; a `default`-constructed row carries `0.0`, which is a flat surface
    /// and is what a row nobody wrote should be.
    ///
    /// Read only where [`normal_texture`](Self::normal_texture) is non-zero.
    pub normal_scale: f32,
    /// Which layer of the metallic-roughness-occlusion page this material
    /// samples.
    ///
    /// **Laid out and read by nothing.** `docs/plan/43-render-standards.md`
    /// §2's table sizes the row for four page rows at once, and this slice wires
    /// the normal row alone; the shaders declare this field, no shader reads it,
    /// and every producer writes zero. The rung that spends it is the same
    /// section's — the packed ORM texture that turns
    /// [`metallic`](Self::metallic) and [`roughness`](Self::roughness) into
    /// per-texel numbers.
    pub metallic_roughness_occlusion_texture: u32,
    /// Which layer of the emissive page this material samples, on
    /// [`metallic_roughness_occlusion_texture`](Self::metallic_roughness_occlusion_texture)'s
    /// terms: **laid out and read by nothing** until §2's emissive-page rung.
    /// [`emissive`](Self::emissive) is the factor half, and it has shipped.
    pub emissive_texture: u32,
    /// The alpha below which a masked material discards, glTF's
    /// `alphaCutoff`.
    ///
    /// **Laid out and read by nothing**, on
    /// [`metallic_roughness_occlusion_texture`](Self::metallic_roughness_occlusion_texture)'s
    /// terms. §2's fourth rung is the `discard` that spends it, and it needs an
    /// alpha mode in [`flags`](Self::flags) beside it — a cutoff with no mode
    /// selecting it is a number, not a behaviour, which is why nothing here
    /// reads either yet.
    pub alpha_cutoff: f32,
    /// Per-material bits, **all of them unassigned**.
    ///
    /// The word §2's table names beside the alpha cutoff, and the home the
    /// alpha modes of its fourth rung will take: `OPAQUE` is the absence of
    /// every bit, `MASK` and `BLEND` are one each. Laid out now for
    /// [`normal_texture`](Self::normal_texture)'s reason — the row's stride is
    /// mirrored in five shader declarations and a pinned offsets test, and
    /// widening it twice is that work twice.
    pub flags: u32,
}

impl GpuMaterial {
    /// The material that tints nothing: a plain dielectric with a soft
    /// highlight, on the page's white layer.
    ///
    /// **Not "every factor `1.0`" any more**, and the two that are not are the
    /// point. [`base_color`](Self::base_color) is still `[1.0; 4]`, because a
    /// factor of one is what a multiply into the vertex albedo has to be to
    /// change nothing. The shading factors have no such neutral value: a lobe is
    /// evaluated, not multiplied by, so *some* pair of numbers is what an
    /// instance shades with when nobody has asked. These are
    /// `metallic 0.0, roughness 0.5` — an ordinary painted surface, and roughly
    /// what the engine already looked like, since the Blinn exponent of 32 this
    /// row's lobe replaced sits near a GGX roughness of a half.
    ///
    /// Named for the same reason it always was: a table's rows are black until
    /// something writes them, and the numbers spelled at each such call site
    /// would be ones a reader has to recognise rather than read.
    /// The row's neutral: no page, on any of the four rows.
    ///
    /// Layer 0 by the convention `PageDesc` keeps — the base-colour page's
    /// white texel and the normal page's neutral one — and *also* the value the
    /// fragment stage tests to decide whether a page was named at all. See
    /// [`normal_texture`](Self::normal_texture), which is where the second half
    /// of that is argued.
    pub const NO_PAGE: u32 = 0;

    pub const UNTINTED: Self = Self {
        base_color: [1.0; 4],
        base_color_texture: Self::NO_PAGE,
        metallic: 0.0,
        roughness: 0.5,
        tiling: Self::TILING_AUTHORED,
        tile_metres: 1.0,
        emissive: [0.0; 3],
        normal_texture: Self::NO_PAGE,
        // The glTF default, and the factor that leaves an authored normal map
        // as it was authored — see `normal_scale`.
        normal_scale: 1.0,
        metallic_roughness_occlusion_texture: Self::NO_PAGE,
        emissive_texture: Self::NO_PAGE,
        // glTF's own `alphaCutoff` default. Read by nothing today; spelled
        // rather than left at zero so the rung that reads it inherits the
        // specification's value from every row spread out of this one.
        alpha_cutoff: 0.5,
        flags: 0,
    };

    /// [`tiling`](Self::tiling): sample the base-colour texture at the vertex's
    /// own UV, the way every material did before physical tiling existed.
    ///
    /// Zero, so it is what [`default`](Self::default) and every `..UNTINTED`
    /// spread that does not name a mode already carry — this mode is the added
    /// branch, not a replacement.
    pub const TILING_AUTHORED: u32 = 0;

    /// [`tiling`](Self::tiling): derive the UV from the surface's world-space
    /// extent, so the texture repeats once per [`tile_metres`](Self::tile_metres).
    ///
    /// See `physical_tile_uv` in `shaders/mesh.slang` for the projection.
    pub const TILING_PHYSICAL: u32 = 1;

    /// The four page layer indices as the two words the row carries them in:
    /// base colour in the low half of the first and the normal page in its
    /// high half, then metallic-roughness-occlusion and emissive the same way.
    ///
    /// Saturating rather than truncating at [`MAX_PAGE_LAYER`], for the reason
    /// that constant gives: an index that wrapped would name a real layer and
    /// shade a surface with somebody else's texture, where one that saturates
    /// names a layer the page does not have and is refused before a frame is
    /// drawn.
    #[must_use]
    fn page_words(&self) -> [u32; 2] {
        let pair = |low: u32, high: u32| low.min(MAX_PAGE_LAYER) | (high.min(MAX_PAGE_LAYER) << 16);
        [
            pair(self.base_color_texture, self.normal_texture),
            pair(
                self.metallic_roughness_occlusion_texture,
                self.emissive_texture,
            ),
        ]
    }

    /// The bytes one material-table element holds, in `std430` order.
    ///
    /// The row has no padding: sixty-four is a multiple of the `float4`'s
    /// alignment already, and every word after it is spent. Two of them are the
    /// four page indices packed in pairs — see [`MATERIAL_STRIDE`], which is
    /// where that arithmetic is argued, and `page_words`, which does it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; MATERIAL_STRIDE] {
        let mut bytes = [0u8; MATERIAL_STRIDE];
        let mut at = 0usize;
        let pages = self.page_words();
        for value in &self.base_color {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&pages[0].to_le_bytes());
        at += 4;
        for value in [self.metallic, self.roughness] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&self.tiling.to_le_bytes());
        at += 4;
        bytes[at..at + 4].copy_from_slice(&self.tile_metres.to_le_bytes());
        at += 4;
        for value in &self.emissive {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&pages[1].to_le_bytes());
        at += 4;
        for value in [self.normal_scale, self.alpha_cutoff] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&self.flags.to_le_bytes());
        at += 4;
        debug_assert_eq!(at, MATERIAL_STRIDE);
        bytes
    }

    /// The inverse of [`GpuMaterial::to_bytes`].
    ///
    /// So a test can decode what the table actually holds rather than trusting
    /// a host-side copy of it, which is the same reason
    /// [`GpuMesh::from_bytes`] exists.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; MATERIAL_STRIDE]) -> Self {
        let float_at = |offset: usize| {
            f32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let uint_at = |offset: usize| {
            u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let low = |word: u32| word & MAX_PAGE_LAYER;
        let high = |word: u32| word >> 16;
        Self {
            base_color: [float_at(0), float_at(4), float_at(8), float_at(12)],
            base_color_texture: low(uint_at(16)),
            metallic: float_at(20),
            roughness: float_at(24),
            tiling: uint_at(28),
            tile_metres: float_at(32),
            emissive: [float_at(36), float_at(40), float_at(44)],
            normal_texture: high(uint_at(16)),
            normal_scale: float_at(52),
            metallic_roughness_occlusion_texture: low(uint_at(48)),
            emissive_texture: high(uint_at(48)),
            alpha_cutoff: float_at(56),
            flags: uint_at(60),
        }
    }
}

/// Where one draw call's run of visible instances starts and what geometry it
/// draws, matching `struct DrawConstants` in `shaders/mesh.slang`.
///
/// **[`base`](Self::base) would be `draw_indexed`'s own base instance if the
/// four targets agreed about what that does to `SV_InstanceID`, and they do
/// not.** That shader's header measures the disagreement on all four; the
/// consequence for a producer of these bytes is that every draw passes zero for
/// its own bases, and the instance is looked up rather than named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawConstants {
    /// The draw's bucket's first slot in the list `draw_gen.slang` scatters
    /// surviving instances into. `SV_InstanceID` counts from zero within the
    /// draw and indexes the run from here.
    pub base: u32,
    /// The bucket's mesh, as an index into the mesh table — where the vertex
    /// stage takes [`GpuMesh::base_vertex`] from.
    ///
    /// **The bucket's, not the drawn instance's.** They name the same geometry
    /// for every instance a bucket can hold, because an indexed draw has one
    /// index range and `draw_gen.slang` takes it from this same entry. What the
    /// distinction buys is `docs/plan/25-lod.md`'s uniform cut: a DAG's levels
    /// are mesh table entries of their own and a bucket is one of them, while
    /// the instance goes on naming level 0 — the entry the cull pass reads a
    /// bounding box out of.
    pub mesh: u32,
}

impl DrawConstants {
    /// The bytes one draw's block holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; DRAW_CONSTANTS_SIZE] {
        let mut bytes = [0u8; DRAW_CONSTANTS_SIZE];
        bytes[0..4].copy_from_slice(&self.base.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.mesh.to_le_bytes());
        // The two trailing `uint`s are padding and stay zero.
        bytes
    }
}

/// One face of the cube: an outward normal, an albedo, and its four corners in
/// counter-clockwise order **as seen from outside**.
///
/// The winding is the load-bearing part. `crcbl-render`'s forward pass draws
/// with [`CullMode::Back`] and [`FrontFace::Ccw`], so a face wound the other way
/// simply disappears — which is a far better failure than a face that renders
/// with its lighting inside out.
///
/// [`CullMode::Back`]: https://docs.rs/crcbl-hal
/// [`FrontFace::Ccw`]: https://docs.rs/crcbl-hal
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    /// Human-readable name, used in test failures.
    pub name: &'static str,
    /// Outward unit normal.
    pub normal: [f32; 3],
    /// Linear RGB albedo; alpha is always 1.
    pub color: [f32; 3],
    /// The four corners, counter-clockwise from outside.
    pub corners: [[f32; 3]; 4],
}

/// Half the cube's edge length, so it spans `[-0.5, 0.5]` on every axis.
///
/// A unit cube centred on the origin: the model matrix is then a pure rotation
/// and the mesh needs no normalisation anywhere.
const H: f32 = 0.5;

/// The six faces, each with a distinct hue.
///
/// The colours are diagnostic rather than decorative, exactly as
/// [`triangle::VERTICES`](crate::triangle::VERTICES)' are: no two faces share
/// one, and opposite faces are complementary, so a mirrored axis or a
/// transposed view matrix produces a *visibly different* picture rather than a
/// plausible one.
pub const FACES: [Face; 6] = [
    Face {
        name: "+X",
        normal: [1.0, 0.0, 0.0],
        color: [0.90, 0.20, 0.20],
        corners: [[H, -H, H], [H, -H, -H], [H, H, -H], [H, H, H]],
    },
    Face {
        name: "-X",
        normal: [-1.0, 0.0, 0.0],
        color: [0.20, 0.75, 0.80],
        corners: [[-H, -H, -H], [-H, -H, H], [-H, H, H], [-H, H, -H]],
    },
    Face {
        name: "+Y",
        normal: [0.0, 1.0, 0.0],
        color: [0.25, 0.80, 0.30],
        corners: [[-H, H, H], [H, H, H], [H, H, -H], [-H, H, -H]],
    },
    Face {
        name: "-Y",
        normal: [0.0, -1.0, 0.0],
        color: [0.85, 0.25, 0.75],
        corners: [[-H, -H, -H], [H, -H, -H], [H, -H, H], [-H, -H, H]],
    },
    Face {
        name: "+Z",
        normal: [0.0, 0.0, 1.0],
        color: [0.25, 0.35, 0.90],
        corners: [[-H, -H, H], [H, -H, H], [H, H, H], [-H, H, H]],
    },
    Face {
        name: "-Z",
        normal: [0.0, 0.0, -1.0],
        color: [0.90, 0.80, 0.20],
        corners: [[H, -H, -H], [-H, -H, -H], [-H, H, -H], [H, H, -H]],
    },
];

/// Vertices in the cube: four per face, so each face gets its own flat normal.
pub const CUBE_VERTEX_COUNT: usize = FACES.len() * 4;

/// Indices in the cube: two triangles per face.
pub const CUBE_INDEX_COUNT: usize = FACES.len() * 6;

/// The texture coordinates of a quad's four corners, in the order [`Face`]
/// declares them and [`cube_indices`] triangulates them.
///
/// One copy shared by the cube's faces and the pyramid's base, because both are
/// the same quad wound the same way — a second table would be a second thing to
/// get the corner order wrong in.
const QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The cube's vertices, in face order.
///
/// Every face carries the whole of the layer — `0..=1` in both axes — so each
/// of the six samples the material's page layer once over, which is what makes
/// a texture that differs per material differ over the whole silhouette rather
/// than in one corner.
#[must_use]
pub fn cube_vertices() -> Vec<MeshVertex> {
    let range = demo_uv_range();
    let mut vertices = Vec::with_capacity(CUBE_VERTEX_COUNT);
    for face in &FACES {
        for (corner, uv) in face.corners.iter().zip(&QUAD_UV) {
            vertices.push(MeshVertex::from_normal(
                *corner,
                face.normal,
                [face.color[0], face.color[1], face.color[2], 1.0],
                *uv,
                &range,
            ));
        }
    }
    vertices
}

/// The [`UvRange`] every mesh in this module quantises its coordinates against.
///
/// All three are authored on the unit square: the cube's and the pyramid's
/// coordinates are `QUAD_UV` and `TRIANGLE_UV` exactly, and the open box's
/// are `(column + corner) / OPEN_BOX_SUBDIVISIONS`, which reaches zero and one
/// and nothing outside either. Derived from those two tables rather than
/// written down, and `the_demo_meshes_are_authored_on_the_range_they_declare`
/// is what would fail if a mesh here grew a coordinate this range does not
/// cover.
///
/// A description carries it beside the vertex bytes — see `Geometry::Flat`'s
/// `uv_range` in `crcbl-render` — because a lane means nothing without the
/// scale and offset it was quantised against, and the pool writes the pair into
/// [`GpuMesh::uv_range`] for the shaders to read back through.
#[must_use]
pub fn demo_uv_range() -> UvRange {
    let mut uvs = Vec::with_capacity(QUAD_UV.len() + TRIANGLE_UV.len());
    uvs.extend_from_slice(&QUAD_UV);
    uvs.extend_from_slice(&TRIANGLE_UV);
    UvRange::from_uvs(&uvs)
}

/// The cube's indices: `0 1 2, 0 2 3` per face, preserving each face's
/// counter-clockwise corner order.
#[must_use]
pub fn cube_indices() -> Vec<u32> {
    let mut indices = Vec::with_capacity(CUBE_INDEX_COUNT);
    for face in 0..FACES.len() {
        let base = u32::try_from(face * 4).expect("six faces fit in a u32");
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    indices
}

/// [`cube_vertices`] as the bytes a storage buffer holds.
///
/// Little-endian `f32`s in declaration order, which is what `std430` means for
/// a struct of `float4`s and what every target this engine has is.
#[must_use]
pub fn cube_vertex_bytes() -> Vec<u8> {
    vertex_bytes(&cube_vertices())
}

/// [`cube_indices`] as the bytes an index buffer holds.
///
/// `u32` rather than `u16`: the seam's
/// [`IndexFormat`](https://docs.rs/crcbl-hal) has both, and a 24-vertex cube
/// would fit in `u16` — but P9's global index pool is one buffer for every mesh
/// in the world, which does not, and having the demo use the format the engine
/// actually ships avoids a conversion nobody would remember to remove.
#[must_use]
pub fn cube_index_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CUBE_INDEX_COUNT * 4);
    for index in cube_indices() {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

// ---------------------------------------------------------------------------
// The second mesh
// ---------------------------------------------------------------------------

/// Half the pyramid's base edge, so its base spans `[-0.4, 0.4]` on X and Z.
const PYRAMID_HALF_BASE: f32 = 0.4;

/// How far the pyramid's base sits below the origin.
const PYRAMID_BASE_Y: f32 = -0.4;

/// How far its apex sits above the origin.
const PYRAMID_APEX_Y: f32 = 0.5;

/// The pyramid's base albedo, linear RGB.
///
/// No cube face shares it, and neither does any side below: the pyramid exists
/// to be told apart from the cube in a single frame, so the day it draws the
/// cube's vertices the picture is a *different* picture and not a subtly
/// misplaced one. See [`pyramid_vertices`].
pub const PYRAMID_BASE_COLOR: [f32; 3] = [0.95, 0.95, 0.90];

/// The four sides' albedos, in `+Z`-first order around the base.
pub const PYRAMID_SIDE_COLORS: [[f32; 3]; 4] = [
    [0.95, 0.55, 0.15],
    [0.15, 0.65, 0.55],
    [0.60, 0.30, 0.85],
    [0.65, 0.90, 0.25],
];

/// Vertices in the pyramid: four for the base, three for each side, so every
/// face gets its own flat normal exactly as the cube's do.
pub const PYRAMID_VERTEX_COUNT: usize = 4 + PYRAMID_SIDE_COLORS.len() * 3;

/// Indices in the pyramid: two triangles for the base, one per side.
pub const PYRAMID_INDEX_COUNT: usize = 6 + PYRAMID_SIDE_COLORS.len() * 3;

/// The second mesh: a square pyramid, and the reason it exists.
///
/// [`MeshPool`](https://docs.rs/crcbl-render) allocates a mesh wherever it fits,
/// so the *second* resident is the first one at a non-zero
/// `MeshRange::base_vertex` — and a base vertex is exactly what `mesh.slang`'s
/// header shows the four targets disagreeing about. A pool with one mesh in it
/// cannot exercise that at all: every index is already absolute. So this is
/// demo geometry in the same sense the cube is, and it is also the only thing
/// in the tree that can tell a working base vertex from a subtracted one.
///
/// It is deliberately **not** a second box. A mesh built from [`FACES`]' layout
/// would read the cube's own vertices when the base went missing and come out
/// looking like a cube, which is the failure that hid behind one resident in
/// the first place.
///
/// The base is wound counter-clockwise seen from below and each side
/// counter-clockwise seen from outside, so the pass's back-face culling is as
/// legal here as it is for the cube. Normals are computed from the triangles
/// rather than written down, because a hand-normalised vector is arithmetic a
/// reader cannot check.
#[must_use]
pub fn pyramid_vertices() -> Vec<MeshVertex> {
    let base = [
        [-PYRAMID_HALF_BASE, PYRAMID_BASE_Y, -PYRAMID_HALF_BASE],
        [PYRAMID_HALF_BASE, PYRAMID_BASE_Y, -PYRAMID_HALF_BASE],
        [PYRAMID_HALF_BASE, PYRAMID_BASE_Y, PYRAMID_HALF_BASE],
        [-PYRAMID_HALF_BASE, PYRAMID_BASE_Y, PYRAMID_HALF_BASE],
    ];
    let apex = [0.0, PYRAMID_APEX_Y, 0.0];

    let range = demo_uv_range();
    let mut vertices = Vec::with_capacity(PYRAMID_VERTEX_COUNT);
    // The base, in the same corner order as the cube's `-Y` face, so the
    // `0 1 2, 0 2 3` triangulation below is the one `cube_indices` uses — and
    // so is `QUAD_UV`, for the same reason.
    for (corner, uv) in base.iter().zip(&QUAD_UV) {
        vertices.push(MeshVertex::from_normal(
            *corner,
            [0.0, -1.0, 0.0],
            [
                PYRAMID_BASE_COLOR[0],
                PYRAMID_BASE_COLOR[1],
                PYRAMID_BASE_COLOR[2],
                1.0,
            ],
            *uv,
            &range,
        ));
    }
    // One triangle per side. Corner `i + 1` before corner `i` is what makes the
    // winding counter-clockwise from outside; taking them in the other order
    // would make every side vanish under back-face culling.
    for (side, color) in PYRAMID_SIDE_COLORS.iter().enumerate() {
        let corners = [base[(side + 1) % base.len()], base[side], apex];
        let normal = triangle_normal(corners[0], corners[1], corners[2]);
        for (corner, uv) in corners.iter().zip(&TRIANGLE_UV) {
            vertices.push(MeshVertex::from_normal(
                *corner,
                normal,
                [color[0], color[1], color[2], 1.0],
                *uv,
                &range,
            ));
        }
    }
    vertices
}

/// The texture coordinates of a pyramid side: the two base corners along the
/// bottom edge and the apex at the top middle.
///
/// In the order [`pyramid_vertices`] emits a side's corners — base `i + 1`,
/// base `i`, apex — so the layer is upright on every face rather than rotated a
/// quarter turn per side.
const TRIANGLE_UV: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]];

/// The unit normal of the triangle `a b c`, by the right-hand rule — so it
/// points outward exactly when the winding is counter-clockwise seen from
/// outside.
fn triangle_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let edge = |p: [f32; 3], q: [f32; 3]| [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let (u, v) = (edge(a, b), edge(a, c));
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    [cross[0] / length, cross[1] / length, cross[2] / length]
}

/// The pyramid's indices: the base as two triangles, then one per side.
#[must_use]
pub fn pyramid_indices() -> Vec<u32> {
    let mut indices = Vec::with_capacity(PYRAMID_INDEX_COUNT);
    indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    for side in 0..PYRAMID_SIDE_COLORS.len() {
        let base = u32::try_from(4 + side * 3).expect("four sides fit in a u32");
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    indices
}

/// [`pyramid_vertices`] as the bytes a storage buffer holds, on the same terms
/// as [`cube_vertex_bytes`].
#[must_use]
pub fn pyramid_vertex_bytes() -> Vec<u8> {
    vertex_bytes(&pyramid_vertices())
}

// ---------------------------------------------------------------------------
// The third mesh
// ---------------------------------------------------------------------------

/// How many quads each face of the open box is divided into, per axis.
///
/// **This number is the whole reason the mesh exists.** A face is
/// `OPEN_BOX_SUBDIVISIONS²` quads of four vertices each, and at four that is
/// [`MAX_CLUSTER_VERTICES`](crate::meshlet::MAX_CLUSTER_VERTICES) exactly — so
/// `crcbl_scene::meshlet::build_meshlets` closes a cluster on every face
/// boundary and each cluster comes out as one flat face. The cube and the
/// pyramid are one cluster each, which makes per-cluster culling
/// indistinguishable from whole-mesh culling; this is the mesh that is several.
///
/// A smaller number would leave one cluster spanning two perpendicular faces,
/// whose normal cone is then wide enough to reject nothing. A larger one would
/// overrun the vertex bound part way through a face — a legal decomposition,
/// and one whose cluster boundaries no longer line up with anything a reader
/// can see.
pub const OPEN_BOX_SUBDIVISIONS: usize = 4;

/// Quads in one face of the open box.
pub const OPEN_BOX_QUADS_PER_FACE: usize = OPEN_BOX_SUBDIVISIONS * OPEN_BOX_SUBDIVISIONS;

// Four vertices per quad, none shared, is what makes a face exactly one
// cluster. The build fails here rather than in a golden image if either bound
// moves.
const _: () = assert!(OPEN_BOX_QUADS_PER_FACE * 4 == crate::meshlet::MAX_CLUSTER_VERTICES);
const _: () = assert!(OPEN_BOX_QUADS_PER_FACE * 2 <= crate::meshlet::MAX_CLUSTER_TRIANGLES);

/// The open box's five faces, wound counter-clockwise **seen from inside** —
/// which is the side each one's normal points, exactly as [`FACES`]' are.
///
/// A box with its `+Y` face missing, so the five that remain face *inward* and
/// a camera above it looks into the shape rather than at it. That is not
/// decoration either: cone culling can only reject a cluster whose whole cone
/// faces away, so a mesh worth culling needs a camera position from which every
/// cluster is front-facing — which a closed shape has none of, and the inside
/// of an open one has plenty.
///
/// The corners are the unit cube's own, the ones [`FACES`] is built from, and
/// every subdivision of them is a quarter of an edge — so each position, each
/// face centre and each bounding radius is exact in `f32`. No
/// trigonometry appears anywhere in this mesh for that reason: `sinf` and
/// `cosf` are the platform's, they differ in the last place between them, and
/// `crcbl-scene`'s pin test compares the cooked bounds for **equality**.
pub const OPEN_BOX_FACES: [Face; 5] = [
    Face {
        name: "floor",
        normal: [0.0, 1.0, 0.0],
        color: [0.75, 0.72, 0.68],
        corners: [[-H, -H, H], [H, -H, H], [H, -H, -H], [-H, -H, -H]],
    },
    Face {
        name: "-X wall",
        normal: [1.0, 0.0, 0.0],
        color: [0.85, 0.35, 0.30],
        corners: [[-H, -H, -H], [-H, H, -H], [-H, H, H], [-H, -H, H]],
    },
    Face {
        name: "+X wall",
        normal: [-1.0, 0.0, 0.0],
        color: [0.30, 0.70, 0.45],
        corners: [[H, -H, H], [H, H, H], [H, H, -H], [H, -H, -H]],
    },
    Face {
        name: "-Z wall",
        normal: [0.0, 0.0, 1.0],
        color: [0.35, 0.45, 0.85],
        corners: [[-H, -H, -H], [H, -H, -H], [H, H, -H], [-H, H, -H]],
    },
    Face {
        name: "+Z wall",
        normal: [0.0, 0.0, -1.0],
        color: [0.85, 0.70, 0.25],
        corners: [[H, -H, H], [-H, -H, H], [-H, H, H], [H, H, H]],
    },
];

/// Vertices in the open box: four per quad, none shared with its neighbours.
///
/// Sharing them would be smaller and would make a face fewer than
/// [`MAX_CLUSTER_VERTICES`](crate::meshlet::MAX_CLUSTER_VERTICES) vertices, so
/// a cluster would span more than one face — and it would give the shared
/// corners an averaged normal, which is the opposite of what flat faces and a
/// tight normal cone need.
pub const OPEN_BOX_VERTEX_COUNT: usize = OPEN_BOX_FACES.len() * OPEN_BOX_QUADS_PER_FACE * 4;

/// Indices in the open box: two triangles per quad.
pub const OPEN_BOX_INDEX_COUNT: usize = OPEN_BOX_FACES.len() * OPEN_BOX_QUADS_PER_FACE * 6;

/// A point on a quad, `s` of the way along the `corners[0] -> corners[1]` edge
/// and `t` of the way along `corners[0] -> corners[3]`.
///
/// Bilinear, so a point of a planar quad stays in its plane — which is what
/// gives every subdivided face one flat normal and therefore a normal cone of
/// zero half-angle. `a + (b - a) * s` rather than `a * (1 - s) + b * s`: both
/// are exact for the values here, and only the first is exactly `a` at `s = 0`
/// and exactly `b` at `s = 1`. Written out rather than through `f32::mul_add`,
/// because a fused multiply-add rounds once where this rounds twice and only
/// one of the two is what a reader checking these coordinates by hand does.
fn bilinear(corners: &[[f32; 3]; 4], s: f32, t: f32) -> [f32; 3] {
    let lerp = |a: [f32; 3], b: [f32; 3], u: f32| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * u);
    lerp(
        lerp(corners[0], corners[1], s),
        lerp(corners[3], corners[2], s),
        t,
    )
}

/// The third mesh: a box with its lid off, every face divided into
/// [`OPEN_BOX_QUADS_PER_FACE`] quads.
///
/// The cube and the pyramid are one cluster each — 24 and 16 vertices against a
/// bound of 64 — so nothing in this crate's geometry could tell per-cluster
/// culling from whole-mesh culling, and no rendered frame exercised a cluster
/// at a non-zero `vertex_offset` *within* one mesh. This mesh is five clusters,
/// one per face, each facing a different way.
///
/// Quads are emitted a face at a time, rows first, and each carries the whole
/// of the material's page layer across the face it belongs to — [`FACES`]'
/// arrangement, so a texture that differs per material differs over each face
/// rather than over each quad.
#[must_use]
pub fn open_box_vertices() -> Vec<MeshVertex> {
    let range = demo_uv_range();
    let step = 1.0 / OPEN_BOX_SUBDIVISIONS as f32;
    let mut vertices = Vec::with_capacity(OPEN_BOX_VERTEX_COUNT);
    for face in &OPEN_BOX_FACES {
        for row in 0..OPEN_BOX_SUBDIVISIONS {
            for column in 0..OPEN_BOX_SUBDIVISIONS {
                // The quad's own corner order is `QUAD_UV`'s, so the winding it
                // inherits is the face's and the two triangles below are the
                // `0 1 2, 0 2 3` every other mesh here uses.
                for uv in &QUAD_UV {
                    let s = (column as f32 + uv[0]) * step;
                    let t = (row as f32 + uv[1]) * step;
                    let position = bilinear(&face.corners, s, t);
                    vertices.push(MeshVertex::from_normal(
                        position,
                        face.normal,
                        [face.color[0], face.color[1], face.color[2], 1.0],
                        [s, t],
                        &range,
                    ));
                }
            }
        }
    }
    vertices
}

/// The open box's indices: `0 1 2, 0 2 3` per quad, in the order
/// [`open_box_vertices`] emitted them.
#[must_use]
pub fn open_box_indices() -> Vec<u32> {
    let mut indices = Vec::with_capacity(OPEN_BOX_INDEX_COUNT);
    for quad in 0..OPEN_BOX_FACES.len() * OPEN_BOX_QUADS_PER_FACE {
        let base = u32::try_from(quad * 4).expect("a few hundred vertices fit in a u32");
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    indices
}

/// [`open_box_vertices`] as the bytes a storage buffer holds, on the same terms
/// as [`cube_vertex_bytes`].
#[must_use]
pub fn open_box_vertex_bytes() -> Vec<u8> {
    vertex_bytes(&open_box_vertices())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`DECLARED_BINDINGS`] is what `mesh.slang` actually declares.
    ///
    /// Parsed out of the source rather than mirrored by hand a second time: a
    /// second hand-written copy would need a third thing to hold *it* together,
    /// which is the regress this constant exists to end. Set 0 only — a
    /// `[[vk::binding(n, 1)]]` would belong to a different layout.
    #[test]
    fn the_declared_bindings_are_the_ones_mesh_slang_declares() {
        const SOURCE: &str = include_str!("../shaders/mesh.slang");

        let declared: Vec<u32> = SOURCE
            .lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix("[[vk::binding(")?;
                let (number, tail) = rest.split_once(',')?;
                tail.trim_start().strip_prefix("0)]]")?;
                number.trim().parse().ok()
            })
            .collect();

        assert!(
            !declared.is_empty(),
            "no `[[vk::binding(n, 0)]]` was found in mesh.slang at all, so this guard is \
             matching on a spelling the shader no longer uses and is holding nothing"
        );
        assert_eq!(
            declared, DECLARED_BINDINGS,
            "mesh.slang's set-0 bindings and `DECLARED_BINDINGS` disagree. A binding added to \
             the shader has to be added here and to `forward_e2e::depth_probe`'s hand-written \
             layout, or that suite segfaults on a software adapter with nothing naming the \
             descriptor"
        );
    }

    /// The offsets `slangc` actually emitted for `FrameUniforms`, read out of
    /// the disassembly. If the shader's field order changes, this is what says
    /// so before a driver reads the light direction out of the model matrix.
    /// The cascade count is a number in three files and a compiler sees none of
    /// the disagreements.
    ///
    /// The sky's rows are written by this crate and read only by arithmetic in
    /// `mesh.slang`'s fragment stage, so nothing but this test stands between
    /// the two.
    ///
    /// A `mesh.slang` that declared the rows and never added them would leave
    /// `set_sky` writing into a lane nothing reads — a caller who asked for a
    /// sky and got the frame they had before, with no error anywhere. The
    /// widened block would still match `slangc`'s offsets, so the layout test
    /// above cannot see it.
    #[test]
    fn the_fragment_stage_still_adds_the_sky_it_declares() {
        let mesh = include_str!("../shaders/mesh.slang");
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        // Both files declare the three rows, because they are one buffer and a
        // block short of a row on one side is one whose size the two stages
        // disagree about.
        for row in ["sky_sh_r", "sky_sh_g", "sky_sh_b"] {
            for (name, source) in [("mesh.slang", mesh), ("mesh_cluster.slang", cluster)] {
                assert!(
                    source.contains(&format!("float4 {row};")),
                    "{name} does not declare `{row}`"
                );
            }
            // And the reader evaluates every one of them. Two channels read
            // from one row is a sky of the wrong colour, which is a picture.
            assert!(
                mesh.contains(&format!("dot(frame.{row}, basis)")),
                "mesh.slang declares `{row}` and never evaluates it"
            );
        }
        // The basis is the normal and a one, which is what makes the packing
        // this crate's `sky::SkyGradient::irradiance` writes — linear band in
        // `xyz`, constant in `w` — the packing the shader unpacks.
        assert!(
            mesh.contains("float3 sky_irradiance(float3 normal)"),
            "mesh.slang no longer has the evaluator the frame rows exist for"
        );
        // And the ambient term sums it. Named against `probe_irradiance`
        // beside it: the two are the same kind of environment and the sum is
        // what makes them add rather than one replacing the other.
        assert!(
            mesh.contains("frame.ambient.rgb + sky_irradiance(normal)"),
            "mesh.slang's ambient term no longer adds the sky, so `set_sky` writes a lane \
             nothing reads"
        );
    }

    /// A shader whose `shadow_view_proj` array is longer than the CPU's reads
    /// `cascade_far` as the tail of a matrix and `shadow_params` as a split
    /// distance; one that is shorter leaves the last cascade's matrix
    /// unwritten. Both render a picture.
    #[test]
    fn the_cascade_count_matches_the_one_the_shaders_declare() {
        let mesh = include_str!("../shaders/mesh.slang");
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        // The two that size arrays in the block, so both files must agree.
        for declaration in [
            format!("static const uint SHADOW_CASCADES = {SHADOW_CASCADES};"),
            format!("static const uint SHADOW_LIGHT_TILES = {SHADOW_LIGHT_TILES};"),
            format!("static const uint SHADOW_ATLAS_TILES = {SHADOW_ATLAS_TILES};"),
        ] {
            for (name, source) in [("mesh.slang", mesh), ("mesh_cluster.slang", cluster)] {
                assert!(
                    source.contains(&declaration),
                    "{name} does not declare `{declaration}`; a block sized differently on the \
                     two sides puts every member after it at the wrong offset"
                );
            }
        }
        // What is left of the atlas's geometry on the sampling side. The grid's
        // two extents went with `docs/plan/45-shadows.md`'s allocator rung — a
        // slot's place is a rectangle the host hands over in
        // `shadow_atlas_rect` — and the tile's own side went with the priority
        // rung, because a map is no longer always a whole root cell and the one
        // it landed in is read out of that same rectangle.
        // The face count is what is left of it, and only the sampling side reads
        // it: it is how far apart two of a point light's tiles are, so a shader
        // that thought a point light had five faces would sample another light's
        // map for the sixth — a picture, and a plausible one.
        let declaration = format!("static const uint SHADOW_POINT_FACES = {SHADOW_POINT_FACES};");
        assert!(
            mesh.contains(&declaration),
            "mesh.slang does not declare `{declaration}`; the atlas's geometry has drifted"
        );
        assert!(
            !mesh.contains("static const float SHADOW_TILE"),
            "mesh.slang declares a whole cell's side again. Every bias in that file is \
             denominated in the texels of the map being sampled, and since the priority \
             rung a map may be a halving of a cell — a constant there under-biases every \
             demoted light by exactly the factor it was demoted by, which is acne on that \
             light's receivers and on no other's"
        );
        // Not atlas geometry but the same kind of number: the cascade's depth
        // range, which `crcbl_render::shadow::cascade_matrix` builds and
        // `sun_penumbra_texels` inverts. A shader holding a different one sizes
        // every penumbra by the ratio between the two — a picture, and a
        // plausible one, since no frame says how wide a penumbra should be.
        // The tile clear's depth, read off the shader rather than restated: the
        // pass's own `LoadOp::Clear` writes `crcbl_hal::depth::CLEAR` and this
        // stage writes this, and a frame that keeps some tiles uses both — so
        // two different values are a tile that reads as "nothing stored" on one
        // path and as a caster at the near plane on the other.
        assert_eq!(
            shader_float(mesh, "SHADOW_ATLAS_CLEAR_DEPTH"),
            SHADOW_ATLAS_CLEAR_DEPTH,
            "mesh.slang clears a tile to a depth this crate does not declare"
        );
        let reach = format!("static const float SHADOW_CASTER_REACH = {SHADOW_CASTER_REACH:?};");
        assert!(
            mesh.contains(&reach),
            "mesh.slang does not declare `{reach}`, so the blocker search is inverting a box \
             the host does not build"
        );
        const {
            assert!(
                SHADOW_CASCADES >= 1 && SHADOW_CASCADES <= 4,
                "cascade_far is one float4, so it can name at most four splits, and a shadow pass \
                 with no cascades is not a shadow pass"
            );
        }
    }

    /// A `static const float` (or `float3`) declared in a shader source, by
    /// name.
    ///
    /// Reads the *shader's* number rather than restating it here, which is the
    /// whole point of every threshold check below: a constant written twice is
    /// one that can drift, and the copy in a test is the one nobody looks at.
    fn shader_float(source: &str, name: &str) -> f32 {
        let literal = source
            .split_once(&format!("static const float {name} = "))
            .unwrap_or_else(|| panic!("the source declares `{name}`"))
            .1
            .split_once(';')
            .expect("a declaration ends in a semicolon")
            .0;
        literal
            .parse()
            .unwrap_or_else(|_| panic!("`{literal}` is not a float"))
    }

    /// **The occlusion view's threshold lies above the heatmap's**, so one lane
    /// can carry five states.
    ///
    /// The lane holds off, normals, tint, heatmap and occlusion, and the
    /// sentinels ascend — so a `4.0` clears every threshold and only the *order*
    /// of the tests keeps it out of the four lower branches. That order is
    /// asserted here rather than left to a reader, exactly as
    /// [`the_heatmap_view_threshold_lies_above_the_lod_view`] asserts the pair
    /// below it.
    ///
    /// The occlusion view is `mesh.slang`'s alone: it reads a screen-space
    /// channel by `SV_Position`, which the geometry stages neither produce nor
    /// need, so `mesh_cluster.slang` declares no `OCCLUSION_VIEW`. The absence
    /// is asserted, because one appearing there later would be a second place
    /// the interleaving has to hold.
    ///
    /// [`the_heatmap_view_threshold_lies_above_the_lod_view`]: fn@the_heatmap_view_threshold_lies_above_the_lod_view
    #[test]
    fn the_occlusion_view_threshold_lies_above_the_heatmap() {
        let mesh = include_str!("../shaders/mesh.slang");
        let occlusion = shader_float(mesh, "OCCLUSION_VIEW");
        assert!(
            FrameUniforms::HEATMAP_VIEW_ON < occlusion
                && occlusion < FrameUniforms::OCCLUSION_VIEW_ON,
            "mesh.slang switches at {occlusion}, which does not separate the heatmap's {} from \
             the occlusion view's {}",
            FrameUniforms::HEATMAP_VIEW_ON,
            FrameUniforms::OCCLUSION_VIEW_ON,
        );

        let occlusion_at = mesh
            .find("if (frame.ambient.w >= OCCLUSION_VIEW)")
            .expect("mesh.slang's fragment stage tests the occlusion view");
        let lod_at = mesh
            .find("if (frame.ambient.w >= LOD_VIEW)")
            .expect("mesh.slang's fragment stage tests the LOD view");
        assert!(
            occlusion_at < lod_at,
            "the occlusion test must come first, or a frame asking for it is caught by the LOD \
             threshold and draws an unshaded vertex colour instead"
        );

        let cluster = include_str!("../shaders/mesh_cluster.slang");
        assert!(
            !cluster.contains("OCCLUSION_VIEW"),
            "mesh_cluster.slang has grown an OCCLUSION_VIEW; the interleaving now has to hold in \
             two files and this test only checks one"
        );
    }

    /// **The motion view's threshold lies above the occlusion view's**, so one
    /// lane can carry six states.
    ///
    /// [`the_occlusion_view_threshold_lies_above_the_heatmap`]'s claim one
    /// sentinel further out, and for its reason exactly: the sentinels ascend,
    /// so the outermost clears every threshold below it and only the *order* of
    /// the tests keeps it out of the five lower branches.
    ///
    /// The motion view is `mesh.slang`'s alone, like the occlusion view: it
    /// reads two interpolants the geometry stages produce but never look at, so
    /// `mesh_cluster.slang` declares no `MOTION_VIEW`. The absence is asserted,
    /// because one appearing there later would be a second place the
    /// interleaving has to hold.
    ///
    /// [`the_occlusion_view_threshold_lies_above_the_heatmap`]: fn@the_occlusion_view_threshold_lies_above_the_heatmap
    #[test]
    fn the_motion_view_threshold_lies_above_the_occlusion_view() {
        let mesh = include_str!("../shaders/mesh.slang");
        let motion = shader_float(mesh, "MOTION_VIEW");
        assert!(
            FrameUniforms::OCCLUSION_VIEW_ON < motion && motion < FrameUniforms::MOTION_VIEW_ON,
            "mesh.slang switches at {motion}, which does not separate the occlusion view's {} \
             from the motion view's {}",
            FrameUniforms::OCCLUSION_VIEW_ON,
            FrameUniforms::MOTION_VIEW_ON,
        );

        let motion_at = mesh
            .find("if (frame.ambient.w >= MOTION_VIEW)")
            .expect("mesh.slang's fragment stage tests the motion view");
        let occlusion_at = mesh
            .find("if (frame.ambient.w >= OCCLUSION_VIEW)")
            .expect("mesh.slang's fragment stage tests the occlusion view");
        assert!(
            motion_at < occlusion_at,
            "the motion test must come first, or a frame asking for it is caught by the \
             occlusion threshold and draws the ambient-occlusion channel instead"
        );

        let cluster = include_str!("../shaders/mesh_cluster.slang");
        assert!(
            !cluster.contains("MOTION_VIEW"),
            "mesh_cluster.slang has grown a MOTION_VIEW; the interleaving now has to hold in two \
             files and this test only checks one"
        );
    }

    /// The scale the motion view encodes with is the one this crate declares.
    ///
    /// Written on both sides because the shader applies it and a test reading
    /// the picture has to undo it, and a constant written twice is one that can
    /// drift — the same argument every threshold check here is made on. The
    /// failure it prevents is silent: a scale that disagreed would leave a
    /// picture that still looks like a motion field and decode to the wrong
    /// magnitudes.
    #[test]
    fn the_motion_view_scale_is_the_one_the_host_declares() {
        let mesh = include_str!("../shaders/mesh.slang");
        assert_eq!(
            shader_float(mesh, "MOTION_VIEW_SCALE"),
            MOTION_VIEW_SCALE,
            "mesh.slang and this crate disagree about the motion view's scale"
        );
        assert!(
            mesh.contains("motion * MOTION_VIEW_SCALE + 0.5"),
            "mesh.slang no longer encodes with the constant it declares, so this crate's copy is \
             a number nothing applies"
        );
    }

    /// **Both geometry stages carry the previous position the block exists
    /// for**, and the fragment stage subtracts it.
    ///
    /// `the_uniform_block_matches_the_offsets_slangc_emits` says the member is
    /// at the offset the host writes it to; it cannot say that anything reads
    /// it. A `mesh.slang` that declared `previous_view_proj` and never
    /// multiplied by it would leave the renderer advancing a matrix nothing
    /// looks at, and the target would hold a plausible field — the *camera's*
    /// motion missing from every pixel — with no error anywhere. That is the
    /// same gap `the_fragment_stage_still_adds_the_sky_it_declares` exists for.
    ///
    /// Both files, because the two geometry stages feed one fragment stage: a
    /// mesh-shader frame whose previous position came from the current camera
    /// would disagree with a raster frame of the same scene, and only a device
    /// with a mesh stage would ever see it.
    ///
    /// **And the vertex it carries is the previous frame's, not this one's.**
    /// A deformed instance's vertices are rewritten every frame, so the stage
    /// that put *this* frame's deformed vertex through the previous transform
    /// would draw a swinging limb with the motion of the body it hangs off —
    /// a field that is right everywhere a mesh is rigid, which is everywhere
    /// this crate's own scenes look. [`GpuInstance::previous_base_vertex`] is
    /// where the other run is named and the fetch below is what has to read it.
    #[test]
    fn both_geometry_stages_emit_the_previous_clip_position() {
        let mesh = include_str!("../shaders/mesh.slang");
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        for (name, source) in [("mesh.slang", mesh), ("mesh_cluster.slang", cluster)] {
            assert!(
                source.contains("float4x4 previous_view_proj;"),
                "{name} does not declare `previous_view_proj`, so the two blocks are not one \
                 buffer"
            );
            assert!(
                source.contains("mul(instance.previous_transform, float4(previous_position, 1.0))"),
                "{name} declares `previous_view_proj` and never carries a vertex through it"
            );
            assert!(
                !source.contains("mul(instance.previous_transform, float4(vertex.position, 1.0))"),
                "{name} puts this frame's vertex through the previous transform, which is the \
                 reading a skinned instance's deformation is invisible to"
            );
            assert!(
                source.contains("instance.previous_base_vertex"),
                "{name} never reads the base the previous frame's skinning dispatch wrote, so \
                 its previous position cannot be a deformed one"
            );
            assert!(
                source.contains("load_position(index + previous_base"),
                "{name} does not fetch its previous position out of that base"
            );
            assert!(
                source.contains("frame.previous_view_proj,"),
                "{name} transforms the previous position through something other than the \
                 previous frame's camera"
            );
        }
        // And the fragment stage subtracts the pair rather than only receiving
        // it. Two interpolants nothing differences are two interpolants.
        assert!(
            mesh.contains("motion_vector(input.clip_position, input.previous_clip_position)"),
            "mesh.slang no longer subtracts the two clip positions, so the motion target holds \
             whatever the last branch happened to leave"
        );
    }

    /// **The heatmap's threshold lies above the LOD view's**, so one lane can
    /// carry four states.
    ///
    /// The lane now holds off, normals, tint and heatmap, and the sentinels
    /// ascend — so a `3.0` clears all three thresholds and only the *order* of
    /// the tests keeps it out of the two lower branches. The order in
    /// `mesh_cluster.slang` is asserted here rather than left to a reader,
    /// exactly as `the_lod_view_threshold_lies_above_the_normals_view` asserts
    /// the pair below it.
    ///
    /// The occlusion view sits one sentinel further out again and is tested
    /// ahead of both — see `the_occlusion_view_threshold_lies_above_the_heatmap`.
    ///
    /// `mesh.slang` deliberately has **no** `HEATMAP_VIEW`: its fragment stage
    /// treats every overlay the same way — pass the colour through unshaded —
    /// so it needs one threshold for the set, and that is what its `LOD_VIEW`
    /// doc says. The absence is asserted, because a `HEATMAP_VIEW` appearing
    /// there later would be a second place the interleaving has to hold.
    #[test]
    fn the_heatmap_view_threshold_lies_above_the_lod_view() {
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        let heatmap = shader_float(cluster, "HEATMAP_VIEW");
        let tint = shader_float(cluster, "LOD_VIEW");
        assert!(
            FrameUniforms::LOD_VIEW_ON < heatmap && heatmap < FrameUniforms::HEATMAP_VIEW_ON,
            "mesh_cluster.slang switches at {heatmap}, which does not separate the LOD view's \
             {} from the heatmap's {}",
            FrameUniforms::LOD_VIEW_ON,
            FrameUniforms::HEATMAP_VIEW_ON,
        );
        assert!(
            tint < heatmap,
            "the tint's threshold ({tint}) must lie below the heatmap's ({heatmap}), or the \
             heatmap's sentinel never reaches a branch of its own"
        );

        let heat_at = cluster
            .find("if (frame.ambient.w >= HEATMAP_VIEW)")
            .expect("mesh_cluster.slang's mesh stage tests the heatmap");
        let tint_at = cluster
            .find("else if (frame.ambient.w >= LOD_VIEW)")
            .expect("mesh_cluster.slang's mesh stage tests the tint after it");
        assert!(
            heat_at < tint_at,
            "the heatmap must be tested first, or its sentinel is caught by the tint's threshold \
             and the overlay is a mosaic of levels wearing the heatmap's name"
        );
        assert!(
            !include_str!("../shaders/mesh.slang").contains("static const float HEATMAP_VIEW"),
            "mesh.slang has grown a heatmap threshold of its own; the interleaving above now has \
             a second place to hold and this test only checks one of them"
        );
    }

    /// **The heatmap ramp climbs in luminance**, stop by stop, including across
    /// the two deliberate hue breaks.
    ///
    /// That climb is the whole legibility argument for the ramp: the overlay
    /// answers "how close to the budget is this", so it has to be readable as an
    /// ordering, and a rainbow — the usual choice — has none. A reader who
    /// cannot separate teal from amber still reads amber as hotter, and so does
    /// a greyscale screenshot.
    ///
    /// The stops are read out of the shader rather than restated, and the
    /// weights are Rec. 709's, which is the luma the sRGB primaries are defined
    /// against. **The two hue breaks are where this can fail quietly**: the ramp
    /// jumps teal → amber at the hold budget and yellow → white at the expand
    /// budget, so a stop chosen for its hue alone can easily land *below* the
    /// one before it and leave two bands the eye orders backwards.
    #[test]
    fn the_heatmap_ramp_climbs_in_luminance() {
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        let stop = |name: &str| -> [f32; 3] {
            let literal = cluster
                .split_once(&format!("static const float3 {name} = float3("))
                .unwrap_or_else(|| panic!("mesh_cluster.slang declares `{name}`"))
                .1
                .split_once(')')
                .expect("a float3 literal closes its parenthesis")
                .0;
            let channels: Vec<f32> = literal
                .split(',')
                .map(|channel| {
                    channel
                        .trim()
                        .parse()
                        .unwrap_or_else(|_| panic!("`{channel}` of {name} is not a float"))
                })
                .collect();
            channels
                .try_into()
                .unwrap_or_else(|_| panic!("{name} is not three channels"))
        };
        // Rec. 709, the luma weights of the primaries sRGB is defined against.
        let luma = |rgb: [f32; 3]| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        let names = [
            "HEAT_UNDER_LOW",
            "HEAT_UNDER_HIGH",
            "HEAT_BAND_LOW",
            "HEAT_BAND_HIGH",
            "HEAT_OVER",
        ];
        let mut previous = f32::NEG_INFINITY;
        for name in names {
            let rgb = stop(name);
            for channel in rgb {
                assert!(
                    (0.0..=1.0).contains(&channel),
                    "{name} has a channel outside [0, 1]: {rgb:?}"
                );
            }
            let here = luma(rgb);
            assert!(
                here > previous,
                "{name} is at luminance {here}, which does not climb past the stop before it \
                 ({previous}) — the ramp is no longer readable as an ordering"
            );
            previous = here;
        }
        assert_eq!(
            stop("HEAT_OVER"),
            [1.0, 1.0, 1.0],
            "the over-budget stop is white, which is what makes the expand budget the brightest \
             thing in the frame"
        );
    }

    /// **The LOD view's threshold lies above the normals view's**, and both
    /// shaders that declare it agree.
    ///
    /// One float lane carries three states, so the thresholds have to be
    /// ordered as well as separated: a `2.0` clears the normals threshold on its
    /// way past, and only the order of the two tests keeps it out of the normals
    /// branch. That order is asserted here rather than left to a reader.
    #[test]
    fn the_lod_view_threshold_lies_above_the_normals_view() {
        for (name, source) in [
            ("mesh.slang", include_str!("../shaders/mesh.slang")),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
            ),
        ] {
            let literal = source
                .split_once("static const float LOD_VIEW = ")
                .unwrap_or_else(|| panic!("{name} declares the LOD view's threshold"))
                .1
                .split_once(';')
                .expect("a declaration ends in a semicolon")
                .0;
            let threshold: f32 = literal
                .parse()
                .unwrap_or_else(|_| panic!("`{literal}` is not a float"));
            assert!(
                FrameUniforms::NORMALS_VIEW_ON < threshold
                    && threshold < FrameUniforms::LOD_VIEW_ON,
                "{name} switches at {threshold}, which does not separate the \
             normals view's {} from the LOD view's {}",
                FrameUniforms::NORMALS_VIEW_ON,
                FrameUniforms::LOD_VIEW_ON,
            );
        }

        let mesh = include_str!("../shaders/mesh.slang");
        let lod = mesh
            .find("if (frame.ambient.w >= LOD_VIEW)")
            .expect("mesh.slang's fragment stage tests the LOD view");
        let normals = mesh
            .find("if (frame.ambient.w >= NORMALS_VIEW)")
            .expect("mesh.slang's fragment stage tests the normals view");
        assert!(
            lod < normals,
            "the LOD test must come first, or a frame asking for it is caught by \
         the normals threshold instead"
        );
    }

    /// **The shader's normals-view threshold really separates the two values the
    /// host writes**, and it is `ambient`'s `w` that it compares.
    ///
    /// A threshold outside the pair has no symptom on one side and is therefore
    /// the drift worth a test: at or below
    /// [`FrameUniforms::NORMALS_VIEW_OFF`] every frame the engine draws is the
    /// debug view, and above [`FrameUniforms::NORMALS_VIEW_ON`] the key does
    /// nothing and the renderer's own block test still passes, because that one
    /// is about the bytes and this one is about what the shader does with them.
    #[test]
    fn the_normals_view_threshold_lies_between_the_two_values_the_host_writes() {
        let mesh = include_str!("../shaders/mesh.slang");
        let literal = mesh
            .split_once("static const float NORMALS_VIEW = ")
            .expect("mesh.slang declares the normals view's threshold")
            .1
            .split_once(';')
            .expect("a declaration ends in a semicolon")
            .0;
        let threshold: f32 = literal
            .parse()
            .unwrap_or_else(|_| panic!("`{literal}` is not a float"));
        assert!(
            FrameUniforms::NORMALS_VIEW_OFF < threshold
                && threshold < FrameUniforms::NORMALS_VIEW_ON,
            "the shader switches at {threshold}, which does not separate \
             {} from {}",
            FrameUniforms::NORMALS_VIEW_OFF,
            FrameUniforms::NORMALS_VIEW_ON,
        );

        assert!(
            mesh.contains("if (frame.ambient.w >= NORMALS_VIEW)"),
            "mesh.slang no longer compares the ambient's `w` against the threshold, so this crate \
             is writing the switch into a lane nothing reads"
        );
    }

    #[test]
    fn the_uniform_block_matches_the_offsets_slangc_emits() {
        assert_eq!(
            FRAME_UNIFORMS_SIZE, 1648,
            "at two cascades, fourteen light tiles and sixteen atlas rectangles"
        );
        // `OpMemberDecorate %FrameUniforms_std140 n Offset …` — 0, 64, 80, 96,
        // 224, 240, 256, 272, 1168, 1184, 1200, 1216, 1232, 1248, 1264, 1280,
        // 1296, 1312, 1376, 1392 — and
        // `OpDecorate %_arr_mat4v4float_int_2 ArrayStride 64` beside
        // `%_arr_mat4v4float_int_14`, which is the light array's own length, and
        // `%_arr_v4float_int_16 ArrayStride 16`, which is the atlas
        // rectangles'. Three of the middle rows are the grid header's, which
        // this side writes as one group; then the fog's two and the sky's three.
        // Read out of `spirv/mesh.spv` with `spirv-dis`, not derived from the
        // arithmetic below — that is the point of them.
        let cascades = 64 * SHADOW_CASCADES;
        let lights = 64 * SHADOW_LIGHT_TILES;
        let rects = 16 * SHADOW_ATLAS_TILES;
        let offsets = [
            0usize,
            64,
            80,
            96,
            96 + cascades,
            112 + cascades,
            128 + cascades,
            144 + cascades,
            144 + cascades + lights,
            144 + cascades + lights + PROBE_VOLUME_SIZE,
            160 + cascades + lights + PROBE_VOLUME_SIZE,
            176 + cascades + lights + PROBE_VOLUME_SIZE,
            192 + cascades + lights + PROBE_VOLUME_SIZE,
            208 + cascades + lights + PROBE_VOLUME_SIZE,
            224 + cascades + lights + PROBE_VOLUME_SIZE,
            240 + cascades + lights + PROBE_VOLUME_SIZE,
            304 + cascades + lights + PROBE_VOLUME_SIZE,
            320 + cascades + lights + PROBE_VOLUME_SIZE,
        ];
        let sizes = [
            64usize,
            16,
            16,
            cascades,
            16,
            16,
            16,
            lights,
            PROBE_VOLUME_SIZE,
            16,
            16,
            16,
            16,
            16,
            16,
            64,
            16,
            rects,
        ];
        for (index, (offset, size)) in offsets.iter().zip(&sizes).enumerate() {
            assert_eq!(
                offset + size,
                offsets
                    .get(index + 1)
                    .copied()
                    .unwrap_or(FRAME_UNIFORMS_SIZE),
                "member {index} is followed by a gap the CPU side does not write"
            );
        }

        // A different value per member, and a different one per *cascade*: a
        // `to_bytes` that wrote one matrix twice would pass a test whose
        // cascades were equal, and the cascade array is the member this block
        // grew.
        let mut shadow_view_proj = [[0.0f32; 16]; SHADOW_CASCADES];
        for (index, matrix) in shadow_view_proj.iter_mut().enumerate() {
            *matrix = [7.0 + index as f32; 16];
        }
        // The same again for the light slots, and in a range nothing above uses:
        // a `to_bytes` that wrote the cascade array where the light array goes
        // would otherwise pass on values that happened to be equal.
        let mut light_view_proj = [[0.0f32; 16]; SHADOW_LIGHT_TILES];
        for (index, matrix) in light_view_proj.iter_mut().enumerate() {
            *matrix = [50.0 + index as f32; 16];
        }
        let uniforms = FrameUniforms {
            view_proj: [1.0; 16],
            camera_position: [3.0; 4],
            ambient: [6.0; 4],
            shadow_view_proj,
            cascade_far: [20.0; 4],
            shadow_params: [30.0; 4],
            cluster_grid: [41, 42, 43, 44],
            light_view_proj,
            probes: ProbeVolume {
                origin: [60.0, 61.0, 62.0],
                inv_spacing: [63.0, 64.0, 65.0],
                counts: [2, 3, 4],
            },
            lod_params: [70.0, 71.0, 72.0, 73.0],
            fog_params: [80.0, 81.0, 82.0, 83.0],
            fog_color: [90.0, 91.0, 92.0, 93.0],
            sky_sh_r: [100.0, 101.0, 102.0, 103.0],
            sky_sh_g: [110.0, 111.0, 112.0, 113.0],
            sky_sh_b: [120.0, 121.0, 122.0, 123.0],
            // A range nothing above uses, and every lane of it filled: a
            // `to_bytes` that wrote `view_proj` here — which is what a renderer
            // that forgot to advance the camera's history would upload — is the
            // one mistake this member has, and equal values would hide it.
            previous_view_proj: core::array::from_fn(|lane| 130.0 + lane as f32),
            vertex_pool: [150, 151, 152, 153],
            // A distinct value per slot *and* per lane, in a range nothing
            // above uses: a rectangle is four numbers of one width, so a writer
            // that transposed the scale and the offset, or wrote one slot's row
            // into another's, would put a map somewhere plausible in the atlas
            // and equal values would hide both.
            shadow_atlas_rect: core::array::from_fn(|slot| {
                core::array::from_fn(|lane| 200.0 + 4.0 * slot as f32 + lane as f32)
            }),
        };
        let bytes = uniforms.to_bytes();
        let at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let word_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(at(0), 1.0, "view_proj at offset 0");
        assert_eq!(at(64), 3.0, "camera_position at offset 64");
        assert_eq!(at(80), 6.0, "ambient at offset 80");
        for index in 0..SHADOW_CASCADES {
            assert_eq!(
                at(96 + 64 * index),
                7.0 + index as f32,
                "shadow_view_proj[{index}] at offset {}",
                96 + 64 * index
            );
        }
        assert_eq!(at(96 + cascades), 20.0, "cascade_far");
        assert_eq!(at(112 + cascades), 30.0, "shadow_params");
        // Every lane, not just the first: the grid's three numbers are all
        // `uint`s of the same width and would permute silently — a fragment
        // reading the slice count as the tile stride is a picture, not an error.
        for (lane, expected) in [41u32, 42, 43, 44].into_iter().enumerate() {
            assert_eq!(
                word_at(128 + cascades + lane * 4),
                expected,
                "cluster_grid lane {lane}"
            );
        }
        // And the light tiles, which sit **after** the integer member — the one
        // place `to_bytes` writes through a second cursor, and so the one place
        // a member could land on the wrong side of it.
        for index in 0..SHADOW_LIGHT_TILES {
            assert_eq!(
                at(144 + cascades + 64 * index),
                50.0 + index as f32,
                "light_view_proj[{index}] at offset {}",
                144 + cascades + 64 * index
            );
        }
        // And the grid header past the end of them, row by row: three `std140`
        // rows with a padding lane in each of the first two, and the total in
        // the last one's.
        let probes = 144 + cascades + lights;
        assert_eq!(
            [at(probes), at(probes + 4), at(probes + 8)],
            [60.0, 61.0, 62.0],
            "the probe grid's origin"
        );
        assert_eq!(
            [at(probes + 16), at(probes + 20), at(probes + 24)],
            [63.0, 64.0, 65.0],
            "the probe grid's reciprocal spacing"
        );
        assert_eq!(
            [
                word_at(probes + 32),
                word_at(probes + 36),
                word_at(probes + 40),
                word_at(probes + 44)
            ],
            [2, 3, 4, 24],
            "the probe grid's counts and their product"
        );
        // And the selection numbers past the header, which is where the block
        // now ends. Every lane, because all four are `float`s of one width and
        // a permutation between the two budgets is an overlay whose ramp is
        // anchored on the wrong one — a picture, not an error.
        let lod = probes + PROBE_VOLUME_SIZE;
        assert_eq!(
            [at(lod), at(lod + 4), at(lod + 8), at(lod + 12)],
            [70.0, 71.0, 72.0, 73.0],
            "the selection numbers"
        );
        // And the two fog rows past them, which is where the block now ends.
        // Every lane again: the density, the falloff and the reference height
        // are three `float`s of one width, and a permutation between them is a
        // frame that fogs by the wrong law rather than one that fails.
        let fog = lod + 16;
        assert_eq!(
            [at(fog), at(fog + 4), at(fog + 8), at(fog + 12)],
            [80.0, 81.0, 82.0, 83.0],
            "the fog parameters"
        );
        assert_eq!(
            [at(fog + 16), at(fog + 20), at(fog + 24), at(fog + 28)],
            [90.0, 91.0, 92.0, 93.0],
            "the fog colour"
        );
        // And the sky's three rows past them, which is where the block now
        // ends. Every lane once more, and here the reason is sharper than
        // elsewhere: all twelve numbers are coefficients of one basis, so a
        // channel written into the wrong row is a sky of the wrong colour and a
        // lane written into the wrong slot is a sky lit from the wrong
        // direction. Neither is an error anything would raise.
        let sky = fog + 32;
        for (row, base) in [(0usize, 100.0f32), (1, 110.0), (2, 120.0)] {
            let at_row = sky + row * 16;
            assert_eq!(
                [at(at_row), at(at_row + 4), at(at_row + 8), at(at_row + 12)],
                [base, base + 1.0, base + 2.0, base + 3.0],
                "the sky's spherical-harmonic row {row}"
            );
        }
        // And the previous frame's camera past them, which is where the block
        // now ends. Lane by lane, because a matrix written transposed or one
        // column short is a reprojection that lands every pixel somewhere
        // plausible and wrong — and no picture of a still scene tells it from
        // the right one.
        let previous = sky + 48;
        for lane in 0..16 {
            assert_eq!(
                at(previous + lane * 4),
                130.0 + lane as f32,
                "lane {lane} of the previous frame's view-projection"
            );
        }
        // And the pool's boundary past that, which is where the block now ends.
        // Every lane, though only `x` is read: three `uint`s of padding written
        // as something else would be a block whose *size* the two sides agree
        // about and whose tail the shader is free to read.
        let pool = previous + 64;
        for (lane, expected) in [150u32, 151, 152, 153].into_iter().enumerate() {
            assert_eq!(
                word_at(pool + lane * 4),
                expected,
                "vertex_pool lane {lane}"
            );
        }
        // And the atlas's rectangles past that, which is where the block now
        // ends. Slot by slot and lane by lane, because a rectangle is four
        // `float`s of one width: a scale swapped with an offset puts a map in
        // the corner of the atlas at four times its size, and a slot's row
        // written into another slot's samples another light's map — both
        // pictures, and neither an error.
        let atlas = pool + 16;
        for slot in 0..SHADOW_ATLAS_TILES {
            let row = atlas + slot * 16;
            let base = 200.0 + 4.0 * slot as f32;
            assert_eq!(
                [at(row), at(row + 4), at(row + 8), at(row + 12)],
                [base, base + 1.0, base + 2.0, base + 3.0],
                "the atlas rectangle of slot {slot}"
            );
        }
        assert_eq!(
            atlas + 16 * SHADOW_ATLAS_TILES,
            FRAME_UNIFORMS_SIZE,
            "the rectangles are not what ends the block"
        );
    }

    /// The offsets `slangc` actually emitted for `GpuInstance`, read out of the
    /// disassembly. The trailing ids are all the same width and would silently
    /// permute — a mesh id read as a material id is a picture, not a crash, and
    /// a base vertex read as the previous frame's is a motion field that looks
    /// plausible — so the byte each lands on is pinned rather than assumed.
    #[test]
    fn the_instance_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_GpuInstance_std430 ArrayStride 160`, and
        // `OpMemberDecorate %GpuInstance_std430 n Offset …`.
        assert_eq!(INSTANCE_STRIDE, 160);
        assert_eq!(
            INSTANCE_STRIDE % 16,
            0,
            "a std430 struct containing a float4x4 is 16-byte aligned, so its \
             stride must be a multiple of 16 or every element after the first \
             lands short"
        );

        let instance = GpuInstance {
            transform: [1.0; 16],
            previous_transform: [7.0; 16],
            mesh: 2,
            material: 3,
            sector: 4,
            flags: 5,
            base_vertex: 6,
            previous_base_vertex: 7,
        };
        let bytes = instance.to_bytes();
        assert_eq!(bytes.len(), INSTANCE_STRIDE);
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(float_at(0), 1.0, "transform at offset 0");
        assert_eq!(float_at(60), 1.0, "and it is 64 bytes wide");
        assert_eq!(float_at(64), 7.0, "previous_transform at offset 64");
        assert_eq!(float_at(124), 7.0, "and it is 64 bytes wide too");
        assert_eq!(uint_at(128), 2, "mesh at offset 128");
        assert_eq!(uint_at(132), 3, "material at offset 132");
        assert_eq!(uint_at(136), 4, "sector at offset 136");
        assert_eq!(uint_at(140), 5, "flags at offset 140");
        assert_eq!(uint_at(144), 6, "base_vertex at offset 144");
        assert_eq!(uint_at(148), 7, "previous_base_vertex at offset 148");
        // The lanes `std430`'s rounding-up adds are padding, and a record that
        // wrote a field into them would be a field the shader reads at another
        // offset. Written rather than assumed: this buffer is uploaded whole.
        let pad = INSTANCE_STRIDE - INSTANCE_PAD_WORDS * size_of::<u32>();
        assert_eq!(pad, 152, "the last field ends at byte 152");
        assert!(
            bytes[pad..].iter().all(|byte| *byte == 0),
            "the tail past the last field is padding and is zero: {:?}",
            &bytes[pad..]
        );

        // And the decode agrees with the encode, field for field — six `u32`s
        // in a row would permute silently, and this is what says they did not.
        assert_eq!(GpuInstance::from_bytes(&bytes), instance);
    }

    /// The base-vertex override is bit 1 of the flags word, it is not the
    /// liveness bit, and a default record has neither.
    ///
    /// The default is again the half that matters: an element nothing has
    /// written must not read as "draw out of pool vertex 0", which is what a
    /// sentinel in the field itself would have made it — see
    /// [`GpuInstance::BASE_VERTEX_OVERRIDE`], where that choice is argued.
    #[test]
    fn a_default_instance_overrides_no_base_vertex() {
        assert_eq!(GpuInstance::BASE_VERTEX_OVERRIDE, 2);
        assert_eq!(
            GpuInstance::BASE_VERTEX_OVERRIDE & GpuInstance::LIVE,
            0,
            "the two bits are separate, or setting one would set the other"
        );
        assert_eq!(
            GpuInstance::default().flags & GpuInstance::BASE_VERTEX_OVERRIDE,
            0
        );
        assert_eq!(GpuInstance::default().base_vertex, 0);
        assert_eq!(GpuInstance::default().previous_base_vertex, 0);

        // And the bit survives the round trip through the bytes a shader reads,
        // beside a base a shader would resolve.
        let deformed = GpuInstance {
            flags: GpuInstance::LIVE | GpuInstance::BASE_VERTEX_OVERRIDE,
            base_vertex: 1024,
            ..GpuInstance::default()
        };
        let bytes = deformed.to_bytes();
        assert_eq!(
            u32::from_le_bytes(bytes[140..144].try_into().expect("4"))
                & GpuInstance::BASE_VERTEX_OVERRIDE,
            GpuInstance::BASE_VERTEX_OVERRIDE
        );
        assert_eq!(
            u32::from_le_bytes(bytes[144..148].try_into().expect("4")),
            1024
        );
        assert_eq!(GpuInstance::from_bytes(&bytes), deformed);
    }

    /// The liveness bit is bit 0 of the flags word, and a default record does
    /// not have it.
    ///
    /// The default is the half that matters: an element nothing has written is
    /// all zeroes, and this is what says such an element reads as *dead* rather
    /// than as a live instance at the origin drawing mesh 0.
    #[test]
    fn a_default_instance_is_not_live() {
        assert_eq!(GpuInstance::LIVE, 1);
        assert_eq!(GpuInstance::default().flags & GpuInstance::LIVE, 0);
        assert_eq!(GpuInstance::default().to_bytes(), [0u8; INSTANCE_STRIDE]);

        // And the bit survives the round trip through the bytes a shader reads,
        // at the offset the layout test pins the flags word to.
        let live = GpuInstance {
            flags: GpuInstance::LIVE,
            ..GpuInstance::default()
        };
        let bytes = live.to_bytes();
        assert_eq!(
            u32::from_le_bytes(bytes[140..144].try_into().expect("4")),
            GpuInstance::LIVE
        );
        assert_eq!(GpuInstance::from_bytes(&bytes), live);
    }

    /// The transform is written the way `glam` produces a column-major matrix,
    /// with no transpose — the same contract `FrameUniforms::view_proj` has,
    /// and the one the shader header explains. A transposed instance transform
    /// is a plausible picture rather than an obviously broken one.
    #[test]
    fn the_instance_transform_is_written_in_glam_order() {
        let mut transform = [0.0f32; 16];
        for (index, value) in transform.iter_mut().enumerate() {
            *value = index as f32;
        }
        let bytes = GpuInstance {
            transform,
            ..GpuInstance::default()
        }
        .to_bytes();
        for index in 0..16 {
            let at = index * 4;
            assert_eq!(
                f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4")),
                index as f32,
                "element {index} did not land at byte {at}"
            );
        }
    }

    /// The record is a `float3` and five `uint`s, with no padding on either
    /// side of the boundary the pool splits at.
    #[test]
    fn the_vertex_layout_is_two_streams_with_no_padding() {
        let bytes = cube_vertex_bytes();
        assert_eq!(POSITION_STRIDE, 3 * size_of::<f32>());
        assert_eq!(ATTRIBUTE_STRIDE, 5 * size_of::<u32>());
        assert_eq!(VERTEX_STRIDE, POSITION_STRIDE + ATTRIBUTE_STRIDE);
        assert_eq!(bytes.len(), CUBE_VERTEX_COUNT * VERTEX_STRIDE);
        assert_eq!(cube_vertices().len(), CUBE_VERTEX_COUNT);
        assert_eq!(cube_indices().len(), CUBE_INDEX_COUNT);
        assert_eq!(cube_index_bytes().len(), CUBE_INDEX_COUNT * 4);
    }

    /// **The UV lanes are the third attribute word**, and every mesh really
    /// carries a coordinate that spans its layer.
    ///
    /// Read out of the packed bytes rather than off the struct: the record is
    /// built by two functions and one that dropped a field would produce a
    /// buffer of the right *length* only by accident — and would silently give
    /// every fragment the UV of the vertex after it. Both meshes are checked
    /// because they are packed by two call sites.
    ///
    /// Decoded through [`demo_uv_range`] the way the shader does, so what is
    /// asserted is the coordinate a fragment actually receives rather than the
    /// lanes on the way to it.
    #[test]
    fn every_vertex_carries_its_uv_in_the_third_attribute_word() {
        let range = demo_uv_range();
        for (name, vertices, bytes) in [
            ("cube", cube_vertices(), cube_vertex_bytes()),
            ("pyramid", pyramid_vertices(), pyramid_vertex_bytes()),
        ] {
            for (index, vertex) in vertices.iter().enumerate() {
                let at = index * VERTEX_STRIDE + POSITION_STRIDE + 8;
                let word = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
                assert_eq!(
                    [word as u16, (word >> 16) as u16],
                    vertex.uv0,
                    "{name} vertex {index}'s uv is not at byte {at}"
                );
            }
            // And the coordinates actually span the layer rather than sitting
            // at one corner, which is what makes a page layer visible over a
            // whole face instead of as a single flat colour.
            let decoded: Vec<[f32; 2]> = vertices
                .iter()
                .map(|vertex| range.decode(vertex.uv0))
                .collect();
            for (axis, name_of) in [(0usize, "u"), (1, "v")] {
                let lane: Vec<f32> = decoded.iter().map(|uv| uv[axis]).collect();
                assert!(
                    lane.contains(&0.0) && lane.contains(&1.0),
                    "{name} never reaches both edges of the layer in {name_of}"
                );
            }
        }
    }

    /// Every coordinate this module's meshes carry survives the trip through
    /// [`demo_uv_range`], which is what says that one range really does cover
    /// all three.
    ///
    /// The open box is the one that could drift: its coordinates are generated
    /// from [`OPEN_BOX_SUBDIVISIONS`] rather than read out of a table, so a
    /// subdivision count that stopped reaching the edges — or a face laid out
    /// past them — would be a mesh quantised against a range that does not
    /// contain it, and every lane would clamp to an edge instead.
    #[test]
    fn the_demo_meshes_are_authored_on_the_range_they_declare() {
        let range = demo_uv_range();
        assert_eq!(range, UvRange::from_uvs(&[[0.0, 0.0], [1.0, 1.0]]));
        let step = 1.0 / OPEN_BOX_SUBDIVISIONS as f32;
        let intended: Vec<(&str, Vec<[f32; 2]>)> = vec![
            (
                "cube",
                FACES.iter().flat_map(|_| QUAD_UV).collect::<Vec<_>>(),
            ),
            (
                "pyramid",
                QUAD_UV
                    .into_iter()
                    .chain(
                        PYRAMID_SIDE_COLORS
                            .iter()
                            .flat_map(|_| TRIANGLE_UV.into_iter()),
                    )
                    .collect(),
            ),
            (
                "open box",
                OPEN_BOX_FACES
                    .iter()
                    .flat_map(|_| {
                        (0..OPEN_BOX_SUBDIVISIONS).flat_map(move |row| {
                            (0..OPEN_BOX_SUBDIVISIONS).flat_map(move |column| {
                                QUAD_UV.into_iter().map(move |uv| {
                                    [(column as f32 + uv[0]) * step, (row as f32 + uv[1]) * step]
                                })
                            })
                        })
                    })
                    .collect(),
            ),
        ];
        for ((name, coordinates), vertices) in
            intended
                .into_iter()
                .zip([cube_vertices(), pyramid_vertices(), open_box_vertices()])
        {
            assert_eq!(
                coordinates.len(),
                vertices.len(),
                "{name} carries a different number of coordinates than vertices"
            );
            for (index, (uv, vertex)) in coordinates.iter().zip(&vertices).enumerate() {
                let decoded = range.decode(vertex.uv0);
                for axis in 0..2 {
                    assert!(
                        (decoded[axis] - uv[axis]).abs() <= UvRange::MAX_RELATIVE_ERROR,
                        "{name} vertex {index} decodes {decoded:?} where it was authored {uv:?}"
                    );
                }
            }
        }
    }

    /// Every face is wound counter-clockwise **as seen from outside**, which is
    /// what makes back-face culling legal. Checked by the right-hand rule
    /// rather than by eye: the cross product of two consecutive edges must point
    /// the same way as the declared normal.
    #[test]
    fn every_face_is_wound_counter_clockwise_from_outside() {
        for face in &FACES {
            let [a, b, c, d] = face.corners;
            let edge = |p: [f32; 3], q: [f32; 3]| [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            let (u, v) = (edge(a, b), edge(a, c));
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let dot = cross
                .iter()
                .zip(&face.normal)
                .map(|(l, r)| l * r)
                .sum::<f32>();
            assert!(
                dot > 0.0,
                "face {} is wound clockwise from outside, so back-face culling \
                 would delete it: cross {cross:?} vs normal {:?}",
                face.name,
                face.normal
            );

            // And the fourth corner is coplanar with the first three, or the
            // "quad" is two triangles that do not meet.
            let w = edge(a, d);
            let out_of_plane = w
                .iter()
                .zip(&face.normal)
                .map(|(l, r)| l * r)
                .sum::<f32>()
                .abs();
            assert!(out_of_plane < 1e-6, "face {} is not planar", face.name);
        }
    }

    /// The diagnostic property the golden images depend on: no two faces share
    /// a colour, and every normal is a unit axis.
    #[test]
    fn faces_are_individually_identifiable() {
        for (index, face) in FACES.iter().enumerate() {
            let length = face.normal.iter().map(|c| c * c).sum::<f32>().sqrt();
            assert!(
                (length - 1.0).abs() < 1e-6,
                "face {} normal is not unit length",
                face.name
            );
            for other in &FACES[index + 1..] {
                assert_ne!(
                    face.color, other.color,
                    "{} and {} share a colour, so a mirrored axis would be invisible",
                    face.name, other.name
                );
            }
        }
        // Opposite faces exist in both directions on every axis, so no
        // orientation is unrepresented.
        let mut normals: Vec<[f32; 3]> = FACES.iter().map(|face| face.normal).collect();
        normals.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a constant"));
        normals.dedup();
        assert_eq!(normals.len(), 6);
    }

    /// Every index addresses a vertex that exists. A cube is small enough that
    /// an off-by-one would still render *something*, which is why this is
    /// checked rather than eyeballed.
    #[test]
    fn every_index_is_in_range() {
        let vertices = cube_vertices().len();
        for index in cube_indices() {
            assert!(
                (index as usize) < vertices,
                "index {index} addresses past the {vertices} vertices"
            );
        }
    }

    /// The cube really is centred on the origin and half a unit across, which is
    /// what lets the model matrix stay a pure rotation.
    #[test]
    fn the_cube_is_centred_on_the_origin() {
        let vertices = cube_vertices();
        for axis in 0..3 {
            let sum: f32 = vertices.iter().map(|v| v.position[axis]).sum();
            assert!(sum.abs() < 1e-5, "axis {axis} is off-centre by {sum}");
            let max = vertices
                .iter()
                .map(|v| v.position[axis].abs())
                .fold(0.0f32, f32::max);
            assert!((max - H).abs() < 1e-6, "axis {axis} half-extent is {max}");
        }
    }

    /// The stretch of a shader source that decodes a vertex: from the first
    /// stream constant to the close of `struct MeshVertex`, with `//` comments
    /// dropped and runs of whitespace collapsed.
    ///
    /// Comments are dropped deliberately, on
    /// `crcbl_shaders::volumetric::one_function`'s terms: what has to agree
    /// between three copies is the arithmetic, and a copy that says in its own
    /// words why it exists is a copy doing its job.
    fn decode_block(source: &str) -> String {
        let start = source
            .find("static const uint POSITION_WORDS")
            .expect("the shader declares the position stream's width");
        let tail = &source[start..];
        let struct_at = tail
            .find("struct MeshVertex")
            .expect("the shader declares the decoded vertex");
        let end = struct_at
            + tail[struct_at..]
                .find("\n};")
                .expect("the decoded vertex closes")
            + 3;
        tail[..end]
            .lines()
            .map(|line| line.split("//").next().unwrap_or(line))
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Every shader that touches the vertex pool spells the decode the same
    /// way.
    ///
    /// `mesh.slang` pulls the pool, `mesh_cluster.slang` pulls the same pool
    /// from a mesh stage, and `skinning.slang` writes it — and there is no
    /// shared header, because the compile script hashes one source per artifact
    /// and an `#include` would be a file whose edits nothing downstream
    /// notices. A drift is not a compile error in any of the three: every file
    /// builds, and one pass reads the bytes another wrote as different numbers.
    ///
    /// The two stream widths are checked against this module's strides as well,
    /// because a shader stepping by four words where the pool strides five is
    /// the same defect arriving from the other side — and no amount of
    /// agreement *between* the shaders would see it.
    #[test]
    fn every_shader_decodes_a_vertex_the_same_way() {
        const MESH: &str = include_str!("../shaders/mesh.slang");
        const CLUSTER: &str = include_str!("../shaders/mesh_cluster.slang");
        const SKINNING: &str = include_str!("../shaders/skinning.slang");

        let declared = decode_block(MESH);
        assert!(
            declared.contains("decode_qtangent") && declared.contains("unpack_unorm16x2"),
            "the block matched in mesh.slang carries no decode, so this comparison \
             checked nothing: {declared}"
        );
        for (name, source) in [
            ("mesh_cluster.slang", CLUSTER),
            ("skinning.slang", SKINNING),
        ] {
            assert_eq!(
                declared,
                decode_block(source),
                "the vertex decode differs between mesh.slang and {name}; one pass would \
                 read the vertex pool with arithmetic another wrote it with"
            );
        }

        // The loaders are `mesh.slang`'s and `mesh_cluster.slang`'s alone —
        // `skinning.slang` addresses the pool through its own block rather than
        // a frame one — so they are compared over the two files that have them.
        for line in [
            "uint word = at * POSITION_WORDS;",
            "uint word = frame.vertex_pool.x + at * ATTRIBUTE_WORDS;",
            "vertex.uv0 = range.zw + range.xy * unpack_unorm16x2(vertices[word + 2]);",
        ] {
            for (name, source) in [("mesh.slang", MESH), ("mesh_cluster.slang", CLUSTER)] {
                assert!(
                    source.contains(line),
                    "{name} does not address the pool with `{line}`"
                );
            }
        }

        for (words, stride) in [
            ("POSITION_WORDS", POSITION_STRIDE),
            ("ATTRIBUTE_WORDS", ATTRIBUTE_STRIDE),
        ] {
            let spelling = format!("static const uint {words} = {};", stride / 4);
            for (name, source) in [
                ("mesh.slang", MESH),
                ("mesh_cluster.slang", CLUSTER),
                ("skinning.slang", SKINNING),
            ] {
                assert!(
                    source.contains(&spelling),
                    "{name} does not declare `{spelling}`, so it walks the pool at a stride \
                     this module does not write it at"
                );
            }
        }
    }

    /// The comparison above must be able to see a difference, which a matcher
    /// that returned the same thing for every input could not.
    #[test]
    fn the_decode_comparison_notices_a_changed_line() {
        const MESH: &str = include_str!("../shaders/mesh.slang");
        let drifted = MESH.replace("/ 65535.0", "/ 65536.0");
        assert_ne!(
            decode_block(MESH),
            decode_block(&drifted),
            "a changed divisor is invisible to the comparison"
        );
        // And a reworded comment is not a difference, or the guard is one
        // authors route around.
        let recommented = MESH.replace("// low lane first.", "// the low lane first.");
        assert_eq!(decode_block(MESH), decode_block(&recommented));
    }

    /// One generated entry point's function body: from the line that opens it
    /// to the `}` in the first column that closes it.
    ///
    /// Both text targets indent everything inside a function, so a brace at the
    /// margin is the close and nothing else is — which is what lets one matcher
    /// read WGSL and MSL alike. The name is matched with its opening
    /// parenthesis so `vertexMain(` cannot be found inside `depthVertexMain(`,
    /// and both artifacts spell an entry point's name verbatim (this crate's
    /// `every_shipped_shader_has_wgsl_naming_the_same_entry_points` is what
    /// holds them to it).
    fn entry_point_body<'a>(artifact: &'a str, name: &str) -> &'a str {
        let opening = format!("{name}(");
        let at = artifact
            .find(&opening)
            .unwrap_or_else(|| panic!("the artifact declares `{name}`"));
        let end = at
            + artifact[at..]
                .find("\n}\n")
                .unwrap_or_else(|| panic!("`{name}` closes at the margin"));
        &artifact[at..end]
    }

    /// **`depthVertexMain` fetches the position stream and nothing else**,
    /// which is the whole of what `docs/plan/43-render-standards.md` §2's split
    /// vertex pool was for.
    ///
    /// The claim is about the code a driver compiles, so it is asked of the
    /// committed artifacts rather than of the Slang source: `load_vertex` is
    /// the only reader of the attribute region — `every_shader_decodes_a_vertex_the_same_way`
    /// above pins the arithmetic that makes it so — and `previous_transform` is
    /// the motion vector's, which costs the second `load_position` a depth pass
    /// has no use for. `vertexMain` is read beside it in the same artifact so
    /// the matcher is shown to be able to find both, which a body that stopped
    /// at the wrong brace could not.
    ///
    /// SPIR-V and DXIL are not read here because neither is text and both are
    /// compiled from this same source by the same script; what a target-specific
    /// codegen could get wrong is the *name*, and that is
    /// [`crate`]'s cross-target entry-point tests.
    #[test]
    fn the_depth_entry_point_fetches_the_position_stream_alone() {
        let shader = &crate::MESH;
        assert!(
            shader
                .entry_points()
                .iter()
                .any(|entry| entry.name() == "depthVertexMain"
                    && entry.stage() == crate::Stage::Vertex),
            "the committed manifest exposes no `depthVertexMain` vertex stage, so \
             `crcbl_render::forward`'s depth pipeline has nothing to name"
        );
        let artifacts = [
            ("wgsl/mesh.wgsl", shader.wgsl().expect("mesh commits WGSL")),
            ("msl/mesh.metal", shader.msl().expect("mesh commits MSL")),
        ];
        for (name, artifact) in artifacts {
            let depth = entry_point_body(artifact, "depthVertexMain");
            let color = entry_point_body(artifact, "vertexMain");
            // **Both spellings, because inlining moves the evidence.** A body
            // that decodes a whole vertex either calls `load_vertex` or carries
            // that function's own address arithmetic, and `vertex_pool` — the
            // word where the attribute region begins — is the whole of that
            // arithmetic. Checking only the call would pass an inlined one.
            assert!(
                !depth.contains("load_vertex") && !depth.contains("vertex_pool"),
                "{name}: `depthVertexMain` reaches into the attribute region, which is the \
                 {ATTRIBUTE_STRIDE} bytes a vertex it exists to skip"
            );
            assert!(
                !depth.contains("previous_transform"),
                "{name}: `depthVertexMain` builds a motion vector, which costs a second \
                 position fetch for a varying no fragment stage reads"
            );
            // And it does fetch a position, so an empty body — or one the
            // matcher cut at the wrong brace — is not read as a pass.
            assert!(
                depth.contains("load_position") || depth.contains("vertices"),
                "{name}: `depthVertexMain` reads no geometry at all, so this checked nothing"
            );
            assert!(
                (color.contains("load_vertex") || color.contains("vertex_pool"))
                    && color.contains("previous_transform"),
                "{name}: `vertexMain` reads neither the attribute region nor the previous \
                 transform, so the comparison above cannot tell the two entry points apart"
            );
        }
    }

    /// The offsets `slangc` emitted for `DrawConstants`, read out of the
    /// disassembly, and the padding that makes the block's width the same
    /// number on both sides.
    #[test]
    fn the_draw_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %DrawConstants_std140 n Offset …`: 0, 4, 8, 12.
        assert_eq!(DRAW_CONSTANTS_SIZE, 16);
        assert_eq!(
            DRAW_CONSTANTS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a \
             block that is not one already is a block the shader and the CPU \
             disagree about the width of"
        );
        let bytes = DrawConstants { base: 1, mesh: 2 }.to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "base at offset 0");
        assert_eq!(uint_at(4), 2, "mesh at offset 4");
        for pad in [8, 12] {
            assert_eq!(
                uint_at(pad),
                0,
                "the pad at {pad} is written, and it is zero"
            );
        }
    }

    /// The offsets and the stride `slangc` actually emitted for `GpuMesh`, read
    /// out of the disassembly. Nine scalars in a row would permute silently — a
    /// base index read as a base vertex draws *something*, and a swapped bounds
    /// corner culls a mesh that is on screen — so the byte each lands on is
    /// pinned rather than assumed.
    ///
    /// The stride is the half worth pinning hardest. A `std430` struct of
    /// scalars is 4-byte aligned, so 36 is legal and so is any implementation
    /// that rounded it to 48; the entry the CPU writes at `index * 36` and the
    /// entry a shader reads at `index * 48` are the same for element 0 and
    /// different for every element after it, which is the mesh-pool bug that
    /// only a second resident can show.
    #[test]
    fn the_mesh_entry_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_GpuMesh_std430 ArrayStride 56`, and
        // `OpMemberDecorate %GpuMesh_std430 n Offset …`: 0, 4, 8, 12, 16, 20,
        // 24, 28, 32, 36, 40, 44, 48, 52. The WGSL and the MSL declare the same
        // fourteen scalars with no explicit alignment, which is the same
        // layout.
        assert_eq!(MESH_ENTRY_STRIDE, 56);

        let entry = GpuMesh {
            base_vertex: 24,
            base_index: 36,
            index_count: 18,
            bounds_min: [-1.0, -2.0, -3.0],
            bounds_max: [4.0, 5.0, 6.0],
            uv_range: UvRange {
                scale: [7.0, 8.0],
                offset: [9.0, 10.0],
            },
            flags: GpuMesh::MESH_AUTHORED_TANGENTS,
        };
        let bytes = entry.to_bytes();
        assert_eq!(bytes.len(), MESH_ENTRY_STRIDE);
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 24, "base_vertex at offset 0");
        assert_eq!(uint_at(4), 36, "base_index at offset 4");
        assert_eq!(uint_at(8), 18, "index_count at offset 8");
        assert_eq!(float_at(12), -1.0, "bounds_min.x at offset 12");
        assert_eq!(float_at(16), -2.0, "bounds_min.y at offset 16");
        assert_eq!(float_at(20), -3.0, "bounds_min.z at offset 20");
        assert_eq!(float_at(24), 4.0, "bounds_max.x at offset 24");
        assert_eq!(float_at(28), 5.0, "bounds_max.y at offset 28");
        assert_eq!(float_at(32), 6.0, "bounds_max.z at offset 32");
        assert_eq!(float_at(36), 7.0, "uv_range.scale.x at offset 36");
        assert_eq!(float_at(40), 8.0, "uv_range.scale.y at offset 40");
        assert_eq!(float_at(44), 9.0, "uv_range.offset.x at offset 44");
        assert_eq!(float_at(48), 10.0, "uv_range.offset.y at offset 48");
        // **Appended, so every offset above is where it always was.** The flags
        // word is the row's fourteenth scalar and the only one the normal-map
        // slice added; a reader that put it anywhere else would move the UV
        // range the vertex stage decodes through.
        assert_eq!(
            uint_at(52),
            GpuMesh::MESH_AUTHORED_TANGENTS,
            "flags at offset 52"
        );

        // And the decode agrees with the encode, field for field.
        assert_eq!(GpuMesh::from_bytes(&bytes), entry);

        // An entry naming no mesh is all zeroes, which is the contract
        // `MeshPool::free` writes and the cull shader's `index_count == 0`
        // reads. The bounds are zero too, which is a degenerate box at the
        // origin — a shape the cull pass must never decide anything on.
        assert_eq!(GpuMesh::default().to_bytes(), [0u8; MESH_ENTRY_STRIDE]);
        assert_eq!(GpuMesh::default().index_count, 0);
        // **And a zeroed row says "no authored tangents"**, which is the whole
        // reason the bit is spelled that way round: a mesh nobody marked takes
        // the derivative frame, where the other polarity would have every
        // unmarked mesh sample its normal map through
        // `orthonormal_basis`' arbitrary axes.
        assert_eq!(GpuMesh::default().flags, 0);
        assert_eq!(GpuMesh::MESH_AUTHORED_TANGENTS, 1);
    }

    /// The offsets and the stride `slangc` actually emitted for `GpuMaterial`,
    /// read out of the disassembly.
    ///
    /// The stride is what this is really for. A `std430` struct whose members
    /// are a vector and a run of scalars is the case an implementation is most
    /// free to lay out differently from the sum of its parts, and a table the
    /// CPU writes at one stride while a shader reads it at another agrees for
    /// row 0 and for nothing after it — which is the second material, which is
    /// the only reason the table exists at all.
    #[test]
    fn the_material_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_GpuMaterial_std430 ArrayStride 64`, and
        // `OpMemberDecorate %GpuMaterial_std430 0 Offset 0` / `1 Offset 16` /
        // `2 Offset 20` / `3 Offset 24` / `4 Offset 28` / `5 Offset 32` /
        // `6 Offset 36` / `7 Offset 40` / `8 Offset 44` / `9 Offset 48` /
        // `10 Offset 52` / `11 Offset 56` / `12 Offset 60`.
        // Sixty-four with **no** padding, where the row before the material
        // pages was forty-eight with none either: the alignment is the
        // `float4`'s sixteen and thirteen members of four bytes each land on
        // exactly four of them. What the four page rows cost is two words, not
        // four — see `MATERIAL_STRIDE`, and `the_page_words_pack_two_layers_each`
        // for the pairing itself.
        assert_eq!(MATERIAL_STRIDE, 64);

        let material = GpuMaterial {
            base_color: [0.25, 0.5, 0.75, 1.0],
            base_color_texture: 3,
            metallic: 0.125,
            roughness: 0.375,
            tiling: GpuMaterial::TILING_PHYSICAL,
            tile_metres: 2.0,
            emissive: [1.5, 2.5, 3.5],
            normal_texture: 5,
            normal_scale: 0.75,
            metallic_roughness_occlusion_texture: 6,
            emissive_texture: 7,
            alpha_cutoff: 0.25,
            flags: 0x8000_0001,
        };
        let bytes = material.to_bytes();
        assert_eq!(bytes.len(), MATERIAL_STRIDE);
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        for (channel, expected) in material.base_color.iter().enumerate() {
            assert_eq!(
                float_at(channel * 4),
                *expected,
                "base_color[{channel}] at offset {}",
                channel * 4
            );
        }
        // **The base-colour and normal layers share offset 16, low half
        // first.** A row the CPU wrote at some other offset would still
        // round-trip through `from_bytes`, so this is asserted against the bytes
        // rather than against the decode.
        assert_eq!(uint_at(16) & 0xffff, 3, "base_color_texture at offset 16");
        assert_eq!(
            uint_at(16) >> 16,
            5,
            "normal_texture in offset 16's high half"
        );
        assert_eq!(float_at(20), 0.125, "metallic at offset 20");
        assert_eq!(float_at(24), 0.375, "roughness at offset 24");
        // **The tiling pair went into the padding the row had plus the twelve
        // `std430` rounded the size up by**, so the mode selector is at 28 and
        // the tile size at 32 — the offsets `slangc` decorates the shader's
        // struct with, checked against the bytes rather than the decode.
        assert_eq!(
            uint_at(28),
            GpuMaterial::TILING_PHYSICAL,
            "tiling at offset 28"
        );
        assert_eq!(float_at(32), 2.0, "tile_metres at offset 32");
        // **And the emissive triple is the three words that used to be
        // padding**, which is what says the row grew emission without any
        // earlier member moving. Three scalars, so their offsets are the sum of
        // their sizes rather than a vector's alignment.
        assert_eq!(float_at(36), 1.5, "emissive[0] at offset 36");
        assert_eq!(float_at(40), 2.5, "emissive[1] at offset 40");
        assert_eq!(float_at(44), 3.5, "emissive[2] at offset 44");
        // **The sixteen bytes §2's table sized the row for**: the other two page
        // rows in one word, the normal map's scale, the alpha cutoff and the
        // flags. Three of the five are read by nothing today and are pinned all
        // the same — the offsets test is what the rung that reads them will
        // trust, and a member sitting where nobody checked is exactly the drift
        // this test exists for.
        assert_eq!(
            uint_at(48) & 0xffff,
            6,
            "metallic_roughness_occlusion_texture at offset 48"
        );
        assert_eq!(
            uint_at(48) >> 16,
            7,
            "emissive_texture in offset 48's high half"
        );
        assert_eq!(float_at(52), 0.75, "normal_scale at offset 52");
        assert_eq!(float_at(56), 0.25, "alpha_cutoff at offset 56");
        assert_eq!(uint_at(60), 0x8000_0001, "flags at offset 60");
        assert_eq!(GpuMaterial::from_bytes(&bytes), material);

        // A row nothing has written is black, not untinted — the contract the
        // type's docs state, and the one that makes a forgotten material
        // visible instead of harmless. Naming layer 0 does not soften it:
        // zero times the page's white texel is still zero.
        assert_eq!(GpuMaterial::default().to_bytes(), [0u8; MATERIAL_STRIDE]);
        assert_eq!(GpuMaterial::default().base_color, [0.0; 4]);
        assert_eq!(GpuMaterial::default().base_color_texture, 0);
        assert_eq!(GpuMaterial::UNTINTED.base_color, [1.0; 4]);
        assert_eq!(
            GpuMaterial::UNTINTED.base_color_texture,
            0,
            "the untextured material names the page's white layer"
        );
        // **The untinted row is not every factor 1.0**, and a `roughness` of
        // 1.0 would be the fully diffuse extreme rather than the ordinary
        // painted surface the row is meant to be — see its docs.
        assert_eq!(GpuMaterial::UNTINTED.metallic, 0.0);
        assert_eq!(GpuMaterial::UNTINTED.roughness, 0.5);
        // **The untinted row samples the authored UV**, so it and every row
        // spread from it render exactly as they did before physical tiling — the
        // new mode is the added branch, not a replacement. Its `tile_metres` is
        // the one-metre default a row switched to physical tiling inherits.
        assert_eq!(GpuMaterial::UNTINTED.tiling, GpuMaterial::TILING_AUTHORED);
        assert_eq!(GpuMaterial::TILING_AUTHORED, 0);
        assert_eq!(GpuMaterial::UNTINTED.tile_metres, 1.0);
        assert_ne!(GpuMaterial::UNTINTED, GpuMaterial::default());
        // The zeroed row is authored-UV, so no unwritten material silently tiles
        // by world extent.
        assert_eq!(GpuMaterial::default().tiling, GpuMaterial::TILING_AUTHORED);
        // **And it names no page on any of the four rows**, which is what makes
        // a widened row safe: every material in the tree that was written before
        // these columns existed carries zero in each, and zero is "no page" on
        // all four.
        assert_eq!(GpuMaterial::default().normal_texture, GpuMaterial::NO_PAGE);
        assert_eq!(
            GpuMaterial::default().metallic_roughness_occlusion_texture,
            GpuMaterial::NO_PAGE
        );
        assert_eq!(
            GpuMaterial::default().emissive_texture,
            GpuMaterial::NO_PAGE
        );
        assert_eq!(GpuMaterial::UNTINTED.normal_texture, GpuMaterial::NO_PAGE);
        // The untinted row's normal scale is glTF's own default, so a row spread
        // from it and pointed at a normal page gets the map as authored.
        assert_eq!(GpuMaterial::UNTINTED.normal_scale, 1.0);
        assert_eq!(GpuMaterial::UNTINTED.alpha_cutoff, 0.5);
        assert_eq!(GpuMaterial::UNTINTED.flags, 0);

        // Two materials differing in nothing but their texture are different
        // rows, which is the whole of what the second column buys.
        let other = GpuMaterial {
            base_color_texture: 4,
            ..material
        };
        assert_ne!(other, material);
        assert_ne!(other.to_bytes(), bytes);
        // And the same for the row that shares its word: a normal page moved
        // without a base colour moved has to reach the device as a different
        // row, which is exactly what a pairing done wrong would lose.
        let perturbed = GpuMaterial {
            normal_texture: 9,
            ..material
        };
        assert_ne!(perturbed.to_bytes(), bytes);
        assert_eq!(
            GpuMaterial::from_bytes(&perturbed.to_bytes()).base_color_texture,
            material.base_color_texture,
            "the normal layer must not spill into the base-colour layer's half"
        );
    }

    /// The four page layers survive the two words they ride in, at the ends of
    /// the range those halves can hold.
    ///
    /// The pairing is the one thing about the row a reader cannot see from the
    /// field declarations, and getting it wrong is silent: a layer index that
    /// wrapped into its neighbour's half still names *a* layer, and the surface
    /// shades with a texture nobody chose rather than failing.
    #[test]
    fn the_page_words_pack_two_layers_each() {
        let material = GpuMaterial {
            base_color_texture: MAX_PAGE_LAYER,
            normal_texture: 1,
            metallic_roughness_occlusion_texture: 0,
            emissive_texture: MAX_PAGE_LAYER,
            ..GpuMaterial::UNTINTED
        };
        let decoded = GpuMaterial::from_bytes(&material.to_bytes());
        assert_eq!(decoded.base_color_texture, MAX_PAGE_LAYER);
        assert_eq!(decoded.normal_texture, 1);
        assert_eq!(decoded.metallic_roughness_occlusion_texture, 0);
        assert_eq!(decoded.emissive_texture, MAX_PAGE_LAYER);

        // **Saturated, not wrapped.** An index past what sixteen bits hold is
        // a caller mistake either way; what this pins is that the mistake stays
        // a mistake. `MAX_PAGE_LAYER` is far past any page a device will let
        // this engine create, so the renderer's own row check refuses it — see
        // `MAX_PAGE_LAYER`, which is where that is argued.
        let past = GpuMaterial {
            base_color_texture: MAX_PAGE_LAYER + 1,
            normal_texture: MAX_PAGE_LAYER + 9,
            ..GpuMaterial::UNTINTED
        };
        let decoded = GpuMaterial::from_bytes(&past.to_bytes());
        assert_eq!(
            decoded.base_color_texture, MAX_PAGE_LAYER,
            "an index past the half saturates rather than naming layer 0"
        );
        assert_eq!(decoded.normal_texture, MAX_PAGE_LAYER);
    }

    /// The neutral normal texel is **not** exactly flat, which is why
    /// [`GpuMaterial::normal_texture`] is tested for zero in the shader rather
    /// than left to the page's layer 0.
    ///
    /// `(0.5, 0.5, 1.0)` is the neutral a normal map is authored against, and an
    /// eight-bit unorm channel has no `0.5`: `128 / 255` is the nearest, and it
    /// decodes through `t * 2 - 1` to a tangent-space `x` and `y` of about
    /// `0.0039` rather than zero. That is a real tilt — a fifth of a degree —
    /// and this engine's goldens are compared across four backends with no
    /// tolerance, so a fetch that always perturbed would move every frame in the
    /// tree that draws a lit surface.
    ///
    /// The number is measured here rather than asserted to be small, so that a
    /// later change to the neutral texel or to the decode reports what it did.
    #[test]
    fn a_neutral_normal_texel_is_not_exactly_flat() {
        // The texel `crcbl_render::scene::PageDesc` writes into the normal
        // page's layer 0, read back the way an `Rgba8Unorm` sampler reads it.
        let neutral = [0x80u8, 0x80, 0xFF, 0xFF];
        let decoded = crate::vertex::decode_rgba8(neutral);
        let tangent_space = [
            decoded[0] * 2.0 - 1.0,
            decoded[1] * 2.0 - 1.0,
            decoded[2] * 2.0 - 1.0,
        ];
        assert_eq!(
            tangent_space[2], 1.0,
            "the blue channel's 0xFF is exactly 1"
        );
        assert_ne!(
            tangent_space[0], 0.0,
            "if this ever became exact the shader's zero-layer test could go"
        );
        // Half an eight-bit step off a half, doubled by the decode: `0x80` is
        // `128 / 255`, and `2 * (128 / 255) - 1` is `1 / 255` exactly.
        let step = 1.0 / 255.0;
        assert!(
            (tangent_space[0] - step).abs() < 1.0e-7 && tangent_space[0] == tangent_space[1],
            "the neutral texel tilts by {} on each axis, and the measured step is {step}",
            tangent_space[0]
        );
    }

    /// Every pyramid face is wound counter-clockwise as seen from outside, on
    /// the same terms as [`every_face_is_wound_counter_clockwise_from_outside`]
    /// — a side wound the other way would be culled and the mesh would render
    /// with a hole in it.
    ///
    /// Checked against a point known to be *inside* the solid rather than
    /// against a declared normal, because the sides' normals are computed from
    /// the same winding this is trying to check.
    #[test]
    fn every_pyramid_face_faces_away_from_the_interior() {
        let vertices = pyramid_vertices();
        let indices = pyramid_indices();
        // Comfortably inside: the base's centre is on the base plane, so this
        // is lifted off it.
        let inside = [0.0f32, PYRAMID_BASE_Y + 0.1, 0.0];
        for triangle in indices.chunks_exact(3) {
            let corner = |slot: usize| {
                let position = vertices[triangle[slot] as usize].position;
                [position[0], position[1], position[2]]
            };
            let (a, b, c) = (corner(0), corner(1), corner(2));
            let normal = triangle_normal(a, b, c);
            let outward: f32 = (0..3)
                .map(|axis| normal[axis] * (a[axis] - inside[axis]))
                .sum();
            assert!(
                outward > 0.0,
                "triangle {triangle:?} is wound towards the interior, so back-face \
                 culling would delete it"
            );
            // And the declared normals agree with the winding, or the lighting
            // is inside out on a face that still draws.
            for slot in 0..3 {
                let declared = vertices[triangle[slot] as usize].qtangent.decode().normal;
                for axis in 0..3 {
                    assert!(
                        (declared[axis] - normal[axis]).abs() < 1e-4,
                        "vertex {} of {triangle:?} declares {declared:?}, not {normal:?}",
                        triangle[slot]
                    );
                }
            }
        }
    }

    /// The pyramid's counts, and the property that makes it usable as evidence:
    /// it shares no colour with the cube, so a frame that drew it from the
    /// cube's vertices is a visibly different frame.
    #[test]
    fn the_pyramid_is_nothing_like_the_cube() {
        assert_eq!(pyramid_vertices().len(), PYRAMID_VERTEX_COUNT);
        assert_eq!(pyramid_indices().len(), PYRAMID_INDEX_COUNT);
        assert_eq!(
            pyramid_vertex_bytes().len(),
            PYRAMID_VERTEX_COUNT * VERTEX_STRIDE
        );
        let vertices = pyramid_vertices();
        for index in pyramid_indices() {
            assert!(
                (index as usize) < vertices.len(),
                "index {index} addresses past the {} vertices",
                vertices.len()
            );
        }
        for color in PYRAMID_SIDE_COLORS.iter().chain([&PYRAMID_BASE_COLOR]) {
            for face in &FACES {
                assert_ne!(
                    *color, face.color,
                    "the pyramid shares the cube's {} colour, so drawing it from the \
                     cube's vertices would be hard to see",
                    face.name
                );
            }
        }
    }

    /// The open box's counts, its index range, and the property that makes it
    /// evidence rather than a third box: no colour it carries belongs to
    /// another mesh here.
    #[test]
    fn the_open_box_is_nothing_like_the_other_two() {
        assert_eq!(open_box_vertices().len(), OPEN_BOX_VERTEX_COUNT);
        assert_eq!(open_box_indices().len(), OPEN_BOX_INDEX_COUNT);
        assert_eq!(
            open_box_vertex_bytes().len(),
            OPEN_BOX_VERTEX_COUNT * VERTEX_STRIDE
        );
        let vertices = open_box_vertices();
        for index in open_box_indices() {
            assert!(
                (index as usize) < vertices.len(),
                "index {index} addresses past the {} vertices",
                vertices.len()
            );
        }
        for (position, face) in OPEN_BOX_FACES.iter().enumerate() {
            for other in &OPEN_BOX_FACES[position + 1..] {
                assert_ne!(
                    face.color, other.color,
                    "{} and {} share a colour, so a face drawn in another's place \
                     would be invisible",
                    face.name, other.name
                );
            }
            for cube in &FACES {
                assert_ne!(face.color, cube.color, "{} is a cube colour", face.name);
            }
            for pyramid in PYRAMID_SIDE_COLORS.iter().chain([&PYRAMID_BASE_COLOR]) {
                assert_ne!(face.color, *pyramid, "{} is a pyramid colour", face.name);
            }
        }
    }

    /// **Every triangle of a face faces exactly the way that face says**, which
    /// is what makes each cluster's normal cone a single direction and its
    /// cutoff exactly `1.0` — see `crcbl_shaders::meshlet::open_box_clusters`,
    /// whose committed bounds are computed from that claim.
    ///
    /// Checked over the *generated* triangles rather than over
    /// [`OPEN_BOX_FACES`]' four declared corners, because the subdivision is
    /// what could go wrong: a bilinear step that left the plane, or a quad
    /// emitted with its corners transposed, gives a face several normals and a
    /// cone wide enough to cull nothing at all.
    #[test]
    fn every_open_box_triangle_faces_the_way_its_face_says() {
        let vertices = open_box_vertices();
        let indices = open_box_indices();
        let per_face = OPEN_BOX_QUADS_PER_FACE * 6;
        for (triangle, corners) in indices.chunks_exact(3).enumerate() {
            let face = &OPEN_BOX_FACES[triangle * 3 / per_face];
            let corner = |slot: usize| {
                let position = vertices[corners[slot] as usize].position;
                [position[0], position[1], position[2]]
            };
            let normal = triangle_normal(corner(0), corner(1), corner(2));
            for axis in 0..3 {
                assert!(
                    (normal[axis] - face.normal[axis]).abs() < 1e-6,
                    "triangle {triangle} of the {} faces {normal:?}, not {:?}",
                    face.name,
                    face.normal
                );
                // And the vertex normals agree with the winding, or the face
                // still draws and is lit inside out.
                for slot in 0..3 {
                    let declared = vertices[corners[slot] as usize].qtangent.decode().normal;
                    assert!(
                        (declared[axis] - face.normal[axis]).abs() <= QTangent::MAX_COMPONENT_ERROR,
                        "vertex {} of triangle {triangle} declares {declared:?}",
                        corners[slot]
                    );
                }
            }
        }
    }

    /// **Every coordinate of this mesh is a multiple of a quarter**, which is
    /// exact in `f32` on every target.
    ///
    /// That is the property `crcbl_shaders::meshlet::open_box_clusters` rests
    /// on and `crcbl-scene`'s pin test cashes in: it compares cooked bounds
    /// against the builder's for **equality**, so a coordinate that was merely
    /// close — anything through `sinf`/`cosf`, whose last place is the
    /// platform's business — would make that comparison a portability failure
    /// waiting for a runner this team cannot log into.
    #[test]
    fn every_open_box_coordinate_is_exact_in_f32() {
        for vertex in open_box_vertices() {
            for axis in 0..3 {
                let quarters = vertex.position[axis] * 4.0;
                assert_eq!(
                    quarters,
                    quarters.trunc(),
                    "position {:?} is not on the quarter grid",
                    vertex.position
                );
            }
            // The texture coordinates run the same grid over `0..=1`, so a face
            // samples the whole of its layer exactly as the cube's does. On the
            // grid rather than exact in `f32`: an `unorm16` lane is a
            // sixty-five-thousandth of the range and a quarter is not a
            // multiple of it, so what survives quantisation is the coordinate
            // to within one step and not its last bit.
            let uv = demo_uv_range().decode(vertex.uv0);
            for axis in 0..2 {
                let quarters = uv[axis] * OPEN_BOX_SUBDIVISIONS as f32;
                assert!(
                    (quarters - quarters.round()).abs()
                        <= UvRange::MAX_RELATIVE_ERROR * OPEN_BOX_SUBDIVISIONS as f32,
                    "uv {uv:?} is not on the subdivision grid"
                );
                assert!((0.0..=1.0).contains(&uv[axis]), "uv {uv:?}");
            }
        }
    }
    /// `normal_basis`, character for character, as every shader that transforms
    /// a normal must declare it.
    ///
    /// The signature and body only — the doc comment above each copy is
    /// deliberately not compared, on
    /// `crcbl_shaders::cluster_select`'s terms for `projected_error`: each file
    /// says why *it* has the copy, and holding two explanations to one wording
    /// buys nothing.
    const NORMAL_BASIS: &str = concat!(
        "float3x3 normal_basis(float3x3 basis)\n",
        "{\n",
        "    return float3x3(cross(basis[1], basis[2]),\n",
        "                    cross(basis[2], basis[0]),\n",
        "                    cross(basis[0], basis[1]));\n",
        "}\n",
    );

    /// `normal_basis` in Rust, transcribed from [`NORMAL_BASIS`] above.
    ///
    /// Slang's `basis[i]` is a **row** and [`glam::Mat3`] is column-major, so
    /// the rows come out through one transpose and the cofactor rows go back in
    /// through another. That convention is not recalled: it was read off the
    /// WGSL `slangc` emits for `m[0]`, which is the column of a `mat3x3<f32>`
    /// holding what Slang wrote as a row.
    fn normal_basis(basis: glam::Mat3) -> glam::Mat3 {
        let rows = basis.transpose();
        glam::Mat3::from_cols(
            rows.y_axis.cross(rows.z_axis),
            rows.z_axis.cross(rows.x_axis),
            rows.x_axis.cross(rows.y_axis),
        )
        .transpose()
    }

    /// **Every shader that transforms a normal builds the transform with one
    /// function.**
    ///
    /// `mesh.slang`'s vertex stage and `mesh_cluster.slang`'s mesh stage draw
    /// the same instances of the same meshes; a normal that differed between
    /// them would be one scene lit two ways, and which way depended on whether
    /// the device reported a mesh stage. `skinning.slang` is the third copy:
    /// it takes a normal through the *blended palette's* linear part rather
    /// than through an instance transform, but a normal is a perpendicular
    /// whatever produced the matrix, so a skinned mesh shaded off a bare 3×3
    /// would be lit unlike the static mesh standing beside it. Equal text
    /// cannot differ under any input, which is the stronger of the two
    /// assertions available here.
    ///
    /// The second half is what stops a shader declaring the function and never
    /// calling it: a copy of `normal_basis` sitting beside a
    /// `mul((float3x3)instance.transform, vertex.normal…)` would pass a
    /// declaration check and light exactly as wrongly as before. Each source
    /// names the matrix it must build one from, because the three do not agree
    /// about where their linear part comes from.
    #[test]
    fn every_shader_that_transforms_a_normal_builds_it_with_one_function() {
        for (name, source, call) in [
            (
                "mesh.slang",
                include_str!("../shaders/mesh.slang"),
                "normal_basis((float3x3)instance.transform)",
            ),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                "normal_basis((float3x3)instance.transform)",
            ),
            (
                "skinning.slang",
                include_str!("../shaders/skinning.slang"),
                "normal_basis((float3x3)blended)",
            ),
        ] {
            assert!(
                source.contains(NORMAL_BASIS),
                "{name} does not carry this exact function, so the geometry paths can light \
                 one scaled instance two ways:\n{NORMAL_BASIS}"
            );
            assert!(
                source.contains(call),
                "{name} declares `normal_basis` and never builds one from `{call}`, so the \
                 declaration is decoration"
            );
            assert!(
                !source.contains("mul((float3x3)instance.transform, vertex.normal"),
                "{name} still takes a normal through the bare 3x3, which is the transform a \
                 tangent takes and not the one a normal takes"
            );
        }
    }

    /// The `frame` word `VertexOutput` carries, as both raster paths must spell
    /// it.
    ///
    /// Held byte for byte for [`NORMAL_BASIS`]' reason and one more of its own:
    /// this word decides which tangent frame a normal map is sampled in *and*
    /// which way its bitangent points, and a copy that built the handedness bit
    /// the other way round would light a mesh-shader frame's normal maps as the
    /// mirror of the raster frame's — two pictures of one scene, differing by
    /// whether the device reported a mesh stage.
    const FRAME_WORD: &str = concat!(
        "uint frame_word(uint mesh_flags, TangentFrame basis)\n",
        "{\n",
        "    uint word = (mesh_flags & MESH_AUTHORED_TANGENTS) != 0u ? FRAME_AUTHORED_TANGENTS : 0u;\n",
    );

    /// **Both raster paths build the tangent-frame varying with one function,
    /// and both feed it the mesh row's own flags.**
    ///
    /// `mesh.slang`'s vertex stage and `mesh_cluster.slang`'s mesh stage
    /// complete the *same* fragment stage, so the word one of them writes is
    /// read by code written against the other. Equal text cannot differ under
    /// any input.
    ///
    /// The second assertion is what stops the function being decoration: a copy
    /// declared and never called leaves `frame` at whatever the struct was
    /// initialised with, and a zero word says "no authored tangents" — which is
    /// a mesh-shader frame silently taking the derivative fallback on every mesh
    /// that shipped a `TANGENT`, and is a difference no golden of an untangented
    /// scene could ever show.
    ///
    /// The third is what stops the tangent going through the wrong matrix. A
    /// tangent lies in the surface and takes the transform itself; the cofactor
    /// matrix is the one a perpendicular needs, and sending both through it
    /// shows up as a normal map lit from the wrong side on every non-uniformly
    /// scaled instance — see `skinning.slang`, which argues the pair at length.
    #[test]
    fn every_shader_builds_the_frame_word_the_same_way() {
        for (name, source, tangent) in [
            (
                "mesh.slang",
                include_str!("../shaders/mesh.slang"),
                "output.world_tangent = mul((float3x3)instance.transform, vertex.basis.tangent);",
            ),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                "output.world_tangent = mul((float3x3)instance.transform, vertex.basis.tangent);",
            ),
        ] {
            assert!(
                source.contains(FRAME_WORD),
                "{name} does not carry this exact function, so the two raster paths can \
                 disagree about which frame a normal map is sampled in:\n{FRAME_WORD}"
            );
            assert!(
                source.contains("output.frame = frame_word(mesh.flags, vertex.basis);"),
                "{name} declares `frame_word` and never fills `VertexOutput::frame` from the \
                 mesh row with it, so the declaration is decoration and every primitive it \
                 emits claims to have no authored tangents"
            );
            assert!(
                source.contains(tangent),
                "{name} does not carry the tangent through the bare 3x3, which is the \
                 transform a tangent takes and not the one `normal_basis` builds"
            );
            assert!(
                source.contains(&format!(
                    "static const uint MESH_AUTHORED_TANGENTS = {};",
                    GpuMesh::MESH_AUTHORED_TANGENTS
                )),
                "{name}'s MESH_AUTHORED_TANGENTS is not the host's {}",
                GpuMesh::MESH_AUTHORED_TANGENTS
            );
        }
    }

    /// Every shader that declares `GpuMesh` declares **all fourteen** of its
    /// scalars, and the flags word last.
    ///
    /// Four files read that table — the two raster paths, the cull pass and the
    /// draw-argument pass — and only the raster ones read the new word. The
    /// other two have to declare it all the same: a row short of a member puts
    /// every *element after it* at the wrong offset, so a cull pass reading a
    /// 52-byte row out of a 56-byte table culls the second mesh in the scene
    /// against the first mesh's bounding box. Nothing anywhere reports that; the
    /// frame simply loses geometry, and only for a scene with more than one
    /// resident mesh.
    #[test]
    fn every_shader_that_reads_the_mesh_table_declares_the_flags_word() {
        for (name, source) in [
            ("mesh.slang", include_str!("../shaders/mesh.slang")),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
            ),
            ("cull.slang", include_str!("../shaders/cull.slang")),
            ("draw_gen.slang", include_str!("../shaders/draw_gen.slang")),
        ] {
            let at = source
                .find("struct GpuMesh\n{")
                .unwrap_or_else(|| panic!("{name} declares no GpuMesh"));
            let body = &source[at..];
            let end = body
                .find("\n};")
                .unwrap_or_else(|| panic!("{name}'s GpuMesh never ends"));
            let body = &body[..end];
            // The scalars, in order, with the comments stripped: what the row
            // has to be for the CPU's `MESH_ENTRY_STRIDE` bytes to land on the
            // members this file names.
            let scalars: Vec<&str> = body
                .lines()
                .map(str::trim)
                .filter(|line| line.ends_with(';'))
                .collect();
            assert_eq!(
                scalars.len(),
                MESH_ENTRY_STRIDE / 4,
                "{name}'s GpuMesh declares {} scalars and the row is {MESH_ENTRY_STRIDE} bytes",
                scalars.len()
            );
            assert_eq!(
                scalars.last().copied(),
                Some("uint flags;"),
                "{name}'s GpuMesh must end with the flags word, or every member before it moves"
            );
        }
    }

    /// Both shaders that declare `GpuMaterial` declare **all thirteen** of its
    /// members, and the two page words in the halves the host packs them into.
    ///
    /// `mesh_cluster.slang` reads no material at all — its copy exists because
    /// the binding is in a layout its pipeline shares — which is exactly why it
    /// is the one that would drift: nothing it draws would look different, and
    /// the *fragment* stage reading the table is `mesh.slang`'s. What a short
    /// row there costs is the layout check, and after that the table itself.
    #[test]
    fn every_shader_that_declares_a_material_row_declares_the_whole_row() {
        for (name, source) in [
            ("mesh.slang", include_str!("../shaders/mesh.slang")),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
            ),
        ] {
            let at = source
                .find("struct GpuMaterial\n{")
                .unwrap_or_else(|| panic!("{name} declares no GpuMaterial"));
            let body = &source[at..];
            let end = body
                .find("\n};")
                .unwrap_or_else(|| panic!("{name}'s GpuMaterial never ends"));
            let members: Vec<&str> = body[..end]
                .lines()
                .map(str::trim)
                .filter(|line| line.ends_with(';'))
                .collect();
            // A `float4` and twelve scalars: thirteen members over
            // `MATERIAL_STRIDE` bytes, of which the vector is four words.
            assert_eq!(
                members.len(),
                MATERIAL_STRIDE / 4 - 3,
                "{name}'s GpuMaterial declares {} members: {members:?}",
                members.len()
            );
            assert_eq!(members.first().copied(), Some("float4 base_color;"));
            for expected in [
                "uint color_normal_pages;",
                "uint mro_emissive_pages;",
                "float normal_scale;",
                "float alpha_cutoff;",
                "uint flags;",
            ] {
                assert!(
                    members.contains(&expected),
                    "{name}'s GpuMaterial is missing `{expected}`, so every member after where \
                     it belongs is read out of the wrong bytes"
                );
            }
        }
    }

    /// **Both shadow lookups move the receiver along its own facet normal, and
    /// only the constant term moves it towards the light.**
    ///
    /// The normal-offset bias is a claim about a *direction*, and a direction is
    /// the one thing a re-blessed golden cannot hold anyone to: a lookup offset
    /// towards the light instead draws a perfectly plausible frame with a lit
    /// strip along the foot of every wall, and somebody would bless it. The two
    /// counts and the frames they were read off are
    /// `crcbl_render::shadow::DEPTH_BIAS_TEXELS` and its `NORMAL_OFFSET_TEXELS`;
    /// `shadow_normal_offset` in `shaders/mesh.slang` is why they travel in
    /// different directions.
    ///
    /// The two expressions are matched whole rather than by their parts, which
    /// is what makes each of them a claim about *one* of the two functions:
    /// nothing else in the file spells `frame.shadow_params.w` beside
    /// `to_light`, and nothing else spells `PUNCTUAL_NORMAL_OFFSET_TEXELS` at
    /// all.
    ///
    /// The last assertion is what stops the tangent form coming back beside the
    /// sine: `shadow_slope` and the ceiling it needed are gone, and a `tan` that
    /// runs to infinity as a surface turns edge-on is a bias with no bound
    /// again — which is the artefact `SHADOW_SLOPE_BIAS_CLAMP` existed for.
    #[test]
    fn both_shadow_lookups_offset_along_the_facet_normal() {
        let source = include_str!("../shaders/mesh.slang");
        let squeezed = source.split_whitespace().collect::<Vec<_>>().join(" ");
        for (function, offset, constant) in [
            (
                "cascade_visibility",
                "frame.shadow_params.w",
                "frame.shadow_params.z",
            ),
            (
                "punctual_visibility",
                "PUNCTUAL_NORMAL_OFFSET_TEXELS",
                "PUNCTUAL_DEPTH_BIAS_TEXELS",
            ),
        ] {
            let expected = format!(
                "float3 biased = world_position + geometric_normal * (texel_world * {offset} \
                 * shadow_normal_offset(geometric_normal, to_light)) + to_light * (texel_world \
                 * {constant});"
            );
            assert!(
                squeezed.contains(&expected),
                "{function} no longer offsets its receiver along the facet normal by {offset} \
                 and towards the light by {constant}, so the bias is not the one the two counts \
                 were measured for:\n{expected}"
            );
        }
        assert!(
            !source.contains("shadow_slope") && !source.contains("SLOPE_BIAS_CLAMP"),
            "mesh.slang has grown a slope-scaled depth bias again, and with it the unbounded \
             tangent the clamp existed to cover"
        );
    }

    /// **Every bias is denominated in the texels of the map it is about**, which
    /// since `docs/plan/45-shadows.md`'s priority rung is not one number.
    ///
    /// A light whose coverage earned it a halving of a root cell has half the
    /// texels across its map and twice the world footprint per texel. Biasing it
    /// by the *cell's* side is therefore exactly a factor of two too little per
    /// level demoted — acne on that one light's receivers, on a frame where
    /// every other light is right, which is as plausible a picture as this file
    /// can draw and one a golden blessed from a scene of whole-cell lights
    /// cannot see at all.
    ///
    /// Each expression is matched whole, and between them they are every
    /// division by a texel count in the file: the sun's bias, the sun's penumbra
    /// estimate and the punctual pair's — with `tile_texels`' own body asserted
    /// beside them, because a reciprocal spelled the wrong way round biases
    /// every map by the square of the atlas's own texel and still compiles.
    ///
    /// Where each divide sits relative to the guard in front of it is
    /// [`no_shadow_lookup_divides_by_a_rectangle_before_it_checks_one`]'s, so
    /// nothing here matches a line and its neighbour as one block: that is how
    /// a text assertion ends up forbidding the very statement that had to be
    /// inserted between them.
    #[test]
    fn every_shadow_bias_is_denominated_in_its_own_maps_texels() {
        let source = include_str!("../shaders/mesh.slang");
        let squeezed = source.split_whitespace().collect::<Vec<_>>().join(" ");
        for (site, expected) in [
            (
                "tile_texels",
                "float tile_texels(float4 rect) { return rect.x / frame.shadow_params.x; }",
            ),
            (
                "cascade_visibility",
                "float texel_world = 2.0 * frame.cascade_far[cascade] / tile_texels(rect);",
            ),
            (
                "sun_penumbra_texels",
                "float texel_world = 2.0 * radius / tile_texels(rect);",
            ),
            (
                "punctual_visibility",
                "float texel_world = map_world / tile_texels(rect);",
            ),
        ] {
            let wanted = expected.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                squeezed.contains(&wanted),
                "{site} no longer reads the side of the map it is sampling, so a light the \
                 allocator demoted is biased by a footprint it does not have:\n{wanted}"
            );
        }
    }

    /// **No shadow lookup divides by a rectangle before it has checked that the
    /// rectangle names a map.**
    ///
    /// A slot no map was rendered into carries `TileRect::EMPTY`, whose `to_uv`
    /// is four zeros, and on a frame with no cascade and no shadowed light
    /// *every* row is that — `crates/crcbl/tests/forward_e2e/depth_probe.rs`
    /// draws exactly such a frame. `tile_texels` divides by `rect.x` and
    /// `atlas_step` by `rect.xy`, so the order of the guard and the divide is
    /// the difference between answering "lit" and producing a `NaN`.
    ///
    /// **And a `NaN` is not merely a wrong number here.** Every comparison
    /// against one is false, so `cascade_visibility`'s own
    /// `any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0` — the return that exists for
    /// fragments outside the map — is false too, and the fragment walks past it
    /// to sample the atlas at `NaN` coordinates. On llvmpipe (LLVM 20.1.2) that
    /// is zero visibility and a black frame; on llvmpipe (LLVM 22.1.8) and on
    /// radv it is not, which is why this went red on CI and green on every tier
    /// this workspace can run locally.
    ///
    /// So the guard is asserted **by position** rather than by presence: a
    /// guard that exists below the divide it is for is a guard that never runs.
    /// That is a claim about the source, checkable on any machine, and it is
    /// what a driver-dependent frame cannot be.
    #[test]
    fn no_shadow_lookup_divides_by_a_rectangle_before_it_checks_one() {
        let source = include_str!("../shaders/mesh.slang");
        for (signature, divide) in [
            (
                "float cascade_visibility(uint cascade, float3 world_position",
                "tile_texels(rect)",
            ),
            (
                "float punctual_visibility(uint tile, float3 world_position",
                "tile_texels(rect)",
            ),
            (
                "float tile_pcf(uint tile, float2 tile_uv, float reference",
                "atlas_step(rect)",
            ),
        ] {
            let body = one_body(source, signature);
            let guard = body.find("atlas_rect_is_empty(rect)").unwrap_or_else(|| {
                panic!("`{signature}` no longer asks whether its rectangle names a map at all")
            });
            let divides = body
                .find(divide)
                .unwrap_or_else(|| panic!("`{signature}` no longer divides by `{divide}`"));
            assert!(
                guard < divides,
                "`{signature}` divides by its rectangle at byte {divides} and only asks \
                 whether the rectangle names a map at {guard}. A slot with no map carries \
                 zeros, so the divide is a NaN — and a NaN is false against every comparison \
                 below it, including the one that would have returned lit"
            );
        }
    }

    /// The text of the function `signature` opens, braces counted.
    ///
    /// Enough for this file: nothing here has a brace inside a string literal,
    /// and a copy that grew one would fail the assertions above rather than
    /// pass them silently.
    fn one_body(source: &str, signature: &str) -> String {
        let at = source
            .find(signature)
            .unwrap_or_else(|| panic!("no `{signature}` in mesh.slang"));
        let tail = &source[at..];
        let open = tail.find('{').expect("a function has a body");
        let mut depth = 0i32;
        for (offset, character) in tail[open..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return tail[open..=open + offset].to_owned();
                    }
                }
                _ => {}
            }
        }
        panic!("`{signature}`'s body never closes")
    }

    /// **The cascade fade grows towards the cascade behind it, over a band at
    /// the outer edge of the one in front.**
    ///
    /// Two ways of writing this draw a plausible frame and neither is the fade:
    /// a `lerp` with its ends swapped mixes *away* from the outer cascade as a
    /// fragment leaves the inner one, so the seam moves to the inner edge of the
    /// band rather than going away; and a band measured from the *near* end of
    /// the cascade blends across the whole of it, which trades one ring for a
    /// frame lit by the coarser map throughout. A golden holds neither to
    /// account — both are smooth, and a re-bless takes whichever it is given.
    ///
    /// What it does hold to account is the seam itself, and that is measured
    /// rather than asserted: `docs/plan/45-shadows.md`'s eighth decision has the
    /// luma steps either side of lantern's cascade boundary, with and without
    /// the band, and the sweep that picked `CASCADE_FADE_FRACTION`'s tenth.
    #[test]
    fn the_cascade_fade_grows_towards_the_outer_cascade() {
        let source = include_str!("../shaders/mesh.slang");
        let squeezed = source.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            // The band sits at the outer edge: `blend` is zero until the
            // fragment is within `band` of this cascade's reach, and one at it.
            "float band = reach * CASCADE_FADE_FRACTION;",
            "float blend = saturate((eye_distance - (reach - band)) / band);",
            // And what it grows towards is the next cascade out, which is the
            // half a swapped `lerp` would keep silent.
            "float next = cascade_visibility(cascade + 1, world_position, to_light, \
             geometric_normal, pixel); return lerp(visibility, next, blend);",
        ] {
            let wanted = expected.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                squeezed.contains(&wanted),
                "sun_visibility no longer fades towards the outer cascade over a band at the \
                 inner one's own outer edge, so the seam the eighth decision measured is back \
                 or has moved:\n{wanted}"
            );
        }
    }

    /// **The blocker search reads the map in the sense reversed-Z gives it, and
    /// sizes the filter from what it found.**
    ///
    /// Three slips here each draw a picture and none of them draws this rung.
    /// Comparing `depth < reference` collects the depths *behind* the receiver,
    /// which is every texel of an unoccluded surface, so every fragment gets the
    /// widest filter the clamp allows and the whole frame goes soft. Dropping
    /// the depth range turns a clip-space difference into metres by accident,
    /// scaling every penumbra by the cascade's own depth — tens of metres of it
    /// — so the clamp saturates and the same thing happens. And a clamp with its
    /// ends the other way round pins every penumbra to one number, which is a
    /// fixed filter wearing sixteen extra taps.
    ///
    /// A golden holds none of them to account: each is a smooth frame that a
    /// re-bless accepts. What is measured rather than asserted is the artefact
    /// the rung exists to remove, and `docs/plan/45-shadows.md`'s tenth decision
    /// carries the sweep — the edge wobble on `apps/lantern`'s far shadow
    /// boundary against `SHADOW_SUN_TAN_RADIUS`, with the acne and grain the
    /// same frames cost.
    #[test]
    fn the_blocker_search_sizes_the_filter_from_what_is_nearer_the_light() {
        let source = include_str!("../shaders/mesh.slang");
        let squeezed = source.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            // Reversed-Z: nearer the light is a *larger* depth, so a blocker is
            // one that compares greater.
            "if (depth > reference) { sum += depth; found += 1.0; }",
            // The depth range, which is `cascade_matrix`'s box inverted.
            "float separation = (sum / found - reference) * (2.0 * radius + \
             SHADOW_CASTER_REACH);",
            // A similar triangle over that height, in texels of this cascade,
            // and clamped to the span the search can speak for.
            "float texel_world = 2.0 * radius / tile_texels(rect);",
            "return clamp(separation * SHADOW_SUN_TAN_RADIUS / texel_world, \
             SHADOW_FILTER_TEXELS, SHADOW_SEARCH_TEXELS);",
            // And the sun is the only light that gets it: a punctual map is a
            // perspective projection whose depths are not a distance. `atlas` is
            // `light_tile(tile)`, resolved once at the top of
            // `punctual_visibility` because the bias there reads that slot's
            // rectangle too.
            "return tile_pcf(atlas, tile_uv, ndc.z, pixel, SHADOW_FILTER_TEXELS);",
        ] {
            let wanted = expected.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                squeezed.contains(&wanted),
                "sun_penumbra_texels no longer sizes the filter the way the tenth decision \
                 measured:\n{wanted}"
            );
        }
    }

    /// Pulls every `float2(x, y)` literal out of one `static const` table in
    /// `shaders/mesh.slang`.
    ///
    /// The two table tests below both need this and neither can ask the shader
    /// compiler for it, so the tables are read the only way a host test can read
    /// them: out of the source, between the declaration and its closing brace.
    fn shader_table(name: &str) -> Vec<(f64, f64)> {
        let source = include_str!("../shaders/mesh.slang");
        let start = source
            .find(&format!("{name}["))
            .unwrap_or_else(|| panic!("mesh.slang has no {name} table"));
        let body = &source[start..];
        let end = body.find("};").expect("an unterminated table");
        body[..end]
            .split("float2(")
            .skip(1)
            .map(|tail| {
                let (pair, _) = tail.split_once(')').expect("an unclosed float2");
                let (x, y) = pair.split_once(',').expect("a float2 of one component");
                (
                    x.trim().parse().expect("a float2 x that is not a number"),
                    y.trim().parse().expect("a float2 y that is not a number"),
                )
            })
            .collect()
    }

    /// **Both shadow discs are the Vogel spirals their doc comments say they
    /// are.**
    ///
    /// Four dozen coordinates transcribed by hand into a shader are four dozen
    /// chances to fat-finger a digit, and nothing downstream would say so: a
    /// wrong tap still samples the map, still returns a fraction, and moves a
    /// penumbra by an amount no golden distinguishes from the filter working.
    /// So each table is re-derived from the formula its doc quotes — radius
    /// `sqrt((i + 0.5) / n)` at angle `i π (3 - sqrt 5)` — rather than pinned to
    /// a copy of itself.
    ///
    /// **Both, and separately**, because the generator's radii depend on the
    /// count it was generated for: `SHADOW_SEARCH_DISC` is not a prefix or a
    /// stride of `SHADOW_DISC` and re-deriving it against the filter's count
    /// would pass a table that is neither.
    ///
    /// The tolerance is what six printed decimals can carry.
    #[test]
    fn the_shadow_discs_are_the_vogel_spirals_they_claim_to_be() {
        let golden_angle = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
        for (table, taps) in [
            ("SHADOW_DISC", "SHADOW_TAPS"),
            ("SHADOW_SEARCH_DISC", "SHADOW_SEARCH_TAPS"),
        ] {
            let disc = shader_table(table);
            let count = disc.len();
            let declared = format!("static const uint {taps} = {count};");
            assert!(
                include_str!("../shaders/mesh.slang").contains(&declared),
                "{table} has {count} taps and {taps} declares another number, so the loop that \
                 walks it reads past the table or stops short of it"
            );
            for (index, &(x, y)) in disc.iter().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a tap index is at most a few dozen"
                )]
                let position = index as f64;
                let radius = ((position + 0.5) / count as f64).sqrt();
                let angle = position * golden_angle;
                let (wanted_x, wanted_y) = (radius * angle.cos(), radius * angle.sin());
                assert!(
                    (x - wanted_x).abs() < 1e-6 && (y - wanted_y).abs() < 1e-6,
                    "{table}[{index}] is ({x}, {y}) where the spiral it claims to be puts \
                     ({wanted_x}, {wanted_y})"
                );
            }
        }
    }

    /// **The rotation table is the sixteen sixteenths of a turn, in order.**
    ///
    /// `tile_pcf` indexes it with a lattice that assumes exactly that: the
    /// coefficients are chosen for the *circular* distance between neighbouring
    /// indices, which is a claim about angles and is only true of a table whose
    /// entry `k` is the turn `2π k / 16`. A shuffled table would leave the index
    /// arithmetic reading as designed and the discs it picks arbitrarily close
    /// together.
    #[test]
    fn the_shadow_rotations_are_sixteenths_of_a_turn() {
        let rotations = shader_table("SHADOW_ROTATIONS");
        assert_eq!(rotations.len(), 16, "SHADOW_ROTATIONS is not sixteen turns");
        for (index, &(cosine, sine)) in rotations.iter().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a rotation index is at most fifteen"
            )]
            let angle = std::f64::consts::TAU * index as f64 / 16.0;
            assert!(
                (cosine - angle.cos()).abs() < 1e-6 && (sine - angle.sin()).abs() < 1e-6,
                "SHADOW_ROTATIONS[{index}] is ({cosine}, {sine}) and turn {index} of sixteen is \
                 ({}, {})",
                angle.cos(),
                angle.sin()
            );
        }
    }

    /// **The dither matrix reaches every rotation and no two neighbours share
    /// one.**
    ///
    /// Both halves fail quietly. A matrix that repeats an entry leaves one disc
    /// unreachable and another taken twice as often, which is a bias in the
    /// filter nothing would report; two adjacent cells holding the same rotation
    /// is a pair of fragments sampling identical texels, which draws a smooth
    /// frame with the banding the rotation exists to break still in it. Neither
    /// is a thing a golden distinguishes from the filter working.
    ///
    /// The minimum separation is only asserted to be non-zero rather than to the
    /// matrix's actual spread: the wrap between the fourth row and the first is
    /// one step in one column, which is a property of tiling sixteen values over
    /// a torus and not a defect to assert away.
    #[test]
    fn the_shadow_dither_covers_every_rotation() {
        let source = include_str!("../shaders/mesh.slang");
        assert!(
            source.contains("SHADOW_ROTATIONS[SHADOW_DITHER[cell.y * 4u + cell.x]]"),
            "tile_pcf no longer picks its rotation through the matrix this test is about"
        );
        let matrix = source
            .split_once("static const uint SHADOW_DITHER[16] = {")
            .expect("mesh.slang has no dither matrix")
            .1;
        let cells = matrix[..matrix.find("};").expect("an unterminated matrix")]
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .trim_end_matches('u')
                    .parse::<u32>()
                    .expect("a dither entry that is not a number")
            })
            .collect::<Vec<_>>();
        let mut sorted = cells.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..16).collect::<Vec<_>>(),
            "SHADOW_DITHER is not a permutation of the sixteen rotations, so some disc is \
             unreachable and another is taken twice as often: {cells:?}"
        );
        let circular = |a: u32, b: u32| {
            let step = a.abs_diff(b) % 16;
            step.min(16 - step)
        };
        let at = |x: usize, y: usize| cells[(y % 4) * 4 + (x % 4)];
        let closest = (0..4)
            .flat_map(|y| {
                (0..4).map(move |x| {
                    circular(at(x, y), at(x + 1, y)).min(circular(at(x, y), at(x, y + 1)))
                })
            })
            .min()
            .expect("a matrix of sixteen cells");
        assert!(
            closest >= 1,
            "two adjacent cells of SHADOW_DITHER take the same rotation, which is no dither \
             at all"
        );
    }

    /// **The filter's probe is a ring about a centre, and every index in it is
    /// a tap the disc has.**
    ///
    /// `tile_pcf` returns a flat `0.0` or `1.0` the moment
    /// `SHADOW_PROBE_INDEX`'s taps agree, so what that list is *shaped* like is
    /// the whole of the early-out's accuracy — and every way it can be wrong is
    /// quiet. Four taps bunched on one side of the disc is a probe that reads a
    /// direction rather than a neighbourhood, and it calls a fragment lit with a
    /// caster sitting in the half it never looked at. A list with no near-centre
    /// tap misses any caster small enough to fall inside the ring. An index past
    /// `SHADOW_TAPS` reads off the end of `SHADOW_DISC`. None of the three
    /// changes a frame in a way a golden attributes to the probe rather than to
    /// the filter.
    ///
    /// So the list is re-derived against the disc it indexes: in range, without
    /// repeats, one tap inside a quarter of the disc's reach, and the rest out
    /// past four fifths of it with no angular gap wider than a third of a turn.
    /// The bounds are the loose ones the shape needs, not a pin on the four
    /// indices shipped — a different four that were still a ring would pass, and
    /// that is the property being asserted.
    #[test]
    fn the_shadow_probe_is_a_ring_about_a_centre() {
        const SOURCE: &str = include_str!("../shaders/mesh.slang");

        assert!(
            SOURCE.contains("SHADOW_DISC[SHADOW_PROBE_INDEX[spot]]"),
            "tile_pcf no longer takes its probe through the index list this test is about"
        );
        assert!(
            SOURCE.contains("if (probe >= float(SHADOW_PROBE_TAPS))"),
            "tile_pcf no longer returns early on a unanimously lit neighbourhood, so the \
             probe is five taps nothing reads"
        );

        let declaration = SOURCE
            .split_once("static const uint SHADOW_PROBE_INDEX[SHADOW_PROBE_TAPS] = {")
            .expect("mesh.slang has no probe index list")
            .1;
        let probe = declaration[..declaration.find("};").expect("an unterminated list")]
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .trim_end_matches('u')
                    .parse::<usize>()
                    .expect("a probe index that is not a number")
            })
            .collect::<Vec<_>>();

        let count = probe.len();
        assert!(
            SOURCE.contains(&format!("static const uint SHADOW_PROBE_TAPS = {count};")),
            "SHADOW_PROBE_INDEX holds {count} indices and SHADOW_PROBE_TAPS declares another \
             number, so the probe loop reads past the list or stops short of it"
        );

        let disc = shader_table("SHADOW_DISC");
        let mut seen = probe.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            count,
            "SHADOW_PROBE_INDEX repeats a tap, so the probe is narrower than its count: \
             {probe:?}"
        );
        for &index in &probe {
            assert!(
                index < disc.len(),
                "SHADOW_PROBE_INDEX names tap {index} and SHADOW_DISC has {}, so the probe \
                 reads off the end of the table",
                disc.len()
            );
        }

        let radius = |index: usize| {
            let (x, y) = disc[index];
            x.hypot(y)
        };
        let (centre, ring): (Vec<usize>, Vec<usize>) =
            probe.iter().partition(|&&index| radius(index) < 0.25);
        assert_eq!(
            centre.len(),
            1,
            "the probe has {} taps inside a quarter of the disc where it wants exactly one: \
             a caster smaller than the ring falls on that tap and on nothing else",
            centre.len()
        );
        assert!(
            ring.len() >= 4,
            "the probe's ring is {} taps, and fewer than four cannot surround a fragment",
            ring.len()
        );

        let mut angles = ring
            .iter()
            .map(|&index| {
                assert!(
                    radius(index) > 0.8,
                    "SHADOW_PROBE_INDEX's tap {index} sits at radius {} and is neither the \
                     centre tap nor out on the rim",
                    radius(index)
                );
                let (x, y) = disc[index];
                y.atan2(x).rem_euclid(std::f64::consts::TAU)
            })
            .collect::<Vec<_>>();
        angles.sort_by(f64::total_cmp);
        let widest = (0..angles.len())
            .map(|at| {
                (angles[(at + 1) % angles.len()] - angles[at]).rem_euclid(std::f64::consts::TAU)
            })
            .fold(0.0_f64, f64::max);
        assert!(
            widest < std::f64::consts::TAU / 3.0,
            "the probe's ring leaves a gap of {} radians, which is a third of the disc it \
             never looks at",
            widest
        );
    }

    /// **A normal taken through `normal_basis` stays perpendicular to its
    /// surface under a non-uniform scale; one taken through the bare 3×3 does
    /// not.**
    ///
    /// The observable is the dot product of the shaded world normal with a
    /// world-space *tangent* of the face it came off. A normal is defined by
    /// being perpendicular to those, so that dot is zero for a correct normal
    /// under any transform, and the vector `fragmentMain` hands to `dot(N, L)`
    /// is exactly this one. Nothing in this workspace can read it off a pixel —
    /// this crate has no backend and `crcbl-render`'s tests have none either —
    /// so this evaluates the shader's rule rather than the shader.
    ///
    /// The case is `Scene::Ao`'s own `6 × 2 × 1.6` under a rotation, on a face
    /// that is not axis aligned. Both halves are needed to see the bug at all:
    /// an axis-aligned scale leaves an axis-aligned normal on its own axis,
    /// which is why the engine's own troughs shade correctly today and why the
    /// false claim survived. A glTF node with a rotation and a scale is the
    /// shipped case that does not — `crcbl_scene::gltf_render` reports one as a
    /// `scale` skip.
    ///
    /// Four assertions, and the first two are what stop the third passing on a
    /// rule that does nothing.
    #[test]
    fn the_normal_basis_keeps_a_normal_perpendicular_under_a_non_uniform_scale() {
        let model = glam::Mat4::from_quat(glam::Quat::from_axis_angle(
            glam::Vec3::new(0.3, 0.8, 0.5).normalize(),
            0.7,
        )) * glam::Mat4::from_scale(glam::Vec3::new(6.0, 2.0, 1.6));
        let basis = glam::Mat3::from_mat4(model);
        let built = normal_basis(basis);

        // 1. The cross-product spelling really is the cofactor matrix, checked
        //    against `det * inverse^T` — which glam computes a different way, so
        //    a transposed or miscycled row cannot agree with it by accident.
        let reference = basis.inverse().transpose() * basis.determinant();
        for (index, (a, b)) in built
            .to_cols_array()
            .iter()
            .zip(reference.to_cols_array().iter())
            .enumerate()
        {
            assert!(
                (a - b).abs() < 1e-3 * b.abs().max(1.0),
                "element {index} of the cofactor matrix is {a}, not {b}"
            );
        }

        // An oblique face, of the kind the pyramid has and the open box does
        // not, and two independent directions in its tangent plane.
        let normal = glam::Vec3::new(0.9, 0.8, 0.9).normalize();
        let u = normal.cross(glam::Vec3::X).normalize();
        let v = normal.cross(u).normalize();
        let bare = (basis * normal).normalize();
        let corrected = (built * normal).normalize();

        let mut worst_bare = 0.0f32;
        for tangent in [u, v] {
            let world_tangent = (basis * tangent).normalize();
            worst_bare = worst_bare.max(bare.dot(world_tangent).abs());
            // 3. The corrected normal is perpendicular to the transformed
            //    surface, which is the property the whole change is for.
            assert!(
                corrected.dot(world_tangent).abs() < 1e-6,
                "`normal_basis` left the normal {} off perpendicular",
                corrected.dot(world_tangent)
            );
        }
        // 2. And the bare 3×3 is a long way off it, so the check above has
        //    something it could fail.
        assert!(
            worst_bare > 0.5,
            "the bare 3×3 leaves the normal only {worst_bare} off perpendicular here, so this \
             case cannot tell the two rules apart"
        );

        // 4. **The identity is its own cofactor matrix, exactly.** This is why
        //    the change moved no golden image: `apps/lantern`, `apps/quarry` and
        //    `crcbl::screenshot`'s dunes patch all place their instances at the
        //    identity, so the vertex stage writes the numbers it always wrote.
        assert_eq!(
            normal_basis(glam::Mat3::IDENTITY).to_cols_array(),
            glam::Mat3::IDENTITY.to_cols_array()
        );

        // And the two other shapes the committed goldens are drawn with: a
        // uniform scale, and an axis-aligned scale on an axis-aligned normal.
        // Both give a *normalized* normal equal to the one the bare 3×3 gave,
        // which is the rest of that no-rebless claim.
        for scale in [
            glam::Vec3::splat(8.0),
            glam::Vec3::new(6.0, 2.0, 1.6),
            glam::Vec3::splat(0.3),
        ] {
            let diagonal = glam::Mat3::from_diagonal(scale);
            let cofactor = normal_basis(diagonal);
            for axis in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z] {
                for face in [axis, -axis] {
                    assert_eq!(
                        (cofactor * face).normalize(),
                        (diagonal * face).normalize(),
                        "a {scale:?} scale moved the {face:?} face's normal"
                    );
                }
            }
        }
    }
}
