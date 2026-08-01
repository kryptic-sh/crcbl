//! The comparison: a per-pixel bound and a structural one, because either
//! alone is wrong.
//!
//! `docs/plan/12-testing.md` specifies both — "compare with per-pixel
//! tolerance + SSIM-style metric (rasterizers differ slightly)" — and the
//! pairing is the whole point:
//!
//! * **A per-pixel bound alone is too weak.** Rasterisers disagree on edge
//!   coverage, so the bound has to be loose enough to admit a differently
//!   antialiased triangle edge. Loose enough for that is loose enough for a
//!   triangle that moved a little, or lost a channel, or came out dimmer —
//!   which is the "over-tolerant comparator silently passes everything" failure.
//! * **A structural metric alone is too weak in the other direction.** SSIM is
//!   computed on luma, so any recolour that preserves luma is invisible to it —
//!   a red/green channel swap scores a perfect 1.0. That is not a hypothetical:
//!   a channel-order mistake is the most common way a readback comes out wrong.
//!
//! Together they are hard to fool: the per-pixel bound catches "wrong values",
//! the structural one catches "wrong picture", and a real rasteriser difference
//! passes both because it is a handful of edge pixels off by a little.
//!
//! # SSIM, on blocks
//!
//! [`ssim`] is computed over non-overlapping 8×8 luma blocks and averaged —
//! "SSIM-style", as the plan asks, rather than the 11×11 Gaussian-windowed
//! original. The block form is what the metric is *for* here: an edge that
//! differs by one pixel of coverage barely moves its block's mean and variance,
//! while a triangle drawn in the wrong place changes several blocks completely.
//!
//! Everything in this module is pure and allocation-light, and it is tested
//! against images this crate constructs — including the cases where a naive
//! implementation would pass something it should not.

use crate::image::Image;

/// The bounds a comparison must satisfy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// Largest per-channel absolute difference a pixel may have and still be
    /// counted as matching.
    pub max_channel_delta: u8,
    /// Fraction of pixels allowed to exceed `max_channel_delta`, in `0.0..=1.0`.
    ///
    /// Not zero, because that is what makes the per-pixel bound survive two
    /// rasterisers: the pixels that differ are the ones on a triangle's edge,
    /// and how many there are scales with the perimeter, not the area.
    pub max_failing_ratio: f64,
    /// Smallest mean block SSIM that still counts as the same picture, in
    /// `0.0..=1.0`.
    pub min_ssim: f64,
}

impl Tolerance {
    /// Exact equality. What a deterministic encoder or a CPU-side transform
    /// should be held to; **not** what two rasterisers can meet.
    pub const EXACT: Self = Self {
        max_channel_delta: 0,
        max_failing_ratio: 0.0,
        min_ssim: 1.0,
    };

    /// The bound a golden image is held to across rasterisers.
    ///
    /// The numbers are **measured, not guessed**: radv and lavapipe differ on
    /// P1.2's triangle by at most one level per channel, on 3.83% of the frame,
    /// for a block SSIM of 0.999933. See the crate docs for the full table and
    /// for why the difference is spread across the interior rather than confined
    /// to the edges. Each bound here sits at least twice as far out as the
    /// observed difference, and the tests in this module pin that they are still
    /// nowhere near loose enough to admit a triangle that moved.
    pub const RASTERISER: Self = Self {
        max_channel_delta: 2,
        max_failing_ratio: 0.02,
        min_ssim: 0.99,
    };
}

/// Why a comparison failed. Empty means it did not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The images are different sizes, so nothing else was computed.
    SizeMismatch,
    /// Too many pixels exceeded [`Tolerance::max_channel_delta`].
    TooManyDifferingPixels,
    /// The structural metric fell below [`Tolerance::min_ssim`].
    StructuralMismatch,
}

