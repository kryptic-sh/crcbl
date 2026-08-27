//! The render-scale upscale, measured rather than looked at.
//!
//! `crcbl_render::ForwardRenderer::set_render_scale` draws the frame into a
//! smaller internal target and reconstructs it into the caller's own with
//! `shaders/upscale.slang`'s Catmull-Rom filter. What is worth a GPU test here
//! is not a blessed image — it is that the knob reaches the frame at all:
//!
//! * **The reconstruction put values in the frame that were not in the scene.**
//!   A flat-shaded cube at full resolution has a couple of hundred distinct
//!   colours; a resampled one has thousands, because every silhouette texel is
//!   now a weighted blend. That is the difference between a filter that ran and
//!   a copy that did.
//! * **It is still the same picture, filling the same target.** The upscale is
//!   the only pass that writes an image of a different extent from the one it
//!   reads, so a filter whose UV mapping is wrong draws the scene into a corner,
//!   or a quarter of it across the whole frame, and either is a perfectly good
//!   picture of something. What rules that out is the size of the difference
//!   from the full-scale frame, not the size of the image: a frame that had
//!   drifted, flipped or lost its aspect differs by tens per channel where a
//!   reconstruction differs by ones. Compared against the full-scale frame of
//!   the same scene rather than against a file, on the terms
//!   `crates/crcbl/tests/render_e2e.rs` sets out for a pass a golden cannot
//!   claim much about.
//! * **The knob reached the frame at all**, which a `set_render_scale` that
//!   recorded a pass while going on rendering at the caller's extent could not
//!   show: two identical submissions differ by exactly zero.
//! * **And the scale itself decides how much resampling there is.** Lower scales
//!   diverge further from the full-scale frame, monotonically, which separates a
//!   render scale from a fixed downsample somebody wired in once.
//!
//! # Not asserted: that the frame is softer, because it is not
//!
//! The obvious claim about an upscale — less fine detail than the full-scale
//! frame — is false here and was measured to be false before this module
//! settled on the assertions above. Catmull-Rom's outer lobes are negative, so
//! the filter sharpens as it reconstructs; on a scene whose high-frequency
//! content is one hard silhouette rather than texture detail, the overshoot at
//! that silhouette puts *more* neighbour-to-neighbour difference in the frame
//! than the full-resolution render has, not less. Measured on llvmpipe at
//! 2026-08-27: mean neighbour difference 0.747 at full scale against 0.888 at
//! half. That is the filter behaving as designed and is why the assertions below
//! are about divergence and about the values resampling introduces.

use crate::harness::Headless;
use crate::mesh_scene::{mesh_camera, place_cube, render_mesh};
use crcbl::render::{ForwardRenderer, MIN_RENDER_SCALE, Projection, TransientPool};
use crcbl_golden::Image;

/// The middle scale: half in each dimension, a quarter of the pixels.
const HALF: f32 = 0.5;

/// How far the most-scaled frame may sit from the full-scale one, per channel on
/// average, before it stops being a reconstruction of the same picture.
///
/// Swept on llvmpipe at 2026-08-27 over scales 0.9, 0.75, 0.5, 0.35 and
/// [`MIN_RENDER_SCALE`]: 0.267, 0.334, 0.639, 0.976 and 1.527. This is a little
/// over twice the largest of those, which leaves room for a rasteriser that
/// resolves the silhouette a texel differently while staying far under what a
/// drifted or flipped frame produces — two frames of this scene misaligned by
/// one texel differ by tens, not by units.
const MAX_MEAN_ABS_ERROR: f64 = 4.0;

/// How far the half-scale frame must sit from the full-scale one before the knob
/// counts as having reached the frame.
///
/// The same sweep measured 0.639 here, and a render scale that changed nothing
/// would measure exactly zero — the two frames would be the same submission.
/// A third of the measurement, so the floor is a floor rather than the value.
const MIN_MEAN_ABS_ERROR: f64 = 0.2;

