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
use crcbl::render::{DebugView, DirectionalLight};

use crate::harness::{
    COARSE_BUDGET, DEFAULT_BUDGET, DOLLY_END, DOLLY_START, EXTENT, Levels, MIXING_BUDGET, Quarry,
    backend,
};

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

/// What one overlay frame holds, measured rather than compared against a file.
///
/// The goldens above are what catch a frame that changed; these are the numbers
/// the two overlay tests below reason about, and they are printed on success for
/// `Tolerance::RASTERISER`'s reason — a run that quietly passed teaches nothing
/// about the bars it passed.
struct Overlay {
    /// Distinct colours, capped so a shaded frame does not count every gradient
    /// step.
    colours: usize,
    /// The mean Rec. 709 luminance of the pixels that are **not** background, on
    /// `0..=1`.
    ///
    /// The heatmap's ramp climbs in luminance by construction — `crcbl_shaders`'
    /// `the_heatmap_ramp_climbs_in_luminance` is what holds it to that — so this
    /// is where the frame sits on the ramp, averaged over the face.
    luma: f64,
    /// How many pixels that was, so a comparison between two frames can say
    /// whether they had comparable amounts of face in them.
    covered: usize,
}

/// Renders one frame of `view` at `budget` and measures it.
///
/// **The background is taken from the frame's own top-left pixel**, not from a
/// constant: at `DOLLY_START` the face is a ridge against the sky and the
/// corners are clear colour, and reading it out of the frame means the exclusion
/// cannot drift when the renderer's clear does. Every frame compared below is
/// asserted to have found the same background, which is what makes two means
/// comparable.
fn overlay(path: GeometryPath, view: DebugView, budget: f32) -> Overlay {
    let mut quarry = Quarry::open_on(Levels::Dag, budget, path);
    quarry.renderer.set_heatmap(view == DebugView::Heatmap);
    quarry.renderer.set_lod_view(view == DebugView::LodTint);
    let frame = quarry.lit_frame(DOLLY_START, &sun());
    quarry.finish();
    let (width, height) = EXTENT;
    let image = crcbl_golden::Image::from_rgba8(width, height, frame.pixels_rgba)
        .expect("the readback is one RGBA8 frame of the ring's extent");
    let background = image.pixel(0, 0).expect("the frame has a top-left pixel");
    let (mut total, mut covered) = (0.0f64, 0usize);
    for pixel in image.pixels().chunks_exact(4) {
        if pixel[..4] == background[..] {
            continue;
        }
        let channel = |at: usize| f64::from(pixel[at]) / 255.0;
        total += 0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2);
        covered += 1;
    }
    let measured = Overlay {
        colours: image.distinct_colors(64),
        luma: if covered == 0 {
            0.0
        } else {
            total / covered as f64
        },
        covered,
    };
    eprintln!(
        "quarry overlay: {path:?} {view:?} at {budget}px — {} colour(s), {} covered pixel(s), \
         mean luma {:.4}, background {background:?}",
        measured.colours, measured.covered, measured.luma,
    );
    measured
}

