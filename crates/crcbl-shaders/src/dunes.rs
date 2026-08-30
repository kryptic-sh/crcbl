//! The dunes patch: a terrain-sized height field, and the model per-cluster LOD
//! exists for.
//!
//! # Why this mesh and not another
//!
//! `docs/plan/25-lod.md`'s per-cluster selection scales one stored error by the
//! distance to the group's sphere, so **the variation across a frame comes from
//! the distance term**. A mesh that subtends one distance — the cube, the
//! pyramid, the open box — asks the same question of every one of its clusters
//! and gets the same answer, whatever the DAG under it looks like. Showing the
//! mechanism at all therefore needs a mesh that is *large in world units
//! relative to its features*: a ground plane whose far edge is many times
//! further away than its near edge, so a single cut through the DAG legitimately
//! draws the two ends at different levels.
//!
//! Two further properties earn this particular surface the job, and they are
//! `crcbl_scene::simplify`'s reasons for the fixture this shares its height
//! function with:
//!
//! * **Curvature that varies by region.** A dome's cap costs real error to
//!   decimate and the quartic-flat valleys between domes decimate for nearly
//!   nothing, so different parts of one surface genuinely want different detail.
//! * **No trigonometry anywhere.** [`height`](crate::dunes::height) and
//!   [`normal_at`](crate::dunes::normal_at) are `+`, `-`,
//!   `*`, `/`, [`f32::floor`] and one [`f32::sqrt`], every one of which IEEE 754
//!   pins exactly. `sinf`/`cosf` are not correctly rounded and differ in the
//!   last place between glibc, Apple's libm and MSVC, which would make the
//!   committed artifact this module's geometry feeds a per-platform artifact.
//!
//! # The DAG is cooked, this geometry is not
//!
//! The vertices and indices below are computed here, because a closed-form
//! height field is smaller and clearer as code than as data. The **cluster DAG**
//! over them is not: it comes out of `crcbl_scene::cluster_dag::build_cluster_dag`,
//! which this crate cannot call, so it is a committed artifact with a generator
//! and a `--check` beside it. See [`crate::cluster_dag`].

use crate::mesh::MeshVertex;
use crate::vertex::UvRange;

/// Quads along each side of the patch.
///
/// The size is what makes the mesh worth having, and it is set by two measured
/// facts about the builder rather than by taste:
///
/// * a group's sphere is grown to contain every sphere below it, so radii
///   compound up the DAG — on a 32-unit patch the second level's groups already
///   bound essentially the whole mesh and have no spatial discrimination left;
/// * the decimator's error roughly rises by an order of magnitude per level, so
///   the deep levels are only ever selected at large distance.
///
/// Both say the same thing: the patch has to span many multiples of a group's
/// radius before a camera can tell one end of it from the other. `EXTENT` is
/// what that costs in triangles, and [`crate::cluster_dag::dunes_dag`]'s tests
/// are what hold the discrimination to more than a claim.
pub const DUNES_SIDE: usize = 64;

/// How far the patch reaches from the origin along `x` and `z`, in the units
/// [`height`] is written in — one unit per quad, and centred, so the mesh needs
/// no translation to sit on the origin.
pub const DUNES_EXTENT: f32 = DUNES_SIDE as f32 * 0.5;

/// Vertices in the patch: a grid of `DUNES_SIDE + 1` squared, every one shared
/// by the quads that meet at it.
///
/// Shared rather than unwelded — unlike [`crate::mesh::open_box_vertices`],
/// whose quads each carry four of their own — because the DAG's simplifier
/// collapses *edges*, and an unwelded grid has no edge between two quads to
/// collapse. A mesh split per face decimates to nothing at all.
pub const DUNES_VERTEX_COUNT: usize = (DUNES_SIDE + 1) * (DUNES_SIDE + 1);

/// Indices in the patch: two triangles per quad.
pub const DUNES_INDEX_COUNT: usize = DUNES_SIDE * DUNES_SIDE * 6;

/// The height field's peak, in the same units as [`DUNES_EXTENT`].
const AMPLITUDE: f32 = 4.0;

/// The dome lattice's periods along `x` and `z`.
///
/// Coprime with each other and with the one-unit grid step, so the domes line up
/// neither with the clustering nor with each other and no two regions of the
/// patch are the same geometry twice.
const PERIOD_X: f32 = 9.0;
const PERIOD_Z: f32 = 7.0;

