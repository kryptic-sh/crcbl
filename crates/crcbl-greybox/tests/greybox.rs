//! The pack's guarantees, checked through the public API: every primitive is
//! well-formed geometry with valid clusters, it is the size in metres it claims
//! to be, its winding faces out, and [`scene3d`] assembles into a description
//! [`ForwardRenderer::with_scene`] would accept.

use crcbl_greybox::{
    GREYBOX_ALBEDO, GREYBOX_CAPSULE, GREYBOX_COLUMN, GREYBOX_CUBE, GREYBOX_CYLINDER,
    GREYBOX_DOORWAY, GREYBOX_GREY, GREYBOX_GRID, GREYBOX_MESH_COUNT, GREYBOX_PLATFORM,
    GREYBOX_QUAD, GREYBOX_RAMP, GREYBOX_SPHERE, GREYBOX_STAIRS, GREYBOX_TILE_EXTENT,
    GREYBOX_TILE_M, GREYBOX_WALL, GRID_EXTENT, GRID_LAYER, GreyboxColor, HeightfieldTile, capsule,
    column, cube, cylinder, doorway, greybox_color_material, greybox_color_texels,
    greybox_material, greybox_page, grid_material, heightfield_tiles, platform, quad, ramp,
    scene3d, sphere, stairs, unit_cube, wall,
};
use crcbl_render::scene::{Geometry, PageDesc, PageKind};
use crcbl_shaders::mesh::{GpuMaterial, MeshVertex, VERTEX_STRIDE};
use crcbl_shaders::meshlet::{MAX_CLUSTER_TRIANGLES, MAX_CLUSTER_VERTICES};
use crcbl_shaders::vertex::UvRange;
use glam::Vec3;

/// The position of every vertex, read out of a flat geometry's bytes.
fn positions(geometry: &Geometry<'_>) -> Vec<Vec3> {
    records(geometry)
        .into_iter()
        .map(|vertex| Vec3::from_array(vertex.position))
        .collect()
}

/// The normal every vertex's tangent frame decodes to.
///
/// Decoded rather than read: the frame is a quantised quaternion now, so what
/// a shader receives is the rotation of `(0, 0, 1)` by it and not a stored
/// vector — and this is the number the checks below are about.
fn normals(geometry: &Geometry<'_>) -> Vec<Vec3> {
    records(geometry)
        .into_iter()
        .map(|vertex| Vec3::from_array(vertex.qtangent.decode().normal))
        .collect()
}

/// Every vertex of a flat geometry, decoded out of the bytes the pool takes.
fn records(geometry: &Geometry<'_>) -> Vec<MeshVertex> {
    let Geometry::Flat { vertices, .. } = geometry else {
        panic!("a greybox primitive is always Geometry::Flat");
    };
    vertices
        .as_chunks::<VERTEX_STRIDE>()
        .0
        .iter()
        .map(MeshVertex::from_bytes)
        .collect()
}

/// The triangle index buffer.
fn indices(geometry: &Geometry<'_>) -> Vec<u32> {
    let Geometry::Flat { indices, .. } = geometry else {
        panic!("a greybox primitive is always Geometry::Flat");
    };
    indices.to_vec()
}

/// The axis-aligned bounds of a geometry's positions, as `(min, max)`.
fn bounds(geometry: &Geometry<'_>) -> (Vec3, Vec3) {
    let positions = positions(geometry);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for position in positions {
        min = min.min(position);
        max = max.max(position);
    }
    (min, max)
}

