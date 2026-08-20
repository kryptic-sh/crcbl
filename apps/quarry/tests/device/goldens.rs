//! The committed frames: one per [`GeometryPath`], at each end of the dolly.
//!
//! `docs/plan/sample/14-quarry.md`'s exit criteria ask for "golden frames per
//! `GeometryPath` from the fixed dolly". Everything else this suite asserts is a
//! **count** — coverage, the cut, the triangle totals — and
//! [`Frame::pixels_rgba`](crate::harness::Frame) exists because a face lit from
//! the wrong side leaves every one of them unchanged. This module is the half
//! that would notice.
//!
//! # Why six, and why these six
//!
//! Three paths at two dolly stops. The dolly **translates into the quarry**
//! rather than zooming: at the start the face is a lit ridge against the sky, at
//! the end the camera is inside it and the face fills the frame. Named for the
//! stop rather than for a distance, because "near" and "far" invert depending on
//! whether you mean the camera or the geometry — and they were written the wrong
//! way round the first time.
//!
//! One stop per path would catch a path that
//! breaks outright and miss one that breaks partway down the dolly, which is
//! where the cut changes and therefore where this sample's whole subject lives.
//! Nine stops per path would be twenty-seven images for one fixture.
//!
//! The budget is [`MIXING_BUDGET`] rather than the default, because that is
//! where the three paths genuinely disagree: the mesh path draws levels 1 and 2
//! per cluster while the other two select one level per instance. At the default
//! budget all three draw the base mesh and one golden would do for all of them —
//! a per-path golden that cannot tell the paths apart is a per-path golden in
//! name only.
//!
//! # The tolerance is measured, not guessed
//!
//! [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER)'s numbers were
//! taken between radv and lavapipe, which are exactly the two drivers this
//! content has to survive: the frames are blessed on an RX 7900 XTX here and
//! compared on lavapipe in CI. `crcbl-golden`'s crate docs carry the
//! measurements.

use crcbl::hal::GeometryPath;
use crcbl::render::DirectionalLight;

use crate::harness::{DOLLY_END, DOLLY_START, EXTENT, Levels, MIXING_BUDGET, Quarry, backend};

/// The sun the goldens are lit by.
///
/// Fixed rather than defaulted so a change to the engine's default direction
/// shows up as a failing golden here — which is the point — rather than as six
/// images that quietly re-bless to something else.
fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: crcbl::math::Vec3::new(1.0, 0.8, 0.6).normalize(),
        ..DirectionalLight::default()
    }
}

/// Renders one frame and checks it against its committed reference.
fn check(path: GeometryPath, at: f32, name: &str) {
    let mut quarry = Quarry::open_on(Levels::Dag, MIXING_BUDGET, path);
    let frame = quarry.lit_frame(at, &sun());
    quarry.finish();

    let (width, height) = EXTENT;
    let image = crcbl_golden::Image::from_rgba8(width, height, frame.pixels_rgba)
        .expect("the readback is one RGBA8 frame of the ring's extent");

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    let golden =
        crcbl_golden::Golden::new(reference).with_tolerance(crcbl_golden::Tolerance::RASTERISER);
    let outcome = golden.check(&image).expect("the reference is readable");
    let comparison = outcome
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    // Printed on success too: the numbers are how the tolerance stays honest
    // across two drivers, and a run that quietly passes teaches nothing.
    eprintln!("quarry goldens: {name} — {}", comparison.summary());
}

/// The guard every test below shares: the Null backend draws nothing.
fn drew_nothing(name: &str) -> bool {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry goldens: {name} — the Null backend draws nothing, so there are no pixels to \
             compare; run with CRCBL_GPU=vk"
        );
        return true;
    }
    false
}

/// One test per image rather than one over six.
///
/// `crcbl-golden` **fails a blessing run on purpose**, so that re-blessing can
/// never be mistaken for comparing. A single test would therefore write one
/// image and stop, and blessing six would take six runs. One test per golden
/// blesses the set in one pass with `--no-fail-fast`, and names in the failure
/// which frame moved.
macro_rules! golden_test {
    ($fn_name:ident, $path:expr, $at:expr, $name:literal) => {
        #[test]
        fn $fn_name() {
            if drew_nothing($name) {
                return;
            }
            check($path, $at, $name);
        }
    };
}

golden_test!(
    the_mesh_shader_path_matches_its_golden_at_the_dolly_start,
    GeometryPath::MeshShader,
    DOLLY_START,
    "mesh-shader-dolly-start"
);
golden_test!(
    the_mesh_shader_path_matches_its_golden_at_the_dolly_end,
    GeometryPath::MeshShader,
    DOLLY_END,
    "mesh-shader-dolly-end"
);
golden_test!(
    the_indirect_count_path_matches_its_golden_at_the_dolly_start,
    GeometryPath::IndirectCount,
    DOLLY_START,
    "indirect-count-dolly-start"
);
golden_test!(
    the_indirect_count_path_matches_its_golden_at_the_dolly_end,
    GeometryPath::IndirectCount,
    DOLLY_END,
    "indirect-count-dolly-end"
);
golden_test!(
    the_indirect_per_batch_path_matches_its_golden_at_the_dolly_start,
    GeometryPath::IndirectPerBatch,
    DOLLY_START,
    "indirect-per-batch-dolly-start"
);
golden_test!(
    the_indirect_per_batch_path_matches_its_golden_at_the_dolly_end,
    GeometryPath::IndirectPerBatch,
    DOLLY_END,
    "indirect-per-batch-dolly-end"
);