/// The valley colour and the crest colour, linear RGB.
///
/// Diagnostic rather than decorative, as [`crate::mesh::FACES`]' are: the ramp
/// between them makes the surface's shape legible in a still frame, so a level
/// change that flattened a dome shows up as a colour that stops climbing rather
/// than as geometry a reader has to infer from shading.
const VALLEY: [f32; 3] = [0.16, 0.20, 0.26];
const CREST: [f32; 3] = [0.86, 0.72, 0.44];

/// One period of a smooth bump: zero with zero slope at every integer, one at
/// every half-integer, and a quartic in between.
///
/// `16 f²(1-f)²` for `f` the fractional part of `t`. Its derivative vanishes at
/// both ends of a period, so tiling it leaves no crease at the joins — which a
/// sine would also give, and this gives without a sine.
fn bump(t: f32) -> f32 {
    let f = t - t.floor();
    let g = f * (1.0 - f);
    16.0 * g * g
}

/// [`bump`]'s derivative, `32 f (1-f) (1-2f)`.
///
/// Differentiating `16 g²` for `g = f(1-f)` gives `32 g dg/df`, and `dg/df` is
/// `1 - 2f`. Written out rather than sampled, because a finite difference over a
/// quartic is a second approximation nothing needs.
fn bump_slope(t: f32) -> f32 {
    let f = t - t.floor();
    32.0 * f * (1.0 - f) * (1.0 - 2.0 * f)
}

/// The patch's height above the `xz` plane at `(x, z)`: a field of rounded
/// domes on a lattice of `PERIOD_X` by `PERIOD_Z`, `AMPLITUDE` tall.
///
/// Defined for every pair of coordinates rather than only at the grid, which is
/// what lets [`normal_at`] shade a vertex the DAG's simplifier *moved* against
/// the surface it came from.
///
/// `crcbl_scene::simplify`'s test fixture calls this, so the surface the
/// decimator is tested against and the surface this crate ships are one
/// function.
#[must_use]
pub fn height(x: f32, z: f32) -> f32 {
    AMPLITUDE * bump(x / PERIOD_X) * bump(z / PERIOD_Z)
}

/// The unit normal of the surface [`height`] describes, at `(x, z)`.
///
/// The analytic gradient — `(-∂h/∂x, 1, -∂h/∂z)` normalised — rather than an
/// average of the neighbouring faces' normals. It is exact at every point
/// instead of at the grid alone, which is the property a coarser DAG level
/// needs: its vertices sit wherever the decimator put them, and shading them
/// against the surface keeps a level change from also being a lighting change.
#[must_use]
pub fn normal_at(x: f32, z: f32) -> [f32; 3] {
    let dx = AMPLITUDE * bump_slope(x / PERIOD_X) / PERIOD_X * bump(z / PERIOD_Z);
    let dz = AMPLITUDE * bump(x / PERIOD_X) * bump_slope(z / PERIOD_Z) / PERIOD_Z;
    let length = (dx * dx + 1.0 + dz * dz).sqrt();
    [-dx / length, 1.0 / length, -dz / length]
}

/// One vertex of the patch, at a position that need not be on the grid.
///
/// The DAG's coarser levels are arrays of positions and nothing else — the
/// decimator carries no attributes — so this is what turns any of them into
/// geometry a vertex stage can read. Level 0 and level 3 are shaded by the same
/// rule, from the same surface.
///
/// The colour ramps `VALLEY` to `CREST` by the vertex's own height, and the
/// texture coordinates put one copy of the material's page over the whole patch.
#[must_use]
pub fn vertex_at(position: [f32; 3]) -> MeshVertex {
    let [x, y, z] = position;
    let normal = normal_at(x, z);
    let climb = (y / AMPLITUDE).clamp(0.0, 1.0);
    let color =
        [0, 1, 2].map(|channel| VALLEY[channel] + climb * (CREST[channel] - VALLEY[channel]));
    let uv = [x, z].map(|axis| (axis + DUNES_EXTENT) / (2.0 * DUNES_EXTENT));
    MeshVertex::from_normal(
        position,
        normal,
        [color[0], color[1], color[2], 1.0],
        uv,
        &uv_range(),
    )
}