/// Every named primitive at its default size, for the checks that hold for all.
fn every_primitive() -> Vec<(&'static str, Geometry<'static>)> {
    vec![
        ("cube", cube(1.0)),
        ("quad", quad(1.0, 1.0)),
        ("wall", wall(1.0, 2.0, 0.1)),
        ("doorway", doorway(1.6, 2.5, 0.1, 1.0, 2.1)),
        ("ramp", ramp(1.0, 30.0)),
        ("stairs", stairs(1.0, 8, 0.2, 0.28)),
        ("column", column(0.3, 3.0)),
        ("cylinder", cylinder(0.5, 1.0, 24)),
        ("sphere", sphere(0.5, 16, 24)),
        ("capsule", capsule(0.25, 1.8, 8, 16)),
        ("platform", platform(1.0, 1.0, 0.2)),
        (
            "heightfield tile",
            hills_tiles(TILE_CELLS).swap_remove(0).geometry,
        ),
    ]
}

/// The point counts a side of [`hills`], and the cells a side of the tiles the
/// checks cut it into: neither count a multiple of the tile, so the last tile
/// of each row and column is narrower, and one tile is several clusters.
const HILLS_COLUMNS: usize = 41;
const HILLS_ROWS: usize = 27;
const TILE_CELLS: usize = 16;
const HILLS_CELL_M: f32 = 2.5;

/// Rolling ground, steep enough that a wrong normal or a flipped triangle shows.
fn hills() -> Vec<f32> {
    (0..HILLS_ROWS)
        .flat_map(|row| {
            (0..HILLS_COLUMNS).map(move |column| {
                let (x, z) = (column as f32 * 0.37, row as f32 * 0.23);
                3.0 * x.sin() * z.cos() + 0.5 * (x + z)
            })
        })
        .collect()
}

/// [`hills`] cut into tiles of `tile_cells` cells a side.
fn hills_tiles(tile_cells: usize) -> Vec<HeightfieldTile> {
    heightfield_tiles(
        &hills(),
        None,
        HILLS_COLUMNS,
        HILLS_ROWS,
        HILLS_CELL_M,
        tile_cells,
    )
}

#[test]
fn every_primitive_is_well_formed_geometry() {
    for (name, geometry) in every_primitive() {
        let Geometry::Flat {
            vertices, indices, ..
        } = &geometry
        else {
            panic!("{name} is not Geometry::Flat");
        };
        assert!(!vertices.is_empty(), "{name} has no vertices");
        assert_eq!(
            vertices.len() % VERTEX_STRIDE,
            0,
            "{name} vertex bytes are not a whole number of {VERTEX_STRIDE}-byte vertices"
        );
        assert!(!indices.is_empty(), "{name} has no indices");
        assert_eq!(
            indices.len() % 3,
            0,
            "{name} indices are not whole triangles"
        );
    }
}

#[test]
fn every_primitive_has_clusters_that_decode_to_its_index_buffer() {
    for (name, geometry) in every_primitive() {
        let Geometry::Flat {
            indices, clusters, ..
        } = &geometry
        else {
            panic!("{name} is not Geometry::Flat");
        };
        assert!(!clusters.clusters.is_empty(), "{name} has no clusters");

        let mut decoded = Vec::with_capacity(indices.len());
        for cluster in &clusters.clusters {
            assert!(
                cluster.vertex_count as usize <= MAX_CLUSTER_VERTICES,
                "{name} cluster references {} vertices, over the {MAX_CLUSTER_VERTICES} cap",
                cluster.vertex_count
            );
            assert!(
                cluster.triangle_count as usize <= MAX_CLUSTER_TRIANGLES,
                "{name} cluster holds {} triangles, over the {MAX_CLUSTER_TRIANGLES} cap",
                cluster.triangle_count
            );
            assert!(
                cluster.bounds.radius.is_finite() && cluster.bounds.radius >= 0.0,
                "{name} cluster radius is not a valid distance"
            );

            let run = &clusters.vertices[cluster.vertex_offset as usize..]
                [..cluster.vertex_count as usize];
            let corners = &clusters.corners[cluster.triangle_offset as usize..]
                [..cluster.triangle_count as usize * 3];
            for &corner in corners {
                decoded.push(run[usize::from(corner)]);
            }
        }
        assert_eq!(
            decoded,
            indices.to_vec(),
            "{name} clusters describe triangles the mesh does not have"
        );
    }
}