/// How many times the full-scale frame's palette a resampled frame must carry.
///
/// The same sweep: 175 distinct colours at full scale, against 1293 at half and
/// 2390 at [`MIN_RENDER_SCALE`], and 172/1301/2396 on the discrete adapter.
/// Flat-shaded faces and a hard silhouette give the full-resolution frame very
/// few values; every weighted blend the filter writes is a new one. A factor of
/// two is the claim, where the measurement is a factor of seven.
const RESAMPLED_COLOUR_FACTOR: usize = 2;

/// The ceiling [`Image::distinct_colors`] counts to here.
///
/// Above every measurement in the sweep, so the counts below are counts and not
/// two frames that both saturated the same ceiling and compared equal.
const COLOUR_CEILING: usize = 4096;

/// Mean absolute difference between two frames of the same extent, per channel.
fn mean_abs_error(left: &Image, right: &Image) -> f64 {
    let total: f64 = left
        .pixels()
        .iter()
        .zip(right.pixels())
        .map(|(one, other)| f64::from(one.abs_diff(*other)))
        .sum();
    total / left.pixels().len() as f64
}

/// **The render scale reaches the frame, and the frame is still the scene.**
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_scaled_frame_is_the_same_picture_resampled_by_as_much_as_the_scale_says() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    place_cube(&mut renderer);
    let camera = mesh_camera(Projection::default());

    let full = render_mesh(&headless, &mut renderer, &mut pool, &camera, None);

    let frame_at = |renderer: &mut ForwardRenderer, pool: &mut TransientPool, scale: f32| {
        renderer.set_render_scale(scale);
        assert!(
            (renderer.render_scale() - scale).abs() < f32::EPSILON,
            "the scale has to have been taken, or this compares a frame against \
             itself"
        );
        render_mesh(&headless, renderer, pool, &camera, None)
    };
    let half = frame_at(&mut renderer, &mut pool, HALF);
    let least = frame_at(&mut renderer, &mut pool, MIN_RENDER_SCALE);

    let full_colours = full.distinct_colors(COLOUR_CEILING);
    let half_colours = half.distinct_colors(COLOUR_CEILING);
    let least_colours = least.distinct_colors(COLOUR_CEILING);
    let half_error = mean_abs_error(&full, &half);
    let least_error = mean_abs_error(&full, &least);
    eprintln!(
        "crcbl mesh e2e: render scale — mean abs error {half_error:.3} at {HALF} and \
         {least_error:.3} at {MIN_RENDER_SCALE}; distinct colours {full_colours} at \
         full, {half_colours} and {least_colours} scaled"
    );

    // **Something was reconstructed**, before any of the measurements below is
    // about anything. A pass that copied its source through would leave the
    // frame's palette exactly where the full-scale render left it.
    assert!(
        half_colours >= full_colours * RESAMPLED_COLOUR_FACTOR
            && least_colours >= full_colours * RESAMPLED_COLOUR_FACTOR,
        "the scaled frames carry {half_colours} and {least_colours} distinct \
         colours against the full frame's {full_colours}, which is not what a \
         filter writing weighted blends into every silhouette texel produces"
    );

    // **It is the same picture, in the same place.** Same scene, same camera,
    // same framing; what separates them is the resampling alone.
    assert!(
        least_error < MAX_MEAN_ABS_ERROR,
        "the most-scaled frame differs from the full one by {least_error:.3} on \
         average, which is not a reconstruction of the same picture"
    );

    // **The knob reached the frame**, which a scale that changed no extent could
    // not do: two identical submissions differ by exactly zero.
    assert!(
        half_error > MIN_MEAN_ABS_ERROR,
        "a halved frame differs from the full one by only {half_error:.3} on \
         average — the scale did not reach the frame"
    );

    // **And the scale decides how much.** Monotonic in the knob, which is what
    // separates a render scale from a fixed downsample somebody wired in once.
    assert!(
        least_error > half_error,
        "the frame at {MIN_RENDER_SCALE} diverges by {least_error:.3} and the one \
         at {HALF} by {half_error:.3} — the scale is not what decides the \
         resampling"
    );

    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