/// The range every coordinate [`vertex_at`] produces is quantised against: the
/// unit square.
///
/// The patch spans `[-DUNES_EXTENT, DUNES_EXTENT]` on both axes and the
/// coordinate is that mapped onto `0..=1`, so the corners reach both ends
/// exactly and nothing reaches outside. **Every level of the DAG shares it**,
/// which is what the mesh path needs: that path resolves a row through
/// `instance.mesh`, so a coarser level's vertices are decoded through level
/// 0's range — see [`GpuMesh::uv_range`](crate::mesh::GpuMesh::uv_range). A
/// decimated vertex sits inside the patch, so the shared range covers it.
#[must_use]
pub fn uv_range() -> UvRange {
    UvRange::from_uvs(&[[0.0, 0.0], [1.0, 1.0]])
}

/// The patch's grid positions, row by row along `z`, each row ascending in `x`.
///
/// This is what `crcbl_scene::cluster_dag::build_cluster_dag` is handed, so the
/// committed DAG's level 0 is this array verbatim and every cooked index is an
/// index into it.
#[must_use]
pub fn positions() -> Vec<[f32; 3]> {
    let mut positions = Vec::with_capacity(DUNES_VERTEX_COUNT);
    for row in 0..=DUNES_SIDE {
        for column in 0..=DUNES_SIDE {
            let x = column as f32 - DUNES_EXTENT;
            let z = row as f32 - DUNES_EXTENT;
            positions.push([x, height(x, z), z]);
        }
    }
    positions
}

/// The patch's triangles, two per quad, wound counter-clockwise seen from `+Y`.
///
/// `+Y` is up here, as it is for every other mesh in this crate, so the winding
/// makes the lit side the one a camera above the patch sees — and a mesh wound
/// the other way is culled away entirely rather than drawn dark.
#[must_use]
pub fn indices() -> Vec<u32> {
    let stride = (DUNES_SIDE + 1) as u32;
    let mut indices = Vec::with_capacity(DUNES_INDEX_COUNT);
    for row in 0..DUNES_SIDE as u32 {
        for column in 0..DUNES_SIDE as u32 {
            let near = row * stride + column;
            let far = near + stride;
            indices.extend_from_slice(&[near, far, near + 1, near + 1, far, far + 1]);
        }
    }
    indices
}

/// The patch's grid vertices, ready to upload.
#[must_use]
pub fn vertices() -> Vec<MeshVertex> {
    positions().into_iter().map(vertex_at).collect()
}

/// [`vertices`] as the bytes a storage buffer holds, on
/// [`crate::mesh::cube_vertex_bytes`]' terms exactly.
#[must_use]
pub fn vertex_bytes() -> Vec<u8> {
    crate::mesh::vertex_bytes(&vertices())
}