#[test]
fn every_normal_is_unit_length() {
    for (name, geometry) in every_primitive() {
        for (i, normal) in normals(&geometry).into_iter().enumerate() {
            assert!(
                (normal.length() - 1.0).abs() < 1e-4,
                "{name} vertex {i} has a non-unit normal of length {}",
                normal.length()
            );
        }
    }
}

#[test]
fn every_triangle_is_wound_to_face_its_normals() {
    // A triangle's geometric normal — from its winding — must point the same way
    // as its vertices' stored normals, or the pass's back-face cull drops a face
    // that should draw. Degenerate triangles (a sphere's poles) name no
    // direction and are skipped.
    for (name, geometry) in every_primitive() {
        let positions = positions(&geometry);
        let normals = normals(&geometry);
        for triangle in indices(&geometry).as_chunks::<3>().0 {
            let [a, b, c] = [
                triangle[0] as usize,
                triangle[1] as usize,
                triangle[2] as usize,
            ];
            let geometric = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
            if geometric.length() < 1e-9 {
                continue;
            }
            let stored = normals[a] + normals[b] + normals[c];
            assert!(
                geometric.dot(stored) > 0.0,
                "{name} has a triangle wound against its normals"
            );
        }
    }
}

// --- The sizing guarantee: every default primitive is the metres it claims. ---

/// Asserts a geometry's per-axis extents (`max - min`) are `[x, y, z]` metres.
fn assert_extents(geometry: &Geometry<'_>, x: f32, y: f32, z: f32, name: &str) {
    let (min, max) = bounds(geometry);
    let extent = max - min;
    let want = Vec3::new(x, y, z);
    assert!(
        (extent - want).abs().max_element() < 1e-4,
        "{name} spans {extent:?} m, not the {want:?} m it should"
    );
}

#[test]
fn the_default_cube_is_a_metre_centred_on_the_origin() {
    let (min, max) = bounds(&cube(1.0));
    assert!(
        (min - Vec3::splat(-0.5)).abs().max_element() < 1e-4,
        "the cube's minimum corner is {min:?}, not -0.5 m on every axis"
    );
    assert!(
        (max - Vec3::splat(0.5)).abs().max_element() < 1e-4,
        "the cube's maximum corner is {max:?}, not +0.5 m on every axis"
    );
    // A size argument is a size in metres: doubling it doubles the span.
    assert_extents(&cube(2.0), 2.0, 2.0, 2.0, "cube(2.0)");
    // The unit cube is the one-metre cube.
    assert_extents(&unit_cube(), 1.0, 1.0, 1.0, "unit_cube");
}

#[test]
fn the_default_primitives_are_their_documented_metres() {
    assert_extents(&quad(1.0, 1.0), 1.0, 0.0, 1.0, "quad");
    assert_extents(&wall(1.0, 2.0, 0.1), 1.0, 2.0, 0.1, "wall");
    assert_extents(&doorway(1.6, 2.5, 0.1, 1.0, 2.1), 1.6, 2.5, 0.1, "doorway");
    assert_extents(&column(0.3, 3.0), 0.3, 3.0, 0.3, "column");
    assert_extents(&platform(1.0, 1.0, 0.2), 1.0, 0.2, 1.0, "platform");
    assert_extents(&stairs(1.0, 8, 0.2, 0.28), 1.0, 1.6, 2.24, "stairs");
    assert_extents(&cylinder(0.5, 1.0, 24), 1.0, 1.0, 1.0, "cylinder");
    assert_extents(&sphere(0.5, 16, 24), 1.0, 1.0, 1.0, "sphere");
}

