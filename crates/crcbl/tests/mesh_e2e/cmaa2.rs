//! CMAA2 through the frame, measured against the same scene without it.
//!
//! `crcbl_render::cmaa2` records three passes into the resolve slot —
//! `cmaa2-edges`, `cmaa2-shapes` and `cmaa2-apply` — where
//! [`RenderEffects::ANTIALIASING`] records one. What a GPU
//! test can say about that is not what a golden would say (a blessed picture of
//! an antialiased cube is a picture of *something*, and stays green when the
//! filter degrades into a blur or into a copy). It is the shape of the
//! difference, and these are the retired SMAA tier's three measurements on the
//! same scene — the antialiasing ladder held CMAA2 to its observer in as many
//! words (`docs/notes/rendering.md`):
//!
//! * **The frame changed at all.** Three passes that ran and wrote their source
//!   through would leave the frame byte-identical to the no-AA one, which is
//!   the whole failure mode a blessed image cannot see.
//! * **It changed in a band along the silhouettes and nowhere else.** CMAA2
//!   classifies only pixels its edge pass marked, so a pixel with no luma
//!   discontinuity anywhere near it must come out of the apply untouched. A
//!   filter that lost its edge predicate, read the wrong texel or accumulated a
//!   weight nothing derived touches the flat faces too — and the flat faces are
//!   most of this frame.
//! * **It changed by a little, not by a lot.** A reconstructed line is at most
//!   half a pixel from the aliased boundary it replaces, so a fully-weighted
//!   edge pixel moves by half the discontinuity. Bytes moving by tens across the
//!   band is a blur or a shifted read, not an antialias.
//! * **And the silhouettes came out softer than they went in**, counted rather
//!   than looked at: fewer pixels whose neighbour-to-neighbour luma step is
//!   still nearly the whole discontinuity. That is the one measurement that
//!   says the filter did the thing it is for, and the threshold it is counted
//!   at matters — see [`HARD_LUMA_STEP`], which is where the obvious version of
//!   this count moves the wrong way.
//!
//! Two more live here that the tier this replaced had no need of, because it
//! was three fullscreen draws and this one is a scatter into shared memory:
//! [`the_same_frame_resolves_to_the_same_bytes_twice`] and
//! [`a_dense_edge_frame_resolves_to_the_same_bytes_every_time`]. Each carries
//! its own argument, and between them they cover the two ways a scatter stops
//! being a function of its inputs — the arithmetic it sums in, and what it does
//! when there is more to sum than it planned for.
//!
//! # Both tiers are drawn, because they share one slot
//!
//! `crcbl_render::forward` records CMAA2 *instead of* FXAA, never both, so the
//! frame this compares against is not only "CMAA2 off" but "the cheap tier in
//! the same slot". Drawing all three — no resolve, FXAA, CMAA2 — is what
//! separates a wired-up CMAA2 from a request that fell through to the tier that
//! was already there: two identical submissions differ by exactly zero, and
//! that is the assertion.
//!
//! # The thresholds
//!
//! Swept on both local adapters before they were pinned; each constant carries
//! its own measurements. The two adapters are radv on the discrete card and
//! lavapipe, which rasterise this silhouette a texel apart — every bound here
//! is set off the worse of the two with room, on
//! `docs/notes/process.md`'s terms for a measurement that has to survive a
//! different rasteriser.

use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, MESH_SECONDS, mesh_camera, place, place_cube, render_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{DEMO_CUBE, DEMO_UNTINTED};
use crcbl::render::{
    Camera, EffectOverride, EffectRequest, ForwardRenderer, Projection, RenderEffects,
    TransientPool,
};
use crcbl_golden::Image;

/// The neighbour-to-neighbour luma step, in `u8` units, that counts as an edge.
///
/// Well above the couple of units of shading gradient across a lit face and
/// well under the tens a silhouette carries, so the mask below is the
/// silhouette and not the shading.
const EDGE_LUMA_STEP: f64 = 12.0;

