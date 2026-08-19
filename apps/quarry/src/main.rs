//! Reports what the quarry face is, and what the builders make of it.
//!
//! **A measuring tool rather than a viewer**, and it opens no window and no
//! adapter on purpose: a sample binary that needed a GPU to tell you how many
//! triangles it generates would be unrunnable in every job this repository has
//! on a machine without one.
//!
//! `docs/plan/sample/14-quarry.md`'s exit criteria ask for "triangle count and
//! draw count per path, at a stated camera position, recorded". The counts that
//! need no device are here. **The ones that need one are measured and asserted
//! rather than printed** — `tests/device/` draws the face on every
//! `GeometryPath`, reads back what covered the frame, and reports the drawn
//! level and triangle count per path. Run it with
//! `CRCBL_GPU=vk apps/quarry/tests/run-quarry-e2e.sh`.

use crcbl_quarry::{dag, face, scene};

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

    // The hierarchy the sample is actually about. Reported per level rather than
    // as a total, because "how many levels and how fast do they shrink" is the
    // question a reader has about a DAG and a single number answers neither.
    let dag = match dag::quarry_dag(&face) {
        Ok(dag) => dag,
        Err(error) => {
            eprintln!("quarry: the face could not be coarsened into a cluster DAG: {error}");
            std::process::exit(1);
        }
    };
    let per_level: Vec<String> = dag
        .levels
        .iter()
        .map(|level| {
            format!(
                "{} cluster(s)/{} tri",
                level.clusters.clusters.len(),
                level.indices().len() / 3
            )
        })
        .collect();
    println!(
        "quarry: {} DAG level(s), finest first — {}",
        dag.levels.len(),
        per_level.join(", "),
    );
    let levelled = dag::dag_capacities(&dag);
    println!(
        "quarry: the levelled scene reserves {} vertices and {} indices across {} mesh table \
         entries",
        levelled.vertices, levelled.indices, levelled.meshes,
    );
}