#[test]
fn the_capsule_is_a_human_scale_figure() {
    // 0.5 m diameter across, 1.8 m tall from the floor — the scale reference.
    assert_extents(&capsule(0.25, 1.8, 8, 16), 0.5, 1.8, 0.5, "capsule");
    let (min, max) = bounds(&capsule(0.25, 1.8, 8, 16));
    assert!(
        min.y.abs() < 1e-4,
        "the capsule's feet are at {} m, not 0",
        min.y
    );
    assert!(
        (max.y - 1.8).abs() < 1e-4,
        "the capsule's crown is at {} m, not 1.8",
        max.y
    );
}

#[test]
fn a_forty_five_degree_ramp_rises_as_far_as_it_runs() {
    let (min, max) = bounds(&ramp(1.0, 45.0));
    let extent = max - min;
    assert!(
        (extent.y - extent.z).abs() < 1e-4,
        "a 45° ramp rises {} m over a {} m run",
        extent.y,
        extent.z
    );
    assert!(
        (extent.z - 1.0).abs() < 1e-4,
        "the run is {} m, not 1",
        extent.z
    );
}

// --- scene3d(): a description with_scene would accept. ---

#[test]
fn scene3d_makes_every_primitive_resident_with_two_materials() {
    let scene = scene3d();
    assert_eq!(
        scene.meshes.len(),
        GREYBOX_MESH_COUNT,
        "scene3d resident mesh count and GREYBOX_MESH_COUNT disagree"
    );
    assert_eq!(scene.materials.len(), 2, "the grey row and the grid row");
    assert_eq!(scene.materials[GREYBOX_GREY], greybox_material());
    assert_eq!(scene.materials[GREYBOX_GRID], grid_material());
    assert_ne!(
        scene.materials[GREYBOX_GREY], scene.materials[GREYBOX_GRID],
        "the two rows must be distinguishable"
    );
}

#[test]
fn the_scene_constants_name_their_own_meshes() {
    let scene = scene3d();
    for (constant, label) in [
        (GREYBOX_CUBE, "greybox cube"),
        (GREYBOX_QUAD, "greybox quad"),
        (GREYBOX_WALL, "greybox wall"),
        (GREYBOX_DOORWAY, "greybox doorway"),
        (GREYBOX_RAMP, "greybox ramp"),
        (GREYBOX_STAIRS, "greybox stairs"),
        (GREYBOX_COLUMN, "greybox column"),
        (GREYBOX_CYLINDER, "greybox cylinder"),
        (GREYBOX_SPHERE, "greybox sphere"),
        (GREYBOX_CAPSULE, "greybox capsule"),
        (GREYBOX_PLATFORM, "greybox platform"),
    ] {
        assert!(
            constant < scene.meshes.len(),
            "{label} constant is out of range"
        );
        assert_eq!(
            scene.meshes[constant].label, label,
            "the constant for {label} names a different mesh"
        );
    }
}

#[test]
fn the_scene_page_is_the_grid_and_nothing_else() {
    let scene = scene3d();
    assert_eq!(scene.page.extent(PageKind::BaseColor), GRID_EXTENT);
    let texels = (GRID_EXTENT * GRID_EXTENT) as usize * 4;
    let layers = scene.page.layers(PageKind::BaseColor);
    assert_eq!(
        layers.len(),
        1,
        "the grid is the page: the neutral material names no page at all"
    );
    assert_eq!(
        layers[GRID_LAYER as usize].len(),
        texels,
        "the grid layer is a full {GRID_EXTENT}×{GRID_EXTENT} RGBA8 image"
    );
    assert!(
        layers[GRID_LAYER as usize]
            .iter()
            .any(|&texel| texel != PageDesc::WHITE),
        "the grid layer must draw something, or it is a flat white field"
    );
    assert_eq!(
        scene.page.extent(PageKind::Normal),
        0,
        "a greybox scene has no normal map, so that page is one placeholder texel"
    );
}