/// What a comparison found.
///
/// Every field is reported whether or not the comparison passed, because the
/// numbers are the useful output: "0.9997 against a 0.99 floor" and "0.62
/// against a 0.99 floor" call for very different next steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Comparison {
    /// Reference size; `actual`'s too, unless [`Failure::SizeMismatch`].
    pub width: u32,
    /// See [`Comparison::width`].
    pub height: u32,
    /// Largest per-channel absolute difference anywhere.
    pub max_channel_delta: u8,
    /// Pixels that differ **at all**, whatever the tolerance says.
    ///
    /// Reported separately from [`failing_pixels`](Self::failing_pixels)
    /// because it is the number a tolerance is tuned *from*: "77 pixels differ,
    /// all by 1" and "77 pixels differ, one by 200" call for very different
    /// bounds, and a count that already had the tolerance applied cannot tell
    /// them apart.
    pub differing_pixels: u64,
    /// Pixels with any channel exceeding [`Tolerance::max_channel_delta`].
    pub failing_pixels: u64,
    /// `failing_pixels` over the pixel count.
    pub failing_ratio: f64,
    /// Mean absolute per-channel difference, over RGB and alpha.
    pub mean_abs_error: f64,
    /// Root-mean-square per-channel difference.
    pub rmse: f64,
    /// Mean block SSIM on luma, in `0.0..=1.0`; `1.0` is identical.
    pub ssim: f64,
    /// Why it failed. Empty when it passed.
    pub failures: [Option<Failure>; 3],
}

impl Comparison {
    /// Whether the comparison passed.
    #[must_use]
    pub fn is_match(&self) -> bool {
        self.failures.iter().all(Option::is_none)
    }

    /// The failures, without the `Option` wrapper.
    pub fn failures(&self) -> impl Iterator<Item = Failure> + '_ {
        self.failures.iter().filter_map(|failure| *failure)
    }

    /// A one-paragraph summary with every number in it, for a test failure
    /// message and for a CI log.
    #[must_use]
    pub fn summary(&self) -> String {
        // In `u64`, like `compare`'s own `pixel_count`: `width * height` in
        // `u32` overflows at 4K by 16K, and a percentage computed from a
        // wrapped denominator is worse than no percentage.
        let pixel_count = u64::from(self.width.max(1)) * u64::from(self.height.max(1));
        let mut text = format!(
            "{}x{}: {} pixel(s) differ at all ({:.4}%), max channel delta {}, {} over \
             tolerance ({:.4}%), mean abs error {:.4}, rmse {:.4}, ssim {:.6}",
            self.width,
            self.height,
            self.differing_pixels,
            self.differing_pixels as f64 / pixel_count as f64 * 100.0,
            self.max_channel_delta,
            self.failing_pixels,
            self.failing_ratio * 100.0,
            self.mean_abs_error,
            self.rmse,
            self.ssim,
        );
        if !self.is_match() {
            text.push_str(" — failed:");
            for failure in self.failures() {
                text.push_str(&format!(" {failure:?}"));
            }
        }
        text
    }
}

