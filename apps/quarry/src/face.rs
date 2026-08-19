//! The quarry face: one dense mesh whose depth range is the point.
//!
//! `docs/plan/sample/14-quarry.md`'s scope asks for "high-polygon rock ...
//! content with a wide depth range, **chosen so that per-cluster selection has
//! something to select differently across a single mesh**". That sentence is
//! the whole specification for this module, and it rules out the obvious
//! shapes: a flat wall is one distance from the camera, so every cluster in it
//! wants the same level and per-cluster selection has nothing to prove over
//! per-instance selection.
//!
//! So the face recedes. It is a heightfield over `X` (across) and `Z` (away),
//! and `Z` spans [`DEPTH_METRES`] — far enough that the near clusters and the
//! far ones of the *same* mesh sit at screen-space errors an order of magnitude
//! apart.
//!
//! # Deterministic, and not by convention
//!
//! Every height comes from [`crcbl::core::rand::hash_u64`] of the lattice
//! coordinate, so a vertex's value depends on *where it is* and on nothing
//! else — not on iteration order, not on how many octaves ran before it, not on
//! which thread got there first. A generator seeded once and walked in order
//! would be equally reproducible today and would quietly stop being so the
//! first time somebody parallelised the loop.
//!
//! `the_same_seed_and_size_give_identical_bytes` is the check, and
//! `docs/plan/sample/14-quarry.md`'s exit criteria want golden meshes for
//! exactly this reason.

use crcbl::math::Vec3;

/// How far the face recedes from the near edge, in metres.
///
/// The number that makes per-cluster selection observable rather than a
/// formality — see the [module docs](self).
pub const DEPTH_METRES: f32 = 180.0;

/// How wide the face is, in metres.
pub const WIDTH_METRES: f32 = 120.0;

/// The tallest the rock stands above the floor, in metres.
pub const HEIGHT_METRES: f32 = 34.0;

/// The seed every height is hashed against.
///
/// A constant rather than a parameter: this sample has one face, the goldens
/// are taken against it, and a seed nobody varies is a knob that only breaks
/// them.
const SEED: u64 = 0x0000_5175_4152_5259;

/// Lattice cells per octave of the value noise, coarsest first.
///
/// Coarse cells make the quarry's benches and the fine ones its rubble, and the
/// amplitudes halve as the frequency doubles — the ordinary construction, which
/// is worth naming because it is the reason a single mesh has both a
/// metres-wide feature and a centimetres-wide one for a cluster hierarchy to
/// have something to collapse.
const OCTAVES: [(u32, f32); 4] = [(3, 1.0), (7, 0.5), (17, 0.25), (41, 0.125)];

/// One generated mesh: positions, normals and triangle indices.
///
/// Deliberately the three arrays `crcbl_scene::build_meshlets` and
/// `crcbl_render` already take, rather than a struct-of-vertex: the meshlet
/// builder wants positions alone, and handing it a slice of a larger vertex
/// would mean a copy per call.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// One position per vertex, in metres.
    pub positions: Vec<[f32; 3]>,
    /// One unit normal per vertex.
    pub normals: Vec<[f32; 3]>,
    /// Three indices per triangle.
    pub indices: Vec<u32>,
}