#[test]
fn scene3d_fits_inside_the_capacities_it_reserves() {
    // with_scene refuses a description whose geometry overflows its pools; this
    // is that same sizing check on the CPU, without a device.
    let scene = scene3d();
    let (mut vertices, mut index_count) = (0u32, 0u32);
    for mesh in &scene.meshes {
        let Geometry::Flat {
            vertices: bytes,
            indices,
            ..
        } = &mesh.geometry
        else {
            panic!("a greybox scene is flat meshes");
        };
        vertices += u32::try_from(bytes.len() / VERTEX_STRIDE).expect("a small mesh");
        index_count += u32::try_from(indices.len()).expect("a small mesh");
    }
    assert!(
        vertices <= scene.capacities.vertices,
        "the scene's {vertices} vertices overflow the reserved {}",
        scene.capacities.vertices
    );
    assert!(
        index_count <= scene.capacities.indices,
        "the scene's {index_count} indices overflow the reserved {}",
        scene.capacities.indices
    );
    assert!(scene.meshes.len() <= scene.capacities.meshes as usize);
    assert!(scene.materials.len() <= scene.capacities.materials as usize);
}

/// The seven colour tiles, their layers, page and materials stay in lockstep:
/// `ALL` order is layer order from zero, every material names its own
/// physical-tiling row, and the page holds one tile per colour at the declared
/// extent and nothing else.
#[test]
fn every_greybox_colour_tiles_by_physical_size_out_of_its_own_layer() {
    assert_eq!(
        GreyboxColor::ALL.len(),
        7,
        "grey, red, green, blue, orange, brown, black"
    );

    let page = greybox_page();
    assert_eq!(
        page.extent(PageKind::BaseColor),
        GREYBOX_TILE_EXTENT,
        "the colour page is authored at the declared extent"
    );
    assert_eq!(
        page.layers(PageKind::BaseColor).len(),
        GreyboxColor::ALL.len(),
        "one tile per colour, and nothing burned ahead of them"
    );

    let texel_count = (GREYBOX_TILE_EXTENT * GREYBOX_TILE_EXTENT) as usize * 4;
    for (index, color) in GreyboxColor::ALL.into_iter().enumerate() {
        let layer = color.layer();
        assert_eq!(
            layer as usize,
            index,
            "{}'s layer must be its own ALL position",
            color.label()
        );

        // The material a caller picks samples exactly that layer, physically
        // tiled at one metre — which is what makes the grid a metric ruler.
        let material = greybox_color_material(color);
        assert_eq!(
            material.base_color_texture,
            layer,
            "{} names its own layer",
            color.label()
        );
        assert_eq!(
            material.tiling,
            GpuMaterial::TILING_PHYSICAL,
            "{} must tile by physical size",
            color.label()
        );
        assert_eq!(
            material.tile_metres,
            GREYBOX_TILE_M,
            "{} tiles one metre",
            color.label()
        );

        // The layer bytes are a full RGBA8 image of the page's extent, and the
        // page holds exactly them — so an upload of `extent² × 4` bytes lands.
        let texels = greybox_color_texels(color);
        assert_eq!(
            texels.len(),
            texel_count,
            "{} is a full page-sized image",
            color.label()
        );
        assert_eq!(
            page.layers(PageKind::BaseColor)[layer as usize].as_ref(),
            texels.as_slice()
        );

        // A ruled tile is not a flat field: it has both line and field texels,
        // or there is no grid to read a size off.
        let distinct: std::collections::BTreeSet<[u8; 4]> = texels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect();
        assert!(
            distinct.len() >= 2,
            "{}'s tile is a flat field with no grid lines",
            color.label()
        );
    }

    // Distinct colours are distinct materials and distinct tiles — a blockout
    // that paints two volumes different colours must be able to tell them apart.
    assert_ne!(
        greybox_color_material(GreyboxColor::Red),
        greybox_color_material(GreyboxColor::Blue)
    );
    assert_ne!(
        greybox_color_texels(GreyboxColor::Red),
        greybox_color_texels(GreyboxColor::Blue)
    );
}

