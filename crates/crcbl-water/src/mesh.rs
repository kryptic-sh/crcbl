//! The surface of a still body: a flat grid clipped to its outline.
//!
//! # Why a grid and not the outline's own triangles
//!
//! A still surface is a plane, and the outline triangulated once would draw it
//! exactly. The grid is for the rung after this one: `docs/plan/55-water.md`'s
//! lakes and pools move by waves evaluated at each vertex, and a triangle
//! spanning the whole body has no vertex in its middle to move. So the surface
//! is built the way it will be displaced — a regular grid of `spacing` over the
//! outline's bounds, with every cell clipped to the outline — and at this rung
//! the grid's only cost is vertices a flat plane did not need.
//!
//! # How
//!
//! 1. The outline is cut into triangles by **ear clipping** (Meisters, *Polygons
//!    have ears*, 1975): repeatedly remove a convex corner whose triangle holds
//!    no other outline point. Quadratic, which a pool's tens of points afford.
//! 2. Every grid cell overlapping a triangle is clipped against it by
//!    **Sutherland–Hodgman** (1974), which is exact for a convex clip region —
//!    and a triangle is one, where the whole outline need not be.
//! 3. Each clipped piece is convex, so it fans into triangles from its first
//!    corner, and corners that land on identical coordinates are welded into
//!    one vertex.
//!
//! The pieces tile the outline without overlap, because the triangles do and
//! the grid cells do; [`surface_mesh`]'s tests hold the total area to the
//! outline's.

use std::collections::HashMap;

use crate::body::{BodyError, WaterBody, orient, signed_area};

/// The most grid cells an outline's bounds may hold at the spacing asked for.
///
/// About four million — a two-kilometre lake at one metre — and past it
/// [`surface_mesh`] refuses rather than allocating. A body that size wants the
/// camera-centred rings of `docs/plan/55-water.md`'s twelfth decision, not a
/// CPU grid over all of it, so reaching this is a caller's mistake worth
/// naming rather than a mesh worth building.
pub const MAX_GRID_CELLS: u64 = 1 << 22;

/// A body's surface as an indexed triangle list.
///
/// Every triangle faces **up**: for corners `a`, `b`, `c` in index order,
/// `(b - a) × (c - a)` points along `+Y`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceMesh {
    /// World-space positions in metres, every one at the body's level.
    pub positions: Vec<[f32; 3]>,
    /// Three indices into [`SurfaceMesh::positions`] per triangle.
    pub indices: Vec<u32>,
}

/// Meshes `body`'s surface as a grid of `spacing` metres clipped to its
/// outline.
///
/// # Errors
///
/// [`BodyError`] when the body is not well formed — see [`WaterBody`] — when
/// `spacing` is not a finite positive length, when the outline's bounds hold
/// more than [`MAX_GRID_CELLS`] cells at that spacing, or when the clipped grid
/// has more vertices than a `u32` addresses. Nothing about a body makes this
/// panic.
pub fn surface_mesh(body: &WaterBody, spacing: f32) -> Result<SurfaceMesh, BodyError> {
    body.validate()?;
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(BodyError::InvalidSpacing { spacing });
    }

    let outline = &body.outline;
    let (low, high) = bounds(outline);
    let columns = cells_across(low[0], high[0], spacing);
    let rows = cells_across(low[1], high[1], spacing);
    let cells = columns.saturating_mul(rows);
    if cells > MAX_GRID_CELLS {
        return Err(BodyError::TooManyCells {
            cells,
            max: MAX_GRID_CELLS,
        });
    }

    let mut builder = Builder {
        level: body.level,
        mesh: SurfaceMesh::default(),
        welded: HashMap::new(),
    };
    // A grid line is computed from its own index, never as its neighbour plus
    // `spacing`, so two cells sharing an edge round it to the same `f32` and
    // tile without a crack.
    let edge = |origin: f32, index: u64| origin + index as f32 * spacing;
    for triangle in ear_clip(outline) {
        let (from, to) = bounds(&triangle);
        let first_column = cell_of(from[0], low[0], spacing);
        let last_column = cell_of(to[0], low[0], spacing).min(columns - 1);
        let first_row = cell_of(from[1], low[1], spacing);
        let last_row = cell_of(to[1], low[1], spacing).min(rows - 1);
        for row in first_row..=last_row {
            for column in first_column..=last_column {
                let (x0, x1) = (edge(low[0], column), edge(low[0], column + 1));
                let (z0, z1) = (edge(low[1], row), edge(low[1], row + 1));
                let cell = vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
                builder.fan(&clip(cell, &triangle))?;
            }
        }
    }
    Ok(builder.mesh)
}