/// How far from an edge pixel the resolve is allowed to reach, in pixels.
///
/// CMAA2 moves a pixel toward one *immediate* neighbour — the one on the other
/// side of the boundary the run was found along — so a changed pixel is either
/// on an edge or beside one. The extra pixel of slack is for
/// the mask itself: the shader detects its edges on its own luma estimate at
/// its own threshold, which need not agree pixel-for-pixel with the one this
/// file computes on the read-back frame.
const BAND_RADIUS: u32 = 2;

/// The luma step that counts as a *hard* one, in the same `u8` units.
///
/// [`EDGE_LUMA_STEP`] finds the silhouette; this measures how sharply it still
/// steps. **Counting the pixels over the lower threshold cannot say a filter
/// antialiased anything** — it goes *up*, because a hard step spread over three
/// pixels is three steps that each still clear 12. Measured on this scene: 616
/// pixels over 12 with no resolve. What falls is the count of steps that are
/// still nearly the whole discontinuity, which is what the gradient histogram
/// shows a resolve doing.
const HARD_LUMA_STEP: f64 = 96.0;

/// How much of the frame CMAA2 must move before the passes count as having run.
///
/// Swept on radv and lavapipe at 2026-09-06: 0.0162 of the frame on both. A
/// quarter of that, so the floor is a floor rather than the measurement — and a
/// resolve that wrote its source through would measure exactly zero.
const MIN_CHANGED_FRACTION: f64 = 0.004;

/// How much of the frame CMAA2 may move before it has stopped being a band.
///
/// Four times the same measurement, which still leaves it an order of magnitude
/// under a filter that touched the flat faces: this scene's silhouette band is
/// 3215 pixels of 49152, and everything outside it is flat shading.
const MAX_CHANGED_FRACTION: f64 = 0.065;

/// How far the band's pixels may move, per channel on average.
///
/// The same sweep: 25.919 on radv and 25.813 on lavapipe, against a
/// discontinuity of well over a hundred — a reconstructed boundary is at most
/// half a pixel from the aliased one, so a fully-weighted edge pixel moves by
/// half the step and the average over the band is far less. A little over
/// 1.7 times the worse measurement.
const MAX_MEAN_BAND_DELTA: f64 = 44.0;

/// What fraction of the unfiltered frame's hard steps may survive the resolve.
///
/// The sweep at [`HARD_LUMA_STEP`]: 332 pixels with no resolve, against 145 on
/// both adapters — well under half of them, and better than FXAA in the same
/// slot (150 on radv, 154 on lavapipe). Pinned at a half again the worse
/// measurement, so a resolve has to remove a third of the hard steps to pass and
/// the assertion is nowhere near the measurement in either direction.
const MAX_HARD_STEP_RATIO: f64 = 0.65;

/// Rec. 709 luma of an RGBA8 pixel, in the same `u8` units.
fn luma(pixel: [u8; 4]) -> f64 {
    0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])
}

/// The larger of the two forward neighbour luma steps at `(x, y)`.
///
/// Zero on the last row and column, which have no forward neighbour to step to.
fn gradient(image: &Image, x: u32, y: u32) -> f64 {
    let here = luma(image.pixel(x, y).expect("inside the image"));
    let right = image.pixel(x + 1, y).map_or(here, luma);
    let down = image.pixel(x, y + 1).map_or(here, luma);
    (here - right).abs().max((here - down).abs())
}

/// Every pixel whose luma steps by at least `step` into a neighbour.
fn step_mask(image: &Image, step: f64) -> Vec<bool> {
    let mut mask = vec![false; (image.width() * image.height()) as usize];
    for y in 0..image.height() {
        for x in 0..image.width() {
            mask[(y * image.width() + x) as usize] = gradient(image, x, y) >= step;
        }
    }
    mask
}

/// How many pixels [`step_mask`] marks.
fn steps_over(image: &Image, step: f64) -> usize {
    step_mask(image, step).iter().filter(|on| **on).count()
}