/// Compares two images against `tolerance`.
///
/// Differently sized images short-circuit: every other metric would be
/// comparing pixels that do not correspond, and a number computed from that is
/// worse than no number.
#[must_use]
pub fn compare(reference: &Image, actual: &Image, tolerance: &Tolerance) -> Comparison {
    if reference.width() != actual.width() || reference.height() != actual.height() {
        return Comparison {
            width: reference.width(),
            height: reference.height(),
            max_channel_delta: u8::MAX,
            differing_pixels: u64::from(reference.width()) * u64::from(reference.height()),
            failing_pixels: u64::from(reference.width()) * u64::from(reference.height()),
            failing_ratio: 1.0,
            mean_abs_error: f64::from(u8::MAX),
            rmse: f64::from(u8::MAX),
            ssim: 0.0,
            failures: [Some(Failure::SizeMismatch), None, None],
        };
    }

    let pixel_count = u64::from(reference.width()) * u64::from(reference.height());
    let mut max_channel_delta = 0u8;
    let mut differing_pixels = 0u64;
    let mut failing_pixels = 0u64;
    let mut sum_abs = 0f64;
    let mut sum_squares = 0f64;

    for (left, right) in reference
        .pixels()
        .chunks_exact(4)
        .zip(actual.pixels().chunks_exact(4))
    {
        let mut pixel_worst = 0u8;
        for channel in 0..4 {
            let delta = left[channel].abs_diff(right[channel]);
            pixel_worst = pixel_worst.max(delta);
            sum_abs += f64::from(delta);
            sum_squares += f64::from(delta) * f64::from(delta);
        }
        max_channel_delta = max_channel_delta.max(pixel_worst);
        if pixel_worst > 0 {
            differing_pixels += 1;
        }
        if pixel_worst > tolerance.max_channel_delta {
            failing_pixels += 1;
        }
    }

    let channels = (pixel_count * 4).max(1) as f64;
    let failing_ratio = failing_pixels as f64 / pixel_count.max(1) as f64;
    let ssim = ssim(reference, actual);

    let mut failures = [None, None, None];
    let mut next = 0;
    if failing_ratio > tolerance.max_failing_ratio {
        failures[next] = Some(Failure::TooManyDifferingPixels);
        next += 1;
    }
    if ssim < tolerance.min_ssim {
        failures[next] = Some(Failure::StructuralMismatch);
    }

    Comparison {
        width: reference.width(),
        height: reference.height(),
        max_channel_delta,
        differing_pixels,
        failing_pixels,
        failing_ratio,
        mean_abs_error: sum_abs / channels,
        rmse: (sum_squares / channels).sqrt(),
        ssim,
        failures,
    }
}

/// Side of the SSIM block, in pixels.
const BLOCK: u32 = 8;

/// Stabilising constants, `(0.01 * 255)^2` and `(0.03 * 255)^2` — the standard
/// values, which keep the metric defined on flat blocks where the variances are
/// zero.
const C1: f64 = 6.5025;
const C2: f64 = 58.5225;

/// Mean SSIM over non-overlapping 8×8 luma blocks.
///
/// Returns `1.0` for identical images and for an image compared with itself,
/// including the degenerate case of an image smaller than one block — where the
/// single partial block is used rather than the metric being skipped, because
/// silently returning `1.0` for a small image is exactly the over-tolerance this
/// module exists to avoid.
#[must_use]
pub fn ssim(reference: &Image, actual: &Image) -> f64 {
    if reference.width() != actual.width() || reference.height() != actual.height() {
        return 0.0;
    }
    if reference.width() == 0 || reference.height() == 0 {
        return 1.0;
    }
    let left = luma(reference);
    let right = luma(actual);
    let width = reference.width() as usize;

    let mut total = 0f64;
    let mut blocks = 0u32;
    let mut block_y = 0;
    while block_y < reference.height() {
        let mut block_x = 0;
        while block_x < reference.width() {
            let (mut sum_l, mut sum_r) = (0f64, 0f64);
            let (mut sum_ll, mut sum_rr, mut sum_lr) = (0f64, 0f64, 0f64);
            let mut count = 0f64;
            for y in block_y..(block_y + BLOCK).min(reference.height()) {
                for x in block_x..(block_x + BLOCK).min(reference.width()) {
                    let index = y as usize * width + x as usize;
                    let (l, r) = (left[index], right[index]);
                    sum_l += l;
                    sum_r += r;
                    sum_ll += l * l;
                    sum_rr += r * r;
                    sum_lr += l * r;
                    count += 1.0;
                }
            }
            let mean_l = sum_l / count;
            let mean_r = sum_r / count;
            let var_l = (sum_ll / count) - mean_l * mean_l;
            let var_r = (sum_rr / count) - mean_r * mean_r;
            let covariance = (sum_lr / count) - mean_l * mean_r;

            let numerator = (2.0 * mean_l * mean_r + C1) * (2.0 * covariance + C2);
            let denominator = (mean_l * mean_l + mean_r * mean_r + C1) * (var_l + var_r + C2);
            total += numerator / denominator;
            blocks += 1;

            block_x += BLOCK;
        }
        block_y += BLOCK;
    }
    // Floating-point error can push a perfect match a hair past 1.0, and a
    // caller comparing against a 1.0 floor deserves not to be surprised by it.
    (total / f64::from(blocks.max(1))).clamp(0.0, 1.0)
}