/// The smallest and largest coordinate on each axis.
fn bounds(points: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    points.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(low, high), point| {
            (
                [low[0].min(point[0]), low[1].min(point[1])],
                [high[0].max(point[0]), high[1].max(point[1])],
            )
        },
    )
}

/// How many cells of `spacing` it takes to cover `low..=high`: at least one,
/// and saturating rather than wrapping for a span no count could hold.
fn cells_across(low: f32, high: f32, spacing: f32) -> u64 {
    let cells = ((f64::from(high) - f64::from(low)) / f64::from(spacing)).ceil();
    if cells >= u64::MAX as f64 {
        u64::MAX
    } else {
        (cells as u64).max(1)
    }
}

/// The cell `value` falls in along an axis whose grid starts at `origin`.
fn cell_of(value: f32, origin: f32, spacing: f32) -> u64 {
    // Non-negative by construction — every triangle lies inside the outline's
    // bounds — and bounded by `cells_across`, which was checked above.
    ((f64::from(value) - f64::from(origin)) / f64::from(spacing)).floor() as u64
}

/// The outline cut into counter-clockwise triangles by ear clipping.
///
/// The outline has been validated: it is simple and has area. A corner whose
/// two edges are collinear adds no area and cannot be an ear under the strict
/// convexity test below, so such corners are dropped as they are met — that
/// keeps the polygon simple and the two-ears theorem true of what remains.
fn ear_clip(outline: &[[f32; 2]]) -> Vec<[[f32; 2]; 3]> {
    let mut ring: Vec<[f32; 2]> = outline.to_vec();
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    let mut triangles = Vec::with_capacity(ring.len().saturating_sub(2));
    while ring.len() > 3 {
        let count = ring.len();
        let mut clipped = false;
        for corner in 0..count {
            let previous = ring[(corner + count - 1) % count];
            let at = ring[corner];
            let next = ring[(corner + 1) % count];
            let turn = orient(previous, at, next);
            if turn == 0.0 {
                ring.remove(corner);
                clipped = true;
                break;
            }
            if turn < 0.0 {
                continue;
            }
            let holds_another = ring.iter().enumerate().any(|(index, &point)| {
                index != corner
                    && index != (corner + count - 1) % count
                    && index != (corner + 1) % count
                    && orient(previous, at, point) >= 0.0
                    && orient(at, next, point) >= 0.0
                    && orient(next, previous, point) >= 0.0
            });
            if !holds_another {
                triangles.push([previous, at, next]);
                ring.remove(corner);
                clipped = true;
                break;
            }
        }
        // A validated outline always has an ear. This is the loop's guarantee
        // of termination rather than an expected path: were it ever reached,
        // stopping short would leave part of the surface undrawn, which the
        // area tests would report.
        if !clipped {
            break;
        }
    }
    if ring.len() == 3 && orient(ring[0], ring[1], ring[2]) > 0.0 {
        triangles.push([ring[0], ring[1], ring[2]]);
    }
    triangles
}