/// The geometric normal of every triangle of `tile`, from its winding.
fn face_normals(tile: &HeightfieldTile) -> Vec<Vec3> {
    tile.triangles
        .iter()
        .map(|triangle| {
            let [a, b, c] = triangle.map(|i| Vec3::from_array(tile.positions[i as usize]));
            (b - a).cross(c - a)
        })
        .collect()
}

#[test]
fn heightfield_tiles_cover_the_field_and_narrow_at_its_far_edges() {
    let heights = hills();
    let tiles = hills_tiles(TILE_CELLS);
    // 40 cells across in tiles of 16 is 16, 16 and 8; 26 down is 16 and 10.
    let widths = [17, 17, 9];
    let depths = [17, 11];
    assert_eq!(tiles.len(), widths.len() * depths.len());
    let mut seen = vec![false; heights.len()];
    for (t, tile) in tiles.iter().enumerate() {
        let (width, depth) = (widths[t % widths.len()], depths[t / widths.len()]);
        assert_eq!(tile.positions.len(), width * depth, "tile {t}'s points");
        assert_eq!(
            tile.triangles.len(),
            (width - 1) * (depth - 1) * 2,
            "tile {t}'s triangles"
        );
        for &[x, y, z] in &tile.positions {
            let (column, row) = ((x / HILLS_CELL_M) as usize, (z / HILLS_CELL_M) as usize);
            assert_eq!(column as f32 * HILLS_CELL_M, x, "a point off the grid");
            assert_eq!(row as f32 * HILLS_CELL_M, z, "a point off the grid");
            let index = row * HILLS_COLUMNS + column;
            assert_eq!(
                y, heights[index],
                "point ({column}, {row}) at the wrong height"
            );
            seen[index] = true;
        }
    }
    assert!(seen.iter().all(|&s| s), "a grid point no tile carries");
}

#[test]
fn a_heightfield_tile_hands_back_the_triangles_it_draws() {
    for (t, tile) in hills_tiles(TILE_CELLS).iter().enumerate() {
        let drawn: Vec<[f32; 3]> = positions(&tile.geometry)
            .into_iter()
            .map(|position| position.to_array())
            .collect();
        assert_eq!(drawn, tile.positions, "tile {t}'s positions");
        assert_eq!(
            indices(&tile.geometry),
            tile.triangles.concat(),
            "tile {t}'s triangles"
        );
    }
}

#[test]
fn heightfield_tiles_meet_with_the_same_points_and_normals() {
    let tiles = hills_tiles(TILE_CELLS);
    let mut first_seen: Vec<([f32; 3], Vec3, usize)> = Vec::new();
    let mut shared = 0;
    for (t, tile) in tiles.iter().enumerate() {
        for (position, normal) in tile.positions.iter().zip(normals(&tile.geometry)) {
            match first_seen.iter().find(|(seen, ..)| seen == position) {
                Some(&(_, seen_normal, seen_tile)) => {
                    assert_eq!(
                        normal, seen_normal,
                        "{position:?} lights differently in tiles {seen_tile} and {t}"
                    );
                    shared += 1;
                }
                None => first_seen.push((*position, normal, t)),
            }
        }
    }
    // Every edge between two tiles is repeated, corners in up to four tiles:
    // two vertical seams of 27 points and one horizontal seam of 41, the two
    // crossings counted once more each for the third and fourth tile there.
    assert_eq!(shared, 2 * 27 + 41 + 2);
}

#[test]
fn every_heightfield_triangle_faces_up() {
    for (t, tile) in hills_tiles(TILE_CELLS).iter().enumerate() {
        for (i, normal) in face_normals(tile).into_iter().enumerate() {
            assert!(normal.y > 0.0, "tile {t} triangle {i} faces {normal}");
        }
    }
}

