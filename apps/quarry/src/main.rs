//! Reports what the quarry face is, and what the meshlet builder makes of it.
//!
//! **A measuring tool rather than a viewer, and that is the milestone talking.**
//! `docs/plan/sample/14-quarry.md`'s exit criteria ask for "triangle count and
//! draw count per path, at a stated camera position, recorded". The triangle
//! and cluster halves of that need no device, so they are answerable now and
//! this answers them; the draw counts arrive with the renderer.
//!
//! It opens no window and no adapter on purpose. A sample binary that needed a
//! GPU to tell you how many triangles it generates would be untestable in every
//! job this repository runs on a machine without one.

use crcbl_quarry::{face, scene};

/// Quads per side of the reported face.
///
/// 256 is 131,072 triangles — dense enough that a cluster hierarchy has
/// something to collapse, small enough that generating it is not a wait. The
/// renderer will want its own figure and can pass one.
const CELLS: u32 = 256;

fn main() {
    let face = face::quarry_face(CELLS);
    println!(
        "quarry: {cells}x{cells} cells — {vertices} vertices, {triangles} triangles",
        cells = CELLS,
        vertices = face.positions.len(),
        triangles = face.triangles(),
    );

    let scene = match scene::quarry_scene(&face) {
        Ok(scene) => scene,
        Err(error) => {
            eprintln!("quarry: the face could not be partitioned into meshlets: {error}");
            std::process::exit(1);
        }
    };

    let clusters = match &scene.meshes[0].geometry {
        crcbl::render::scene::Geometry::Flat { clusters, .. } => clusters.clusters.len(),
        crcbl::render::scene::Geometry::Dag { .. } => 0,
    };
    let reserved = scene.capacities;
    println!("quarry: {clusters} meshlet cluster(s)");
    println!(
        "quarry: pools reserve {} vertices and {} indices — the defaults are {} and {}",
        reserved.vertices,
        reserved.indices,
        crcbl::render::scene::Capacities::default().vertices,
        crcbl::render::scene::Capacities::default().indices,
    );
}