/// The mask grown by [`BAND_RADIUS`] in every direction.
fn dilate(mask: &[bool], width: u32, height: u32) -> Vec<bool> {
    let mut grown = vec![false; mask.len()];
    for y in 0..height {
        for x in 0..width {
            if !mask[(y * width + x) as usize] {
                continue;
            }
            let (x0, y0) = (x.saturating_sub(BAND_RADIUS), y.saturating_sub(BAND_RADIUS));
            let x1 = (x + BAND_RADIUS).min(width - 1);
            let y1 = (y + BAND_RADIUS).min(height - 1);
            for gy in y0..=y1 {
                for gx in x0..=x1 {
                    grown[(gy * width + gx) as usize] = true;
                }
            }
        }
    }
    grown
}

/// What one frame's difference from the no-AA frame looks like.
struct Difference {
    /// Pixels differing in any channel.
    changed: usize,
    /// Of those, the ones with no edge within [`BAND_RADIUS`].
    changed_off_band: usize,
    /// Mean absolute per-channel move over the changed pixels.
    mean_delta: f64,
    /// The largest single-channel move anywhere.
    max_delta: u8,
}

/// Compares `frame` against `base` under `base`'s own dilated edge mask.
fn difference(base: &Image, frame: &Image, band: &[bool]) -> Difference {
    let (width, height) = (base.width(), base.height());
    let mut changed = 0;
    let mut changed_off_band = 0;
    let mut total_delta = 0u64;
    let mut max_delta = 0u8;
    for y in 0..height {
        for x in 0..width {
            let one = base.pixel(x, y).expect("inside the image");
            let other = frame.pixel(x, y).expect("the same extent");
            // Alpha is the swapchain's own and never a claim about the filter.
            let delta: u32 = (0..3).map(|c| u32::from(one[c].abs_diff(other[c]))).sum();
            if delta == 0 {
                continue;
            }
            changed += 1;
            total_delta += u64::from(delta);
            max_delta = max_delta.max((0..3).map(|c| one[c].abs_diff(other[c])).max().unwrap_or(0));
            if !band[(y * width + x) as usize] {
                changed_off_band += 1;
            }
        }
    }
    Difference {
        changed,
        changed_off_band,
        mean_delta: if changed == 0 {
            0.0
        } else {
            total_delta as f64 / (changed as f64 * 3.0)
        },
        max_delta,
    }
}

/// The demo cube drawn with the resolve slot set as the caller asks.
fn cube_frame(effects: EffectOverride) -> Image {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: effects,
        ..EffectRequest::default()
    });
    place_cube(&mut renderer);
    let camera = mesh_camera(Projection::default());
    render_mesh(&headless, &mut renderer, &mut pool, &camera, None)
}

/// The override that puts the named tier in the resolve slot, and nothing else
/// in it.
fn tier(fxaa: bool, cmaa2: bool) -> EffectOverride {
    EffectOverride::none()
        .force(RenderEffects::ANTIALIASING, Some(fxaa))
        .force(RenderEffects::CMAA2, Some(cmaa2))
}

/// How many cells across the frame [`dense_frame`] cuts it into, and how many
/// down.
///
/// Sixteen by twelve is the frame's own 4:3 in whole cells, so a cell is a
/// square sixteen texels on a side and every cube in the grid covers the same
/// area of the frame as every other.
const DENSE_COLUMNS: u32 = 16;

/// The other half of that grid — see [`DENSE_COLUMNS`].
const DENSE_ROWS: u32 = 12;

/// Half the world height [`dense_frame`]'s orthographic camera covers.
///
/// The unit, and it is arbitrary: an orthographic projection has no perspective
/// divide, so the grid's pixel geometry is fixed by [`DENSE_COLUMNS`] and
/// [`DENSE_ROWS`] alone and this is only the scale everything else is measured
/// in.
const DENSE_HALF_HEIGHT: f32 = 1.0;

/// How wide each cube in the grid is, as a fraction of its cell.
///
/// Under one so the cubes do not touch. A grid of cubes that met would have one
/// silhouette around the outside of it instead of
/// [`DENSE_COLUMNS`] × [`DENSE_ROWS`] of them, which is the opposite of what
/// this fixture exists for.
const DENSE_CUBE_FILL: f32 = 0.65;

