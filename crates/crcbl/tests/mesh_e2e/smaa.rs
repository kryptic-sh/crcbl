//! SMAA 1x through the frame, measured against the same scene without it.
//!
//! `crcbl_render::smaa` records three passes into the resolve slot —
//! `smaa-edges`, `smaa-weights`, `smaa-blend` — where
//! [`RenderEffects::ANTIALIASING`] records one. What a GPU test can say about
//! that is not what a golden would say (a blessed picture of an antialiased
//! cube is a picture of *something*, and stays green when the filter degrades
//! into a blur or into a copy). It is the shape of the difference:
//!
//! * **The frame changed at all.** Three passes that ran and wrote their source
//!   through would leave the frame byte-identical to the no-AA one, which is
//!   the whole failure mode a blessed image cannot see.
//! * **It changed in a band along the silhouettes and nowhere else.** SMAA
//!   reads its own edge mask, so a pixel with no luma discontinuity anywhere
//!   near it must come out of the blend untouched. A filter that lost its mask,
//!   sampled the wrong texture, or blended by a weight it never looked up
//!   touches the flat faces too — and the flat faces are most of this frame.
//! * **It changed by a little, not by a lot.** The blend mixes a pixel with one
//!   neighbour by a weight in `[0, 0.5]`, so even a fully-weighted edge pixel
//!   moves by half the discontinuity. Bytes moving by tens across the band is a
//!   blur or a shifted read, not an antialias.
//! * **And the silhouettes came out softer than they went in**, counted rather
//!   than looked at: fewer pixels whose neighbour-to-neighbour luma step is
//!   still nearly the whole discontinuity. That is the one measurement that
//!   says the filter did the thing it is for, and the threshold it is counted
//!   at matters — see [`HARD_LUMA_STEP`], which is where the obvious version of
//!   this count moves the wrong way.
//!
//! # Both tiers are drawn, because they share one slot
//!
//! `crcbl_render::forward` records SMAA *instead of* FXAA, never both, so the
//! frame this compares against is not only "SMAA off" but "the cheap tier in
//! the same slot". Drawing all three — no resolve, FXAA, SMAA — is what
//! separates a wired-up SMAA from a request that fell through to the tier that
//! was already there: two identical submissions differ by exactly zero, and
//! that is the assertion.
//!
//! # The thresholds
//!
//! Swept on both local adapters before they were pinned; each constant carries
//! its own measurements. The two adapters are radv on the discrete card and
//! lavapipe, which rasterise this silhouette a texel apart — every bound here
//! is set off the worse of the two with room, on
//! `docs/plan/12-testing.md`'s terms for a measurement that has to survive a
//! different rasteriser.

use crate::harness::Headless;
use crate::mesh_scene::{mesh_camera, place_cube, render_mesh};
use crcbl::render::{
    EffectOverride, EffectRequest, ForwardRenderer, Projection, RenderEffects, TransientPool,
};
use crcbl_golden::Image;

/// The neighbour-to-neighbour luma step, in `u8` units, that counts as an edge.
///
/// Well above the couple of units of shading gradient across a lit face and
/// well under the tens a silhouette carries, so the mask below is the
/// silhouette and not the shading.
const EDGE_LUMA_STEP: f64 = 12.0;

/// How far from an edge pixel the blend is allowed to reach, in pixels.
///
/// SMAA's blend mixes a pixel with one *immediate* neighbour, so a changed
/// pixel is either on an edge or beside one. The extra pixel of slack is for
/// the mask itself: the shader detects its edges on its own luma estimate at
/// its own threshold, which need not agree pixel-for-pixel with the one this
/// file computes on the read-back frame.
const BAND_RADIUS: u32 = 2;

/// The luma step that counts as a *hard* one, in the same `u8` units.
///
/// [`EDGE_LUMA_STEP`] finds the silhouette; this measures how sharply it still
/// steps. **Counting the pixels over the lower threshold cannot say a filter
/// antialiased anything** — it goes *up*, because a hard step spread over three
/// pixels is three steps that each still clear 12. Measured on this scene:
/// 616 pixels over 12 with no resolve, 1037 with SMAA and 1060 with FXAA. What
/// falls is the count of steps that are still nearly the whole discontinuity,
/// which is what the gradient histogram shows the resolve doing.
const HARD_LUMA_STEP: f64 = 96.0;

/// How much of the frame SMAA must move before the passes count as having run.
///
/// Swept on radv and lavapipe at 2026-08-30: 0.0152 and 0.0151 of the frame.
/// A quarter of that, so the floor is a floor rather than the measurement — and
/// a resolve that wrote its source through would measure exactly zero.
const MIN_CHANGED_FRACTION: f64 = 0.004;

