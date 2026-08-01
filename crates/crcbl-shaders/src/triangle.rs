//! The geometry `triangle.slang` pulls, in the byte layout that shader
//! declares.
//!
//! # Why the vertices live beside the shader
//!
//! `struct Vertex` in `shaders/triangle.slang` fixes a `std430` layout — two
//! `float4`s, 32 bytes, no padding — and **any** producer of those bytes has to
//! agree with it exactly. Keeping the layout in the same crate as the source
//! that declares it means there is one place to change when the shader changes,
//! rather than one place per consumer that silently keeps working while reading
//! the wrong offsets.
//!
//! There are two consumers already: `apps/sandbox` draws this, and
//! `crcbl-vk`'s golden-image test renders it. If they each carried their own
//! copy, the golden image would stop being evidence about what the sandbox
//! draws the first time one of them was edited.
//!
//! This is demo geometry, and the plan expects some:
//! `docs/plan/02-vulkan-backend.md`'s ladder puts a "hardcoded cube/sphere" at
//! rung 3. Real meshes arrive from assets at P9 and are ranges in a global pool
//! (`docs/plan/03-gpu-driven-rendering.md` §3.1), not constants.

/// Bytes per vertex under `std430`: two `float4`s.
pub const VERTEX_STRIDE: usize = 32;

/// One vertex, matching `struct Vertex` in `shaders/triangle.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Clip-space position. There is no camera until milestone 3, so this is
    /// what reaches `SV_Position` unchanged.
    pub position: [f32; 4],
    /// Linear RGBA, interpolated across the triangle by the fragment stage.
    pub color: [f32; 4],
}

/// The milestone-2 triangle.
///
/// # Winding
///
/// A backend that flips Y with a negative-height viewport — which `crcbl-vk`
/// does, so the seam's Y-up convention survives Vulkan's inverted clip space —
/// sees these three as **counter-clockwise in framebuffer space**, matching
/// `FrontFace::Ccw`. Nothing is culled at P1.2, but the order is free to get
/// right now and is the first thing a depth-tested mesh would otherwise have to
/// debug.
///
/// # The colours are diagnostic, not decorative
///
/// One saturated primary per corner, no two corners alike in either axis. A Y
/// flip, an X mirror, a channel swap and a vertex-order mistake each produce a
/// *visibly different* picture rather than a plausible one — which is what makes
/// comparing against a golden image worth doing.
pub const VERTICES: [Vertex; 3] = [
    // Apex, red.
    Vertex {
        position: [0.0, 0.7, 0.5, 1.0],
        color: [1.0, 0.0, 0.0, 1.0],
    },
    // Bottom right, green.
    Vertex {
        position: [0.6, -0.6, 0.5, 1.0],
        color: [0.0, 1.0, 0.0, 1.0],
    },
    // Bottom left, blue.
    Vertex {
        position: [-0.6, -0.6, 0.5, 1.0],
        color: [0.0, 0.0, 1.0, 1.0],
    },
];

/// [`VERTICES`] as the bytes a storage buffer holds.
///
/// Little-endian `f32`s in declaration order, which is what `std430` means for
/// a struct of `float4`s and what every target this engine has is.
#[must_use]
pub fn vertex_bytes() -> Vec<u8> {
    crate::pack_f32_le(
        VERTICES
            .iter()
            .flat_map(|vertex| vertex.position.iter().chain(&vertex.color)),
        VERTICES.len() * VERTEX_STRIDE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout claim, checked rather than asserted in prose: three vertices
    /// of exactly 32 bytes, with no padding anywhere.
    #[test]
    fn the_byte_layout_is_two_float4s_per_vertex_with_no_padding() {
        let bytes = vertex_bytes();
        assert_eq!(bytes.len(), VERTICES.len() * VERTEX_STRIDE);
        assert_eq!(VERTEX_STRIDE, 8 * size_of::<f32>());

        // The first vertex's position really is at offset 0 and its colour at
        // offset 16 — the two offsets the shader indexes.
        assert_eq!(f32::from_le_bytes(bytes[0..4].try_into().expect("4")), 0.0);
        assert_eq!(f32::from_le_bytes(bytes[4..8].try_into().expect("4")), 0.7);
        assert_eq!(
            f32::from_le_bytes(bytes[16..20].try_into().expect("4")),
            1.0
        );
        assert_eq!(
            f32::from_le_bytes(bytes[20..24].try_into().expect("4")),
            0.0
        );
    }

    /// The diagnostic property the golden image depends on: no two corners
    /// share a position component or a colour, so any mirror or swap is visible.
    #[test]
    fn no_two_corners_are_alike_in_either_axis_or_in_colour() {
        for axis in 0..2 {
            let mut values: Vec<f32> = VERTICES.iter().map(|v| v.position[axis]).collect();
            values.sort_by(f32::total_cmp);
            values.dedup();
            assert!(
                values.len() >= 2,
                "axis {axis} must distinguish the corners, got {values:?}"
            );
        }
        // Exactly one saturated channel each, and a different one each time.
        for (index, vertex) in VERTICES.iter().enumerate() {
            let saturated: Vec<usize> = (0..3).filter(|c| vertex.color[*c] == 1.0).collect();
            assert_eq!(saturated, vec![index], "vertex {index} colour");
            assert_eq!(vertex.color[3], 1.0, "alpha must survive to the target");
        }
    }

    /// Every vertex must be inside the clip volume, or the "triangle" is a
    /// clipped polygon and the golden image is of something else.
    #[test]
    fn every_vertex_is_inside_the_clip_volume() {
        for vertex in &VERTICES {
            let [x, y, z, w] = vertex.position;
            assert_eq!(w, 1.0);
            assert!(x.abs() <= w, "x {x}");
            assert!(y.abs() <= w, "y {y}");
            // Vulkan clips z to 0..=w, not -w..=w.
            assert!((0.0..=w).contains(&z), "z {z}");
        }
    }
}
