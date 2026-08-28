//! The blocks, the constants and the arithmetic `exposure.slang` runs, in the
//! layouts that shader declares — and the host mirror of both of its halves.
//!
//! Same reason as [`crate::ssao`]: the shader fixes a byte layout, every
//! producer of those bytes has to agree with it exactly, and keeping the mirror
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.
//!
//! # The mirror is what makes the pass checkable
//!
//! [`bin_of`] and [`measure`] are the shader's own expressions, spelled the way
//! it spells them. That is not documentation: `crcbl`'s `mesh_e2e/exposure.rs`
//! bins a frame it read back and compares the result against the buffer the GPU
//! filled, which is the one thing that can tell a wrong bin from a wrong
//! dispatch extent. The binning is integer arithmetic on the exponent field of
//! an IEEE-754 float, so the two agree exactly rather than to a tolerance —
//! `exposure.slang`'s header says why that is worth having.

/// Bytes of the uniform block: two `uint`s and the padding `std140` rounds them
/// up to. See [`ExposureParams::to_bytes`].
pub const PARAMS_SIZE: usize = 16;

/// Bins in the histogram, and elements in the buffer `exposure.slang` writes.
///
/// [`OCTAVES`] octaves of luminance at [`BINS_PER_OCTAVE`] bins each. The range
/// is what a linear HDR frame in this engine actually occupies: the bottom is
/// far below anything a display shows and the top is above the specular
/// highlights `mesh_e2e/hdr.rs` measures, and both ends saturate rather than
/// wrapping — see [`bin_of`].
pub const BIN_COUNT: u32 = OCTAVES * BINS_PER_OCTAVE;

/// Bytes per bin: one `uint` of population.
pub const BIN_STRIDE: usize = 4;

/// Octaves of luminance the histogram covers, from [`MIN_EXPONENT`] up.
pub const OCTAVES: u32 = 24;

/// Bins each octave is split into, which is how many values the top of the
/// mantissa is read as.
///
/// Four, so the split is the two leading mantissa bits and a bin is a quarter
/// of an octave — about 0.75 of a stop. Finer than the exposure control's own
/// resolution and coarse enough that a 256×192 test frame still fills bins
/// densely enough to compare.
pub const BINS_PER_OCTAVE: u32 = 4;

/// How far the mantissa is shifted down to become the sub-octave index: the 23
/// mantissa bits of an `f32` less the two [`BINS_PER_OCTAVE`] reads.
pub const MANTISSA_SHIFT: u32 = 21;

/// The exponent the first bin starts at, so the histogram's floor is `2^-12`.
pub const MIN_EXPONENT: i32 = -12;

/// The factor from a bin's lower edge to the luminance [`measure`] weights it
/// at: the **geometric** centre of a quarter-octave bin, `2^(1/8)`.
///
/// A bin covers a ratio rather than an interval, so its lower edge is a biased
/// representative — every value in the bin is at or above it — and the midpoint
/// that removes the bias is the geometric one. Written as the constant rather
/// than computed, because this workspace's rule is that no transcendental
/// reaches a colour and this number reaches one through the exposure.
pub const BIN_CENTRE: f32 = 1.090_507_7;

/// Where [`measure`] starts counting: the darkest half of the frame is ignored.
///
/// A histogram exists so that the exposure follows the *subject* rather than
/// the extremes, and the two fractions are the window it reads. The floor
/// throws away shadow and background — the pixels an eye is not adapting to —
/// and [`HIGH_FRACTION`] throws away the sky, the lamp and the specular
/// highlight, which is the failure a plain average has: one bright object
/// entering frame darkens everything else.
pub const LOW_FRACTION: f32 = 0.5;

/// Where [`measure`] stops counting, for [`LOW_FRACTION`]'s reason.
pub const HIGH_FRACTION: f32 = 0.95;

/// The luminance the measured average is mapped to: middle grey.
///
/// The scene-referred 0.18 every photographic system is keyed to, and the
/// anchor `crcbl_shaders::tonemap::TonemapCurve::apply` is pinned against at
/// the other end of the pass.
pub const KEY: f32 = 0.18;

/// The darkest exposure [`measure`] will return, mirrored by
/// `crcbl_render::EXPOSURE_MIN`, whose documentation carries the argument for
/// the range.
pub const MIN_EXPOSURE: f32 = 1.0 / 32.0;

