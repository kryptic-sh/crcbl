//! Generate `clusters/dunes.dag` from the builder that owns it, or check that
//! the committed bytes are still what that builder produces.
//!
//!     cargo run -p crcbl-shaders --example cook-clusters            # regenerate
//!     cargo run -p crcbl-shaders --example cook-clusters -- --check # verify only
//!
//! # Why there is a generator at all
//!
//! `docs/plan/25-lod.md`'s "How a DAG reaches the renderer": `crcbl-render` must
//! not depend on `crcbl-scene`, because that pulls `gltf` into the renderer, and
//! `crcbl-shaders` has no dependencies at all by design. So a DAG out of
//! `build_cluster_dag` has no path to the renderer except as **cooked data**,
//! and cooked data with no producer beside it is data nobody can regenerate.
//! This is that producer, and `--check` is what stops the artifact and the
//! builder drifting apart — the same arrangement, for the same reason, as
//! `tools/compile-shaders.sh --check` and the SPIR-V it verifies.
//!
//! # Why an example and not a binary
//!
//! It needs `crcbl-scene`, which depends on `crcbl-shaders` — a normal
//! dependency the other way would be a cycle and cargo refuses one. A
//! **dev**-dependency cycle cargo allows, and a `[[bin]]` cannot see
//! dev-dependencies where an `[[example]]` can. So the arrangement is the one
//! that keeps `cargo build -p crcbl-shaders` pulling in nothing whatsoever,
//! which is the property this crate exists to have.

use std::path::PathBuf;
use std::process::ExitCode;

use crcbl_scene::cluster_dag::{ClusterDag as BuiltDag, build_cluster_dag};
use crcbl_shaders::cluster_dag::ClusterDag;
use crcbl_shaders::dunes;

/// Where the committed artifact lives, relative to this crate.
///
/// Resolved against `CARGO_MANIFEST_DIR` rather than the working directory, so
/// the generator writes the same file wherever it is run from — a `--check` that
/// silently compared against nothing because of a `cd` is the failure this
/// avoids.
const ARTIFACT: &str = "clusters/dunes.dag";

/// Eyes the two `projected_error` implementations are compared at.
///
/// Spread over the patch's own scale: at one edge, off a corner, high above, and
/// inside the geometry — the last so the "eye inside the sphere" branch is one
/// of the cases compared rather than one of the cases assumed.
fn probe_eyes() -> Vec<[f32; 3]> {
    let reach = dunes::DUNES_EXTENT;
    vec![
        [0.0, 4.0, -reach - 2.0],
        [-reach - 8.0, 12.0, -reach - 8.0],
        [0.0, 4.0 * reach, 0.0],
        [reach * 0.5, 2.0, reach * 0.25],
        [0.0, 0.0, 0.0],
    ]
}

/// How many pixels one unit of length subtends one unit from the eye, for the
/// comparison below.
///
/// A round number rather than a camera's: nothing here turns on the figure, it
/// only has to be the same one on both sides of the comparison.
const PIXELS_PER_UNIT: f32 = 166.0;

