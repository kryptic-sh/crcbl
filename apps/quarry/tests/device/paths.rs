//! The same face on every [`GeometryPath`] — the sample's own subject.
//!
//! `docs/plan/sample/14-quarry.md`'s milestone 3 and its "three-way comparison"
//! exit criterion. The three paths are not one per backend: they are reached by
//! **subtracting features from one capable adapter**, which is what lets a
//! machine with a mesh stage measure the two indirect paths as well — see
//! [`Quarry::open_on`](crate::harness::Quarry::open_on).
//!
//! # What is compared, and what is not
//!
//! Coverage, not pixels. The three paths emit the same triangles through
//! different draw machinery, so they agree about the *silhouette* while
//! `MeshShader` culls per cluster and the indirect paths cull per instance —
//! which is the whole reason all three exist. A byte-for-byte comparison is
//! `crates/crcbl/tests/render_e2e.rs`'s, over scenes chosen so the two paths
//! draw identical geometry; this face is chosen so they do not.

use crcbl::hal::GeometryPath;

use crate::harness::{DEFAULT_BUDGET, Levels, MIXING_BUDGET, Quarry, backend};

/// How far two paths' coverage of one frame may differ, in pixels.
///
/// **Measured, and it is not a picture tolerance.** At a one-pixel budget the
/// three cover an identical 28,650 pixels of 49,152 — the mesh path's cut is the
/// base mesh there, so all three draw the same triangles. At a sixteen-pixel
/// budget the mesh path draws levels 1 and 2 while the indirect paths select per
/// instance, and they still land within **four** pixels of each other. A tenth
/// of a percent of the frame is 49 pixels, which is margin over that and far
/// under "one path drew a different face".
const SPREAD: usize = 49;

/// Coverage of one frame of the levelled face on `path`, at `budget`.
fn coverage(path: GeometryPath, budget: f32) -> usize {
    let mut quarry = Quarry::open_on(Levels::Dag, budget, path);
    let frame = quarry.frame(crate::harness::DOLLY_START);
    quarry.finish();
    eprintln!(
        "quarry paths: {path:?} at {budget}px covered {} of {} pixels",
        frame.covered, frame.pixels,
    );
    frame.covered
}

/// **The face draws on all three geometry paths, and they agree about it.**
///
/// Read at two budgets, because they say different things. At
/// [`DEFAULT_BUDGET`] the mesh path's cut is the base mesh, so all three draw
/// the same triangles and the agreement is exact — anything else is a defect in
/// one path's draw machinery. At [`MIXING_BUDGET`] the mesh path is drawing
/// levels 1 and 2 per cluster while the other two select per instance, so they
/// are drawing *different geometry* and agreeing anyway: that is the
/// three-way comparison the sample exists for, and the same property as "no LOD
/// popping" seen from another angle.
#[test]
fn every_geometry_path_draws_the_same_face() {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry paths: the Null backend draws nothing, so there is no coverage to compare \
             across paths — run with CRCBL_GPU=vk"
        );
        return;
    }
    const PATHS: [GeometryPath; 3] = [
        GeometryPath::MeshShader,
        GeometryPath::IndirectCount,
        GeometryPath::IndirectPerBatch,
    ];

    let fine = PATHS.map(|path| coverage(path, DEFAULT_BUDGET));
    assert!(
        fine.iter().all(|covered| *covered == fine[0]),
        "at a {DEFAULT_BUDGET}px budget every path draws the base mesh, so their coverage should \
         be identical and it is {fine:?}"
    );
    assert!(
        fine[0] > 0,
        "every path covered nothing, so this compared three empty frames"
    );

    let coarse = PATHS.map(|path| coverage(path, MIXING_BUDGET));
    let spread =
        coarse.iter().max().expect("three paths") - coarse.iter().min().expect("three paths");
    assert!(
        spread <= SPREAD,
        "at a {MIXING_BUDGET}px budget the paths' coverage spans {spread} pixels, over {SPREAD} \
         — the mesh path culls per cluster and the others per instance, so some spread is the \
         design, but this is one path drawing a different face: {coarse:?}"
    );
}