/// `polygon` clipped to the counter-clockwise `triangle`, by Sutherland–Hodgman.
fn clip(mut polygon: Vec<[f32; 2]>, triangle: &[[f32; 2]; 3]) -> Vec<[f32; 2]> {
    for edge in 0..3 {
        let (a, b) = (triangle[edge], triangle[(edge + 1) % 3]);
        let input = std::mem::take(&mut polygon);
        for (index, &current) in input.iter().enumerate() {
            let previous = input[(index + input.len() - 1) % input.len()];
            let current_side = orient(a, b, current);
            let previous_side = orient(a, b, previous);
            if current_side >= 0.0 {
                if previous_side < 0.0 {
                    polygon.push(crossing(previous, current, previous_side, current_side));
                }
                polygon.push(current);
            } else if previous_side >= 0.0 {
                polygon.push(crossing(previous, current, previous_side, current_side));
            }
        }
        if polygon.is_empty() {
            break;
        }
    }
    polygon
}

/// Where the segment from `from` to `to` crosses the clip line, given each
/// end's signed distance to it — which have opposite signs, or one is zero.
fn crossing(from: [f32; 2], to: [f32; 2], from_side: f64, to_side: f64) -> [f32; 2] {
    let t = from_side / (from_side - to_side);
    let lerp = |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * t) as f32;
    [lerp(from[0], to[0]), lerp(from[1], to[1])]
}

/// Accumulates welded vertices and up-facing triangles.
struct Builder {
    level: f32,
    mesh: SurfaceMesh,
    /// The index each coordinate was given, keyed by its bits — with `-0.0`
    /// folded onto `0.0`, which is the one pair of equal floats with two
    /// spellings.
    welded: HashMap<[u32; 2], u32>,
}

impl Builder {
    /// The index of the vertex at `point`, adding it if it is new.
    ///
    /// # Errors
    ///
    /// [`BodyError::TooManyVertices`] when a new vertex would need an index past
    /// `u32::MAX`. [`MAX_GRID_CELLS`] bounds the cells, not the pieces an outline
    /// of many points clips each cell into, so this is checked rather than
    /// assumed.
    fn vertex(&mut self, point: [f32; 2]) -> Result<u32, BodyError> {
        let key = [(point[0] + 0.0).to_bits(), (point[1] + 0.0).to_bits()];
        if let Some(index) = self.welded.get(&key) {
            return Ok(*index);
        }
        let index =
            u32::try_from(self.mesh.positions.len()).map_err(|_| BodyError::TooManyVertices)?;
        self.mesh.positions.push([point[0], self.level, point[1]]);
        self.welded.insert(key, index);
        Ok(index)
    }