/// How much of the frame SMAA may move before it has stopped being a band.
///
/// Four times the same measurement, which still leaves it an order of magnitude
/// under a filter that touched the flat faces: this scene's silhouette band is
/// 3215 pixels of 49152, and everything outside it is flat shading.
const MAX_CHANGED_FRACTION: f64 = 0.06;

/// How far the band's pixels may move, per channel on average.
///
/// The same sweep: 23.104 on radv and 23.305 on lavapipe, against a
/// discontinuity of well over a hundred — the blend mixes a pixel with one
/// neighbour by a weight in `[0, 0.5]`, so a fully-weighted edge pixel moves by
/// half the step and the average over the band is far less. A little over
/// 1.7 times the worse measurement.
const MAX_MEAN_BAND_DELTA: f64 = 40.0;

/// What fraction of the unfiltered frame's hard steps may survive the resolve.
///
/// The sweep at [`HARD_LUMA_STEP`]: 332 pixels with no resolve, against 164 on
/// radv and 167 on lavapipe — half of them, and FXAA lands in the same place
/// (150 and 154). Pinned at three quarters, so a resolve has to remove a
/// quarter of the hard steps to pass and the assertion is nowhere near the
/// measurement in either direction.
const MAX_HARD_STEP_RATIO: f64 = 0.75;

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

/// **SMAA softens the silhouettes and leaves the rest of the frame alone.**
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn smaa_changes_a_band_along_the_edges_and_nothing_else() {
    let none = cube_frame(
        EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::SMAA, Some(false)),
    );
    let fxaa = cube_frame(
        EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(true))
            .force(RenderEffects::SMAA, Some(false)),
    );
    let smaa = cube_frame(
        EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::SMAA, Some(true)),
    );

    let (width, height) = (none.width(), none.height());
    let total = (width * height) as f64;
    let mask = step_mask(&none, EDGE_LUMA_STEP);
    let band = dilate(&mask, width, height);
    let edges_none = mask.iter().filter(|on| **on).count();
    let band_pixels = band.iter().filter(|on| **on).count();
    let hard_none = steps_over(&none, HARD_LUMA_STEP);
    let hard_fxaa = steps_over(&fxaa, HARD_LUMA_STEP);
    let hard_smaa = steps_over(&smaa, HARD_LUMA_STEP);

    let against_smaa = difference(&none, &smaa, &band);
    let against_fxaa = difference(&none, &fxaa, &band);
    let smaa_vs_fxaa = difference(&fxaa, &smaa, &band);

    eprintln!(
        "crcbl mesh e2e: smaa — {width}x{height}; edge pixels {edges_none}, band \
         {band_pixels}; hard steps {hard_none} none, {hard_fxaa} fxaa, \
         {hard_smaa} smaa; smaa changed {} ({:.4} of frame, {} off band), mean \
         {:.3}, max {}; fxaa changed {} ({} off band), mean {:.3}, max {}; smaa \
         vs fxaa changed {}, mean {:.3}, max {}",
        against_smaa.changed,
        against_smaa.changed as f64 / total,
        against_smaa.changed_off_band,
        against_smaa.mean_delta,
        against_smaa.max_delta,
        against_fxaa.changed,
        against_fxaa.changed_off_band,
        against_fxaa.mean_delta,
        against_fxaa.max_delta,
        smaa_vs_fxaa.changed,
        smaa_vs_fxaa.mean_delta,
        smaa_vs_fxaa.max_delta,
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
        against_smaa.changed as f64 / total >= MIN_CHANGED_FRACTION,
        "smaa moved {} of {total} pixels, which is a resolve that wrote its \
         source through",
        against_smaa.changed
    );
    // **And it is not the cheap tier under another name.**
    assert!(
        smaa_vs_fxaa.changed > 0,
        "the smaa frame is byte-identical to the fxaa one, so the request fell \
         through to the tier that was already in the slot"
    );

    // **The band is a band.**
    assert!(
        against_smaa.changed as f64 / total <= MAX_CHANGED_FRACTION,
        "smaa moved {} of {total} pixels, which is the whole frame rather than \
         its edges",
        against_smaa.changed
    );
    assert_eq!(
        against_smaa.changed_off_band, 0,
        "smaa moved {} pixels with no luma discontinuity within {BAND_RADIUS}, \
         so the blend is not reading its edge mask",
        against_smaa.changed_off_band
    );

    // **By a little.**
    assert!(
        against_smaa.mean_delta <= MAX_MEAN_BAND_DELTA,
        "smaa moved the band by {:.3} per channel on average, which is a blur \
         rather than a blend",
        against_smaa.mean_delta
    );

    // **And the silhouettes are softer than they were.**
    assert!(
        (hard_smaa as f64) <= hard_none as f64 * MAX_HARD_STEP_RATIO,
        "smaa left {hard_smaa} pixels stepping by {HARD_LUMA_STEP} where the \
         unfiltered frame has {hard_none}, so the resolve ran without \
         antialiasing anything"
    );
}