fn main() -> ExitCode {
    let check = match std::env::args().skip(1).collect::<Vec<String>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        arguments => {
            eprintln!("cook-clusters: unexpected arguments {arguments:?}");
            eprintln!("usage: cook-clusters [--check]");
            return ExitCode::from(2);
        }
    };

    let positions = dunes::positions();
    let indices = dunes::indices();
    let built = match build_cluster_dag(&positions, &indices) {
        Ok(built) => built,
        Err(error) => {
            eprintln!("cook-clusters: the dunes patch does not build a DAG: {error}");
            return ExitCode::FAILURE;
        }
    };

    // The artifact does not carry level 0's positions — it is the array the
    // builder was handed, and `dunes_dag` supplies it from `dunes::positions`.
    // That is only sound while the builder really does pass them through, so it
    // is checked here rather than relied on.
    assert_eq!(
        built.levels()[0].positions(),
        positions,
        "level 0 is no longer the array the builder was given, so the artifact \
         cannot leave its positions out"
    );

    let cooked = built.cook();
    assert_metrics_agree(&built, &cooked);

    let bytes = cooked.to_bytes();
    assert_eq!(
        ClusterDag::from_bytes(&bytes, positions.clone()).as_ref(),
        Ok(&cooked),
        "the codec is not its own inverse, so the artifact does not say what \
         the builder produced"
    );

    report(&cooked, bytes.len());

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(ARTIFACT);
    if check {
        let committed = match std::fs::read(&path) {
            Ok(committed) => committed,
            Err(error) => {
                eprintln!("cook-clusters: cannot read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        };
        if committed == bytes {
            println!("cook-clusters: {ARTIFACT} matches the builder");
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "cook-clusters: {ARTIFACT} is {} bytes and the builder produces {} — {}",
            committed.len(),
            bytes.len(),
            difference(&committed, &bytes)
        );
        eprintln!(
            "  the committed DAG is not what `build_cluster_dag` produces from \
             the dunes patch."
        );
        eprintln!("  Regenerate with: cargo run -p crcbl-shaders --example cook-clusters");
        return ExitCode::FAILURE;
    }

    if let Some(directory) = path.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        eprintln!(
            "cook-clusters: cannot create {}: {error}",
            directory.display()
        );
        return ExitCode::FAILURE;
    }
    if let Err(error) = std::fs::write(&path, &bytes) {
        eprintln!("cook-clusters: cannot write {}: {error}", path.display());
        return ExitCode::FAILURE;
    }
    println!("cook-clusters: wrote {ARTIFACT}");
    ExitCode::SUCCESS
}

/// Where two artifacts first differ, so a failing `--check` says something more
/// useful than that they are not equal.
fn difference(committed: &[u8], fresh: &[u8]) -> String {
    match committed
        .iter()
        .zip(fresh)
        .position(|(left, right)| left != right)
    {
        Some(at) => format!("they first differ at byte {at}"),
        None => "one is a prefix of the other".to_string(),
    }
}

/// **The two `projected_error`s are one metric**, over the whole DAG and every
/// probe eye.
///
/// `crcbl_shaders::cluster_dag::ClusterGroup::projected_error` is a second
/// spelling of `crcbl_scene::ClusterGroup::projected_error`, on the far side of
/// a crate boundary neither can cross. That is exactly the shape two copies
/// drift in, and the drift would be invisible — a level chosen slightly wrong
/// looks like a level. So they are compared here for **bit equality**, which
/// they can be held to: both are the same three arithmetic operations over the
/// same `f32`s.
fn assert_metrics_agree(built: &BuiltDag, cooked: &ClusterDag) {
    let mut compared = 0usize;
    let mut infinite = 0usize;
    for (depth, level) in built.levels().iter().enumerate() {
        for (index, group) in level.groups().iter().enumerate() {
            let mirror = &cooked.levels[depth].groups[index];
            for eye in probe_eyes() {
                let theirs = group.projected_error(glam::Vec3::from_array(eye), PIXELS_PER_UNIT);
                let ours = mirror.projected_error(eye, PIXELS_PER_UNIT);
                assert_eq!(
                    ours.to_bits(),
                    theirs.to_bits(),
                    "level {depth} group {index} from {eye:?}: the cooked metric \
                     says {ours} and the builder's says {theirs}"
                );
                compared += 1;
                infinite += usize::from(ours.is_infinite());
            }
        }
    }
    assert!(
        compared > 0,
        "no group was compared, so nothing was checked"
    );
    assert!(
        infinite > 0,
        "no probe eye landed inside a group's sphere, so the branch that \
         returns infinity was never compared"
    );
}

/// What the DAG turned out to be, so a run says whether the model is still
/// worth its size rather than only whether the bytes match.
fn report(cooked: &ClusterDag, bytes: usize) {
    println!(
        "cook-clusters: {} vertices, {} triangles, {} levels, {bytes} bytes",
        dunes::DUNES_VERTEX_COUNT,
        dunes::DUNES_INDEX_COUNT / 3,
        cooked.levels.len()
    );
    for (depth, level) in cooked.levels.iter().enumerate() {
        let triangles: u32 = level
            .clusters
            .clusters
            .iter()
            .map(|cluster| cluster.triangle_count)
            .sum();
        let error = level.errors.iter().copied().fold(0.0f32, f32::max);
        // The top level is grouped by nothing, and a maximum over no groups is
        // not a radius of zero — it is no radius, and says so rather than
        // printing a bound the level does not have.
        let radius = match level
            .groups
            .iter()
            .map(|group| group.bounds.radius)
            .fold(f32::NEG_INFINITY, f32::max)
        {
            radius if radius.is_finite() => format!("group radius <= {radius:.2}"),
            _ => "ungrouped".to_string(),
        };
        println!(
            "  level {depth}: {:>4} clusters, {triangles:>6} triangles, \
             {:>3} groups, error <= {error:.3}, {radius}",
            level.clusters.clusters.len(),
            level.groups.len()
        );
    }
}