/// How many distinct colours a frame holds, capped so a shaded frame does not
/// count every gradient step.
fn colours(path: GeometryPath, lod_view: bool) -> usize {
    let mut quarry = Quarry::open_on(Levels::Dag, MIXING_BUDGET, path);
    quarry.renderer.set_lod_view(lod_view);
    let frame = quarry.lit_frame(DOLLY_START, &sun());
    quarry.finish();
    let (width, height) = EXTENT;
    let image = crcbl_golden::Image::from_rgba8(width, height, frame.pixels_rgba)
        .expect("the readback is one RGBA8 frame of the ring's extent");
    let count = image.distinct_colors(64);
    eprintln!("quarry lod view: {path:?} lod_view={lod_view} — {count} distinct colour(s)");
    count
}

/// **The LOD tint shows a mosaic on the mesh path and cannot on the others.**
///
/// The claim cluster LOD makes is that one mesh spans several levels across its
/// own surface, and every other assertion in this suite is a count that a flat
/// tint would satisfy just as well. This is the one that looks.
///
/// The two indirect paths are not a shortfall here: they select one level per
/// *instance*, so there is no per-cluster level for them to tint by and
/// `mesh.slang`'s vertex stage writes one flat grey instead. Standing the two
/// beside each other is the comparison.
#[test]
fn the_lod_view_tints_the_mesh_path_by_level_and_the_indirect_paths_flat() {
    if drew_nothing("the LOD view") {
        return;
    }

    let mesh = colours(GeometryPath::MeshShader, true);
    let per_batch = colours(GeometryPath::IndirectPerBatch, true);
    let shaded = colours(GeometryPath::MeshShader, false);

    // Measured on radv at `MIXING_BUDGET`: the mesh path holds 3 — the
    // background and two level hues, which is the same "levels 1 and 2" the
    // coverage tests find at this budget — the per-batch path holds 2, and a
    // shaded frame saturates the 64 cap. The bars are written against those
    // numbers rather than as bare inequalities, because "more than the other
    // one" would pass on a tint that produced two hues by accident.
    assert!(
        mesh >= 3,
        "the mesh path tints per cluster, so a cut spanning two levels must show \
         the background and both hues — {mesh} colour(s) is a flat tint wearing \
         a mosaic's name"
    );
    assert_eq!(
        per_batch, 2,
        "the per-batch path has no per-cluster level, so its frame is the \
         background and one flat grey and nothing else"
    );
    assert!(
        shaded > mesh,
        "a shaded frame is a gradient and a tinted one is a handful of flat \
         hues, so shading must hold more colours: {shaded} against {mesh}"
    );
}

/// The committed golden for one path at one dolly stop.
fn golden_path(tag: &str, stop: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{tag}-dolly-{stop}.png"))
}

/// **The three paths' committed frames agree**, which is the sample's own
/// three-way comparison, asserted rather than eyeballed.
///
/// # Why this needs no device
///
/// The six goldens are in the repository, so the comparison is between files and
/// runs on any machine — including a CI job with no GPU at all. That matters
/// more than it sounds: the exit criteria ask for the three-way comparison to be
/// *recorded*, and a record that only a machine with a mesh stage can reproduce
/// is a record most readers cannot check.
///
/// # What "agree" means here, and what it does not
///
/// [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER), the same bar
/// the per-path checks use. The three are **not** expected to be identical: at
/// `MIXING_BUDGET` the mesh path selects a level per cluster while the other two
/// select one per instance, so they are drawing different geometry and agreeing
/// about the picture anyway. That is the claim. A test demanding equality would
/// be asserting the paths are the same mechanism, which they are not.
#[test]
fn the_three_paths_committed_goldens_agree_at_both_dolly_stops() {
    for stop in ["start", "end"] {
        let reference = golden_path("mesh-shader", stop);
        for tag in ["indirect-count", "indirect-per-batch"] {
            let actual = crcbl_golden::Image::load_png(golden_path(tag, stop))
                .expect("the committed golden is a readable PNG");
            let outcome = crcbl_golden::Golden::new(&reference)
                .with_tolerance(crcbl_golden::Tolerance::RASTERISER)
                .check(&actual)
                .expect("the reference is readable");
            let comparison = outcome.into_result().unwrap_or_else(|message| {
                panic!("mesh-shader against {tag} at the dolly {stop}: {message}")
            });
            // Printed on success: the numbers *are* the recorded comparison, and
            // a run that passes silently records nothing.
            eprintln!(
                "quarry three-way: mesh-shader against {tag} at the dolly {stop} — {}",
                comparison.summary()
            );
        }
    }
}
