//! A modular wall tile, and the border locking that lets two of them meet.
//!
//! `docs/plan/sample/14-quarry.md`'s scope names "a tiling modular wall piece
//! for border locking", and its exit criteria ask that the QEM generator survive
//! it: **"border locking on a tiling mesh"**. This is that piece.
//!
//! # The property, and the thing that turned out not to be needed
//!
//! Decimation is free to move or remove any vertex it likes, which is correct
//! for a mesh drawn alone and wrong for one drawn beside a copy of itself: the
//! moment two tiles' shared edge is simplified independently, their vertices
//! stop agreeing and a crack opens between them that no error budget closes.
//! **That two tiles still meet is what this module asserts**, and it is the
//! sample's "border locking on a tiling mesh" exit criterion.
//!
//! **It needs no explicit locking, and finding that out is the point.** This was
//! first written calling `crcbl_scene::simplify_with_locked_edges` with every
//! border edge named — and the red-check, passing `&[]` instead, **passed**. The
//! reason is in `crcbl_scene::simplify`'s own module docs: "an edge used by any
//! number of faces other than two is a border… an open mesh keeps its boundary
//! loop exactly". A tile's outer border *is* a mesh border, so the decimator
//! locks it whether or not a caller asks.
//!
//! `simplify_with_locked_edges` is for boundaries that are **interior** to the
//! mesh, which no rule over the two arrays can find — a cluster group's outer
//! edge, which is what `crcbl_scene::cluster_dag` passes it. Naming a tile's
//! border to it is ceremony, so this does not, and the tests below prove the
//! property directly instead.
//!
//! # Why the tiles agree in the first place
//!
//! [`quarry_tile`] samples the same height field [`crate::face`] does, at world
//! coordinates rather than tile-local ones. Tile `index` covers the span
//! `index..index + 1`, so tile 0's last column and tile 1's first column are the
//! same coordinates and therefore the same heights — by construction, with no
//! stitching pass and nothing to keep in sync.

use crcbl::scene::{Simplified, SimplifyError, simplify};

use crate::face::height_at;

/// Quads per side of one tile.
///
/// Smaller than a face: a tile is a wall piece repeated along a run, and the
/// property under test is its border rather than its density.
pub const TILE_CELLS: u32 = 32;

/// How wide and deep one tile is, in metres. Square, because a modular piece
/// that is not square tiles in one direction only.
pub const TILE_METRES: f32 = 8.0;

/// How tall the tile's relief is, in metres.
pub const TILE_HEIGHT_METRES: f32 = 3.0;

/// One tile: its vertices and its triangles.
#[derive(Clone, Debug)]
pub struct Tile {
    /// Vertices, row-major, `(TILE_CELLS + 1)` per row.
    pub positions: Vec<[f32; 3]>,
    /// Triangle indices into [`positions`](Self::positions).
    pub indices: Vec<u32>,
}

impl Tile {
    /// The positions on the tile's `+X` side, ordered along the side.
    #[must_use]
    pub fn far_edge(&self) -> Vec<[f32; 3]> {
        let side = TILE_CELLS + 1;
        (0..side)
            .map(|row| self.positions[(row * side + TILE_CELLS) as usize])
            .collect()
    }

    /// The positions on the tile's `-X` side, ordered along the side.
    #[must_use]
    pub fn near_edge(&self) -> Vec<[f32; 3]> {
        let side = TILE_CELLS + 1;
        (0..side)
            .map(|row| self.positions[(row * side) as usize])
            .collect()
    }