/// The brightest exposure [`measure`] will return, for [`MIN_EXPOSURE`]'s
/// reason.
pub const MAX_EXPOSURE: f32 = 32.0;

/// Bytes in the buffer `reduceMain` writes: one `float`, the exposure the
/// tonemap is to apply.
pub const MEASURED_SIZE: usize = 4;

/// Invocations per workgroup in `clearMain` and `histogramMain`, matching the
/// `numthreads` both declare.
///
/// `reduceMain` is the exception and runs on one invocation, for the reason
/// `exposure.slang` gives: a tree would sum the bins in an order a device
/// schedules, and float addition is not associative.
pub const WORKGROUP_SIZE: u32 = 64;

/// Rec. 709 relative luminance of a **linear** colour, the weights
/// `exposure.slang` and `bloom_down.slang` both declare.
#[must_use]
pub fn luma(color: [f32; 3]) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

/// The bin a luminance falls in, exactly as `exposure.slang`'s `bin_of` does.
///
/// The exponent field of an IEEE-754 float is the floor of its base-two
/// logarithm, so this is integer arithmetic and not an approximation of one:
/// the octave above [`MIN_EXPONENT`] the value sits in, times
/// [`BINS_PER_OCTAVE`], plus the two leading bits of its mantissa.
///
/// Everything below the first bin — zero, a denormal, a negative value — lands
/// in bin 0, and everything above the last, an infinity included, in the last.
#[must_use]
pub fn bin_of(luminance: f32) -> u32 {
    let bits = luminance.to_bits();
    // The sign bit rides into the exponent, so a negative value lands far above
    // the last bin rather than aliasing onto a positive one.
    let exponent = i32::try_from(bits >> 23).expect("nine bits are inside an i32") - 127;
    let fraction = (bits >> MANTISSA_SHIFT) & (BINS_PER_OCTAVE - 1);
    let index = (exponent - MIN_EXPONENT) * i32::try_from(BINS_PER_OCTAVE).expect("four")
        + i32::try_from(fraction).expect("two bits are inside an i32");
    u32::try_from(index.clamp(0, i32::try_from(BIN_COUNT).expect("96") - 1))
        .expect("the clamp leaves a non-negative index")
}

/// The lower edge of a bin, built out of the exponent field rather than out of
/// an `exp2` — the inverse of [`bin_of`] on the values that survive it.
///
/// `exposure.slang`'s `bin_luminance` is the same shift, and
/// `mesh.slang`'s `asfloat(uint(127 - int(n)) << 23)` is the same trick: a
/// power of two written directly into the exponent field is exact on every
/// target, where the intrinsic that computes it is exact on none of them.
#[must_use]
pub fn bin_luminance(bin: u32) -> f32 {
    let exponent = MIN_EXPONENT + i32::try_from(bin / BINS_PER_OCTAVE).expect("under 96");
    let fraction = (bin % BINS_PER_OCTAVE) << MANTISSA_SHIFT;
    let biased = u32::try_from(exponent + 127).expect("the range starts above -127");
    f32::from_bits((biased << 23) | fraction)
}

/// The exposure a histogram asks for, exactly as `exposure.slang`'s
/// `reduceMain` computes it.
///
/// The population between [`LOW_FRACTION`] and [`HIGH_FRACTION`] of the frame,
/// averaged at each bin's [`BIN_CENTRE`], and [`KEY`] divided by that. Bin 0 is
/// left out: it is everything below the histogram's floor, which is black and
/// near-black, and including it would drag the average toward whatever fraction
/// of the frame is empty background.
///
/// A frame with nothing in that window — every texel black, or the histogram
/// never filled — returns [`crate::tonemap::DEFAULT_EXPOSURE`], which is the
/// picture the pass draws with no auto-exposure at all.
#[must_use]
pub fn measure(histogram: &[u32]) -> f32 {
    let total: u32 = histogram
        .iter()
        .skip(1)
        .take(BIN_COUNT as usize - 1)
        .fold(0, |sum, count| sum.saturating_add(*count));
    if total == 0 {
        return crate::tonemap::DEFAULT_EXPOSURE;
    }
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the shader's own conversions, and a count of texels is far inside \
                  an f32's exact range"
    )]
    let (low, high) = (
        (total as f32 * LOW_FRACTION) as u32,
        (total as f32 * HIGH_FRACTION) as u32,
    );
    let mut seen = 0u32;
    let mut weighted = 0.0f32;
    let mut population = 0.0f32;
    for bin in 1..BIN_COUNT {
        let count = histogram.get(bin as usize).copied().unwrap_or(0);
        let start = seen;
        seen = seen.saturating_add(count);
        let (lower, upper) = (start.max(low), seen.min(high));
        if upper > lower {
            #[expect(
                clippy::cast_precision_loss,
                reason = "the shader's own conversion; a texel count is exact in an f32"
            )]
            let part = (upper - lower) as f32;
            // Spelled as the shader spells it — a multiply and an add, not a
            // fused one — so the two arrive at the same float.
            weighted += part * (bin_luminance(bin) * BIN_CENTRE);
            population += part;
        }
    }
    if population == 0.0 {
        return crate::tonemap::DEFAULT_EXPOSURE;
    }
    (KEY / (weighted / population)).clamp(MIN_EXPOSURE, MAX_EXPOSURE)
}

