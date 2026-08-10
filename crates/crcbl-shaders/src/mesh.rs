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

/// Bytes per vertex: three `float4`s, no padding.
pub const VERTEX_STRIDE: usize = 48;

/// Bytes in the frame uniform block.
///
/// One `float4x4` (64) then four `float4` (16 each). Checked against the
/// `Offset` decorations `slangc` emits — 0, 64, 80, 96, 112 — by this module's
/// `the_uniform_block_matches_the_offsets_slangc_emits`.
pub const FRAME_UNIFORMS_SIZE: usize = 128;

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
    /// World-space direction *towards* the light in `xyz`, unit length.
    pub light_direction: [f32; 4],
    /// Light colour premultiplied by intensity in `rgb`.
    pub light_color: [f32; 4],
    /// Flat ambient term in `rgb`.
    pub ambient: [f32; 4],
}

impl FrameUniforms {
    /// The bytes a uniform buffer holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; FRAME_UNIFORMS_SIZE] {
        let mut bytes = [0u8; FRAME_UNIFORMS_SIZE];
        let mut at = 0usize;
        let mut put = |values: &[f32]| {
            for value in values {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        };
        put(&self.view_proj);
        put(&self.camera_position);
        put(&self.light_direction);
        put(&self.light_color);
        put(&self.ambient);
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
/// # Two of the five fields are reserved, and that is deliberate
///
/// [`GpuInstance::transform`] and [`GpuInstance::mesh`] are read by the vertex
/// stage and [`GpuInstance::flags`] by the cull pass. The other two are here
/// because **changing this layout after a shader, a cull pass and a draw
/// generator all index it is the expensive path**, and adding a field is the
/// cheap one now. Each field's own docs say which slice consumes it; neither of
/// them is working camera-relative rendering or a material system, and neither
/// should be read as evidence that one exists.
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
    /// Which material to shade with.
    ///
    /// **Reserved and unconsumed.** The material table is the other half of
    /// §3.2 and is not built; nothing indexes anything with this yet.
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

/// Where one draw call's run of visible instances starts, matching
/// `struct DrawConstants` in `shaders/mesh.slang`.
///
/// **This would be `draw_indexed`'s own base instance if the four targets
/// agreed about what that does to `SV_InstanceID`, and they do not.** That
/// shader's header measures the disagreement on all four; the consequence for a
/// producer of these bytes is that every draw passes zero for its own bases, and
/// the instance is looked up rather than named.
///
/// The base *vertex* used to sit beside it and does not any more: it is
/// [`GpuMesh::base_vertex`], reached through the drawn instance's
/// [`mesh`](GpuInstance::mesh). A per-draw block can say only one thing per
/// draw, so a base vertex here made every instance in a draw share a mesh —
/// which is exactly what §3.3's cull pass cannot promise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawConstants {
    /// The draw's bucket's first slot in the list `draw_gen.slang` scatters
    /// surviving instances into. `SV_InstanceID` counts from zero within the
    /// draw and indexes the run from here.
    pub base: u32,
}

impl DrawConstants {
    /// The bytes one draw's block holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; DRAW_CONSTANTS_SIZE] {
        let mut bytes = [0u8; DRAW_CONSTANTS_SIZE];
        bytes[0..4].copy_from_slice(&self.base.to_le_bytes());
        // The three trailing `uint`s are padding and stay zero.
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

/// The cube's vertices, in face order.
#[must_use]
pub fn cube_vertices() -> Vec<MeshVertex> {
    let mut vertices = Vec::with_capacity(CUBE_VERTEX_COUNT);
    for face in &FACES {
        for corner in &face.corners {
            vertices.push(MeshVertex {
                position: [corner[0], corner[1], corner[2], 1.0],
                normal: [face.normal[0], face.normal[1], face.normal[2], 0.0],
                color: [face.color[0], face.color[1], face.color[2], 1.0],
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
    // `0 1 2, 0 2 3` triangulation below is the one `cube_indices` uses.
    for corner in base {
        vertices.push(MeshVertex {
            position: [corner[0], corner[1], corner[2], 1.0],
            normal: [0.0, -1.0, 0.0, 0.0],
            color: [
                PYRAMID_BASE_COLOR[0],
                PYRAMID_BASE_COLOR[1],
                PYRAMID_BASE_COLOR[2],
                1.0,
            ],
        });
    }
    // One triangle per side. Corner `i + 1` before corner `i` is what makes the
    // winding counter-clockwise from outside; taking them in the other order
    // would make every side vanish under back-face culling.
    for (side, color) in PYRAMID_SIDE_COLORS.iter().enumerate() {
        let corners = [base[(side + 1) % base.len()], base[side], apex];
        let normal = triangle_normal(corners[0], corners[1], corners[2]);
        for corner in corners {
            vertices.push(MeshVertex {
                position: [corner[0], corner[1], corner[2], 1.0],
                normal: [normal[0], normal[1], normal[2], 0.0],
                color: [color[0], color[1], color[2], 1.0],
            });
        }
    }
    vertices
}

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
    #[test]
    fn the_uniform_block_matches_the_offsets_slangc_emits() {
        assert_eq!(FRAME_UNIFORMS_SIZE, 128);
        // `OpMemberDecorate %FrameUniforms_std140 n Offset …`
        let offsets = [0usize, 64, 80, 96, 112];
        let sizes = [64usize, 16, 16, 16, 16];
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

        let uniforms = FrameUniforms {
            view_proj: [1.0; 16],
            camera_position: [3.0; 4],
            light_direction: [4.0; 4],
            light_color: [5.0; 4],
            ambient: [6.0; 4],
        };
        let bytes = uniforms.to_bytes();
        let at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(at(0), 1.0, "view_proj at offset 0");
        assert_eq!(at(64), 3.0, "camera_position at offset 64");
        assert_eq!(at(80), 4.0, "light_direction at offset 80");
        assert_eq!(at(96), 5.0, "light_color at offset 96");
        assert_eq!(at(112), 6.0, "ambient at offset 112");
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
    fn the_vertex_layout_is_three_float4s_with_no_padding() {
        let bytes = cube_vertex_bytes();
        assert_eq!(VERTEX_STRIDE, 12 * size_of::<f32>());
        assert_eq!(bytes.len(), CUBE_VERTEX_COUNT * VERTEX_STRIDE);
        assert_eq!(cube_vertices().len(), CUBE_VERTEX_COUNT);
        assert_eq!(cube_indices().len(), CUBE_INDEX_COUNT);
        assert_eq!(cube_index_bytes().len(), CUBE_INDEX_COUNT * 4);
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
        let bytes = DrawConstants { base: 1 }.to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "base at offset 0");
        for pad in [4, 8, 12] {
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
}