/// Where [`dense_frame`]'s camera stands, and the depth range it sees.
///
/// Down the `+Z` axis at a grid that sits in the `z = 0` plane, with the cubes'
/// own half-extent well inside the slab: an orthographic camera clips on a
/// finite far plane, unlike the perspective one every other frame in this suite
/// is drawn with.
const DENSE_CAMERA_DISTANCE: f32 = 4.0;

/// The near plane of that camera — see [`DENSE_CAMERA_DISTANCE`].
const DENSE_NEAR: f32 = 0.1;

/// Its far plane, likewise.
const DENSE_FAR: f32 = 10.0;

/// How many times [`a_dense_edge_frame_resolves_to_the_same_bytes_every_time`]
/// resolves its frame.
///
/// Eight, and the count is the sensitivity. Whether two runs of a
/// scheduling-dependent resolve happen to agree is itself a matter of chance,
/// so one repeat only catches a defect that fires more often than not and every
/// repeat past that is another chance for a rarer one to show. Eight is where
/// that stops being worth another device and another frame — and it is well
/// past what the shape this replaced needed, which moved this frame by
/// thousands of pixels on every comparison of every attempt.
const DENSE_RESOLVES: usize = 8;

/// **A frame packed with silhouettes**, resolved through CMAA2.
///
/// [`cube_frame`]'s scene is one cube on a flat background, which is a few
/// thousand edge pixels in a frame of 49152 — comfortably inside anything this
/// tier would size a working set for. This one is a grid of small spun cubes
/// covering the whole frame, so the edges are a large fraction of it rather
/// than a band across the middle, which is the regime where a per-frame budget
/// and the picture start to interact.
///
/// The cubes are placed on the `z = 0` plane and drawn through an
/// **orthographic** camera, which is the one projection in this suite that maps
/// the grid onto the frame with no perspective divide: every cube is the same
/// number of texels across, so the fixture's density is a property of
/// [`DENSE_COLUMNS`], [`DENSE_ROWS`] and [`DENSE_CUBE_FILL`] and not of where a
/// cube happens to sit.
fn dense_frame(effects: EffectOverride) -> Image {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: effects,
        ..EffectRequest::default()
    });

    let (width, height) = MESH_EXTENT;
    let half_width = DENSE_HALF_HEIGHT * width as f32 / height as f32;
    let cell = 2.0 * DENSE_HALF_HEIGHT / DENSE_ROWS as f32;
    // The spin every mesh frame in this suite is drawn at, so each cube shows
    // three differently-coloured faces and the grid carries interior edges as
    // well as silhouettes.
    let spin = ForwardRenderer::spin(MESH_SECONDS);
    for row in 0..DENSE_ROWS {
        for column in 0..DENSE_COLUMNS {
            let x = -half_width + (column as f32 + 0.5) * (2.0 * half_width / DENSE_COLUMNS as f32);
            let y = -DENSE_HALF_HEIGHT + (row as f32 + 0.5) * cell;
            place(
                &mut renderer,
                DEMO_CUBE,
                DEMO_UNTINTED,
                Mat4::from_translation(Vec3::new(x, y, 0.0))
                    * Mat4::from_scale(Vec3::splat(cell * DENSE_CUBE_FILL))
                    * spin,
            );
        }
    }

    let camera = Camera {
        eye: Vec3::new(0.0, 0.0, DENSE_CAMERA_DISTANCE),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::Orthographic {
            half_height: DENSE_HALF_HEIGHT,
            near: DENSE_NEAR,
            far: DENSE_FAR,
        },
    };
    render_mesh(&headless, &mut renderer, &mut pool, &camera, None)
}