#[test]
fn a_sloped_plane_has_its_slope_as_every_normal_edges_included() {
    let (columns, rows, cell_m) = (6, 5, 2.0);
    let (slope_x, slope_z) = (0.3, -0.2);
    let heights: Vec<f32> = (0..rows)
        .flat_map(|row| {
            (0..columns)
                .map(move |column| slope_x * column as f32 * cell_m + slope_z * row as f32 * cell_m)
        })
        .collect();
    let expected = Vec3::new(-slope_x, 1.0, -slope_z).normalize();
    for tile in heightfield_tiles(&heights, None, columns, rows, cell_m, 3) {
        for normal in normals(&tile.geometry) {
            assert!(
                normal.distance(expected) < 2e-3,
                "{normal} on a plane whose normal is {expected}"
            );
        }
    }
}

#[test]
fn a_heightfield_edge_takes_the_one_sided_difference() {
    // h = x²: a central difference is exact inside (2x), and the edge clamps to
    // the cell beside it, (h(1) − h(0)) / 1 = 1, not half that.
    let (columns, rows) = (5, 2);
    let heights: Vec<f32> = (0..rows)
        .flat_map(|_| (0..columns).map(|column| (column * column) as f32))
        .collect();
    let tile = heightfield_tiles(&heights, None, columns, rows, 1.0, columns).swap_remove(0);
    let normals = normals(&tile.geometry);
    for (column, slope) in [(0, 1.0), (2, 4.0), (4, 7.0)] {
        let expected = Vec3::new(-slope, 1.0, 0.0).normalize();
        assert!(
            normals[column].distance(expected) < 2e-3,
            "column {column}: {} where the slope {slope} gives {expected}",
            normals[column]
        );
    }
}

#[test]
fn a_tile_wider_than_the_field_is_the_whole_field() {
    let tiles = hills_tiles(HILLS_COLUMNS * 2);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].positions.len(), HILLS_COLUMNS * HILLS_ROWS);
}

#[test]
#[should_panic(expected = "one height per point")]
fn a_heightfield_refuses_heights_that_do_not_fill_it() {
    let _ = heightfield_tiles(&[0.0; 5], None, 3, 2, 1.0, 1);
}

#[test]
#[should_panic(expected = "one colour per point")]
fn a_heightfield_refuses_colours_that_do_not_fill_it() {
    let _ = heightfield_tiles(&[0.0; 6], Some(&[[1.0; 4]; 5]), 3, 2, 1.0, 1);
}

#[test]
fn a_heightfield_point_wears_its_own_colour_or_the_greybox_grey() {
    let heights = hills();
    let colors: Vec<[f32; 4]> = (0..heights.len())
        .map(|i| {
            // A colour no other point has, in the eight bits a channel keeps.
            [
                (i % 256) as f32 / 255.0,
                (i / 256) as f32 / 255.0,
                0.25,
                1.0,
            ]
        })
        .collect();
    let colored = heightfield_tiles(
        &heights,
        Some(&colors),
        HILLS_COLUMNS,
        HILLS_ROWS,
        HILLS_CELL_M,
        TILE_CELLS,
    );
    for (t, (tile, plain)) in colored.iter().zip(hills_tiles(TILE_CELLS)).enumerate() {
        for ((vertex, plain), &[x, _, z]) in records(&tile.geometry)
            .into_iter()
            .zip(records(&plain.geometry))
            .zip(&tile.positions)
        {
            let index = (z / HILLS_CELL_M) as usize * HILLS_COLUMNS + (x / HILLS_CELL_M) as usize;
            let expected = MeshVertex::from_normal(
                vertex.position,
                [0.0, 1.0, 0.0],
                colors[index],
                [0.0, 0.0],
                &UvRange::from_uvs(&[[0.0, 0.0]]),
            );
            assert_eq!(vertex.color, expected.color, "tile {t} at ({x}, {z})");
            let grey = MeshVertex::from_normal(
                plain.position,
                [0.0, 1.0, 0.0],
                GREYBOX_ALBEDO,
                [0.0, 0.0],
                &UvRange::from_uvs(&[[0.0, 0.0]]),
            );
            assert_eq!(plain.color, grey.color, "tile {t} at ({x}, {z}) uncoloured");
        }
    }
}
