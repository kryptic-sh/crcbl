//! The geometry and the uniform block `mesh.slang` reads, in the byte layouts
//! that shader declares.
//!
//! Same argument as [`triangle`](crate::triangle): `struct MeshVertex` and
//! `struct FrameUniforms` in `shaders/mesh.slang` fix a memory layout, and any
//! producer of those bytes has to agree with it exactly. Keeping both in the
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
/// Two `float4x4` (64 each) then four `float4` (16 each). Checked against the
/// `Offset` decorations `slangc` emits — 0, 64, 128, 144, 160, 176 — by this
/// module's `the_uniform_block_matches_the_offsets_slangc_emits`.
pub const FRAME_UNIFORMS_SIZE: usize = 192;

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
    /// Model → world, column-major. Must be **rigid** — rotation and
    /// translation only — because the shader transforms normals with its 3×3
    /// part and no inverse-transpose.
    pub model: [f32; 16],
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
        put(&self.model);
        put(&self.camera_position);
        put(&self.light_direction);
        put(&self.light_color);
        put(&self.ambient);
        debug_assert_eq!(at, FRAME_UNIFORMS_SIZE);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets `slangc` actually emitted for `FrameUniforms`, read out of
    /// the disassembly. If the shader's field order changes, this is what says
    /// so before a driver reads the light direction out of the model matrix.
    #[test]
    fn the_uniform_block_matches_the_offsets_slangc_emits() {
        assert_eq!(FRAME_UNIFORMS_SIZE, 192);
        // `OpMemberDecorate %FrameUniforms_std140 n Offset …`
        let offsets = [0usize, 64, 128, 144, 160, 176];
        let sizes = [64usize, 64, 16, 16, 16, 16];
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
            model: [2.0; 16],
            camera_position: [3.0; 4],
            light_direction: [4.0; 4],
            light_color: [5.0; 4],
            ambient: [6.0; 4],
        };
        let bytes = uniforms.to_bytes();
        let at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(at(0), 1.0, "view_proj at offset 0");
        assert_eq!(at(64), 2.0, "model at offset 64");
        assert_eq!(at(128), 3.0, "camera_position at offset 128");
        assert_eq!(at(144), 4.0, "light_direction at offset 144");
        assert_eq!(at(160), 5.0, "light_color at offset 160");
        assert_eq!(at(176), 6.0, "ambient at offset 176");
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
}
