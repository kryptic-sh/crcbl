//! The triangle `mesh_shader.slang` emits, and the tint its amplification
//! stage applies.
//!
//! # Why these live beside the shader rather than in the test that samples them
//!
//! `mesh_shader.slang` hardcodes its three vertices — a mesh shader reading a
//! storage buffer needs mesh-stage descriptor visibility, which is the next
//! slice, so the geometry is constants for now. Anything that wants to know
//! *where* the triangle landed therefore has to restate them, and the same
//! argument [`crate::triangle`] makes applies: the restatement belongs in the
//! crate that owns the source it mirrors, so there is one place to change when
//! the shader changes rather than one per consumer.
//!
//! Unlike [`crate::triangle`], nothing here is uploaded anywhere and there is
//! no byte layout to agree on. **What holds these in step with the shader is
//! the golden image**, not a layout check: move the triangle in the `.slang`
//! and leave these alone, and `crcbl-vk`'s `mesh_shader_triangle.png` stops
//! matching.

/// Clip-space positions, matching `POSITIONS` in `shaders/mesh_shader.slang`.
///
/// Apex **down**, unlike [`crate::triangle::VERTICES`], so a golden image of
/// the mesh path can never be satisfied by a copy of the raster path's.
pub const POSITIONS: [[f32; 4]; 3] = [
    [0.0, -0.7, 0.5, 1.0],
    [-0.6, 0.6, 0.5, 1.0],
    [0.6, 0.6, 0.5, 1.0],
];

/// Linear RGBA per corner, matching `COLORS` in `shaders/mesh_shader.slang`.
///
/// One saturated primary each, no two alike, for the reason
/// [`crate::triangle::VERTICES`] gives: a Y flip, an X mirror or a reversed
/// vertex order each produce a visibly different picture.
pub const COLORS: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 1.0],
    [0.0, 1.0, 0.0, 1.0],
    [0.0, 0.0, 1.0, 1.0],
];

/// The tint `taskMain` puts in its payload, which `amplifiedMeshMain`
/// multiplies every colour by.
///
/// Red is killed, so a frame drawn through the amplification stage has a black
/// apex and unchanged green and blue corners — the difference that makes the
/// stage observable rather than assumed.
pub const AMPLIFICATION_TINT: [f32; 4] = [0.0, 1.0, 1.0, 1.0];

#[cfg(test)]
mod tests {
    use super::*;

    /// The diagnostic properties the golden image and its sample points depend
    /// on, stated as assertions rather than as prose in the shader's comments.
    #[test]
    fn every_corner_is_distinguishable_from_every_other() {
        for (index, color) in COLORS.iter().enumerate() {
            let saturated: Vec<usize> = (0..3).filter(|c| color[*c] == 1.0).collect();
            assert_eq!(saturated, vec![index], "corner {index} colour");
            assert_eq!(color[3], 1.0, "alpha must survive to the target");
        }
        for axis in 0..2 {
            let mut values: Vec<f32> = POSITIONS.iter().map(|p| p[axis]).collect();
            values.sort_by(f32::total_cmp);
            values.dedup();
            assert!(
                values.len() >= 2,
                "axis {axis} must distinguish the corners"
            );
        }
    }

    /// Every vertex inside the clip volume, or the "triangle" is a clipped
    /// polygon and the golden image is of something else. Vulkan clips z to
    /// `0..=w`, not `-w..=w`.
    #[test]
    fn every_vertex_is_inside_the_clip_volume() {
        for [x, y, z, w] in POSITIONS {
            assert_eq!(w, 1.0);
            assert!(x.abs() <= w && y.abs() <= w, "({x}, {y})");
            assert!((0.0..=w).contains(&z), "z {z}");
        }
    }

    /// The apex points the other way from [`crate::triangle::VERTICES`], which
    /// is what stops one golden image standing in for the other.
    #[test]
    fn the_apex_points_the_opposite_way_from_the_raster_triangle() {
        let raster_apex = crate::triangle::VERTICES[0].position[1];
        assert!(raster_apex > 0.0, "the raster triangle's apex is up");
        assert!(POSITIONS[0][1] < 0.0, "this one's apex is down");
    }

    /// The tint has to change the picture, or the amplification stage is
    /// unobservable and the test that renders through it cannot fail.
    #[test]
    fn the_amplification_tint_changes_at_least_one_corner() {
        assert_ne!(AMPLIFICATION_TINT, [1.0, 1.0, 1.0, 1.0]);
        let tinted: Vec<[f32; 4]> = COLORS
            .iter()
            .map(|color| std::array::from_fn(|c| color[c] * AMPLIFICATION_TINT[c]))
            .collect();
        assert_ne!(tinted[0], COLORS[0], "the red apex must go dark");
        assert_eq!(tinted[1], COLORS[1], "green must survive");
        assert_eq!(tinted[2], COLORS[2], "blue must survive");
    }
}