/// Rec. 601 luma, which is what SSIM is conventionally computed on.
///
/// Alpha is deliberately ignored: SSIM is a *structural* metric about what the
/// picture looks like, and an image whose alpha differs but whose colour does
/// not is caught by the per-pixel bound, which does look at alpha.
fn luma(image: &Image) -> Vec<f64> {
    image
        .pixels()
        .chunks_exact(4)
        .map(|pixel| {
            0.299 * f64::from(pixel[0]) + 0.587 * f64::from(pixel[1]) + 0.114 * f64::from(pixel[2])
        })
        .collect()
}

/// One pixel that two images disagree about.
///
/// [`Comparison`] says *how much* two images differ; this says **where**. A
/// cross-backend failure is read by a human who has to decide "the tolerance is
/// too tight" from "the frame moved", and a summary line alone cannot tell them
/// apart: a mean absolute error of 0.2 is the same number whether it is one
/// level spread over the background or a black square in one corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelDelta {
    /// Column, from the left.
    pub x: u32,
    /// Row, from the top.
    pub y: u32,
    /// The reference's RGBA at `(x, y)`.
    pub reference: [u8; 4],
    /// The actual image's RGBA at `(x, y)`.
    pub actual: [u8; 4],
    /// The largest per-channel absolute difference at `(x, y)`.
    pub max_delta: u8,
}

impl core::fmt::Display for PixelDelta {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "({:>5},{:>5}) reference {:?} actual {:?} delta {}",
            self.x, self.y, self.reference, self.actual, self.max_delta
        )
    }
}

/// The `limit` pixels that differ most, worst first.
///
/// Ties break by position (top-to-bottom, left-to-right) so the output is
/// **deterministic**: a failure message that names a different pixel on every
/// run is not evidence anyone can compare across runs.
///
/// Differently sized images return nothing — no pixel of one corresponds to a
/// pixel of the other, and inventing correspondences would be worse than saying
/// nothing.
#[must_use]
pub fn worst_pixels(reference: &Image, actual: &Image, limit: usize) -> Vec<PixelDelta> {
    if reference.width() != actual.width() || reference.height() != actual.height() {
        return Vec::new();
    }
    let mut worst: Vec<PixelDelta> = Vec::new();
    for y in 0..reference.height() {
        for x in 0..reference.width() {
            let (Some(left), Some(right)) = (reference.pixel(x, y), actual.pixel(x, y)) else {
                continue;
            };
            let max_delta = (0..4)
                .map(|channel| left[channel].abs_diff(right[channel]))
                .max()
                .unwrap_or(0);
            if max_delta == 0 {
                continue;
            }
            worst.push(PixelDelta {
                x,
                y,
                reference: left,
                actual: right,
                max_delta,
            });
        }
    }
    // Sorted by delta descending; the scan above already visited positions in
    // reading order and `sort_by` is stable, so equal deltas keep it.
    worst.sort_by_key(|pixel| core::cmp::Reverse(pixel.max_delta));
    worst.truncate(limit);
    worst
}

