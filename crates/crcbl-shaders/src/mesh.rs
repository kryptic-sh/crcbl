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

/// Bytes per vertex: four `float4`s, no padding.
pub const VERTEX_STRIDE: usize = 64;

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
pub const SHADOW_LIGHT_TILES: usize = 6;

/// Tiles one point light's shadow map is: the six faces of a cube.
///
/// `docs/plan/18-render-features.md`: six atlas tiles rather than a cube map, so
/// one image, one sampler, one barrier story and one allocator serve the sun,
/// the spots and the points alike. The face order is `+X, -X, +Y, -Y, +Z, -Z`
/// — the cube-map convention — and `crcbl_render::shadow::face_axis` is the one
/// place it is written down on the host, `point_face` in `shaders/mesh.slang`
/// the one place on the device.
pub const SHADOW_POINT_FACES: usize = 6;

/// The side of one shadow-atlas tile, in texels.
///
/// Read by the shader as well as by the host: every shadow bias `mesh.slang`
/// applies — a cascade's as much as a cone's — is denominated in *tile texels*,
/// so it has to know how many of them the tile has. See
/// `PUNCTUAL_DEPTH_BIAS_TEXELS` there, and `crcbl_render::shadow`'s
/// `DEPTH_BIAS_TEXELS` for the sun's.
pub const SHADOW_TILE: u32 = 1024;

/// Tiles across the shadow atlas.
///
/// At least [`SHADOW_CASCADES`], which is what keeps the cascades in the top row
/// at the origins they have always had — see `crcbl_render::shadow::tile_origin`,
/// and the cascade goldens, which are what say the arrangement survived a change
/// to the grid's shape.
pub const SHADOW_ATLAS_COLUMNS: u32 = 4;

/// Tiles down it.
///
/// A grid rather than one row: a point light is [`SHADOW_POINT_FACES`] tiles of
/// exactly this kind, and a row long enough to hold them beside the cascades
/// would be an image wider than some devices' limit. `mesh.slang` addresses a
/// tile through both extents, so the shape is free to change without the
/// sampling side changing with it.
pub const SHADOW_ATLAS_ROWS: u32 = 2;

const _: () = assert!(
    (SHADOW_ATLAS_COLUMNS * SHADOW_ATLAS_ROWS) as usize == SHADOW_CASCADES + SHADOW_LIGHT_TILES,
    "every tile of the grid is either a cascade's or a light's"
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
/// `float4x4` and the irradiance grid's [`PROBE_VOLUME_SIZE`] header. Checked
/// against the `Offset` decorations `slangc` emits by this module's
/// `the_uniform_block_matches_the_offsets_slangc_emits`.
pub const FRAME_UNIFORMS_SIZE: usize =
    96 + 64 * SHADOW_CASCADES + 48 + 64 * SHADOW_LIGHT_TILES + PROBE_VOLUME_SIZE;

/// Bytes per [`GpuInstance`], and the stride of the instance storage buffer.
///
/// One `float4x4` (64) then four `uint` (4 each). Checked against the
/// `ArrayStride 80` and the `Offset` decorations `slangc` emits by this
/// module's `the_instance_layout_matches_the_offsets_slangc_emits`.
pub const INSTANCE_STRIDE: usize = 80;

/// Bytes per [`GpuMesh`], and the stride of the mesh-table storage buffer.
///
/// Three `uint` then six `float`, no padding. Checked against the
/// `ArrayStride 36` and the `Offset` decorations `slangc` emits by this module's
/// `the_mesh_entry_layout_matches_the_offsets_slangc_emits` — which is the test
/// that says all three targets really did lay it out this way, since a `std430`
/// struct of scalars is one of the few whose stride an implementation could
/// round up without anything else noticing.
pub const MESH_ENTRY_STRIDE: usize = 36;

/// Bytes per [`GpuMaterial`], and the stride of the material-table storage
/// buffer.
///
/// One `float4`, one `uint`, two `float`, then the tiling pair — one `uint`
/// selector and one `float` of tile size — and three `uint` of padding: 48
/// rather than 36, because the row's alignment is the `float4`'s 16 and
/// `std430` rounds the size up to a multiple of it. Checked against the
/// `ArrayStride` and the `Offset` decorations `slangc` emits by this module's
/// `the_material_layout_matches_the_offsets_slangc_emits`.
pub const MATERIAL_STRIDE: usize = 48;

/// Bytes in one draw's constant block.
///
/// One `uint` and three more of padding. `std140` requires a uniform block's
/// size to be a multiple of 16, and the padding is in the shader struct rather
/// than implied so that both sides write the same number — see
/// `DrawConstants` in `shaders/mesh.slang`.
pub const DRAW_CONSTANTS_SIZE: usize = 16;

/// One vertex, matching `struct MeshVertex` in `shaders/mesh.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    /// Object-space position in `xyz`; `w` unused and written as `1.0`.
    pub position: [f32; 4],
    /// Object-space normal in `xyz`, unit length; `w` unused and written as
    /// `0.0`.
    pub normal: [f32; 4],
    /// Linear RGBA albedo.
    pub color: [f32; 4],
    /// Base-colour texture coordinates in `xy`; `zw` unused and written as
    /// `0.0`.
    ///
    /// A whole `float4` for two numbers because the shader's struct is four
    /// `float4`s with no padding anywhere — see `struct MeshVertex` in
    /// `shaders/mesh.slang`, where a `float2` would cost the same eight bytes
    /// of tail padding and put a hole in a layout that has none.
    pub uv: [f32; 4],
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
    /// Flat ambient term in `rgb`.
    ///
    /// **Not a light and not a row.** It stands in for the bounces the direct
    /// terms do not carry, so it has no position, no froxel and no shadow. The
    /// sun sat beside it here as a direction and a colour until
    /// `docs/plan/18-render-features.md`'s light list existed; it is
    /// [`GpuLight`](crate::light::GpuLight) row now, and the shader has no
    /// special case for it.
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
    /// depth bias the **cascades** compare with. `w`: the slope-scaled
    /// coefficient on top of it.
    ///
    /// Both texel sizes are carried even where the grid is square and they are
    /// equal: the shader's kernel steps in tile space and scales back by
    /// [`SHADOW_ATLAS_COLUMNS`] and [`SHADOW_ATLAS_ROWS`], and the grid stops
    /// being square the moment a point light's six tiles arrive.
    ///
    /// The two biases are the sun's alone. A spot's map is a perspective
    /// projection whose depth precision is distributed nothing like a cascade's,
    /// so it biases in world units before projecting instead — see
    /// `spot_visibility` in `shaders/mesh.slang`, which is where that is argued.
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
}

