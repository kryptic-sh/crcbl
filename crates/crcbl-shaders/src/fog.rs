//! Exponential height fog: how much of a surface survives the air in front of
//! it, computed without calling `exp`.
//!
//! The model is the standard one. Fog density falls off exponentially with
//! height above a reference plane,
//!
//! ```text
//! rho(h) = density * exp(-h / falloff)
//! ```
//!
//! and what a shading pass wants is not the density but the **transmittance**
//! along the segment from the eye to the surface — the fraction of that
//! surface's radiance which arrives:
//!
//! ```text
//! T = exp(-tau),   tau = the integral of rho along the ray
//! ```
//!
//! [`optical_depth`] is `tau` in closed form and [`transmittance`] is `T`; a
//! caller composites `lerp(fog_colour, shaded, T)`.
//!
//! # Why this module exists, when `exp` is one instruction
//!
//! `docs/plan/44-lighting.md` states this workspace's shading rule: **no
//! transcendental function may reach a colour**, because the four backends'
//! implementations of them differ in the last place and this engine blesses one
//! set of golden images across all four. The `log2` calls in `mesh.slang`'s
//! `froxel_of` are not a precedent — their result is floored into an integer
//! slice index, where a last-place disagreement changes nothing.
//!
//! `docs/plan/43-render-standards.md` §4 ranks height fog as the cheapest large
//! win left on its gap list and then names that rule as what blocks it, with
//! three exits: fit the exponential the way `tonemap.slang` fits the ACES RRT,
//! let the fog goldens be the first in the tree to carry a tolerance, or drop
//! the closed form and march a froxel grid. **This module is a fourth exit and
//! it is cheaper than all three**: an exponential built out of the operations
//! the rule already permits, with no fit to be wrong about, no exception to
//! declare and no pass to write.
//!
//! # The construction
//!
//! It is the one every `libm` uses, which is exactly what makes it safe here —
//! each step is an operation IEEE-754 pins down, so no part of it is a vendor's
//! choice:
//!
//! 1. **Range reduction.** `n = round(x * LOG2_E)` and `r = x - n * ln2`, which
//!    leaves `|r|` at half of `ln 2` or less. `ln 2` is spent in two parts,
//!    [`LN2_HI`] then [`LN2_LO`], because `n * ln2` rounded once would lose the
//!    low bits of that product and hand the subtraction an argument that was
//!    already wrong — the classic way a reduced argument comes out inaccurate
//!    precisely where the cancellation makes accuracy matter.
//! 2. **The kernel.** `exp(-r)` over that interval, by [`KERNEL_DEGREE`] terms
//!    of its Taylor series in Horner form. The coefficients are the reciprocal
//!    factorials — exact rationals rather than a fit, so there is no published
//!    digit to transcribe wrongly, and `the_kernel_coefficients_are_reciprocal_factorials`
//!    checks each one against the factorial it claims to be.
//! 3. **The scale.** `2^-n`, by writing `n` into an IEEE-754 exponent field.
//!    Exact for every `n` the clamp admits.
//!
//! Measured against `f64::exp` over the whole domain it is within
//! [`MAX_KERNEL_ULP`] units in the last place — see
//! `the_exponential_tracks_the_real_one`.
//!
//! # What is not claimed
//!
//! **Not bit-identical output across backends.** No shading in this tree is:
//! `crcbl-golden` compares every image under `Tolerance::RASTERISER`, and
//! `Tolerance::EXACT` appears in no image test at all. What this construction
//! buys is a *known* ceiling on the disagreement — the one unit in the last
//! place a compiler can move a result by contracting a multiply and an add into
//! an FMA — where a call to a platform's `exp` has whatever ceiling that
//! platform's library chose, unstated and free to move with a driver release.
//! That is the same freedom the ACES fit in [`crate::tonemap`] runs under, and
//! it has been blessed on all four backends.