    /// How many triangles this tile holds.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// The wall tile at `index` along the run.
///
/// Tiles are placed along `X`, so tile `index` spans
/// `index * TILE_METRES ..= (index + 1) * TILE_METRES` and shares its `-X` side
/// with tile `index - 1`'s `+X` side — the same coordinates, so the same
/// heights.
#[must_use]
pub fn quarry_tile(index: u32) -> Tile {
    let side = TILE_CELLS + 1;
    let mut positions = Vec::with_capacity((side * side) as usize);
    for row in 0..side {
        for column in 0..side {
            // **World coordinates, not tile-local ones.** This is the whole
            // reason two tiles agree at their shared edge without a stitching
            // pass: the coordinate a vertex samples the height field at depends
            // on where it is in the world, so the same place gives the same
            // answer whichever tile asks.
            let across = (index as f32 + column as f32 / TILE_CELLS as f32) / TILING_SPAN;
            let along = row as f32 / TILE_CELLS as f32 / TILING_SPAN;
            positions.push([
                (index as f32 + column as f32 / TILE_CELLS as f32) * TILE_METRES,
                height_at(across, along) * TILE_HEIGHT_METRES,
                row as f32 / TILE_CELLS as f32 * TILE_METRES,
            ]);
        }
    }

    let mut indices = Vec::with_capacity((TILE_CELLS * TILE_CELLS * 6) as usize);
    for row in 0..TILE_CELLS {
        for column in 0..TILE_CELLS {
            let top_left = row * side + column;
            let (top_right, bottom_left) = (top_left + 1, top_left + side);
            let bottom_right = bottom_left + 1;
            // Counter-clockwise seen from +Y, as `crate::face` winds.
            indices.extend_from_slice(&[top_left, bottom_left, top_right]);
            indices.extend_from_slice(&[top_right, bottom_left, bottom_right]);
        }
    }

    Tile { positions, indices }
}

/// How many tiles the height field is spread over.
///
/// The field takes coordinates in `0..1`, so a run of tiles has to divide that
/// span between them or every tile would sample the whole field and none would
/// look like part of a wall.
const TILING_SPAN: f32 = 8.0;

/// The tile simplified to `target_triangles`.
///
/// No locked edges are passed, and the module header says why: the tile's border
/// is a mesh border, which `crcbl_scene::simplify` already holds exactly.
///
/// # Errors
///
/// [`SimplifyError`], from `crcbl_scene::simplify`.
pub fn simplify_tile(tile: &Tile, target_triangles: usize) -> Result<Simplified, SimplifyError> {
    simplify(&tile.positions, &tile.indices, target_triangles)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target the interior can reach with the border held. The border is 128
    /// vertices of a 1089-vertex tile, so a quarter of the triangles is well
    /// inside what an untouchable boundary leaves reachable.
    const TARGET: usize = (TILE_CELLS * TILE_CELLS * 6 / 3 / 4) as usize;

    /// Positions of `edge`, rounded so two floats that came out of the same
    /// arithmetic compare equal and two that did not do not.
    fn keys(edge: &[[f32; 3]]) -> Vec<[u32; 3]> {
        edge.iter()
            .map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
            .collect()
    }

    /// **Two tiles meet before anything is simplified**, along a seam that is not
    /// flat.
    ///
    /// The premise, and the second half is what stops the first being vacuous: a
    /// flat seam matches between *any* two tiles, including two that sampled the
    /// height field wrongly, so the comparison would prove nothing. Measured, it
    /// spans about 0.64 m of [`TILE_HEIGHT_METRES`].
    #[test]
    fn adjacent_tiles_share_their_seam_exactly() {
        let (left, right) = (quarry_tile(0), quarry_tile(1));
        let seam: Vec<f32> = left.far_edge().iter().map(|p| p[1]).collect();
        let low = seam.iter().copied().fold(f32::INFINITY, f32::min);
        let high = seam.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            high - low > TILE_HEIGHT_METRES * 0.1,
            "the shared seam spans {:.3} m of {TILE_HEIGHT_METRES}, flat enough that any two \
             tiles would match along it and this comparison would prove nothing",
            high - low,
        );
        assert_eq!(
            keys(&left.far_edge()),
            keys(&right.near_edge()),
            "tile 0's +X side and tile 1's -X side are the same places and must be the same \
             vertices, bit for bit"
        );
    }

    /// **The seam survives simplification.** The property the sample asks for:
    /// two tiles decimated independently still meet, so a wall built from them
    /// has no crack in it.
    #[test]
    fn the_seam_survives_independent_simplification() {
        let (left, right) = (quarry_tile(0), quarry_tile(1));
        let (thin_left, thin_right) = (
            simplify_tile(&left, TARGET).expect("the tile simplifies"),
            simplify_tile(&right, TARGET).expect("the tile simplifies"),
        );

        let seam = |simplified: &Simplified, edge: &[[f32; 3]]| -> Vec<[u32; 3]> {
            let surviving: std::collections::HashSet<[u32; 3]> =
                keys(simplified.positions()).into_iter().collect();
            keys(edge)
                .into_iter()
                .filter(|key| surviving.contains(key))
                .collect()
        };
        let left_seam = seam(&thin_left, &left.far_edge());
        let right_seam = seam(&thin_right, &right.near_edge());

        assert_eq!(
            left_seam.len(),
            left.far_edge().len(),
            "the left tile lost {} of its {} seam vertices to decimation",
            left.far_edge().len() - left_seam.len(),
            left.far_edge().len(),
        );
        assert_eq!(
            left_seam, right_seam,
            "the two tiles' shared edge no longer matches after simplifying them apart"
        );
    }

    /// **And the seam test can fail**, which nothing above shows: the decimator
    /// holds any mesh border, so removing the locking does not break it and a
    /// red-check has to come from the *generator* instead. A tile that sampled
    /// the height field in tile-local coordinates would look right alone and
    /// meet nothing.
    #[test]
    fn a_tile_that_ignored_its_neighbour_would_not_meet_it() {
        let left = quarry_tile(0);
        let tile_local = {
            // **Tile 0's geometry, placed where tile 1 goes** — which is what
            // sampling the height field in tile-local coordinates produces: every
            // tile identical, and so every seam a step between one tile's far
            // heights and the next's near ones.
            let mut wrong = quarry_tile(0);
            for position in &mut wrong.positions {
                position[0] += TILE_METRES;
            }
            wrong
        };
        assert_ne!(
            keys(&left.far_edge()),
            keys(&tile_local.near_edge()),
            "a tile generated without its world offset happened to match its neighbour, so the \
             seam comparison cannot tell the two cases apart"
        );
    }

    /// **And the interior really was decimated**, so the test above is not
    /// passing because nothing happened.
    #[test]
    fn the_interior_is_decimated_while_the_border_is_held() {
        let tile = quarry_tile(0);
        let thin = simplify_tile(&tile, TARGET).expect("the tile simplifies");
        assert!(
            thin.indices().len() / 3 < tile.triangles(),
            "the tile came back with {} of its {} triangles, so locking the border stopped every \
             collapse and the seam assertions are vacuous",
            thin.indices().len() / 3,
            tile.triangles(),
        );
        assert!(
            thin.positions().len() < tile.positions.len(),
            "no vertex was removed at all"
        );
    }
}