    /// Fans the convex, counter-clockwise `piece` into triangles, skipping any
    /// that cover no area.
    ///
    /// Counter-clockwise on the XZ plane in [`orient`]'s sense is **clockwise**
    /// seen from above with `+Y` up — `(b - a) × (c - a)` of a triangle wound
    /// that way points along `-Y` — so each triangle is emitted as `a, c, b`.
    ///
    /// # Errors
    ///
    /// [`Builder::vertex`]'s.
    fn fan(&mut self, piece: &[[f32; 2]]) -> Result<(), BodyError> {
        if piece.len() < 3 {
            return Ok(());
        }
        for index in 1..piece.len() - 1 {
            let (a, b, c) = (piece[0], piece[index], piece[index + 1]);
            if orient(a, b, c) <= 0.0 {
                continue;
            }
            let triangle = [self.vertex(a)?, self.vertex(c)?, self.vertex(b)?];
            // Welding can fold a sliver's corners onto one vertex; such a
            // triangle has no area and nothing to draw.
            if triangle[0] != triangle[1]
                && triangle[1] != triangle[2]
                && triangle[0] != triangle[2]
            {
                self.mesh.indices.extend_from_slice(&triangle);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::body::Medium;

    const CLEAR: Medium = Medium {
        absorption: [0.4, 0.1, 0.05],
        scattering: [0.01, 0.01, 0.01],
    };

    fn body(outline: &[[f32; 2]], level: f32) -> WaterBody {
        WaterBody {
            outline: outline.to_vec(),
            level,
            medium: CLEAR,
        }
    }

    /// The `y` component of each triangle's `(b - a) × (c - a)`, in `f64` —
    /// the differences too, which are exact there. Taken in `f32`, the two
    /// edges of a sliver whose far corners sit one step apart round to one
    /// vector, and a triangle facing up reads as having no normal at all.
    fn normals_y(mesh: &SurfaceMesh) -> Vec<f64> {
        mesh.indices
            .chunks_exact(3)
            .map(|triangle| {
                let [a, b, c] = [0, 1, 2].map(|i| mesh.positions[triangle[i] as usize]);
                let (ab, ac) = (
                    [
                        f64::from(b[0]) - f64::from(a[0]),
                        f64::from(b[2]) - f64::from(a[2]),
                    ],
                    [
                        f64::from(c[0]) - f64::from(a[0]),
                        f64::from(c[2]) - f64::from(a[2]),
                    ],
                );
                ab[1] * ac[0] - ab[0] * ac[1]
            })
            .collect()
    }

    /// The total area the triangles cover, which is half the sum of those.
    fn area(mesh: &SurfaceMesh) -> f64 {
        normals_y(mesh).iter().sum::<f64>() / 2.0
    }

    /// How far a mesh of `outline` at `spacing` may miss the outline's area:
    /// a relative part, plus what rounding clip points to `f32` can move.
    ///
    /// The mesher computes each clip point in `f64` and rounds it to `f32`,
    /// moving each coordinate by at most half a unit in the last place. No
    /// coordinate exceeds `reach` — the outline's largest, plus one spacing for
    /// the grid's far edge — and that unit, for a value no larger, is at most
    /// `reach × f32::EPSILON`. So each rounded point lies within
    /// `d = reach × f32::EPSILON` of its exact place, which is on an edge of one
    /// of the ear clipper's triangles. To first order, taking the `f64`
    /// arithmetic before the rounding as exact, a triangle edge of length `L`
    /// can gain or lose:
    ///
    /// - `d × L` between the edge and the pieces' sides that run along it;
    /// - `d × spacing / 2` for each rounded point that should also sit on a cell
    ///   edge — at most two per cell the edge crosses, and it crosses at most
    ///   `√2 × L / spacing + 1` cells — so `d × (√2 × L + spacing)`;
    /// - `d × L / 2` where the grid's far edge rounds inside the outline's.
    ///
    /// Together less than `d × (3 × L + spacing)`. Ear clipping cuts `n` points
    /// into at most `n - 2` triangles, whose edges are the perimeter `P` plus
    /// both sides of at most `n - 3` diagonals, each a chord no longer than
    /// `P / 2`: at most `(n - 2) × P` in all, over `3 × (n - 2)` edges. The
    /// bound is `3 × (n - 2) × d × (P + spacing)`.
    fn area_tolerance(outline: &[[f32; 2]], spacing: f32) -> f64 {
        let count = outline.len();
        let perimeter: f64 = (0..count)
            .map(|index| {
                let (a, b) = (outline[index], outline[(index + 1) % count]);
                let dx = f64::from(b[0]) - f64::from(a[0]);
                let dz = f64::from(b[1]) - f64::from(a[1]);
                (dx * dx + dz * dz).sqrt()
            })
            .sum();
        let reach = outline
            .iter()
            .flatten()
            .fold(0.0f64, |reach, value| reach.max(f64::from(value.abs())))
            + f64::from(spacing);
        let d = reach * f64::from(f32::EPSILON);
        let rounding = 3.0 * (count - 2) as f64 * d * (perimeter + f64::from(spacing));
        signed_area(outline).abs() / 2.0 * 1e-4 + rounding
    }

    #[test]
    fn a_square_is_its_grid_welded_at_every_corner() {
        let mesh = surface_mesh(
            &body(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]], 1.5),
            1.0,
        )
        .expect("a square meshes");
        // Four cells, so a three-by-three lattice. The triangulation's own
        // diagonal adds no corner the lattice does not already have.
        assert_eq!(mesh.positions.len(), 9);
        assert!(mesh.positions.iter().all(|position| position[1] == 1.5));
        assert_eq!(area(&mesh), 4.0);
        assert!(normals_y(&mesh).iter().all(|y| *y > 0.0));
        assert!(
            mesh.indices
                .iter()
                .all(|index| (*index as usize) < mesh.positions.len())
        );
    }

    #[test]
    fn neighbouring_cells_share_their_edge_to_the_bit() {
        // 0.3 has no exact `f32`, so a cell's far edge taken as its near edge
        // plus the spacing lands a step away from the next cell's near edge
        // in some columns — a crack the weld cannot close. Along the square's
        // bottom edge the only corners are the cells', so a crack shows there
        // as one vertex too many.
        let mesh = surface_mesh(
            &body(&[[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0]], 0.0),
            0.3,
        )
        .expect("a square meshes");
        let bottom = mesh
            .positions
            .iter()
            .filter(|position| position[2] == 0.0)
            .count();
        assert_eq!(bottom, 11);
    }

    #[test]
    fn a_clockwise_outline_meshes_to_the_same_upward_surface() {
        let square = [[0.0, 0.0], [0.0, 3.0], [3.0, 3.0], [3.0, 0.0]];
        let mesh = surface_mesh(&body(&square, 0.0), 1.0).expect("a square meshes");
        assert_eq!(area(&mesh), 9.0);
        assert!(normals_y(&mesh).iter().all(|y| *y > 0.0));
    }

    #[test]
    fn a_concave_outline_covers_its_own_area_and_nothing_else() {
        let ell = [
            [0.0, 0.0],
            [3.0, 0.0],
            [3.0, 1.0],
            [1.0, 1.0],
            [1.0, 3.0],
            [0.0, 3.0],
        ];
        let mesh = surface_mesh(&body(&ell, 0.0), 0.5).expect("an L meshes");
        // Within rounding: the ear clipper's diagonals cross cell edges at
        // points no `f32` holds exactly.
        assert!((area(&mesh) - 5.0).abs() < 1e-6, "{}", area(&mesh));
        // No triangle's centroid lies in the notch the L leaves out.
        for triangle in mesh.indices.chunks_exact(3) {
            let centroid = triangle.iter().fold([0.0f32; 2], |sum, index| {
                let p = mesh.positions[*index as usize];
                [sum[0] + p[0] / 3.0, sum[1] + p[2] / 3.0]
            });
            assert!(
                !(centroid[0] > 1.0 && centroid[1] > 1.0),
                "a triangle centred at {centroid:?} is in the notch"
            );
        }
    }

    #[test]
    fn a_grid_coarser_than_the_body_still_covers_it() {
        let triangle = [[0.25, 0.25], [1.25, 0.25], [0.25, 1.25]];
        let mesh = surface_mesh(&body(&triangle, 0.0), 10.0).expect("a triangle meshes");
        assert_eq!(area(&mesh), 0.5);
        assert_eq!(mesh.positions.len(), 3);
    }

    #[test]
    fn a_collinear_corner_is_part_of_the_outline_not_a_hole() {
        // Point 1 sits on the straight edge from point 0 to point 2.
        let outline = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        let mesh = surface_mesh(&body(&outline, 0.0), 0.75).expect("it meshes");
        assert_eq!(area(&mesh), 4.0);
    }

    #[test]
    fn a_degenerate_spacing_is_refused() {
        let square = body(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], 0.0);
        for spacing in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(
                    surface_mesh(&square, spacing),
                    Err(BodyError::InvalidSpacing { .. })
                ),
                "a spacing of {spacing} was accepted"
            );
        }
    }

    #[test]
    fn a_body_too_large_for_its_spacing_is_refused() {
        let lake = body(
            &[
                [0.0, 0.0],
                [10_000.0, 0.0],
                [10_000.0, 10_000.0],
                [0.0, 10_000.0],
            ],
            0.0,
        );
        assert!(matches!(
            surface_mesh(&lake, 1.0),
            Err(BodyError::TooManyCells { .. })
        ));
        // And the same lake at a spacing that fits is fine.
        assert!(surface_mesh(&lake, 100.0).is_ok());
    }

    #[test]
    fn a_malformed_body_is_an_error_not_a_panic() {
        let bow_tie = body(&[[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 1.0]], 0.0);
        assert!(matches!(
            surface_mesh(&bow_tie, 0.5),
            Err(BodyError::SelfIntersecting { .. })
        ));
        let empty = body(&[], 0.0);
        assert_eq!(
            surface_mesh(&empty, 0.5),
            Err(BodyError::TooFewPoints { count: 0 })
        );
    }

    /// The star CI's coverage job shrank `a_star_meshes_to_its_own_area` to on
    /// 2026-09-17, held to that property's own assertions. Its third corner is
    /// all but collinear, so the ear clipper cuts a sliver whose clipped pieces
    /// have corners one `f32` step apart.
    #[test]
    fn a_star_with_a_sliver_ear_meshes_to_its_own_area_facing_up() {
        let outline = [
            [-3.1330624, -1.6762365],
            [-1.8307314, -3.3561707],
            [-0.49024773, -1.7773137],
            [0.98649997, -0.03769052],
        ];
        let spacing = 1.3730929;
        let mesh = surface_mesh(&body(&outline, 0.0), spacing).expect("a valid star meshes");
        let want = signed_area(&outline).abs() / 2.0;
        let got = area(&mesh);
        assert!(
            (got - want).abs() <= area_tolerance(&outline, spacing),
            "the mesh covers {got} of an outline of {want}"
        );
        assert!(normals_y(&mesh).iter().all(|y| *y > 0.0));
        assert!(
            mesh.indices
                .iter()
                .all(|index| (*index as usize) < mesh.positions.len())
        );
    }

    /// A star-shaped outline: a point at each angle, at its own radius. Sorted
    /// distinct angles around one centre cannot cross, so every one of these
    /// is simple — convex or not.
    fn star() -> impl Strategy<Value = Vec<[f32; 2]>> {
        prop::collection::vec((0.0f32..1.0, 0.5f32..4.0), 3..12).prop_map(|mut spokes| {
            spokes.sort_by(|a, b| a.0.total_cmp(&b.0));
            spokes.dedup_by(|a, b| (a.0 - b.0).abs() < 0.02);
            spokes
                .into_iter()
                .map(|(turn, radius)| {
                    let angle = f64::from(turn) * std::f64::consts::TAU;
                    [
                        (angle.cos() * f64::from(radius)) as f32,
                        (angle.sin() * f64::from(radius)) as f32,
                    ]
                })
                .collect()
        })
    }

    proptest! {
        /// Every star meshes to exactly its own area, facing up, with every
        /// index in range — at any spacing.
        #[test]
        fn a_star_meshes_to_its_own_area(outline in star(), spacing in 0.2f32..3.0) {
            let body = body(&outline, 0.0);
            prop_assume!(body.validate().is_ok());
            let mesh = surface_mesh(&body, spacing).expect("a valid star meshes");
            let want = signed_area(&outline).abs() / 2.0;
            let got = area(&mesh);
            prop_assert!(
                (got - want).abs() <= area_tolerance(&outline, spacing),
                "the mesh covers {got} of an outline of {want}"
            );
            prop_assert!(normals_y(&mesh).iter().all(|y| *y > 0.0));
            prop_assert!(mesh.indices.iter().all(|index| (*index as usize) < mesh.positions.len()));
        }
    }
}