/// `log2(e)`. Changes the exponent's base from `e` to two, so the reduced
/// argument can be scaled by writing an exponent field.
///
/// Named here rather than used from `core` at the call site because the Slang
/// mirror has to spell it as a literal — there being no `#include` in these
/// shaders — and a named constant is what a guard can hold that literal
/// against.
pub const LOG2_E: f32 = core::f32::consts::LOG2_E;

/// The high part of `ln 2`: the constant with its low mantissa bits cleared, so
/// that `n * LN2_HI` is exact for every `n` [`exp_neg`] can produce.
pub const LN2_HI: f32 = 0.693_115_23;

/// What [`LN2_HI`] dropped, so the pair sums to `ln 2` far inside `f32`.
/// Subtracted in a second step, after the cancellation has already happened.
pub const LN2_LO: f32 = 3.194_618_3e-5;

/// Terms of the Taylor series [`exp_neg`] evaluates on the reduced argument.
///
/// The next term's contribution over the reduced interval is below `f32`'s
/// epsilon, so one more would be arithmetic that cannot change the result;
/// one fewer is visible in the last two bits.
pub const KERNEL_DEGREE: usize = 7;

/// Units in the last place [`exp_neg`] is allowed to differ from `f64::exp`.
///
/// Measured rather than chosen, over the whole domain at a fine step — the
/// worst case sits near the middle of a reduced interval, not at either end of
/// the range.
pub const MAX_KERNEL_ULP: f64 = 2.0;

/// The largest magnitude [`exp_neg`] accepts before it saturates.
///
/// Chosen so the reduced exponent stays inside the **normal** range at both
/// ends: one step further and `2^-n` writes an exponent field that has run out
/// of room, which would return zero for an argument whose true value is still
/// representable. The value at the clamp is already far below one step of an
/// 8-bit channel at one end and past anything a frame holds at the other, so
/// nothing a wider domain admitted could change a pixel.
pub const MAX_ARGUMENT: f32 = 87.0;

/// Optical depth past which [`optical_depth`] stops counting.
///
/// A ray this deep in fog transmits orders of magnitude below one step of an
/// 8-bit channel: whatever is behind it is gone, and staying gone is all a
/// larger value could buy. The clamp is also what keeps a camera far below the
/// fog plane — where the closed form's leading factor grows without bound —
/// from turning a frame into infinities.
pub const MAX_OPTICAL_DEPTH: f32 = 32.0;

/// Below this reduced height difference, [`optical_depth`] takes the series.
///
/// `(1 - exp(-d)) / d` cancels to nothing as `d` does, and the series is what
/// it cancels to. The cutoff is where the two forms agree from both sides —
/// `the_two_forms_meet_at_the_cutoff`.
const SERIES_CUTOFF: f32 = 0.125;

/// Reciprocal factorials, `1/i!`, index `i`, as Horner consumes them.
const KERNEL: [f32; KERNEL_DEGREE + 1] = [
    1.0,
    1.0,
    0.5,
    0.166_666_67,
    0.041_666_668,
    0.008_333_334,
    0.001_388_888_9,
    0.000_198_412_7,
];

/// The same coefficients shifted one factorial along — `1/(i+1)!` — which is
/// the series of `(1 - exp(-d)) / d` in `-d`.
const RATIO_KERNEL: [f32; 5] = [1.0, 0.5, 0.166_666_67, 0.041_666_668, 0.008_333_334];

/// `2^n`, by writing `n` into an IEEE-754 binary32 exponent field.
///
/// Exact over the normal range, which is every `n` [`exp_neg`] produces after
/// its clamp — so the subnormal and infinite ends are not handled here.
fn exp2_exact(n: i32) -> f32 {
    debug_assert!(
        (-126..=126).contains(&n),
        "{n} escaped the clamp MAX_ARGUMENT exists to enforce"
    );
    f32::from_bits(((n + 127) as u32) << 23)
}