/// The uniform block, matching `struct ExposureParams` in
/// `shaders/exposure.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExposureParams {
    /// Width of the image being binned, in texels.
    pub viewport_x: u32,
    /// Its height.
    pub viewport_y: u32,
}

impl ExposureParams {
    /// The block as the shader reads it: two `uint`s and two padding words.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        bytes[0..4].copy_from_slice(&self.viewport_x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.viewport_y.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BIN_CENTRE, BIN_COUNT, BINS_PER_OCTAVE, HIGH_FRACTION, KEY, MAX_EXPOSURE, MIN_EXPONENT,
        MIN_EXPOSURE, bin_luminance, bin_of, luma, measure,
    };

    /// The shader this module mirrors, read at test time.
    ///
    /// Hash-pinned by the manifest, so it is the same file the committed
    /// artifacts were built from — `crate::bloom`'s guards say why that matters.
    const SOURCE: &str = include_str!("../shaders/exposure.slang");

    /// The anchors of the binning, spelled out rather than derived: a value at
    /// the bottom of the range, middle grey, one, and the two ends.
    ///
    /// Derived expectations would restate [`bin_of`]'s own arithmetic and pass
    /// for any arithmetic at all. These are the numbers a reader can check by
    /// hand: `1.0` is `2^0`, twelve octaves above the floor, four bins each.
    #[test]
    fn the_named_luminances_land_in_the_bins_that_cover_them() {
        assert_eq!(bin_of(1.0), 48, "2^0 is twelve octaves above 2^-12");
        assert_eq!(bin_of(1.5), 50, "half an octave up is two bins up");
        assert_eq!(bin_of(2.0), 52, "an octave up is four bins up");
        assert_eq!(bin_of(0.5), 44, "and an octave down is four bins down");
        assert_eq!(bin_of(f32::from_bits((127 - 12) << 23)), 0, "the floor");
        assert_eq!(bin_of(0.0), 0, "black is below the floor, not outside it");
        assert_eq!(bin_of(1e-20), 0, "and so is everything under it");
        assert_eq!(bin_of(-5.0), BIN_COUNT - 1, "a negative saturates upward");
        assert_eq!(bin_of(f32::INFINITY), BIN_COUNT - 1, "so does an infinity");
        assert_eq!(
            bin_of(1e20),
            BIN_COUNT - 1,
            "and so does the top of the range"
        );
    }

    /// Every bin reads back as itself, which is what makes [`bin_luminance`] the
    /// inverse of [`bin_of`] rather than a second guess at the same mapping.
    ///
    /// [`measure`] weights each bin at its own luminance, so a drift between the
    /// two would move the exposure by up to a bin — three quarters of a stop —
    /// with nothing else in the engine able to see it.
    #[test]
    fn every_bin_reads_back_the_bin_it_names() {
        for bin in 0..BIN_COUNT {
            let luminance = bin_luminance(bin);
            assert_eq!(
                bin_of(luminance),
                bin,
                "bin {bin} names luminance {luminance}, which bins as {}",
                bin_of(luminance)
            );
            assert!(luminance > 0.0 && luminance.is_finite());
        }
    }

    /// The bins climb, across the whole range and with no plateau.
    #[test]
    fn the_bins_climb_with_the_luminance_they_cover() {
        let mut previous = bin_luminance(0);
        for bin in 1..BIN_COUNT {
            let luminance = bin_luminance(bin);
            assert!(
                luminance > previous,
                "bin {bin} is at {luminance}, which is not above the {previous} before it"
            );
            previous = luminance;
        }
        // And an octave really is `BINS_PER_OCTAVE` bins: the ratio across one
        // group is exactly two, which is the property the exponent read gives.
        for bin in 0..BIN_COUNT - BINS_PER_OCTAVE {
            let ratio = bin_luminance(bin + BINS_PER_OCTAVE) / bin_luminance(bin);
            assert!(
                (ratio - 2.0).abs() < 1e-6,
                "bin {bin} and the bin an octave above it are a factor {ratio} apart"
            );
        }
    }

    /// A frame that is all one luminance is exposed to put that luminance at
    /// middle grey, which is the whole claim of the reduce.
    #[test]
    fn a_single_bin_is_exposed_to_middle_grey() {
        let bin = 48;
        let mut histogram = vec![0u32; BIN_COUNT as usize];
        histogram[bin as usize] = 10_000;
        let expected = KEY / (bin_luminance(bin) * BIN_CENTRE);
        let measured = measure(&histogram);
        assert!(
            (measured - expected).abs() < 1e-6,
            "a frame at {} should be exposed by {expected}, got {measured}",
            bin_luminance(bin)
        );
        // And the picture it asks for really is middle grey.
        assert!(
            (bin_luminance(bin) * BIN_CENTRE * measured - KEY).abs() < 1e-6,
            "the exposure has to map the average to {KEY}"
        );
    }

    /// **The bright tail does not move the exposure**, which is the reason this
    /// is a histogram and not an average.
    ///
    /// A specular highlight or a lamp entering frame is a small population at a
    /// luminance orders of magnitude above the subject. A mean would chase it
    /// and darken everything; the window above rejects it outright, and this
    /// pins that: adding a hundredth of the frame at four thousand times the
    /// subject's luminance changes nothing at all.
    #[test]
    fn a_bright_tail_leaves_the_exposure_alone() {
        let mut histogram = vec![0u32; BIN_COUNT as usize];
        histogram[48] = 10_000;
        let subject = measure(&histogram);
        histogram[BIN_COUNT as usize - 1] = 100;
        let with_lamp = measure(&histogram);
        assert!(
            (with_lamp - subject).abs() < 1e-6,
            "the lamp moved the exposure from {subject} to {with_lamp}"
        );
        // A mean over the same population would have: this is what was rejected.
        let mean = (10_000.0 * bin_luminance(48) + 100.0 * bin_luminance(BIN_COUNT - 1)) / 10_100.0;
        assert!(
            mean > bin_luminance(48) * 4.0,
            "the fixture has to be one a mean would actually fail: it lifts the mean \
             from {} to {mean}",
            bin_luminance(48)
        );
    }

    /// An empty histogram is the frame the pass draws with no auto-exposure at
    /// all, rather than a division by zero.
    #[test]
    fn an_unfilled_histogram_asks_for_the_default() {
        let histogram = vec![0u32; BIN_COUNT as usize];
        assert!(
            (measure(&histogram) - crate::tonemap::DEFAULT_EXPOSURE).abs() < f32::EPSILON,
            "an empty histogram must ask for the default exposure"
        );
        // And so is one whose whole population is under the floor.
        let mut black = vec![0u32; BIN_COUNT as usize];
        black[0] = 49_152;
        assert!(
            (measure(&black) - crate::tonemap::DEFAULT_EXPOSURE).abs() < f32::EPSILON,
            "a black frame must ask for the default exposure rather than the ceiling"
        );
    }

    /// The clamp holds at both ends, so no frame can drive the exposure
    /// somewhere `crcbl_render::ForwardRenderer::set_exposure` would refuse.
    #[test]
    fn the_measured_exposure_stays_inside_the_range() {
        for bin in 1..BIN_COUNT {
            let mut histogram = vec![0u32; BIN_COUNT as usize];
            histogram[bin as usize] = 1_000;
            let measured = measure(&histogram);
            assert!(
                (MIN_EXPOSURE..=MAX_EXPOSURE).contains(&measured),
                "a frame in bin {bin} asked for {measured}"
            );
        }
        // The ends are actually reached, or the clamp is guarding nothing: the
        // darkest bins want more than the ceiling and the brightest less than
        // the floor.
        let mut dark = vec![0u32; BIN_COUNT as usize];
        dark[1] = 1_000;
        assert!((measure(&dark) - MAX_EXPOSURE).abs() < f32::EPSILON);
        let mut bright = vec![0u32; BIN_COUNT as usize];
        bright[BIN_COUNT as usize - 1] = 1_000;
        assert!((measure(&bright) - MIN_EXPOSURE).abs() < f32::EPSILON);
    }

    /// The window really is a window: population below [`super::LOW_FRACTION`]
    /// and above [`HIGH_FRACTION`] is not what the exposure follows.
    #[test]
    fn the_exposure_follows_the_window_rather_than_the_whole_frame() {
        // Two thirds of the frame is dark background, a third is the subject.
        let mut histogram = vec![0u32; BIN_COUNT as usize];
        histogram[20] = 6_000;
        histogram[60] = 3_000;
        let measured = measure(&histogram);
        // The window starts at the halfway texel, which is inside the dark
        // population, and ends inside the subject — so the average sits between
        // the two, nearer the dark one.
        let dark = KEY / (bin_luminance(20) * BIN_CENTRE);
        let subject = KEY / (bin_luminance(60) * BIN_CENTRE);
        assert!(
            measured < dark && measured > subject,
            "the exposure {measured} has to sit between the two populations' own \
             ({dark} and {subject})"
        );
    }

    /// The luminance weights are the ones the shader dots with, and the ones
    /// `bloom_down.slang` uses — one definition of luminance in the engine.
    #[test]
    fn the_luminance_weights_match_every_source_that_declares_them() {
        for source in [SOURCE, include_str!("../shaders/bloom_down.slang")] {
            assert!(
                source.contains("dot(color, float3(0.2126, 0.7152, 0.0722))"),
                "a source that declares its own luminance has drifted from this module's"
            );
        }
        assert!(
            (luma([1.0, 1.0, 1.0]) - 1.0).abs() < 1e-6,
            "white has to be luminance one, or the weights do not sum to one"
        );
        assert!(luma([0.0, 1.0, 0.0]) > luma([1.0, 0.0, 0.0]));
    }

    /// The constants the shader declares are these constants.
    ///
    /// Nothing else can catch a drift: the shader compiles with any of them and
    /// the buffer is bound either way, and a `MIN_EXPONENT` that disagreed would
    /// have the host model every texel in a bin the GPU never wrote.
    #[test]
    fn the_shader_declares_the_constants_this_module_mirrors() {
        for declaration in [
            format!("static const uint BINS_PER_OCTAVE = {BINS_PER_OCTAVE};"),
            format!("static const uint BIN_COUNT = {BIN_COUNT};"),
            format!("static const int MIN_EXPONENT = {MIN_EXPONENT};"),
            format!(
                "static const uint MANTISSA_SHIFT = {};",
                super::MANTISSA_SHIFT
            ),
            format!("static const float BIN_CENTRE = {BIN_CENTRE:?};"),
            format!("static const float KEY = {KEY:?};"),
            format!(
                "static const float LOW_FRACTION = {:?};",
                super::LOW_FRACTION
            ),
            format!("static const float HIGH_FRACTION = {HIGH_FRACTION:?};"),
            format!("static const float MIN_EXPOSURE = {MIN_EXPOSURE:?};"),
            format!("static const float MAX_EXPOSURE = {MAX_EXPOSURE:?};"),
        ] {
            assert!(
                SOURCE.contains(&declaration),
                "`exposure.slang` does not declare `{declaration}`"
            );
        }
    }

    /// The block the shader declares, member for member and in this order.
    #[test]
    fn the_uniform_block_matches_the_struct_the_source_declares() {
        for declaration in ["uint viewport_x;", "uint viewport_y;"] {
            assert!(
                SOURCE.contains(declaration),
                "`exposure.slang` does not declare `{declaration}`"
            );
        }
        let params = super::ExposureParams {
            viewport_x: 0x0123_4567,
            viewport_y: 0x89ab_cdef,
        };
        let bytes = params.to_bytes();
        assert_eq!(&bytes[0..4], &0x0123_4567u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &0x89ab_cdefu32.to_le_bytes());
        assert_eq!(&bytes[8..], &[0u8; 8], "the tail is padding and stays zero");
    }
}