impl FrameUniforms {
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
        debug_assert_eq!(at, FRAME_UNIFORMS_SIZE);
        bytes
    }
}

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
    /// produces it. Must be **rigid** — rotation and translation only — because
    /// the shader transforms normals with its 3×3 part and no
    /// inverse-transpose.
    ///
    /// Sector-local and `f32` rather than world-space and `f64` because that is
    /// what makes delta upload survive camera motion: an object that does not
    /// move has a transform that does not change, whatever the camera does.
    /// See [`GpuInstance::sector`] for the half of that which is not built.
    ///
    /// [`glam::Mat4::to_cols_array`]: https://docs.rs/glam
    pub transform: [f32; 16],
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

    /// The bytes one storage-buffer element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; INSTANCE_STRIDE] {
        let mut bytes = [0u8; INSTANCE_STRIDE];
        let mut at = 0usize;
        for value in &self.transform {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in [self.mesh, self.material, self.sector, self.flags] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, INSTANCE_STRIDE);
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
        Self {
            transform,
            mesh: uint_at(64),
            material: uint_at(68),
            sector: uint_at(72),
            flags: uint_at(76),
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
}

impl GpuMesh {
    /// The bytes one mesh-table element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; MESH_ENTRY_STRIDE] {
        let mut bytes = [0u8; MESH_ENTRY_STRIDE];
        let mut at = 0usize;
        for value in [self.base_vertex, self.base_index, self.index_count] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in self.bounds_min.iter().chain(&self.bounds_max) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
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
    pub const UNTINTED: Self = Self {
        base_color: [1.0; 4],
        base_color_texture: 0,
        metallic: 0.0,
        roughness: 0.5,
        tiling: Self::TILING_AUTHORED,
        tile_metres: 1.0,
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

    /// The bytes one material-table element holds, in `std430` order.
    ///
    /// The three trailing `uint`s of padding the shader's struct spells out are
    /// left as the zero the array starts as; nothing reads them.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; MATERIAL_STRIDE] {
        let mut bytes = [0u8; MATERIAL_STRIDE];
        let mut at = 0usize;
        for value in &self.base_color {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&self.base_color_texture.to_le_bytes());
        at += 4;
        for value in [self.metallic, self.roughness] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&self.tiling.to_le_bytes());
        at += 4;
        bytes[at..at + 4].copy_from_slice(&self.tile_metres.to_le_bytes());
        at += 4;
        debug_assert_eq!(at + 12, MATERIAL_STRIDE);
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
        Self {
            base_color: [float_at(0), float_at(4), float_at(8), float_at(12)],
            base_color_texture: uint_at(16),
            metallic: float_at(20),
            roughness: float_at(24),
            tiling: uint_at(28),
            tile_metres: float_at(32),
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
    let mut vertices = Vec::with_capacity(CUBE_VERTEX_COUNT);
    for face in &FACES {
        for (corner, uv) in face.corners.iter().zip(&QUAD_UV) {
            vertices.push(MeshVertex {
                position: [corner[0], corner[1], corner[2], 1.0],
                normal: [face.normal[0], face.normal[1], face.normal[2], 0.0],
                color: [face.color[0], face.color[1], face.color[2], 1.0],
                uv: [uv[0], uv[1], 0.0, 0.0],
            });
        }
    }
    vertices
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
    let vertices = cube_vertices();
    crate::pack_f32_le(
        vertices.iter().flat_map(|vertex| {
            vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.color)
                .chain(&vertex.uv)
        }),
        vertices.len() * VERTEX_STRIDE,
    )
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

