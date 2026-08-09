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

/// Bytes in one draw's constant block.
///
/// Two `uint` and a `uint2` of padding. `std140` requires a uniform block's size
/// to be a multiple of 16, and the padding is in the shader struct rather than
/// implied so that both sides write the same number — see
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
/// # Three of the five fields are reserved, and that is deliberate
///
/// Only [`GpuInstance::transform`] is read by anything today. The other three
/// are here because **changing this layout after a shader, a cull pass and a
/// draw generator all index it is the expensive path**, and adding a field is
/// the cheap one now. Each field's own docs say which slice consumes it; none
/// of them is working camera-relative rendering, a material system or a GPU-side
/// mesh table, and none of them should be read as evidence that one exists.
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
    /// Which mesh to draw.
    ///
    /// **Reserved and unconsumed.** The draw resolves its
    /// [`MeshRange`](https://docs.rs/crcbl-render) on the CPU; a GPU-side mesh
    /// table for the cull pass to read is §3.3's.
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
    /// Per-instance bits.
    ///
    /// **Reserved and unconsumed: no bit is defined.** It is free — the struct
    /// is 16-byte aligned, so without it the three ids above would be followed
    /// by four bytes of padding instead of by this.
    pub flags: u32,
}

impl GpuInstance {
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

/// One draw call's bases, matching `struct DrawConstants` in
/// `shaders/mesh.slang`.
///
/// **Both of these would be arguments of `draw_indexed` if the four targets
/// agreed about what its bases do to `SV_VertexID` and `SV_InstanceID`, and they
/// do not.** That shader's header measures the disagreement on all four; the
/// consequence for a producer of these bytes is that every draw passes zero for
/// both of its own bases and puts the real ones here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawConstants {
    /// The mesh's first vertex in the vertex pool —
    /// [`MeshRange::base_vertex`](https://docs.rs/crcbl-render). Added to every
    /// index the draw reads, which is what lets the pool store indices
    /// mesh-relative.
    pub base_vertex: u32,
    /// The draw's instance in the instance array. Added to `SV_InstanceID`,
    /// which is zero for every draw the forward pass records.
    pub base_instance: u32,
}

impl DrawConstants {
    /// The bytes one draw's block holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; DRAW_CONSTANTS_SIZE] {
        let mut bytes = [0u8; DRAW_CONSTANTS_SIZE];
        bytes[0..4].copy_from_slice(&self.base_vertex.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.base_instance.to_le_bytes());
        // The trailing `uint2` is padding and stays zero.
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
    /// disassembly. Two `uint` in a row would permute silently — a base vertex
    /// read as a base instance draws *something* — so the byte each lands on is
    /// pinned rather than assumed.
    #[test]
    fn the_draw_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %DrawConstants_std140 n Offset …`: 0, 4, 8.
        assert_eq!(DRAW_CONSTANTS_SIZE, 16);
        assert_eq!(
            DRAW_CONSTANTS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a \
             block that is not one already is a block the shader and the CPU \
             disagree about the width of"
        );
        let bytes = DrawConstants {
            base_vertex: 24,
            base_instance: 1,
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 24, "base_vertex at offset 0");
        assert_eq!(uint_at(4), 1, "base_instance at offset 4");
        assert_eq!(uint_at(8), 0, "the pad is written, and it is zero");
        assert_eq!(uint_at(12), 0, "both halves of it");
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
