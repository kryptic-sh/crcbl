//! Terrain from a grid of heights, cut into tiles a renderer can cull.
//!
//! A heightfield is one height per grid point, laid out row-major: columns run
//! along `+X` and rows along `+Z`, [`cell_m`](heightfield_tiles) apart, with
//! the first point at the origin. A caller places the field with its instance
//! transform, and builds the collider from the same transform and the
//! [`HeightfieldTile::positions`] and [`HeightfieldTile::triangles`] each tile
//! hands back, so what is drawn and what is walked on are the same triangles.
//!
//! # Why tiles
//!
//! The renderer culls per instance (per cluster only on the mesh-shader path),
//! so a whole map as one mesh is never culled, for the camera or for any shadow
//! cascade. Tiles are what a frustum can reject.
//!
//! # Why tiles meet without a seam
//!
//! Each normal is taken from the **whole** grid, by central differences, before
//! any tile is cut, and the points on a tile's edge are repeated in the tile
//! beside it. So a shared edge carries the same positions and the same normals
//! on both sides, and the lighting does not crease where tiles meet. A normal
//! taken from one tile's points alone would clamp at the tile's edge instead.

use crcbl_render::scene::Geometry;
use glam::Vec3;

use crate::GREYBOX_ALBEDO;
use crate::build::MeshBuilder;

/// One tile of [`heightfield_tiles`]: its geometry, and the same triangles as
/// data for a collider.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightfieldTile {
    /// The tile, ready for a
    /// [`MeshDesc`](crcbl_render::scene::MeshDesc).
    pub geometry: Geometry<'static>,
    /// Every vertex position, in the order the geometry's vertices are in, in
    /// the field's own space.
    pub positions: Vec<[f32; 3]>,
    /// The tile's triangles as indices into [`positions`](Self::positions),
    /// counter-clockwise seen from `+Y` — the same triangles, in the same order,
    /// as the geometry's index buffer.
    pub triangles: Vec<[u32; 3]>,
}

/// Cuts the heightfield `heights` into tiles of at most `tile_cells` cells a
/// side.
///
/// `heights` holds `columns × rows` heights in metres, row-major, column `c`
/// of row `r` at index `r * columns + c` and at `(c · cell_m, h, r · cell_m)`.
/// Tiles come back row by row along `+Z`, each row along `+X`; the last tile of
/// a row or column is narrower when `tile_cells` does not divide the cells.
///
/// `colors`, when given, holds one linear RGBA albedo per point in the same
/// order as `heights`, such as a colour per kind of ground; the material's
/// texel is multiplied by it. Without it every point is
/// [`GREYBOX_ALBEDO`], as the other primitives are.
///
/// Vertex texture coordinates are the point's `X` and `Z` in metres, so
/// [`grid_material`](crate::grid_material)'s one-metre tile reads true to
/// scale on the ground.
///
/// # Panics
///
/// If `heights`, or `colors` when given, does not hold exactly
/// `columns × rows` entries, if either
/// count is below two (a field of no cells), if `tile_cells` is zero, or if
/// `cell_m` is not a positive finite length.
#[must_use]
pub fn heightfield_tiles(
    heights: &[f32],
    colors: Option<&[[f32; 4]]>,
    columns: usize,
    rows: usize,
    cell_m: f32,
    tile_cells: usize,
) -> Vec<HeightfieldTile> {
    assert!(
        columns >= 2 && rows >= 2,
        "a heightfield needs two points a side, got {columns} × {rows}"
    );
    assert_eq!(
        heights.len(),
        columns * rows,
        "a {columns} × {rows} heightfield holds one height per point"
    );
    if let Some(colors) = colors {
        assert_eq!(
            colors.len(),
            columns * rows,
            "a {columns} × {rows} heightfield holds one colour per point"
        );
    }
    assert!(tile_cells > 0, "a tile is at least one cell a side");
    assert!(
        cell_m.is_finite() && cell_m > 0.0,
        "a heightfield cell is a positive length, got {cell_m}"
    );

    let height = |column: usize, row: usize| heights[row * columns + column];
    let normal = |column: usize, row: usize| {
        let (left, right) = (column.saturating_sub(1), (column + 1).min(columns - 1));
        let (near, far) = (row.saturating_sub(1), (row + 1).min(rows - 1));
        let slope_x = (height(right, row) - height(left, row)) / ((right - left) as f32 * cell_m);
        let slope_z = (height(column, far) - height(column, near)) / ((far - near) as f32 * cell_m);
        Vec3::new(-slope_x, 1.0, -slope_z).normalize()
    };

    let mut tiles = Vec::new();
    for row0 in (0..rows - 1).step_by(tile_cells) {
        let row1 = (row0 + tile_cells).min(rows - 1);
        for column0 in (0..columns - 1).step_by(tile_cells) {
            let column1 = (column0 + tile_cells).min(columns - 1);
            let mut builder = MeshBuilder::new();
            let mut positions = Vec::new();
            let grid: Vec<Vec<u32>> = (row0..=row1)
                .map(|row| {
                    (column0..=column1)
                        .map(|column| {
                            let at = Vec3::new(
                                column as f32 * cell_m,
                                height(column, row),
                                row as f32 * cell_m,
                            );
                            positions.push(at.to_array());
                            let albedo = colors
                                .map_or(GREYBOX_ALBEDO, |colors| colors[row * columns + column]);
                            builder.colored_vertex(at, normal(column, row), [at.x, at.z], albedo)
                        })
                        .collect()
                })
                .collect();
            // Rows ascend in +Z and columns in +X, so `stitch_grid`'s winding is
            // counter-clockwise seen from +Y: the ground faces up.
            builder.stitch_grid(&grid);
            let triangles = builder.indices().as_chunks::<3>().0.to_vec();
            tiles.push(HeightfieldTile {
                geometry: builder.finish(),
                positions,
                triangles,
            });
        }
    }
    tiles
}