/// `e^-x` for any finite `x`, using only operations IEEE-754 specifies exactly.
///
/// Saturates rather than overflowing: an `x` past [`MAX_ARGUMENT`] returns what
/// that argument returns, and one past its negation what *its* negation does.
/// Both ends are normal `f32`s, so no infinity and no subnormal originates here
/// — and the saturation is continuous, where a clamp to zero would put a cliff
/// in the middle of the representable range. A `NaN` argument returns `NaN`,
/// and nothing in this module produces one.
///
/// `exp_neg(0.0)` is exactly `1.0`, which is what makes a zero-density fog
/// exactly a no-op rather than nearly one.
#[must_use]
pub fn exp_neg(x: f32) -> f32 {
    let clamped = x.clamp(-MAX_ARGUMENT, MAX_ARGUMENT);

    // e^-x = 2^-n * e^-r, with r = x - n*ln2 and |r| at most half of ln 2.
    let n = (clamped * LOG2_E + 0.5).floor();
    let reduced = (clamped - n * LN2_HI) - n * LN2_LO;

    // Horner over 1/i!, in u = -r, so the kernel evaluates e^u = e^-r.
    let u = -reduced;
    let mut kernel = KERNEL[KERNEL_DEGREE];
    for coefficient in KERNEL[..KERNEL_DEGREE].iter().rev() {
        kernel = kernel * u + coefficient;
    }

    kernel * exp2_exact(-(n as i32))
}

/// `(1 - exp(-d)) / d`, which tends to one as `d` tends to zero.
///
/// The direct form loses every significant bit as `d` shrinks, because the
/// numerator is a difference of two nearly equal numbers; below
/// [`SERIES_CUTOFF`] this takes the series that difference converges to, which
/// has no cancellation in it at all.
fn one_minus_exp_over(d: f32) -> f32 {
    if d.abs() < SERIES_CUTOFF {
        let u = -d;
        let mut series = RATIO_KERNEL[RATIO_KERNEL.len() - 1];
        for coefficient in RATIO_KERNEL[..RATIO_KERNEL.len() - 1].iter().rev() {
            series = series * u + coefficient;
        }
        series
    } else {
        (1.0 - exp_neg(d)) / d
    }
}

/// The optical depth of exponential height fog along one ray.
///
/// `height_a` and `height_b` are the two ends' heights **above the fog
/// reference plane**, `distance` is the length of the segment between them, and
/// `falloff` is the scale height over which density drops by a factor of `e`.
/// The closed form is
///
/// ```text
/// tau = density * distance * exp(-a) * (1 - exp(-d)) / d
/// ```
///
/// with `a = height_a / falloff` and `d = (height_b - height_a) / falloff`,
/// which is the integral of the density along the segment written so the
/// leading `exp` is factored out — the arrangement that keeps a level ray, the
/// commonest one in a frame, on the series branch of `one_minus_exp_over`
/// rather than on a division by nearly zero.
///
/// A `falloff` of zero or less means fog that does not thin with height, and
/// gives `density * distance`. That is the limit the height form approaches as
/// `falloff` grows, so the branch is a continuation rather than a special case.
///
/// The result is clamped to [`MAX_OPTICAL_DEPTH`].
#[must_use]
pub fn optical_depth(
    density: f32,
    falloff: f32,
    height_a: f32,
    height_b: f32,
    distance: f32,
) -> f32 {
    if falloff <= 0.0 {
        return (density * distance).clamp(0.0, MAX_OPTICAL_DEPTH);
    }

    let a = height_a / falloff;
    let d = (height_b - height_a) / falloff;
    let tau = density * distance * exp_neg(a) * one_minus_exp_over(d);
    tau.clamp(0.0, MAX_OPTICAL_DEPTH)
}