/// [`indices`] as the bytes an index buffer holds.
#[must_use]
pub fn index_bytes() -> Vec<u8> {
    indices().into_iter().flat_map(u32::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vertex::QTangent;
    use crate::vertex::encode_rgba8 as crcbl_encode_rgba8;

    /// The two arrays are the sizes the constants promise, and every index
    /// names a vertex that exists.
    #[test]
    fn the_patch_is_the_grid_its_constants_describe() {
        let positions = positions();
        let indices = indices();
        assert_eq!(positions.len(), DUNES_VERTEX_COUNT);
        assert_eq!(indices.len(), DUNES_INDEX_COUNT);
        assert!(
            indices
                .iter()
                .all(|&index| (index as usize) < positions.len()),
            "an index reaches past the grid"
        );
        // Every vertex is referenced, so the decimator is never handed one it
        // would silently drop.
        let mut used = vec![false; positions.len()];
        for &index in &indices {
            used[index as usize] = true;
        }
        assert!(
            used.iter().all(|&used| used),
            "a grid vertex has no triangle"
        );
    }

    /// **The patch is large relative to its own features**, which is the whole
    /// reason it exists: the far corner is many times further from a viewer at
    /// the near edge than the near corner is.
    ///
    /// The ratio is what the distance term in
    /// [`crate::cluster_dag::projected_error`] has to work with, and a mesh
    /// where it is near one cannot show per-cluster selection however good its
    /// DAG is.
    #[test]
    fn the_far_edge_is_far_further_away_than_the_near_edge() {
        // A viewer at the near edge, standing a little above the surface.
        let eye = [0.0f32, AMPLITUDE, -DUNES_EXTENT - 2.0];
        let distance = |corner: [f32; 3]| {
            [0, 1, 2]
                .map(|axis| corner[axis] - eye[axis])
                .iter()
                .map(|delta| delta * delta)
                .sum::<f32>()
                .sqrt()
        };
        let near = distance([0.0, 0.0, -DUNES_EXTENT]);
        let far = distance([0.0, 0.0, DUNES_EXTENT]);
        assert!(
            far > 10.0 * near,
            "the far edge is {far} away and the near edge {near}, a ratio of \
             {} — too flat for a distance term to distinguish",
            far / near
        );
    }

    /// Every triangle faces up, so the winding is the one a camera above the
    /// patch sees the front of.
    ///
    /// A height field's faces all have a positive `y` component in their normal
    /// by construction, so a mesh wound the other way is detectable without
    /// tracking which face came from where.
    #[test]
    fn every_triangle_is_wound_counter_clockwise_seen_from_above() {
        let positions = positions();
        for (face, corners) in indices().chunks_exact(3).enumerate() {
            let point = |corner: usize| positions[corners[corner] as usize];
            let [a, b, c] = [point(0), point(1), point(2)];
            let edge = |to: [f32; 3], from: [f32; 3]| [0, 1, 2].map(|axis| to[axis] - from[axis]);
            let (u, v) = (edge(b, a), edge(c, a));
            let up = u[2] * v[0] - u[0] * v[2];
            assert!(up > 0.0, "face {face} faces down, with {up}");
        }
    }

    /// The analytic normal agrees with the surface it claims to describe, as a
    /// central difference of [`height`] measures it.
    ///
    /// A transcription slip in [`bump_slope`] — a dropped factor, a sign —
    /// produces a normal that still normalises and still points up, and shades
    /// the whole patch subtly wrong. This is the check that names it.
    ///
    /// **The tolerance is the difference's error, not the code's.** A central
    /// difference of a quartic is wrong by about `h²f'''/6`, and subtracting two
    /// nearly equal `f32`s loses about `1e-7/2h` more; the step below is near
    /// where the two are balanced and the tolerance is an order of magnitude
    /// above their sum. Every slip this exists to catch is off by a factor, not
    /// by a rounding.
    #[test]
    fn the_normal_is_the_gradient_of_the_height_it_shades() {
        let step = 1.0e-2f32;
        for &(x, z) in &[(0.0f32, 0.0f32), (1.5, 2.25), (-3.75, 4.5), (7.0, -6.5)] {
            let slope = |along_x: bool| {
                let (ahead, behind) = if along_x {
                    (height(x + step, z), height(x - step, z))
                } else {
                    (height(x, z + step), height(x, z - step))
                };
                (ahead - behind) / (2.0 * step)
            };
            let expected = {
                let (dx, dz) = (slope(true), slope(false));
                let length = (dx * dx + 1.0 + dz * dz).sqrt();
                [-dx / length, 1.0 / length, -dz / length]
            };
            let found = normal_at(x, z);
            for axis in 0..3 {
                assert!(
                    (found[axis] - expected[axis]).abs() < 2.0e-4,
                    "at ({x}, {z}) axis {axis}: {found:?} against {expected:?}"
                );
            }
        }
    }

    /// A vertex the simplifier moved off the surface still shades against the
    /// surface, and still ramps its colour by its own height.
    #[test]
    fn a_vertex_off_the_surface_takes_the_surfaces_normal() {
        let (x, z) = (1.5f32, 2.25f32);
        let lifted = vertex_at([x, height(x, z) + 1.0, z]);
        // Through the encoding, because that is what the vertex now carries:
        // the frame the quantised quaternion decodes to has to be the surface's
        // to within the error `QTangent` states.
        let decoded = lifted.qtangent.decode().normal;
        let surface = normal_at(x, z);
        for axis in 0..3 {
            assert!(
                (decoded[axis] - surface[axis]).abs() <= QTangent::MAX_COMPONENT_ERROR,
                "axis {axis} of the decoded normal is {} where the surface's is {}",
                decoded[axis],
                surface[axis]
            );
        }
        assert_eq!(lifted.position, [x, height(x, z) + 1.0, z]);

        // The ramp's two ends, so the colour is a function of height rather
        // than a constant that happens to look right.
        assert_eq!(
            vertex_at([0.0, 0.0, 0.0]).color,
            crcbl_encode_rgba8([VALLEY[0], VALLEY[1], VALLEY[2], 1.0])
        );
        assert_eq!(
            vertex_at([0.0, AMPLITUDE, 0.0]).color,
            crcbl_encode_rgba8([CREST[0], CREST[1], CREST[2], 1.0])
        );
    }
}