/// **A frame whose edges are most of it resolves to the same bytes every
/// time.**
///
/// [`the_same_frame_resolves_to_the_same_bytes_twice`] makes the same claim on
/// the one-cube scene, and the two do not replace each other: that one holds
/// the accumulation's arithmetic — a float sum in arrival order would move on a
/// scene with any edges at all — and this one holds the tier's *working set*.
/// A pass that keeps its intermediate results in a fixed budget and discards
/// what does not fit has a picture that depends on **which** entries won the
/// room, and which ones win is the device's scheduling of the work-groups that
/// produced them. That is invisible on a scene whose edges fit and is the whole
/// behaviour on one whose edges do not, so the fixture has to be dense — see
/// [`dense_frame`].
///
/// [`DENSE_RESOLVES`] independent runs, each opening its own device and
/// submitting its own frame, all compared against the first. The count is the
/// sensitivity: see that constant.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_dense_edge_frame_resolves_to_the_same_bytes_every_time() {
    let none = dense_frame(tier(false, false));
    let first = dense_frame(tier(false, true));
    let (width, height) = (first.width(), first.height());
    let differing = |a: &Image, b: &Image| {
        (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|&(x, y)| a.pixel(x, y) != b.pixel(x, y))
            .count()
    };

    // **Not vacuous**, and this is the half that makes the rest mean something:
    // a fixture the tier found no edges in would come back identical every run
    // whatever the resolve did with it. The unresolved frame is what says CMAA2
    // moved this one.
    let resolved = differing(&none, &first);
    let mut worst = 0usize;
    for run in 1..DENSE_RESOLVES {
        let again = dense_frame(tier(false, true));
        let moved = differing(&first, &again);
        eprintln!(
            "crcbl mesh e2e: cmaa2 dense determinism — {width}x{height}, the \
             resolve moved {resolved} pixels; run {run} differs from the first \
             in {moved} pixels"
        );
        worst = worst.max(moved);
    }

    assert!(
        resolved > 0,
        "the dense frame is byte-identical with the tier on and off, so this \
         fixture has nothing for the resolve to be nondeterministic about"
    );
    assert_eq!(
        worst, 0,
        "a dense-edge frame resolved {DENSE_RESOLVES} times differs from its \
         own first run in up to {worst} pixels, so what the tier keeps depends \
         on the order the device produced it in"
    );
}