/// The fraction of a surface's radiance that survives `optical_depth` of fog.
///
/// Exactly `1.0` at zero, so a scene with fog configured but no density draws
/// the frame it drew before fog existed.
#[must_use]
pub fn transmittance(optical_depth: f32) -> f32 {
    exp_neg(optical_depth.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `f64::exp` is the oracle throughout: a test is not shading, so the rule
    /// this module exists to satisfy does not bind here.
    fn oracle(x: f32) -> f64 {
        (-f64::from(x)).exp()
    }

    /// How far `got` is from `want`, in units of the last place of an `f32`.
    fn ulps(got: f32, want: f64) -> f64 {
        if want == 0.0 {
            return f64::from(got).abs();
        }
        ((f64::from(got) - want) / want).abs() / f64::from(f32::EPSILON / 2.0)
    }

    #[test]
    fn the_kernel_coefficients_are_reciprocal_factorials() {
        let mut factorial = 1.0f64;
        for (i, coefficient) in KERNEL.iter().enumerate() {
            if i > 1 {
                factorial *= i as f64;
            }
            #[expect(clippy::cast_possible_truncation, reason = "the point of the check")]
            let expected = (1.0 / factorial) as f32;
            assert_eq!(*coefficient, expected, "KERNEL[{i}] is not 1/{i}!");
        }
    }

    #[test]
    fn the_ratio_kernel_is_the_factorials_one_along() {
        for (i, coefficient) in RATIO_KERNEL.iter().enumerate() {
            assert_eq!(
                *coefficient,
                KERNEL[i + 1],
                "RATIO_KERNEL[{i}] is not 1/{}!",
                i + 1
            );
        }
    }

    #[test]
    fn the_exponential_tracks_the_real_one() {
        let mut worst = 0.0f64;
        let mut worst_at = 0.0f32;
        let mut steps = 0u32;
        let mut x = -MAX_ARGUMENT;
        while x <= MAX_ARGUMENT {
            let error = ulps(exp_neg(x), oracle(x));
            if error > worst {
                worst = error;
                worst_at = x;
            }
            steps += 1;
            x += 0.001;
        }
        assert!(steps > 100_000, "the sweep covered only {steps} points");
        assert!(
            worst <= MAX_KERNEL_ULP,
            "exp_neg is {worst} ulp from f64::exp at x = {worst_at}, over the \
             {MAX_KERNEL_ULP} this module documents"
        );
    }

    #[test]
    fn the_exponential_is_exactly_one_at_zero() {
        assert_eq!(exp_neg(0.0), 1.0);
        assert_eq!(transmittance(0.0), 1.0);
        assert_eq!(transmittance(-1.0), 1.0, "a negative depth is no fog");
    }

    #[test]
    fn the_exponential_saturates_instead_of_overflowing() {
        assert_eq!(exp_neg(f32::MAX), exp_neg(MAX_ARGUMENT));
        assert_eq!(exp_neg(-f32::MAX), exp_neg(-MAX_ARGUMENT));
        assert_eq!(exp_neg(MAX_ARGUMENT * 2.0), exp_neg(MAX_ARGUMENT));

        // Both ends stay normal, which is the property MAX_ARGUMENT is set for:
        // a subnormal result would have lost precision the clamp exists to keep.
        for end in [exp_neg(MAX_ARGUMENT), exp_neg(-MAX_ARGUMENT)] {
            assert!(end.is_normal(), "{end} is not a normal f32");
        }
    }

    #[test]
    fn the_exponential_only_falls() {
        let mut previous = f32::INFINITY;
        let mut x = -20.0f32;
        while x <= 20.0 {
            let value = exp_neg(x);
            assert!(value <= previous, "exp_neg rose at x = {x}");
            previous = value;
            x += 0.01;
        }
    }

    #[test]
    fn the_two_forms_meet_at_the_cutoff() {
        for side in [-1.0f32, 1.0] {
            let inside = side * (SERIES_CUTOFF - f32::EPSILON);
            let outside = side * (SERIES_CUTOFF + f32::EPSILON);
            let step = (one_minus_exp_over(outside) - one_minus_exp_over(inside)).abs();
            assert!(
                step < 1e-6,
                "the branches disagree by {step} across the cutoff at {inside}"
            );
        }
        assert_eq!(one_minus_exp_over(0.0), 1.0);
    }

    #[test]
    fn the_series_branch_beats_the_direct_one_where_it_is_used() {
        // The reason the branch exists: at a cutoff-sized argument the direct
        // form is still fine, and three decades below it is not.
        let d = SERIES_CUTOFF / 1000.0;
        let want = (1.0 - (-f64::from(d)).exp()) / f64::from(d);
        let direct = f64::from((1.0 - exp_neg(d)) / d);
        let series = f64::from(one_minus_exp_over(d));
        assert!(
            (series - want).abs() < (direct - want).abs(),
            "series {series} is no closer to {want} than direct {direct}"
        );
    }

    /// The integral this module claims to evaluate, by composite Simpson in
    /// `f64` — an independent construction, so a mistake in the closed form
    /// cannot hide in the check.
    fn quadrature(density: f64, falloff: f64, height_a: f64, height_b: f64, distance: f64) -> f64 {
        let panels = 8192;
        let mut sum = 0.0;
        for step in 0..=panels {
            let t = f64::from(step) / f64::from(panels);
            let height = height_a + t * (height_b - height_a);
            let weight = if step == 0 || step == panels {
                1.0
            } else if step % 2 == 1 {
                4.0
            } else {
                2.0
            };
            sum += weight * density * (-height / falloff).exp();
        }
        sum * distance / (3.0 * f64::from(panels))
    }

    #[test]
    fn optical_depth_matches_quadrature() {
        // Rays that climb, dive, run level, and one that barely tilts — the
        // last is the case the series branch exists for.
        let cases = [
            (0.02f32, 30.0f32, 0.0f32, 40.0f32, 60.0f32),
            (0.02, 30.0, 40.0, 0.0, 60.0),
            (0.05, 12.0, 5.0, 5.0, 100.0),
            (0.05, 12.0, 5.0, 5.000_1, 100.0),
            (0.1, 4.0, -3.0, 9.0, 25.0),
            (0.001, 200.0, 100.0, 250.0, 500.0),
        ];
        for (density, falloff, height_a, height_b, distance) in cases {
            let got = optical_depth(density, falloff, height_a, height_b, distance);
            let want = quadrature(
                f64::from(density),
                f64::from(falloff),
                f64::from(height_a),
                f64::from(height_b),
                f64::from(distance),
            );
            let error = (f64::from(got) - want).abs() / want;
            assert!(
                error < 1e-5,
                "tau {got} against quadrature {want} — {error} relative, for \
                 density {density} falloff {falloff} heights {height_a}..{height_b}"
            );
        }
    }

    #[test]
    fn fog_that_does_not_thin_is_density_times_distance() {
        assert_eq!(optical_depth(0.01, 0.0, 7.0, 9.0, 100.0), 1.0);
        assert_eq!(optical_depth(0.01, -1.0, 7.0, 9.0, 100.0), 1.0);

        // And the height form approaches it as the falloff grows, which is why
        // that branch is a continuation rather than a special case.
        let far = optical_depth(0.01, 1.0e6, 7.0, 9.0, 100.0);
        assert!((far - 1.0).abs() < 1e-4, "a very tall fog gave {far}");
    }

    #[test]
    fn optical_depth_saturates_rather_than_running_away() {
        // A camera far below the plane, where the leading factor explodes.
        let deep = optical_depth(1.0, 1.0, -1000.0, -1000.0, 1000.0);
        assert_eq!(deep, MAX_OPTICAL_DEPTH);
        assert_eq!(transmittance(deep), exp_neg(MAX_OPTICAL_DEPTH));
        assert!(transmittance(deep) < 1.0 / 255.0, "the clamp still shows");
    }

    #[test]
    fn no_fog_is_no_change() {
        assert_eq!(optical_depth(0.0, 30.0, 0.0, 10.0, 500.0), 0.0);
        assert_eq!(
            transmittance(optical_depth(0.0, 30.0, 0.0, 10.0, 500.0)),
            1.0
        );
    }

    #[test]
    fn a_longer_ray_through_the_same_air_transmits_less() {
        let mut previous = 1.0f32;
        for step in 1u16..200 {
            let distance = f32::from(step) * 2.0;
            let survives = transmittance(optical_depth(0.01, 25.0, 2.0, 2.0, distance));
            assert!(
                survives < previous,
                "{distance} metres transmitted {survives}, no less than {previous}"
            );
            previous = survives;
        }
    }
}