impl Face {
    /// Triangles this face holds.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// The face at `cells` quads across and `cells` deep.
///
/// `cells` is the *quad* count per side, so the vertex lattice is
/// `(cells + 1)^2` and the triangle count is `cells * cells * 2` —
/// `a_face_has_two_triangles_per_cell` pins that rather than leaving it to a
/// reader's arithmetic.
///
/// # Panics
///
/// If `cells` is zero: a face with no quads has no surface, and every caller
/// here passes a constant.
#[must_use]
pub fn quarry_face(cells: u32) -> Face {
    assert!(cells > 0, "a quarry face needs at least one quad");
    let side = cells as usize + 1;

    let mut positions = Vec::with_capacity(side * side);
    for row in 0..side {
        for column in 0..side {
            // `row`/`column` in `0..=cells`, mapped to metres. The face is
            // centred on `X` and starts at the origin on `Z`, so the camera
            // that looks down it starts outside rather than inside the rock.
            let across = column as f32 / cells as f32;
            let away = row as f32 / cells as f32;
            positions.push([
                (across - 0.5) * WIDTH_METRES,
                height_at(across, away) * HEIGHT_METRES,
                away * DEPTH_METRES,
            ]);
        }
    }

    let mut indices = Vec::with_capacity(cells as usize * cells as usize * 6);
    for row in 0..cells as usize {
        for column in 0..cells as usize {
            let top_left = (row * side + column) as u32;
            let top_right = top_left + 1;
            let bottom_left = top_left + side as u32;
            let bottom_right = bottom_left + 1;
            // Counter-clockwise seen from +Y, which is the front face for
            // `crcbl`'s right-handed, +Y-up convention — see
            // `docs/plan/03-gpu-driven-rendering.md`.
            indices.extend_from_slice(&[top_left, bottom_left, top_right]);
            indices.extend_from_slice(&[top_right, bottom_left, bottom_right]);
        }
    }

    let normals = normals_of(&positions, &indices);
    Face {
        positions,
        normals,
        indices,
    }
}

/// The surface height at `(across, away)`, both in `0..=1`, in `0..=1`.
///
/// Sums [`OCTAVES`] of bilinear value noise. The result is clamped into the
/// unit range rather than normalised by the amplitude sum, because the sum is
/// what decides how much of the range the rock actually occupies and a
/// normalised one would flatten every octave's contribution equally.
fn height_at(across: f32, away: f32) -> f32 {
    let mut height = 0.0;
    for (cells, amplitude) in OCTAVES {
        height += amplitude * value_noise(across, away, cells);
    }
    height.clamp(0.0, 1.0)
}

/// Bilinearly interpolated value noise on a `cells`-square lattice.
fn value_noise(across: f32, away: f32, cells: u32) -> f32 {
    let x = across * cells as f32;
    let z = away * cells as f32;
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    // Smoothstep on both axes: linear interpolation of a lattice leaves a
    // visible crease along every cell boundary, and a crease is exactly the
    // artefact the seam review in this sample's exit criteria looks for.
    let (sx, sz) = (smoothstep(fx), smoothstep(fz));

    let corner = |dx: u32, dz: u32| lattice(x0 as u32 + dx, z0 as u32 + dz, cells);
    let top = lerp(corner(0, 0), corner(1, 0), sx);
    let bottom = lerp(corner(0, 1), corner(1, 1), sx);
    lerp(top, bottom, sz)
}

/// One lattice point's value, in `0..=1`.
///
/// Keyed on the coordinate and the lattice size, so two octaves never draw the
/// same value for the same cell — which would correlate them and cost the
/// hierarchy the detail it exists to collapse.
fn lattice(x: u32, z: u32, cells: u32) -> f32 {
    let index = u64::from(z) << 40 | u64::from(x) << 8 | u64::from(cells % 256);
    let bits = crcbl::core::rand::hash_u64(SEED, index);
    // The top `MANTISSA_BITS` over their own range: every value in it is
    // exactly representable in `f32`, so the division is exact and the same
    // hash gives the same float on every platform. Taking the *top* bits rather
    // than the low ones matters for a hash whose avalanche is weakest at the
    // bottom.
    (bits >> (u64::BITS - MANTISSA_BITS)) as f32 / MANTISSA_SCALE
}

/// Bits of the hash a lattice value keeps.
///
/// `f32`'s mantissa is 24 bits including its implicit leading one, so this is
/// the widest integer range whose every member survives the conversion
/// unrounded.
const MANTISSA_BITS: u32 = f32::MANTISSA_DIGITS;

/// What [`MANTISSA_BITS`] of hash divide by to land in `0..1`.
const MANTISSA_SCALE: f32 = (1u32 << MANTISSA_BITS) as f32;

/// `3t^2 - 2t^3`, the standard smoothstep.
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Area-weighted vertex normals.
///
/// Area-weighted rather than uniform: a heightfield's triangles vary in area
/// wherever the surface is steep, and averaging their face normals equally
/// leans the result towards whichever side happened to be tessellated finer.
/// The cross product's length *is* twice the triangle's area, so weighting is
/// what you get by not normalising before summing.
fn normals_of(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut sums = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] =
            [triangle[0], triangle[1], triangle[2]].map(|i| Vec3::from(positions[i as usize]));
        let face = (b - a).cross(c - a);
        for index in triangle {
            sums[*index as usize] += face;
        }
    }
    sums.into_iter()
        .map(|sum| {
            // A degenerate corner — every triangle touching it collapsed — has
            // no direction to report, and `normalize` on a zero vector is
            // `NaN`. Up is the honest answer for a heightfield and the one
            // value that cannot make a lighting result look plausible and be
            // wrong.
            sum.try_normalize().unwrap_or(Vec3::Y).to_array()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small enough to be quick, large enough to span several lattice cells of
    /// every octave — the coarsest is [`OCTAVES`]`[0]`, so anything under that
    /// would sample one cell and prove nothing about interpolation.
    const CELLS: u32 = 64;

    /// The counts a caller can rely on, stated rather than left to arithmetic.
    #[test]
    fn a_face_has_two_triangles_per_cell_and_a_vertex_per_lattice_point() {
        let face = quarry_face(CELLS);
        assert_eq!(face.positions.len(), (CELLS as usize + 1).pow(2));
        assert_eq!(face.normals.len(), face.positions.len());
        assert_eq!(face.triangles(), CELLS as usize * CELLS as usize * 2);
        assert_eq!(face.indices.len() % 3, 0);
    }

    /// **Every index names a vertex that exists.** An out-of-range one is a
    /// panic in anything that reads the mesh, and the meshlet builder refuses
    /// the whole face for it.
    #[test]
    fn every_index_is_in_range() {
        let face = quarry_face(CELLS);
        let vertices = face.positions.len() as u32;
        assert!(
            face.indices.iter().all(|index| *index < vertices),
            "an index names a vertex past the end of a {vertices}-vertex face",
        );
    }

    /// **The same size gives identical bytes.** The exit criteria want golden
    /// meshes, and a golden of something that regenerates differently is a
    /// test that fails for no reason.
    #[test]
    fn the_same_size_gives_identical_geometry() {
        assert_eq!(quarry_face(CELLS), quarry_face(CELLS));
    }

    /// **Nothing is `NaN` or infinite.** A single one poisons a bounding box,
    /// a cluster sphere and every normal that touches it — see
    /// `crcbl_core::bounds`, which exists because that happened.
    #[test]
    fn every_position_and_normal_is_finite() {
        let face = quarry_face(CELLS);
        for value in face.positions.iter().chain(face.normals.iter()).flatten() {
            assert!(value.is_finite(), "a coordinate is {value}");
        }
    }

    /// Normals are unit length, which is what shading assumes and nothing else
    /// checks.
    #[test]
    fn every_normal_is_unit_length() {
        for normal in quarry_face(CELLS).normals {
            let length = Vec3::from(normal).length();
            assert!(
                (length - 1.0).abs() <= 1e-4,
                "a normal is {length} long: {normal:?}",
            );
        }
    }

    /// **The face is not flat, and its relief is not uniform across the depth.**
    ///
    /// The guard that stops every test above passing over a useless mesh. A
    /// generator returning one constant height satisfies the counts, the
    /// indices, the determinism and the finiteness — and gives a cluster
    /// hierarchy nothing to collapse and per-cluster selection nothing to
    /// select differently, which is the sample's entire subject.
    #[test]
    fn the_face_has_relief_and_it_differs_near_and_far() {
        let face = quarry_face(CELLS);
        let side = CELLS as usize + 1;
        let height_of = |row: usize, column: usize| face.positions[row * side + column][1];

        let all: Vec<f32> = (0..side)
            .flat_map(|row| (0..side).map(move |column| (row, column)))
            .map(|(row, column)| height_of(row, column))
            .collect();
        let low = all.iter().copied().fold(f32::INFINITY, f32::min);
        let high = all.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            high - low > HEIGHT_METRES * 0.25,
            "the face spans {:.2} m of {HEIGHT_METRES} — too flat for a cluster \
             hierarchy to have anything to collapse",
            high - low,
        );

        // Near and far rows are different rock, not the same profile repeated:
        // a heightfield that varied only across `X` would be a corrugation, and
        // every cluster down one column would want the same level.
        let profile = |row: usize| -> Vec<f32> { (0..side).map(|c| height_of(row, c)).collect() };
        assert_ne!(
            profile(0),
            profile(side - 1),
            "the near and far edges have the same profile, so the mesh does not \
             vary along the axis its depth range exists for",
        );
    }

    /// **The depth range is really there.** It is the reason this shape was
    /// chosen over a wall — see the [module docs](self).
    #[test]
    fn the_face_spans_its_declared_depth() {
        let face = quarry_face(CELLS);
        let z: Vec<f32> = face.positions.iter().map(|p| p[2]).collect();
        let near = z.iter().copied().fold(f32::INFINITY, f32::min);
        let far = z.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((near - 0.0).abs() < 1e-3, "the near edge is at {near}");
        assert!(
            (far - DEPTH_METRES).abs() < 1e-3,
            "the far edge is at {far}, not {DEPTH_METRES}",
        );
    }

    /// **A degenerate corner gets `+Y`, not `NaN`.**
    ///
    /// `normals_of`'s fallback is unreachable from [`quarry_face`] — no
    /// heightfield vertex has every triangle around it collapse — so it is
    /// driven here directly. Found by red-checking: replacing the fallback with
    /// a bare `sum / sum.length()` left every test green, which is a guard
    /// nothing exercises and would be a field of `NaN` normals the first time
    /// content did reach it.
    #[test]
    fn a_degenerate_triangle_yields_up_rather_than_nan() {
        // Three coincident vertices: the cross product is the zero vector, so
        // the normalise has no direction to return.
        let positions = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let normals = normals_of(&positions, &[0, 1, 2]);
        assert_eq!(normals, vec![Vec3::Y.to_array(); 3]);
        for value in normals.iter().flatten() {
            assert!(value.is_finite(), "a degenerate normal is {value}");
        }
    }

    /// **The meshlet builder accepts it**, which is the premise of the whole
    /// sample: a face the builder refuses is not content, whatever else is true
    /// of it.
    #[test]
    fn the_meshlet_builder_accepts_the_face() {
        let face = quarry_face(CELLS);
        let build = crcbl::scene::meshlet::build_meshlets(&face.positions, &face.indices)
            .expect("the meshlet builder takes the quarry face");
        assert!(
            !build.clusters().is_empty(),
            "a face of {} triangles built no clusters",
            face.triangles(),
        );
    }
}