    let mut vertices = Vec::with_capacity(PYRAMID_VERTEX_COUNT);
    // The base, in the same corner order as the cube's `-Y` face, so the
    // `0 1 2, 0 2 3` triangulation below is the one `cube_indices` uses — and
    // so is `QUAD_UV`, for the same reason.
    for (corner, uv) in base.iter().zip(&QUAD_UV) {
        vertices.push(MeshVertex {
            position: [corner[0], corner[1], corner[2], 1.0],
            normal: [0.0, -1.0, 0.0, 0.0],
            color: [
                PYRAMID_BASE_COLOR[0],
                PYRAMID_BASE_COLOR[1],
                PYRAMID_BASE_COLOR[2],
                1.0,
            ],
            uv: [uv[0], uv[1], 0.0, 0.0],
        });
    }
    // One triangle per side. Corner `i + 1` before corner `i` is what makes the
    // winding counter-clockwise from outside; taking them in the other order
    // would make every side vanish under back-face culling.
    for (side, color) in PYRAMID_SIDE_COLORS.iter().enumerate() {
        let corners = [base[(side + 1) % base.len()], base[side], apex];
        let normal = triangle_normal(corners[0], corners[1], corners[2]);
        for (corner, uv) in corners.iter().zip(&TRIANGLE_UV) {
            vertices.push(MeshVertex {
                position: [corner[0], corner[1], corner[2], 1.0],
                normal: [normal[0], normal[1], normal[2], 0.0],
                color: [color[0], color[1], color[2], 1.0],
                uv: [uv[0], uv[1], 0.0, 0.0],
            });
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
    let vertices = pyramid_vertices();
    crate::pack_f32_le(
        vertices.iter().flat_map(|vertex| {
            vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.color)
                .chain(&vertex.uv)
        }),
        vertices.len() * VERTEX_STRIDE,
    )
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
                    vertices.push(MeshVertex {
                        position: [position[0], position[1], position[2], 1.0],
                        normal: [face.normal[0], face.normal[1], face.normal[2], 0.0],
                        color: [face.color[0], face.color[1], face.color[2], 1.0],
                        uv: [s, t, 0.0, 0.0],
                    });
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
    let vertices = open_box_vertices();
    crate::pack_f32_le(
        vertices.iter().flat_map(|vertex| {
            vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.color)
                .chain(&vertex.uv)
        }),
        vertices.len() * VERTEX_STRIDE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets `slangc` actually emitted for `FrameUniforms`, read out of
    /// the disassembly. If the shader's field order changes, this is what says
    /// so before a driver reads the light direction out of the model matrix.
    /// The cascade count is a number in three files and a compiler sees none of
    /// the disagreements.
    ///
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
        ] {
            for (name, source) in [("mesh.slang", mesh), ("mesh_cluster.slang", cluster)] {
                assert!(
                    source.contains(&declaration),
                    "{name} does not declare `{declaration}`; a block sized differently on the \
                     two sides puts every member after it at the wrong offset"
                );
            }
        }
        // The atlas's geometry, which only the sampling side reads. A grid the
        // shader thought was a different shape would address every tile but the
        // first at the wrong place in the atlas — a picture, and a plausible
        // one, since tile 0 is cascade 0 either way.
        for declaration in [
            format!("static const uint SHADOW_ATLAS_COLUMNS = {SHADOW_ATLAS_COLUMNS};"),
            format!("static const uint SHADOW_ATLAS_ROWS = {SHADOW_ATLAS_ROWS};"),
            format!("static const float SHADOW_TILE = {SHADOW_TILE}.0;"),
            // The face count, which only the sampling side reads: it is how far
            // apart two of a point light's tiles are, so a shader that thought a
            // point light had five faces would sample another light's map for the
            // sixth — a picture, and a plausible one.
            format!("static const uint SHADOW_POINT_FACES = {SHADOW_POINT_FACES};"),
        ] {
            assert!(
                mesh.contains(&declaration),
                "mesh.slang does not declare `{declaration}`; the atlas's geometry has drifted"
            );
        }
        const {
            assert!(
                SHADOW_CASCADES >= 1 && SHADOW_CASCADES <= 4,
                "cascade_far is one float4, so it can name at most four splits, and a shadow pass \
                 with no cascades is not a shadow pass"
            );
        }
    }

    #[test]
    fn the_uniform_block_matches_the_offsets_slangc_emits() {
        assert_eq!(
            FRAME_UNIFORMS_SIZE, 704,
            "at two cascades and six light tiles"
        );
        // `OpMemberDecorate %FrameUniforms_std140 n Offset …` — 0, 64, 80, 96,
        // 224, 240, 256, 272, 656, 672, 688 — and
        // `OpDecorate %_arr_mat4v4float_int_2 ArrayStride 64`. The last three
        // are the grid header's rows, which this side writes as one group.
        let cascades = 64 * SHADOW_CASCADES;
        let lights = 64 * SHADOW_LIGHT_TILES;
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
    }

    /// The offsets `slangc` actually emitted for `GpuInstance`, read out of the
    /// disassembly. The four ids are the same width and would silently
    /// permute — a mesh id read as a material id is a picture, not a crash —
    /// so the byte each lands on is pinned rather than assumed.
    #[test]
    fn the_instance_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_GpuInstance_std430 ArrayStride 80`, and
        // `OpMemberDecorate %GpuInstance_std430 n Offset …`.
        assert_eq!(INSTANCE_STRIDE, 80);
        assert_eq!(
            INSTANCE_STRIDE % 16,
            0,
            "a std430 struct containing a float4x4 is 16-byte aligned, so its \
             stride must be a multiple of 16 or every element after the first \
             lands short"
        );

        let instance = GpuInstance {
            transform: [1.0; 16],
            mesh: 2,
            material: 3,
            sector: 4,
            flags: 5,
        };
        let bytes = instance.to_bytes();
        assert_eq!(bytes.len(), INSTANCE_STRIDE);
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(float_at(0), 1.0, "transform at offset 0");
        assert_eq!(float_at(60), 1.0, "and it is 64 bytes wide");
        assert_eq!(uint_at(64), 2, "mesh at offset 64");
        assert_eq!(uint_at(68), 3, "material at offset 68");
        assert_eq!(uint_at(72), 4, "sector at offset 72");
        assert_eq!(uint_at(76), 5, "flags at offset 76");

        // And the decode agrees with the encode, field for field — four `u32`s
        // in a row would permute silently, and this is what says they did not.
        assert_eq!(GpuInstance::from_bytes(&bytes), instance);
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
            u32::from_le_bytes(bytes[76..80].try_into().expect("4")),
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

    #[test]
    fn the_vertex_layout_is_four_float4s_with_no_padding() {
        let bytes = cube_vertex_bytes();
        assert_eq!(VERTEX_STRIDE, 16 * size_of::<f32>());
        assert_eq!(bytes.len(), CUBE_VERTEX_COUNT * VERTEX_STRIDE);
        assert_eq!(cube_vertices().len(), CUBE_VERTEX_COUNT);
        assert_eq!(cube_indices().len(), CUBE_INDEX_COUNT);
        assert_eq!(cube_index_bytes().len(), CUBE_INDEX_COUNT * 4);
    }

    /// **The UV is the fourth `float4`, at byte 48 of each vertex**, and every
    /// mesh really carries one.
    ///
    /// Read out of the packed bytes rather than off the struct: the packer
    /// chains four fields and a chain that forgot the last one would produce a
    /// buffer of the right *length* only by accident — and would silently give
    /// every fragment the UV of the vertex after it. Both meshes are checked
    /// because they are packed by two call sites.
    #[test]
    fn every_vertex_carries_its_uv_in_the_fourth_float4() {
        for (name, vertices, bytes) in [
            ("cube", cube_vertices(), cube_vertex_bytes()),
            ("pyramid", pyramid_vertices(), pyramid_vertex_bytes()),
        ] {
            for (index, vertex) in vertices.iter().enumerate() {
                let at = index * VERTEX_STRIDE + 48;
                let read = |lane: usize| {
                    f32::from_le_bytes(
                        bytes[at + lane * 4..at + lane * 4 + 4]
                            .try_into()
                            .expect("four bytes"),
                    )
                };
                assert_eq!(
                    [read(0), read(1), read(2), read(3)],
                    vertex.uv,
                    "{name} vertex {index}'s uv is not at byte {at}"
                );
            }
            // And the coordinates actually span the layer rather than sitting
            // at one corner, which is what makes a page layer visible over a
            // whole face instead of as a single flat colour.
            let us: Vec<f32> = vertices.iter().map(|vertex| vertex.uv[0]).collect();
            let vs: Vec<f32> = vertices.iter().map(|vertex| vertex.uv[1]).collect();
            assert!(
                us.contains(&0.0) && us.contains(&1.0),
                "{name} never reaches both edges of the layer in u"
            );
            assert!(
                vs.contains(&0.0) && vs.contains(&1.0),
                "{name} never reaches both edges of the layer in v"
            );
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
        // `OpDecorate %_runtimearr_GpuMesh_std430 ArrayStride 36`, and
        // `OpMemberDecorate %GpuMesh_std430 n Offset …`: 0, 4, 8, 12, 16, 20,
        // 24, 28, 32. The WGSL and the MSL declare the same nine scalars with
        // no explicit alignment, which is the same layout.
        assert_eq!(MESH_ENTRY_STRIDE, 36);

        let entry = GpuMesh {
            base_vertex: 24,
            base_index: 36,
            index_count: 18,
            bounds_min: [-1.0, -2.0, -3.0],
            bounds_max: [4.0, 5.0, 6.0],
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

        // And the decode agrees with the encode, field for field.
        assert_eq!(GpuMesh::from_bytes(&bytes), entry);

        // An entry naming no mesh is all zeroes, which is the contract
        // `MeshPool::free` writes and the cull shader's `index_count == 0`
        // reads. The bounds are zero too, which is a degenerate box at the
        // origin — a shape the cull pass must never decide anything on.
        assert_eq!(GpuMesh::default().to_bytes(), [0u8; MESH_ENTRY_STRIDE]);
        assert_eq!(GpuMesh::default().index_count, 0);
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
        // `OpDecorate %_runtimearr_GpuMaterial_std430 ArrayStride 48`, and
        // `OpMemberDecorate %GpuMaterial_std430 0 Offset 0` / `1 Offset 16` /
        // `2 Offset 20` / `3 Offset 24` / `4 Offset 28` / `5 Offset 32`.
        // Forty-eight rather than thirty-six: the row's alignment is the
        // `float4`'s sixteen, so the last member is followed by twelve bytes the
        // shader's own struct spells out as `pad0`/`pad1`/`pad2`.
        assert_eq!(MATERIAL_STRIDE, 48);

        let material = GpuMaterial {
            base_color: [0.25, 0.5, 0.75, 1.0],
            base_color_texture: 3,
            metallic: 0.125,
            roughness: 0.375,
            tiling: GpuMaterial::TILING_PHYSICAL,
            tile_metres: 2.0,
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
        // **The texture index is at offset 16, after the factor.** A row the CPU
        // wrote at some other offset would still round-trip through
        // `from_bytes`, so this is asserted against the bytes rather than
        // against the decode.
        assert_eq!(uint_at(16), 3, "base_color_texture at offset 16");
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
        assert_eq!(
            bytes[36..MATERIAL_STRIDE],
            [0u8; 12],
            "the three pad words the shader declares must stay zero"
        );
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

        // Two materials differing in nothing but their texture are different
        // rows, which is the whole of what the second column buys.
        let other = GpuMaterial {
            base_color_texture: 4,
            ..material
        };
        assert_ne!(other, material);
        assert_ne!(other.to_bytes(), bytes);
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
                let declared = vertices[triangle[slot] as usize].normal;
                for axis in 0..3 {
                    assert!(
                        (declared[axis] - normal[axis]).abs() < 1e-5,
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
                    let declared = vertices[corners[slot] as usize].normal;
                    assert!(
                        (declared[axis] - face.normal[axis]).abs() < 1e-6,
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
            // samples the whole of its layer exactly as the cube's does.
            for axis in 0..2 {
                let quarters = vertex.uv[axis] * OPEN_BOX_SUBDIVISIONS as f32;
                assert_eq!(quarters, quarters.trunc(), "uv {:?}", vertex.uv);
                assert!((0.0..=1.0).contains(&vertex.uv[axis]), "uv {:?}", vertex.uv);
            }
        }
    }
}