/// [`Overlay::colours`] at [`MIXING_BUDGET`], which is what the LOD tint's own
/// assertion is written in terms of.
fn colours(path: GeometryPath, view: DebugView) -> usize {
    overlay(path, view, MIXING_BUDGET).colours
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

    let mesh = colours(GeometryPath::MeshShader, DebugView::LodTint);
    let per_batch = colours(GeometryPath::IndirectPerBatch, DebugView::LodTint);
    let shaded = colours(GeometryPath::MeshShader, DebugView::Shaded);

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

/// **The heatmap shades a mosaic on the mesh path and cannot on the others.**
///
/// The LOD tint's assertion above, for the tint's sibling and for the same
/// reason: a per-cluster projected error exists only where selection is per
/// cluster, so the two indirect paths draw `mesh.slang`'s one flat grey and the
/// comparison is the pair standing beside each other. **Not a shortfall** — it
/// is the capability, stated as a measurement.
#[test]
fn the_heatmap_shades_the_mesh_path_by_error_and_the_indirect_paths_flat() {
    if drew_nothing("the heatmap") {
        return;
    }

    let mesh = colours(GeometryPath::MeshShader, DebugView::Heatmap);
    let per_batch = colours(GeometryPath::IndirectPerBatch, DebugView::Heatmap);
    let count = colours(GeometryPath::IndirectCount, DebugView::Heatmap);

    assert!(
        mesh >= HEATMAP_MESH_COLOURS,
        "the mesh path shades per cluster, so a cut whose groups sit at different distances from \
         the budget must show more than a handful of ramp positions — {mesh} colour(s) is a flat \
         shade wearing a heatmap's name"
    );
    for (path, flat) in [
        (GeometryPath::IndirectPerBatch, per_batch),
        (GeometryPath::IndirectCount, count),
    ] {
        assert_eq!(
            flat, 2,
            "{path:?} has no per-cluster error, so its frame is the background and one flat grey \
             and nothing else"
        );
    }
}

/// How many distinct colours the heatmap must hold on the mesh path at
/// [`MIXING_BUDGET`].
///
/// **Measured, not chosen**: fifteen on an RX 7900 XTX at `DOLLY_START`, against
/// the two the indirect paths hold. Written as a bar at about half of it rather
/// than as an equality, because the ramp is a continuous interpolation and a
/// driver may quantise its last bits differently — the *shape* of the claim is
/// "many, not two", and the indirect paths' exact `2` is what fixes the other
/// end of it.
const HEATMAP_MESH_COLOURS: usize = 8;

/// **The heatmap shades by the error, and the budget is what the error is
/// measured against.**
///
/// This is the assertion that cannot pass by accident, and it takes three
/// frames to make because the cheap versions cannot:
///
/// * "more than one colour" passes on any gradient — a shade taken from depth,
///   from the level, or from the cluster index would satisfy it.
/// * "the frames differ under two budgets" passes on anything keyed to the
///   *cut*, which changes with the budget by itself. The LOD tint would pass it.
///
/// What only a ramp of `error / budget` does is **peak in the middle of the
/// budget range**:
///
/// * At [`DEFAULT_BUDGET`] almost everything expands and the cut is the bottom
///   of the DAG. Those clusters have **no producing group at all** — nothing
///   simplified them, so their cost is exactly zero and the ramp's floor is the
///   whole face. Measured: one colour over the face, and the coldest of the
///   three.
/// * At [`COARSE_BUDGET`] nothing expands and the cut is the top of the DAG.
///   Those groups carry the *largest* errors in the hierarchy, and against a
///   budget of thousands of pixels they still come to almost nothing — cold
///   again, by an entirely different route.
/// * At [`MIXING_BUDGET`] the descent stops part way down, and it stops there
///   *because* a group's projected error fell just under the budget. Those are
///   the ratios near one, and the warm end of the ramp.
///
/// So the middle is the brightest, and that is the shape nothing else produces.
/// A colour ignoring the error reports one luminance at all three. A colour
/// keyed to the DAG level, to the cluster count, to depth or to screen position
/// moves **monotonically** with the budget, because the cut does — it cannot be
/// low at both ends and high between them. Only dividing by the budget can.
///
/// The fine end's colour *count* is asserted beside the luminance, because it is
/// the exact half of the claim: a zero error must read as one colour, not as a
/// dark gradient.
#[test]
fn the_heatmap_is_warmest_where_the_cut_sits_against_the_budget() {
    if drew_nothing("the heatmap's budget sweep") {
        return;
    }

    let fine = overlay(GeometryPath::MeshShader, DebugView::Heatmap, DEFAULT_BUDGET);
    let mixing = overlay(GeometryPath::MeshShader, DebugView::Heatmap, MIXING_BUDGET);
    let coarse = overlay(GeometryPath::MeshShader, DebugView::Heatmap, COARSE_BUDGET);

    for measured in [&fine, &mixing, &coarse] {
        assert!(
            measured.covered > 0,
            "a frame with no face in it says nothing about a ramp over the face"
        );
    }

    assert!(
        mixing.luma > fine.luma + HEATMAP_LUMA_GAP,
        "the mixing budget's cut sits against its budget and the finest cut has no producing \
         group at all, so the middle must be the warmer: {:.4} against {:.4}",
        mixing.luma,
        fine.luma
    );
    assert!(
        mixing.luma > coarse.luma + HEATMAP_LUMA_GAP,
        "the coarsest cut's errors are tiny beside a {COARSE_BUDGET}px budget, so the middle must \
         be the warmer: {:.4} against {:.4}",
        mixing.luma,
        coarse.luma
    );
    assert!(
        fine.colours <= HEATMAP_FLOOR_COLOURS,
        "every cluster of the finest cut costs exactly zero, so the face is one flat ramp floor \
         and the frame holds the background and it — {} colour(s) is a ramp reading something \
         other than the error",
        fine.colours
    );
}

/// How far apart two of the sweep's mean luminances have to be before the
/// difference counts.
///
/// **Measured, not chosen.** On an RX 7900 XTX at `DOLLY_START` the three means
/// come to 0.3616 at one pixel, 0.6997 at sixteen and 0.4325 at 4096, so the two
/// gaps this bar has to clear are 0.34 and 0.27. It is set an order of magnitude
/// under the smaller of them, which leaves room for a driver that encodes the
/// ramp's last bits differently without leaving room for the failures above.
const HEATMAP_LUMA_GAP: f64 = 0.03;

/// How many colours the frame may hold once every cluster in the cut costs zero:
/// the background and the ramp's floor.
///
/// **Measured at exactly two** on an RX 7900 XTX, and written as a bar of three
/// rather than an equality for one reason: the cut at one pixel is not purely
/// level 0 — `harness`' sweep records `[100, 2, …]`, so two clusters of level 1
/// are in it and *do* have a producing group. Their ratio is small enough to
/// quantise onto the floor here, and a driver that rounded the other way would
/// add one colour without making the claim wrong.
const HEATMAP_FLOOR_COLOURS: usize = 3;

/// The committed golden for one path at one dolly stop.
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