/// **CMAA2 softens the silhouettes and leaves the rest of the frame alone.**
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn cmaa2_changes_a_band_along_the_edges_and_nothing_else() {
    let none = cube_frame(tier(false, false));
    let fxaa = cube_frame(tier(true, false));
    let cmaa2 = cube_frame(tier(false, true));

    let (width, height) = (none.width(), none.height());
    let total = (width * height) as f64;
    let mask = step_mask(&none, EDGE_LUMA_STEP);
    let band = dilate(&mask, width, height);
    let edges_none = mask.iter().filter(|on| **on).count();
    let band_pixels = band.iter().filter(|on| **on).count();
    let hard_none = steps_over(&none, HARD_LUMA_STEP);
    let hard_fxaa = steps_over(&fxaa, HARD_LUMA_STEP);
    let hard_cmaa2 = steps_over(&cmaa2, HARD_LUMA_STEP);

    let against_cmaa2 = difference(&none, &cmaa2, &band);
    let against_fxaa = difference(&none, &fxaa, &band);
    let cmaa2_vs_fxaa = difference(&fxaa, &cmaa2, &band);

    eprintln!(
        "crcbl mesh e2e: cmaa2 — {width}x{height}; edge pixels {edges_none}, band \
         {band_pixels}; hard steps {hard_none} none, {hard_fxaa} fxaa, \
         {hard_cmaa2} cmaa2; cmaa2 changed {} ({:.4} of frame, {} off band), mean \
         {:.3}, max {}; fxaa changed {} ({} off band), mean {:.3}, max {}; cmaa2 \
         vs fxaa changed {}, mean {:.3}, max {}",
        against_cmaa2.changed,
        against_cmaa2.changed as f64 / total,
        against_cmaa2.changed_off_band,
        against_cmaa2.mean_delta,
        against_cmaa2.max_delta,
        against_fxaa.changed,
        against_fxaa.changed_off_band,
        against_fxaa.mean_delta,
        against_fxaa.max_delta,
        cmaa2_vs_fxaa.changed,
        cmaa2_vs_fxaa.mean_delta,
        cmaa2_vs_fxaa.max_delta,
    );

    // **The mask is a mask**, before anything measured under it means
    // something: a threshold that matched the whole frame would make the band
    // test vacuous, and one that matched nothing would make it unfalsifiable.
    assert!(
        edges_none > 0 && band_pixels < (total * 0.5) as usize,
        "the edge mask has to be the silhouette: {edges_none} edge pixels, \
         {band_pixels} in the band of {total}"
    );

    // **Three passes ran and something came out of them.**
    assert!(
        against_cmaa2.changed as f64 / total >= MIN_CHANGED_FRACTION,
        "cmaa2 moved {} of {total} pixels, which is a resolve that wrote its \
         source through",
        against_cmaa2.changed
    );
    // **And it is not the cheap tier under another name.**
    assert!(
        cmaa2_vs_fxaa.changed > 0,
        "the cmaa2 frame is byte-identical to the fxaa one, so the request fell \
         through to the tier that was already in the slot"
    );

    // **The band is a band.**
    assert!(
        against_cmaa2.changed as f64 / total <= MAX_CHANGED_FRACTION,
        "cmaa2 moved {} of {total} pixels, which is the whole frame rather than \
         its edges",
        against_cmaa2.changed
    );
    assert_eq!(
        against_cmaa2.changed_off_band, 0,
        "cmaa2 moved {} pixels with no luma discontinuity within {BAND_RADIUS}, \
         so the apply is not reading its edge buffer",
        against_cmaa2.changed_off_band
    );

    // **By a little.**
    assert!(
        against_cmaa2.mean_delta <= MAX_MEAN_BAND_DELTA,
        "cmaa2 moved the band by {:.3} per channel on average, which is a blur \
         rather than a blend",
        against_cmaa2.mean_delta
    );

    // **And the silhouettes are softer than they were.**
    assert!(
        (hard_cmaa2 as f64) <= hard_none as f64 * MAX_HARD_STEP_RATIO,
        "cmaa2 left {hard_cmaa2} pixels stepping by {HARD_LUMA_STEP} where the \
         unfiltered frame has {hard_none}, so the resolve ran without \
         antialiasing anything"
    );
}

/// **The same frame resolves to the same bytes twice.**
///
/// This is the assertion the tier's whole apply design exists for. CMAA2's
/// shares are *scattered*: a pixel's colour is a sum over contributions
/// produced by different work-groups, which reach it in whatever order the
/// device schedules. A float sum in that order would make the frame a function
/// of the scheduler — the determinism rule `docs/notes/rendering.md` records,
/// and the reason a golden could not be blessed on it — so
/// `cmaa2_shapes.slang` sums in fixed point with integer atomics, which are
/// associative and commutative, and `cmaa2_apply.slang` converts once, per
/// pixel, after every share has landed.
///
/// **Two whole runs, not two frames of one run.** Each `cube_frame` opens its
/// own device, records its own graph and submits its own frame, so what this
/// compares is two independent schedules of the same work rather than one warm
/// cache read twice.
///
/// It runs on radv and on lavapipe, which is the point: two rasterisers that
/// distribute work-groups differently is where an order dependence shows up as
/// two different pictures rather than as one that happens to be stable.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_same_frame_resolves_to_the_same_bytes_twice() {
    let first = cube_frame(tier(false, true));
    let second = cube_frame(tier(false, true));

    let differing = (0..first.height())
        .flat_map(|y| (0..first.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| first.pixel(x, y) != second.pixel(x, y))
        .count();
    eprintln!(
        "crcbl mesh e2e: cmaa2 determinism — {}x{}, {differing} pixels differ \
         between two runs of the same frame",
        first.width(),
        first.height(),
    );

    // Not vacuous: the frame under test is one the tier actually resolved, and
    // the observer above is what says so — this pair would also be identical if
    // the resolve had written its source through, which is why both tests
    // exist and neither replaces the other.
    assert_eq!(
        differing, 0,
        "two runs of one frame differ in {differing} pixels, so the apply's \
         accumulation depends on the order its shares arrived in"
    );
}