/// The bounding box `(x, y, width, height)` of every pixel differing by more
/// than `threshold`, or `None` if none does.
///
/// The other half of "where": a box covering the whole frame says the
/// difference is global — a clear colour, a tonemap constant, a gamma step —
/// and a box eight pixels across says something moved in one place. Those two
/// call for completely different investigations and a scalar cannot separate
/// them.
#[must_use]
pub fn differing_bounds(
    reference: &Image,
    actual: &Image,
    threshold: u8,
) -> Option<(u32, u32, u32, u32)> {
    if reference.width() != actual.width() || reference.height() != actual.height() {
        return None;
    }
    let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    let mut any = false;
    for y in 0..reference.height() {
        for x in 0..reference.width() {
            let (Some(left), Some(right)) = (reference.pixel(x, y), actual.pixel(x, y)) else {
                continue;
            };
            let delta = (0..4)
                .map(|channel| left[channel].abs_diff(right[channel]))
                .max()
                .unwrap_or(0);
            if delta <= threshold {
                continue;
            }
            any = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    any.then(|| (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
}

/// Renders the difference between two images, for a human and for a CI artifact.
///
/// The reference is kept as a dimmed greyscale background so the shape is still
/// legible, and every differing pixel is drawn in red at an intensity scaled by
/// how far off it is. A pixel that differs by one is visible but faint; a pixel
/// that differs by a hundred is unmistakable.
///
/// Differently sized images produce a diff the size of the reference, with the
/// region outside `actual` marked as wholly different — which is the honest
/// picture of what happened.
#[must_use]
pub fn diff_image(reference: &Image, actual: &Image) -> Image {
    // `filled_like` rather than `filled`: `reference` is already a valid image,
    // so no size check can fail here and this function needs no `Result`.
    let mut diff = Image::filled_like(reference, [0, 0, 0, 255]);
    for y in 0..reference.height() {
        for x in 0..reference.width() {
            let Some(left) = reference.pixel(x, y) else {
                continue;
            };
            let background = {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let grey = ((0.299 * f64::from(left[0])
                    + 0.587 * f64::from(left[1])
                    + 0.114 * f64::from(left[2]))
                    * 0.25) as u8;
                grey
            };
            let delta = match actual.pixel(x, y) {
                Some(right) => (0..4)
                    .map(|channel| left[channel].abs_diff(right[channel]))
                    .max()
                    .unwrap_or(0),
                // Outside the actual image entirely.
                None => u8::MAX,
            };
            let red = if delta == 0 {
                background
            } else {
                // Amplified so a delta of 1 is still visible; a diff nobody can
                // see is a diff nobody will look at twice.
                background
                    .saturating_add(96)
                    .saturating_add(delta.saturating_mul(2))
            };
            let other = if delta == 0 {
                background
            } else {
                background / 2
            };
            diff.set_pixel(x, y, [red, other, other, 255]);
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with structure: a filled triangle, so "moved" and "missing"
    /// are distinguishable from "slightly different at the edges".
    fn triangle(width: u32, height: u32, offset: i32) -> Image {
        let mut image =
            Image::filled(width, height, [10, 20, 40, 255]).expect("a valid test image");
        for y in 0..height {
            let half = (y * width) / (2 * height.max(1));
            let centre = i64::from(width) / 2 + i64::from(offset);
            for x in 0..width {
                if (i64::from(x) - centre).unsigned_abs() <= u64::from(half) {
                    image.set_pixel(x, y, [220, 60, 30, 255]);
                }
            }
        }
        image
    }

    #[test]
    fn an_image_matches_itself_exactly() {
        let image = triangle(64, 48, 0);
        let result = compare(&image, &image, &Tolerance::EXACT);
        assert!(result.is_match(), "{}", result.summary());
        assert_eq!(result.max_channel_delta, 0);
        assert_eq!(result.differing_pixels, 0);
        assert_eq!(result.failing_pixels, 0);
        assert_eq!(result.mean_abs_error, 0.0);
        assert_eq!(result.rmse, 0.0);
        assert!(
            (result.ssim - 1.0).abs() < 1e-9,
            "ssim of an image with itself must be 1.0, got {}",
            result.ssim
        );
    }

    /// **The test that stops the comparator being a rubber stamp.** A triangle
    /// shifted sideways is plainly a different picture, and it must fail — even
    /// though a per-pixel tolerance loose enough for two rasterisers would let
    /// a great many individual pixels through.
    #[test]
    fn a_shifted_triangle_fails_even_under_a_generous_per_pixel_bound() {
        let reference = triangle(64, 48, 0);
        let shifted = triangle(64, 48, 6);

        let result = compare(&reference, &shifted, &Tolerance::RASTERISER);
        assert!(!result.is_match(), "{}", result.summary());

        // And it is still caught with the per-pixel bound turned off entirely,
        // which is what proves the structural half is doing real work.
        let structural_only = Tolerance {
            max_channel_delta: 255,
            max_failing_ratio: 1.0,
            min_ssim: 0.99,
        };
        let result = compare(&reference, &shifted, &structural_only);
        assert!(
            !result.is_match(),
            "SSIM alone must catch a moved triangle: {}",
            result.summary()
        );
        assert!(
            result.ssim < 0.9,
            "a 6px shift should move SSIM a long way, got {}",
            result.ssim
        );
    }

    /// The other direction, and the reason the per-pixel bound is not
    /// redundant: SSIM is computed on **luma**, so a recolour that preserves
    /// luma is invisible to it however violent it is.
    ///
    /// The example is not contrived — a channel-order mistake is the single
    /// most common way a readback comes out wrong, and `Rgba` vs `Bgra` is a
    /// live choice in this very crate. Pure red at 255 and pure green at 130
    /// have the same Rec. 601 luma to within a fifteenth of a level, so a
    /// structural metric scores them as the same picture while every pixel is
    /// as different as two pixels can be.
    #[test]
    fn a_luma_preserving_recolour_is_invisible_to_ssim_and_caught_per_pixel() {
        let reference = Image::filled(64, 48, [255, 0, 0, 255]).expect("a valid test image");
        let recoloured = Image::filled(64, 48, [0, 130, 0, 255]).expect("a valid test image");

        let structural_only = Tolerance {
            max_channel_delta: 255,
            max_failing_ratio: 1.0,
            min_ssim: 0.99,
        };
        let blind = compare(&reference, &recoloured, &structural_only);
        assert!(
            blind.is_match(),
            "this is the documented blind spot of a luma-based structural metric: {}",
            blind.summary()
        );
        assert!(blind.ssim > 0.999, "ssim {}", blind.ssim);

        // Which is exactly why the per-pixel bound exists.
        let result = compare(&reference, &recoloured, &Tolerance::RASTERISER);
        assert!(!result.is_match(), "{}", result.summary());
        assert_eq!(result.max_channel_delta, 255);
        assert_eq!(result.failing_ratio, 1.0);
        assert!(
            result
                .failures()
                .any(|failure| failure == Failure::TooManyDifferingPixels)
        );
    }

    /// SSIM *does* have a luminance term, so a recolour that changes luma is
    /// caught structurally too. Worth pinning, because the previous test could
    /// otherwise be read as "SSIM ignores colour entirely", which would be a
    /// reason to distrust the metric rather than to pair it.
    #[test]
    fn a_recolour_that_changes_luma_moves_ssim_as_well() {
        let dark = Image::filled(64, 48, [10, 20, 30, 255]).expect("a valid test image");
        let bright = Image::filled(64, 48, [200, 210, 220, 255]).expect("a valid test image");
        let result = compare(&dark, &bright, &Tolerance::RASTERISER);
        assert!(!result.is_match());
        assert!(
            result.ssim < 0.5,
            "the luminance term must react to a large brightness change: {}",
            result.ssim
        );
    }

    /// A plausible rasteriser difference — a thin band of edge pixels, off by a
    /// little — must pass. If it does not, the gate is unusable and every run
    /// against a second driver is a false alarm.
    #[test]
    fn a_handful_of_edge_pixels_off_by_a_little_passes() {
        let reference = triangle(64, 48, 0);
        let mut actual = reference.clone();
        // One pixel per row, changed by 2 — roughly what a differing coverage
        // rule produces on a triangle's edge.
        for y in 0..actual.height() {
            let x = (32 + y / 4).min(actual.width() - 1);
            let pixel = actual.pixel(x, y).expect("in range");
            actual.set_pixel(
                x,
                y,
                [
                    pixel[0].saturating_add(2),
                    pixel[1].saturating_add(1),
                    pixel[2].saturating_sub(2),
                    pixel[3],
                ],
            );
        }
        let result = compare(&reference, &actual, &Tolerance::RASTERISER);
        assert!(
            result.is_match(),
            "an edge-width difference must not fail the gate: {}",
            result.summary()
        );
        assert!(result.max_channel_delta <= 2);
        assert_eq!(
            result.differing_pixels,
            u64::from(actual.height()),
            "the pixels that differ are counted even though none of them fail"
        );
        assert_eq!(result.failing_pixels, 0);
    }

    /// The failing *ratio* is what admits edge noise, so it must be enforced —
    /// the same per-pixel delta applied to every pixel is not edge noise.
    #[test]
    fn the_failing_ratio_is_enforced_rather_than_the_delta_alone() {
        let reference = triangle(64, 48, 0);
        let mut everywhere = reference.clone();
        for y in 0..everywhere.height() {
            for x in 0..everywhere.width() {
                let pixel = everywhere.pixel(x, y).expect("in range");
                everywhere.set_pixel(
                    x,
                    y,
                    [pixel[0].saturating_add(9), pixel[1], pixel[2], pixel[3]],
                );
            }
        }
        let result = compare(&reference, &everywhere, &Tolerance::RASTERISER);
        assert!(
            !result.is_match(),
            "a small delta on every pixel is not edge noise: {}",
            result.summary()
        );
        assert!(
            result
                .failures()
                .any(|failure| failure == Failure::TooManyDifferingPixels)
        );
    }

    /// Nothing rendered at all is the single most likely failure, and it must
    /// not be able to pass under any tolerance short of "anything goes".
    #[test]
    fn a_blank_frame_never_matches_a_drawn_one() {
        let reference = triangle(64, 48, 0);
        let blank = Image::filled(64, 48, [0, 0, 0, 255]).expect("a valid test image");
        let result = compare(&reference, &blank, &Tolerance::RASTERISER);
        assert!(!result.is_match(), "{}", result.summary());
        assert!(result.ssim < 0.5, "ssim {}", result.ssim);
        assert!(result.rmse > 20.0, "rmse {}", result.rmse);
    }

    /// Different sizes must short-circuit rather than compare corresponding
    /// bytes of non-corresponding pixels.
    #[test]
    fn differently_sized_images_report_a_size_mismatch_and_nothing_else() {
        let reference = triangle(64, 48, 0);
        let smaller = triangle(32, 24, 0);
        let result = compare(&reference, &smaller, &Tolerance::RASTERISER);
        assert!(!result.is_match());
        assert_eq!(result.failures().count(), 1);
        assert!(result.failures().any(|f| f == Failure::SizeMismatch));
        assert_eq!(result.ssim, 0.0);
    }

    /// An image smaller than one SSIM block must still be measured, not
    /// silently scored 1.0.
    #[test]
    fn images_smaller_than_a_block_are_still_measured() {
        let reference = Image::filled(3, 3, [0, 0, 0, 255]).expect("a valid test image");
        let other = Image::filled(3, 3, [255, 255, 255, 255]).expect("a valid test image");
        assert!(
            ssim(&reference, &other) < 0.5,
            "a 3x3 black/white pair must not score as identical: {}",
            ssim(&reference, &other)
        );
        assert!((ssim(&reference, &reference) - 1.0).abs() < 1e-9);
    }

    /// SSIM must be symmetric and bounded, or a "reference vs actual" swap
    /// would change the verdict.
    #[test]
    fn ssim_is_symmetric_and_within_zero_to_one() {
        let a = triangle(64, 48, 0);
        let b = triangle(64, 48, 3);
        let forward = ssim(&a, &b);
        let backward = ssim(&b, &a);
        assert!(
            (forward - backward).abs() < 1e-12,
            "{forward} vs {backward}"
        );
        for value in [
            forward,
            ssim(&a, &a),
            ssim(
                &a,
                &Image::filled(64, 48, [0; 4]).expect("a valid test image"),
            ),
        ] {
            assert!((0.0..=1.0).contains(&value), "{value}");
        }
    }

    /// The diff has to make the difference visible, or nobody will read the
    /// artifact CI uploaded.
    #[test]
    fn the_diff_marks_changed_pixels_in_red_and_leaves_the_rest_grey() {
        let reference = triangle(32, 24, 0);
        let mut actual = reference.clone();
        actual.set_pixel(16, 12, [0, 255, 0, 255]);

        let diff = diff_image(&reference, &actual);
        let changed = diff.pixel(16, 12).expect("in range");
        assert!(
            changed[0] > changed[1] && changed[0] > changed[2],
            "a changed pixel must be red-dominant: {changed:?}"
        );
        // An unchanged pixel is grey — all three channels equal.
        let same = diff.pixel(0, 0).expect("in range");
        assert_eq!(same[0], same[1]);
        assert_eq!(same[1], same[2]);
        assert_eq!(diff.width(), reference.width());
    }

    /// "Where and by how much", on a picture whose answer is known: one pixel
    /// moved a long way and a scatter of pixels moved a little. The big one has
    /// to come first, and the order has to be the same on every run.
    #[test]
    fn the_worst_pixels_are_reported_worst_first_and_deterministically() {
        let reference = triangle(32, 24, 0);
        let mut actual = reference.clone();
        actual.set_pixel(20, 5, [0, 0, 0, 0]);
        actual.set_pixel(3, 1, [11, 20, 40, 255]);
        actual.set_pixel(4, 1, [10, 21, 40, 255]);

        let worst = worst_pixels(&reference, &actual, 8);
        assert_eq!(worst.len(), 3, "{worst:?}");
        assert_eq!((worst[0].x, worst[0].y), (20, 5));
        assert_eq!(worst[0].max_delta, 255);
        // The two one-level pixels tie, so reading order decides — and decides
        // the same way every time.
        assert_eq!((worst[1].x, worst[1].y), (3, 1));
        assert_eq!((worst[2].x, worst[2].y), (4, 1));
        assert_eq!(worst, worst_pixels(&reference, &actual, 8));

        // The limit is a limit.
        assert_eq!(worst_pixels(&reference, &actual, 1).len(), 1);
        // And a display line a human reads has the coordinates and both colours
        // in it, because that is the whole point of the type.
        let line = worst[0].to_string();
        for needle in ["20", "5", "255"] {
            assert!(line.contains(needle), "{needle:?} missing from {line}");
        }
    }

    /// The bounding box separates "everything drifted" from "one thing moved",
    /// which is the first question anyone asks about a red cross-backend run.
    #[test]
    fn the_bounding_box_distinguishes_a_local_change_from_a_global_one() {
        let reference = triangle(32, 24, 0);

        let mut local = reference.clone();
        for x in 10..14 {
            local.set_pixel(x, 7, [0, 0, 0, 255]);
        }
        assert_eq!(
            differing_bounds(&reference, &local, 2),
            Some((10, 7, 4, 1)),
            "a four-pixel run must report a four-pixel box"
        );

        // A global one-level shift: invisible at a threshold of 2, and the
        // whole frame at a threshold of 0. Both answers are the truth about a
        // different question, which is why the harness prints both.
        let mut global = reference.clone();
        for y in 0..reference.height() {
            for x in 0..reference.width() {
                let pixel = reference.pixel(x, y).expect("in range");
                global.set_pixel(x, y, [pixel[0] + 1, pixel[1], pixel[2], pixel[3]]);
            }
        }
        assert_eq!(differing_bounds(&reference, &global, 2), None);
        assert_eq!(
            differing_bounds(&reference, &global, 0),
            Some((0, 0, 32, 24))
        );
    }

    /// Differently sized images have no corresponding pixels, so the two
    /// location helpers say nothing rather than something invented.
    #[test]
    fn locating_a_difference_between_differently_sized_images_reports_nothing() {
        let reference = triangle(32, 24, 0);
        let smaller = triangle(16, 12, 0);
        assert!(worst_pixels(&reference, &smaller, 8).is_empty());
        assert_eq!(differing_bounds(&reference, &smaller, 0), None);
    }

    /// The summary is what a failing test prints, so it must actually contain
    /// the numbers someone would act on.
    #[test]
    fn the_summary_names_every_number_and_the_reasons() {
        let reference = triangle(64, 48, 0);
        let shifted = triangle(64, 48, 8);
        let result = compare(&reference, &shifted, &Tolerance::RASTERISER);
        let summary = result.summary();
        for needle in [
            "64x48",
            "differ at all",
            "max channel delta",
            "ssim",
            "rmse",
            "failed:",
        ] {
            assert!(
                summary.contains(needle),
                "{needle:?} missing from {summary}"
            );
        }
    }
}
