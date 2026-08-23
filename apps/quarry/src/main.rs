//! Quarry — the native front end.
//!
//! ```text
//! quarry [--camera fixed|free] [--force-geometry P] [--force-binding B]
//!        [--lod-budget PX] [--lod-view] [--headless] [--report]
//! ```
//!
//! Argv in, exit code out: the fixture itself is the `crcbl_quarry` library this
//! binary links.
//!
//! # `--report` opens no device
//!
//! One thing here is not a run. This binary used to be a *measuring tool* with
//! no window at all, printing the face's counts, the meshlet total and the
//! per-level DAG breakdown — and those numbers are what
//! `docs/plan/sample/14-quarry.md`'s "triangle count per path" leans on, they
//! need no adapter, and they have to stay runnable in every job this repository
//! has on a machine without one. So they are still here, behind `--report`, and
//! answered before the front end opens anything. The counts that *do* need a
//! device are measured and asserted rather than printed: `tests/device/` draws
//! the face on every `GeometryPath` and reports the drawn level and triangle
//! count per path. Run it with `CRCBL_GPU=vk apps/quarry/tests/run-quarry-e2e.sh`.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_quarry::{Invocation, USAGE, cull_row, dag, face, parse, run, scene};

/// Quads per side of the **reported** face.
///
/// 256 is 131,072 triangles — dense enough that a cluster hierarchy has
/// something to collapse, and this path renders nothing, so the eight seconds
/// it takes to coarsen buys a count rather than a wait in front of a window.
/// The face the window makes resident is smaller and says why:
/// [`crcbl_quarry::CELLS`].
const REPORT_CELLS: u32 = 256;

fn main() -> ExitCode {
    let invocation = parse(std::env::args().skip(1));
    // Answered before `run_front_end`, which would open a shell and an adapter:
    // the whole point of this flag is that it needs neither.
    if let Invocation::Run(options) = &invocation
        && options.report
    {
        return report();
    }

    crcbl::args::run_front_end("quarry", USAGE, invocation, run, |summary| {
        format!(
            "quarry: {} frames, {} ticks on the {} shell at {}x{}, {} \
             (camera {}, {:?} / {:?} / {:?}, effects {}, {} triangles at a {}px budget, {}, \
             {:?})",
            summary.frames,
            summary.ticks,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            // What the window system actually did, not what `--fullscreen`
            // asked for. It is free to refuse.
            summary.mode,
            summary.camera.label(),
            // Rule 12's headless half: the three selectors this run's frames
            // were actually drawn through.
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            // And which of topic 18's effects were in those frames, resolved —
            // the observable for this sample's own `[engine.video]` wiring.
            summary.paths.effects.row(),
            // And the charter's "triangle count ... at a stated camera
            // position, recorded — including how much of the reduction is
            // instance culling and how much is cluster culling".
            summary.triangles,
            summary.lod_budget,
            cull_row(summary.cull),
            summary.exit,
        )
    })
}

/// Prints what the face is and what the builders make of it, and exits.
///
/// Every number here is computed on the CPU from [`REPORT_CELLS`], so this runs
/// on a machine with no driver at all.
fn report() -> ExitCode {
    let face = face::quarry_face(REPORT_CELLS);
    println!(
        "quarry: {cells}x{cells} cells — {vertices} vertices, {triangles} triangles",
        cells = REPORT_CELLS,
        vertices = face.positions.len(),
        triangles = face.triangles(),
    );

    let scene = match scene::quarry_scene(&face) {
        Ok(scene) => scene,
        Err(error) => {
            eprintln!("quarry: the face could not be partitioned into meshlets: {error}");
            return ExitCode::FAILURE;
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
            return ExitCode::FAILURE;
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
    ExitCode::SUCCESS
}
